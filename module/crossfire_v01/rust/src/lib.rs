//! crossfire on its production board, revision 0.1: the hardware-bound instantiation.
//!
//! TODAY THIS IS THE BOARD'S BRING-UP FIRMWARE, not the application. The application as it
//! stands drives an OLED and hosts instruments on the native USB port; this board has no
//! display, hosts on a PIO pair that does not exist yet, and puts the native port in its device
//! role. What is here is what the first boards need from a firmware: a console on both
//! transports, the hub held in reset until asked, the indicators and the button exercised, and
//! the two CC lines read and interpreted -- the things the board's "to prove" list wants a
//! probe or a console for. The application replaces the board module below when it is wired
//! to this hardware; the entry points, the console and the board crate stay.

#![no_std]

use light_board_crossfire_v01::board::{self, Peripherals, ADC_FULL_SCALE_MV, REVISION};
use light_core::cli::{Cli, Command, Outcome, Parsed, Words};
use light_core::events::Subscription;
use light_core::{info, log, warn, EventBus, Idle, InputPin, LineReader, Mailbox, Module, Poll, Runtime, StaticCell};
use light_rp2::shell::{panic_report, ShellInfo, UART_BAUD, UART_RX, UART_TX};
use light_rp2::uart::Uart;
use light_rp2::{now_us, Breathe, Clocks};

/// What the console can ask of the board.
#[derive(Clone, Copy, Debug)]
enum Cmd {
        /// Release the hub from reset and tell it its upstream is live, or hold it again.
        Hub(bool),
        /// Read both CC lines and say what the source advertises.
        Cc,
        /// Drive an indicator by hand.
        Led { green: bool, on: bool },
        /// Start or stop the green heartbeat.
        Blink(bool),
        /// Whether USER is pressed right now.
        Button,
}

static EVENTS: EventBus<Cmd, 4, 2> = EventBus::new();
static CONSOLE_BYTES: Mailbox<u8, 128> = Mailbox::new();

fn on_off(w: &mut Words) -> Option<bool> {
        match w.next() {
                Some("on") => Some(true),
                Some("off") => Some(false),
                _ => None,
        }
}

static COMMANDS: &[Command<Cmd>] = &[
        Command { name: "hub", usage: "hub on|off -- release the hub from reset and tell it VBUS is up, or hold it in reset again", parse: |w| on_off(w).map_or(Parsed::Usage, |on| Parsed::Event(Cmd::Hub(on))) },
        Command { name: "cc", usage: "cc -- read both CC lines and say what the source advertises", parse: |_| Parsed::Event(Cmd::Cc) },
        Command {
                name: "led",
                usage: "led red|green on|off",
                parse: |w| {
                        let green = match w.next() {
                                Some("red") => false,
                                Some("green") => true,
                                _ => return Parsed::Usage,
                        };
                        on_off(w).map_or(Parsed::Usage, |on| Parsed::Event(Cmd::Led { green, on }))
                },
        },
        Command { name: "blink", usage: "blink on|off -- the green heartbeat, on by default", parse: |w| on_off(w).map_or(Parsed::Usage, |on| Parsed::Event(Cmd::Blink(on))) },
        Command { name: "button", usage: "button -- whether USER is pressed", parse: |_| Parsed::Event(Cmd::Button) },
];
static CLI: Cli<Cmd> = Cli::new(COMMANDS);

/// Lines from either console transport, dispatched to the table above.
struct ConsoleMod {
        reader: LineReader<96>,
}

impl Module for ConsoleMod {
        fn name(&self) -> &'static str {
                "console"
        }

        fn poll(&mut self) -> Poll {
                let mut result = Poll::Idle;
                while let Some(b) = CONSOLE_BYTES.pop() {
                        if let Some(line) = self.reader.push(b) {
                                match CLI.dispatch(line.as_str()) {
                                        Outcome::Quiet => {}
                                        Outcome::Shutdown => return Poll::Shutdown,
                                        Outcome::Event(e) => {
                                                let _ = EVENTS.publish(e);
                                                result = Poll::Busy;
                                        }
                                        Outcome::Handled => result = Poll::Busy,
                                }
                        }
                }
                result
        }
}

/// The USB-C specification's three Rd voltage bands, as the sink sees them across its 5.1 kΩ:
/// 0.25 to 0.61 V for a default source, 0.70 to 1.16 V for 1.5 A, 1.31 to 2.04 V for 3 A.
/// The thresholds sit midway between the bands; below the lowest band the line has no pull-up
/// on it at all, which is the unused CC line of an attached cable, or no cable.
fn advertisement(mv: u32) -> &'static str {
        match mv {
                0..=199 => "no pull-up",
                200..=655 => "default USB power: 500 mA at most",
                656..=1235 => "1.5 A",
                1236..=2200 => "3 A",
                _ => "above any band: not a USB-C source",
        }
}

fn millivolts(raw: u16) -> u32 {
        u32::from(raw) * ADC_FULL_SCALE_MV / 4095
}

/// The board's own peripherals, exercised: the heartbeat, the button on the red indicator,
/// the hub under console control, and the CC lines on request.
struct BoardMod {
        p: Peripherals,
        events: Subscription,
        blink: bool,
        green: bool,
        last_blink_us: u64,
        pressed: bool,
}

const BLINK_HALF_PERIOD_US: u64 = 500_000;

impl BoardMod {
        fn new(p: Peripherals) -> Self {
                Self { p, events: EVENTS.subscribe().expect("subscriber slot"), blink: true, green: false, last_blink_us: 0, pressed: false }
        }

