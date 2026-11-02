//! crossfire on its production board, revision 0.1: the hardware-bound instantiation. The
//! application is `light_app_crossfire`, with no hardware in it; this crate is everything
//! tangible -- the board's wiring through its own crate, the RP2350 chip feature, the host
//! stack on the PIO pair, the shell ABI, the panic handler -- constructed here and handed to
//! [`app::serve`].
//!
//! What this board brings that the others do not: a hub of its own to run ([`HubMod`]), two
//! indicators in place of a display ([`IndicatorMod`]), the supply's advertisement off the
//! receptacle's CC lines, and a USER button as the navigation control. What it does not bring:
//! glass, a radio, an asset pack -- there is nothing to draw with, so there is nothing to read
//! from the data partition, and the image is complete in itself.
//!
//! THE HOST IS BUILT OUT OF PIO and the hardware USB port is the device: the console rides it,
//! and with it the route into the bootloader over the product's own connector. The PIO host
//! wants a system clock that is a multiple of 12 MHz; the module's CMake chooses one, and the
//! firmware says so at start-up if it did not get one.

#![no_std]

use light_app_crossfire as app;
use app::{CcSense, ConsoleMod, HubMod, IndicatorMod, NavMod, ProbationMod, UsbMod, AppEvent, EVENTS};
use crossfire_rp2350::{vitals, RomProbation, UpdateMod};
use light_board_crossfire_v01::board::{self, ADC_FULL_SCALE_MV, DESIGN_SYS_HZ, PIN_HOST_DP, REVISION};
use light_core::events::Subscription;
use light_core::{info, warn, log, ConstStaticCell, Idle, InputPin, Module, Poll, StaticCell};
use light_rp2::adc::Adc;
use light_rp2::gpio::{Input, Output};
use light_rp2::shell::{panic_report, ShellInfo, UART_BAUD, UART_RX, UART_TX};
use light_rp2::uart::Uart;
use light_rp2::usb_host::UsbMidiHost;
use light_rp2::usb_host_pio::PioHostController;
use light_rp2::{now_us, Breathe, Clocks};

/// The host's transmit DMA channel: the top of the chip's sixteen, where the SDK's own
/// claims (counting up from 0) will not reach.
const HOST_DMA_CH: usize = 15;

/// The receptacle's CC lines, read through the board's ADC and scaled to millivolts.
struct CcLines {
        cc1: Adc,
        cc2: Adc,
}

fn millivolts(raw: u16) -> u32 {
        u32::from(raw) * ADC_FULL_SCALE_MV / 4095
}

impl CcSense for CcLines {
        fn read_mv(&mut self) -> (u32, u32) {
                (millivolts(self.cc1.read()), millivolts(self.cc2.read()))
        }
}

/// What the PIO host's frame engine counts, said on `stats` beside the application's own
/// figures: frames, the worst frame, transfers that failed, packets a pipe lost. The first
/// boards' bring-up reads this before anything else.
struct HostStatsMod {
        events: Subscription,
}

impl Module for HostStatsMod {
        fn name(&self) -> &'static str {
                "host-stats"
        }

        fn poll(&mut self) -> Poll {
                let mut busy = false;
                while let Some(ev) = EVENTS.poll(&self.events) {
                        if let AppEvent::Stats = ev {
                                let s = PioHostController::stats();
                                let attached = match PioHostController::attached() {
                                        None => "nothing attached",
                                        Some(false) => "a full-speed device attached",
                                        Some(true) => "a low-speed device attached (not spoken to)",
                                };
                                //   one line each, inside the log's message capacity
                                info!("pio host: {}; frame {}, worst {} us", attached, PioHostController::frame_number(), s.worst_frame_us);
                                info!("pio host: {} answered, {} transfers failed, {} pipe packets lost", s.answered, s.errors, s.pipe_dropped);
                                let m = s.last_miss;
                                if m.token != 0 {
                                        info!("pio host: last miss after {:#04x}: started {}, {} bytes, {:02x} {:02x}", m.token, m.started, m.bytes, m.first[0], m.first[1]);
                                }
                                busy = true;
                        }
                }
                if busy { Poll::Busy } else { Poll::Idle }
        }
}

