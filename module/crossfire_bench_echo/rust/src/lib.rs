//! The bench's echo on an RP2040-Zero: a USB-MIDI instrument on one of crossfire's ports that
//! sends every message straight back, and toggles a wire to the timer as each USB packet of them
//! arrives. The queueing is `crossfire_bench::echo`'s; this crate is the chip around it.
//!
//! WIRING: this board's GP2 to the timer's GP2, with a ground wire beside it. GP3 held to ground
//! at power-up gives the OUT endpoint four bytes instead of sixty-four: an instrument that takes
//! one event at a time.
//!
//! ONE LOOP ON CORE 0: poll the USB device, toggle the wire for what arrived, queue it, and hand
//! back as much as the IN endpoint will take. Nothing here is reported; the timer measures it all.

#![no_std]

use crossfire_bench::class::MidiClass;
use crossfire_bench::echo::Echo;
use crossfire_bench::PACKETS_PER_TRANSFER;
use light_core::{info, log, ConstStaticCell, InputPin, StaticCell};
use light_rp2::gpio::{Input, Output};
use light_rp2::now_us;
use light_rp2::shell::{panic_report, ShellInfo};
use light_rp2::usb::UsbBus;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid};

/// The wire to the timer.
const PIN_EDGE: usize = 2;
/// Held low at power-up: a four-byte OUT endpoint.
const PIN_NARROW: usize = 3;

/// pid.codes' test identifiers: for a device that never leaves the bench.
const VID: u16 = 0x1209;
const PID: u16 = 0x0001;

/// Core 1: the framework's console loop with no transport, which keeps the log drained.
#[unsafe(no_mangle)]
pub extern "C" fn light_app_core1_main(_info: &ShellInfo) -> ! {
        light_rp2::shell::core1_main(|_| {}, None)
}

#[unsafe(no_mangle)]
pub extern "C" fn light_app_main(_info: &ShellInfo) -> ! {
        log::set_clock(now_us);
        let narrow = Input::new_pull_up(PIN_NARROW).is_low();
        let mut edge = Output::new(PIN_EDGE, false);
        let mut level = false;

        static ALLOC: StaticCell<UsbBusAllocator<UsbBus>> = StaticCell::new();
        // SAFETY: the one construction of the controller, on the core that polls it
        let alloc: &'static UsbBusAllocator<UsbBus> = ALLOC.init(UsbBusAllocator::new(unsafe { UsbBus::new() }));
        let mut midi = MidiClass::new(alloc, if narrow { 4 } else { 64 });
        let mut dev = UsbDeviceBuilder::new(alloc, UsbVidPid(VID, PID))
                .strings(&[StringDescriptors::default().manufacturer(light_core::usb::MANUFACTURER).product("crossfire bench echo").serial_number("echo")])
                .expect("descriptor strings")
                .build();
        info!("crossfire bench echo: OUT endpoint {} bytes", midi.out_max_packet());

        static ECHO: ConstStaticCell<Echo> = ConstStaticCell::new(Echo::new());
        let echo = ECHO.take();
        let mut arrived = [[0u8; 4]; PACKETS_PER_TRANSFER];
        let mut out = [[0u8; 4]; PACKETS_PER_TRANSFER];
        loop {
                dev.poll(&mut [&mut midi]);
                let n = midi.receive(&mut arrived);
                if n > 0 {
                        level = !level;
                        edge.set(level);
                        echo.arrived(&arrived[..n]);
                }
                let k = echo.next(&mut out);
                if k > 0 {
                        let taken = midi.send(&out[..k]);
                        echo.sent(taken);
                }
        }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
        panic_report(info)
}