        fn hub(&mut self, on: bool) {
                if on {
                        //   out of reset first, then told its upstream is there: the part
                        // samples its straps on the way out of reset and attaches when
                        // VBUS_DET rises
                        self.p.hub_reset_n.set(true);
                        self.p.hub_vbus_det.set(true);
                        info!("hub: released from reset, upstream reported live");
                } else {
                        self.p.hub_vbus_det.set(false);
                        self.p.hub_reset_n.set(false);
                        info!("hub: held in reset");
                }
        }

        fn cc(&mut self) {
                let cc1 = millivolts(self.p.cc1.read());
                let cc2 = millivolts(self.p.cc2.read());
                info!("cc1 {} mV ({}); cc2 {} mV ({})", cc1, advertisement(cc1), cc2, advertisement(cc2));
                //   the source pulls up the one line its cable orients to; the other is the
                // cable's own Ra or nothing, so the higher reading is the advertisement
                let (line, mv) = if cc1 >= cc2 { ("CC1", cc1) } else { ("CC2", cc2) };
                info!("source on {line}: {}", advertisement(mv));
        }

        fn led(&mut self, green: bool, on: bool) {
                if green {
                        //   driving the green by hand takes the heartbeat off it, or the next
                        // half-period would undo the command
                        self.blink = false;
                        self.green = on;
                        self.p.led_green.set(on);
                } else {
                        self.p.led_red.set(on);
                }
        }
}

impl Module for BoardMod {
        fn name(&self) -> &'static str {
                "board"
        }

        fn load(&mut self) -> Result<(), ()> {
                info!("board: hub held in reset -- `hub on` releases it; `help` lists the rest");
                Ok(())
        }

        fn poll(&mut self) -> Poll {
                let mut busy = false;
                while let Some(cmd) = EVENTS.poll(&self.events) {
                        match cmd {
                                Cmd::Hub(on) => self.hub(on),
                                Cmd::Cc => self.cc(),
                                Cmd::Led { green, on } => self.led(green, on),
                                Cmd::Blink(on) => self.blink = on,
                                Cmd::Button => info!("button: {}", if self.p.button.is_low() { "pressed" } else { "released" }),
                        }
                        busy = true;
                }
                let now = now_us();
                if self.blink && now - self.last_blink_us >= BLINK_HALF_PERIOD_US {
                        self.green = !self.green;
                        self.p.led_green.set(self.green);
                        self.last_blink_us = now;
                }
                //   the button on the red indicator, and on the console on each edge: the one
                // input the board has, proven without a probe
                let pressed = self.p.button.is_low();
                if pressed != self.pressed {
                        self.pressed = pressed;
                        self.p.led_red.set(pressed);
                        info!("button: {}", if pressed { "pressed" } else { "released" });
                        busy = true;
                }
                if busy { Poll::Busy } else { Poll::Idle }
        }

        fn unload(&mut self) {
                self.hub(false);
                self.p.led_red.set(false);
                self.p.led_green.set(false);
        }
}

/// Core 1: the port's console loop on both transports -- the CDC console over the hardware
/// USB port, which on this board is the device, and the UART on the debug header.
#[unsafe(no_mangle)]
pub extern "C" fn light_app_core1_main(info: &ShellInfo) -> ! {
        // SAFETY: core 1's one construction of the console UART
        let uart = unsafe { Uart::new(UART_TX, UART_RX, UART_BAUD, info.clk_peri_hz) };
        light_rp2::shell::core1_main(
                |b| {
                        let _ = CONSOLE_BYTES.push(b);
                },
                Some(uart),
        )
}

#[unsafe(no_mangle)]
pub extern "C" fn light_app_main(info: &ShellInfo) -> ! {
        log::set_clock(now_us);
        let clocks = Clocks { sys_hz: info.clk_sys_hz, peri_hz: info.clk_peri_hz };
        let p = board::take(&clocks).expect("the board's peripherals are taken once");
        info!("crossfire rev {}: bring-up firmware; sys {} Hz, peri {} Hz; console on USB and the UART", REVISION, clocks.sys_hz, clocks.peri_hz);
        if clocks.sys_hz != board::DESIGN_SYS_HZ {
                info!("the board is designed for {} Hz; this build runs the SDK's default", board::DESIGN_SYS_HZ);
        }
        //   which image the ROM chose: on a board with an A/B pair this is the only answer to
        // "which image am I running?"
        if let Some(b) = light_rp2::shell::boot_info() {
                info!("boot: type {}, partition {:?}, probation {:#x}", b.boot_type, b.partition, b.tbyb_and_update);
        }

        //   the modules live in .bss, never on core 0's small stack
        static BOARD_MOD: StaticCell<BoardMod> = StaticCell::new();
        let board_mod = BOARD_MOD.init(BoardMod::new(p));
        static CONSOLE_MOD: StaticCell<ConsoleMod> = StaticCell::new();
        let console_mod = CONSOLE_MOD.init(ConsoleMod { reader: LineReader::new() });

        let mut rt: Runtime<4> = Runtime::new();
        rt.add(console_mod).expect("capacity");
        rt.add(board_mod).expect("capacity");
        let mut idle = Breathe;
        match rt.run(|| idle.idle()) {
                Ok(()) => warn!("runtime stopped: `quit` was asked for; reset the board to start again"),
                Err(e) => warn!("runtime stopped: {e:?}"),
        }
        loop {
                idle.idle();
        }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
        panic_report(info)
}
