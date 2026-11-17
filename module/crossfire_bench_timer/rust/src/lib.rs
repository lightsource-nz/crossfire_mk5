//! The bench's timer on an RP2350-Zero: a USB-MIDI instrument on one of crossfire's ports that
//! times messages through the board to the echo and back. The schedule and the figures are
//! `crossfire_bench::timer`'s; this crate is the chip around them.
//!
//! WIRING: the echo's GP2 to this board's GP2, with a ground wire beside it. The echo toggles that
//! line as each message reaches it, which is the one-way time. The console is the UART on GP0 and
//! GP1, for a probe's serial port if it has one.
//!
//! ONE LOOP ON CORE 0, and nothing else on it: poll the USB device, look at the wire, step the
//! schedule. Every time is taken in that loop, so its longest pass is the resolution of every
//! figure, and it is reported with them. Core 1 is the framework's console loop, which drains the
//! log to the UART.
//!
//! The figures are [`CROSSFIRE_BENCH_RESULTS`], refreshed every 50 ms, which `scripts/bench.ps1`
//! reads over SWD without halting anything.

#![no_std]

use core::ptr::addr_of_mut;

use crossfire_bench::class::MidiClass;
use crossfire_bench::timer::{self, Event, Link, Results, Timer};
use crossfire_bench::{Packet, PACKETS_PER_TRANSFER};
use light_core::{info, log, ConstStaticCell, InputPin, StaticCell};
use light_rp2::gpio::Input;
use light_rp2::now_us;
use light_rp2::shell::{panic_report, ShellInfo, UART_BAUD, UART_RX, UART_TX};
use light_rp2::uart::Uart;
use light_rp2::usb::UsbBus;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{StringDescriptors, UsbDevice, UsbDeviceBuilder, UsbVidPid};

/// The echo's wire.
const PIN_EDGE: usize = 2;

/// pid.codes' test identifiers: for a device that never leaves the bench.
const VID: u16 = 0x1209;
const PID: u16 = 0x0001;

/// How often the figures are copied out for the debugger.
const PUBLISH_US: u64 = 50_000;

/// The figures, where a debugger finds them by name.
#[unsafe(no_mangle)]
#[used]
pub static mut CROSSFIRE_BENCH_RESULTS: Results = Results::new();

struct Usb {
        dev: UsbDevice<'static, UsbBus>,
        midi: MidiClass<'static, UsbBus>,
}

impl Link for Usb {
        fn send(&mut self, packets: &[Packet]) -> usize {
                self.midi.send(packets)
        }
        fn receive(&mut self, out: &mut [Packet; PACKETS_PER_TRANSFER]) -> usize {
                self.midi.receive(out)
        }
}

fn publish(results: &Results) {
        // SAFETY: written by this loop alone; the debugger only reads it
        unsafe { addr_of_mut!(CROSSFIRE_BENCH_RESULTS).write_volatile(*results) };
}

/// Core 1: the console, on the UART alone.
#[unsafe(no_mangle)]
pub extern "C" fn light_app_core1_main(info: &ShellInfo) -> ! {
        // SAFETY: core 1's one construction of the console UART
        let uart = unsafe { Uart::new(UART_TX, UART_RX, UART_BAUD, info.clk_peri_hz) };
        light_rp2::shell::core1_main(|_| {}, Some(uart))
}

#[unsafe(no_mangle)]
pub extern "C" fn light_app_main(info: &ShellInfo) -> ! {
        log::set_clock(now_us);
        info!("crossfire bench timer: sys {} Hz; the echo's wire on GP{}", info.clk_sys_hz, PIN_EDGE);
        let edge = Input::new_pull_up(PIN_EDGE);

        static ALLOC: StaticCell<UsbBusAllocator<UsbBus>> = StaticCell::new();
        // SAFETY: the one construction of the controller, on the core that polls it
        let alloc: &'static UsbBusAllocator<UsbBus> = ALLOC.init(UsbBusAllocator::new(unsafe { UsbBus::new() }));
        let midi = MidiClass::new(alloc, 64);
        let dev = UsbDeviceBuilder::new(alloc, UsbVidPid(VID, PID))
                .strings(&[StringDescriptors::default().manufacturer(light_core::usb::MANUFACTURER).product("crossfire bench timer").serial_number("timer")])
                .expect("descriptor strings")
                .build();
        static USB: StaticCell<Usb> = StaticCell::new();
        let usb = USB.init(Usb { dev, midi });

        //   built in place: the timer is over 100 KB and core 0's stack is 8
        static TIMER: ConstStaticCell<Timer> = ConstStaticCell::new(Timer::new(timer::DEFAULT));
        let t = TIMER.take();
        publish(&t.results);

        let mut level = edge.is_low();
        let mut worst = 0u32;
        let mut published = now_us();
        loop {
                let start = now_us();
                usb.dev.poll(&mut [&mut usb.midi]);
                let now = now_us();
                let now_level = edge.is_low();
                let edge_at = (now_level != level).then_some(now);
                level = now_level;
                if let Some(e) = t.step(now, edge_at, usb) {
                        report(e, &t.results);
                        t.results.loop_worst_us = worst;
                        publish(&t.results);
                }
                worst = worst.max((now_us() - start) as u32);
                if now - published >= PUBLISH_US {
                        published = now;
                        t.results.loop_worst_us = worst;
                        publish(&t.results);
                }
        }
}

fn report(e: Event, r: &Results) {
        match e {
                Event::Connected => info!("bench: the echo answered; timing {} messages one at a time", timer::DEFAULT.probes),
                Event::LatencyDone => {
                        let (o, t) = (r.one_way, r.round_trip);
                        info!("bench: one way   min {} p50 {} p90 {} p99 {} max {} us ({} without an edge)", o.min, o.p50, o.p90, o.p99, o.max, r.no_edge);
                        info!("bench: round trip min {} p50 {} p90 {} p99 {} max {} us ({} without an echo)", t.min, t.p50, t.p90, t.p99, t.max, r.no_echo);
                }
                Event::RateDone(i) => {
                        let x = r.rates[i];
                        info!("bench: {} /s: {} offered, {} sent, {} echoed, {} skipped, {} behind", x.rate, x.offered, x.sent, x.echoed, x.skipped, x.behind);
                        info!("bench: {} /s: round trip p50 {} p99 {} max {} us", x.rate, x.round_trip.p50, x.round_trip.p99, x.round_trip.max);
                }
                Event::Finished => info!("bench: finished; the loop's longest pass was {} us", r.loop_worst_us),
        }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
        panic_report(info)
}