/// Core 1: the port's console loop on both transports -- the CDC console over the hardware
/// USB port, which on this board is the device, and the UART on the debug header.
#[unsafe(no_mangle)]
pub extern "C" fn light_app_core1_main(info: &ShellInfo) -> ! {
        // SAFETY: core 1's one construction of the console UART
        let uart = unsafe { Uart::new(UART_TX, UART_RX, UART_BAUD, info.clk_peri_hz) };
        light_rp2::shell::core1_main(app::push_console_byte, Some(uart))
}

#[unsafe(no_mangle)]
pub extern "C" fn light_app_main(info: &ShellInfo) -> ! {
        log::set_clock(now_us);
        let clocks = Clocks { sys_hz: info.clk_sys_hz, peri_hz: info.clk_peri_hz };
        let p = board::take(&clocks).expect("the board's peripherals are taken once");
        info!("crossfire rev {}: sys {} Hz, peri {} Hz; host on the PIO pair, console on USB and the UART", REVISION, clocks.sys_hz, clocks.peri_hz);
        if clocks.sys_hz % 12_000_000 != 0 {
                //   the PIO host's dividers are only exact at a multiple of 12 MHz; at anything
                // else every bit carries jitter and the first boards would be debugging the clock
                // rather than the bus
                warn!("the system clock is not a multiple of 12 MHz: the PIO host's bit timing is inexact at this rate (the board is designed for {} Hz)", DESIGN_SYS_HZ);
        }
        //   which image the ROM chose, and what it made of the slot it was asked about: on a
        // board with an A/B pair this is the only answer to "which image am I running?"
        let boot = light_rp2::shell::boot_info();
        if let Some(b) = boot {
                info!("boot: type {}, partition {:?}, probation {:#x}; diagnosing {:?} -> {:#010x}", b.boot_type, b.partition, b.tbyb_and_update, b.diagnostic_partition, b.diagnostic);
        }

        //   the host stack on THIS core: its frame tick's interrupt lands here, and its futures
        // are polled here -- the one rule the PIO host has
        // SAFETY: the one construction; the board wired the pair to the hub and nothing else
        // uses PIO0 or the channel
        let host = unsafe { UsbMidiHost::init_pio(PIN_HOST_DP, HOST_DMA_CH, clocks.sys_hz) };
        info!("USB host stack up on PIO: {} MIDI slots, hub aware; the hub is held until the application is ready", app::USB_SLOTS);

        //   the big modules live in .bss, never on core 0's small stack
        static USB_MOD: StaticCell<UsbMod<UsbMidiHost>> = StaticCell::new();
        let usb_mod = USB_MOD.init(UsbMod::new(host));
        let mut console_mod = ConsoleMod::new();
        //   the USER button, a pin of the board's: pressed is low
        let button: Input = p.button;
        let mut nav_mod = NavMod::new(move || button.is_low());
        //   the hub, released once the host is listening, and the supply read on the way
        static HUB_MOD: StaticCell<HubMod<Output, Output, CcLines>> = StaticCell::new();
        let hub_mod = HUB_MOD.init(HubMod::new(p.hub_reset_n, p.hub_vbus_det, CcLines { cc1: p.cc1, cc2: p.cc2 }));
        let mut indicator_mod = IndicatorMod::new(p.led_green, p.led_red);
        let mut host_stats_mod = HostStatsMod { events: EVENTS.subscribe().expect("subscriber slot") };
        //   an image started on approval keeps itself only once the application has shown it
        // works; until then a reset, or the ROM's own probation watchdog, brings the old one back
        let mut probation_mod = ProbationMod::new(RomProbation::new(boot), vitals);
        //   this board can replace its own firmware, which the portable application cannot know
        //   WORDS, not bytes: the boot facility wants this word-aligned, and a byte array is not
        static COMMIT_SCRATCH: ConstStaticCell<[u32; light_rp2::update::COMMIT_SCRATCH_WORDS]> = ConstStaticCell::new([0; light_rp2::update::COMMIT_SCRATCH_WORDS]);
        static UPDATE_MOD: StaticCell<UpdateMod> = StaticCell::new();
        let update_mod = UPDATE_MOD.init(UpdateMod::new(COMMIT_SCRATCH.take()));

        let mut idle = Breathe;
        app::serve(usb_mod, &mut console_mod, &mut nav_mod, &mut [hub_mod, &mut indicator_mod, &mut host_stats_mod, update_mod], &mut probation_mod, move || idle.idle())
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
        panic_report(info)
}
