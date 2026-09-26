//! crossfire on a Pico 2 with the Waveshare Pico-OLED-1.3 display board: the
//! hardware-bound instantiation. The application is `light_app_crossfire`, with no
//! hardware in it; this crate is everything tangible -- the board wiring, the RP2350 chip
//! feature, the USB host stack, the shell ABI, the panic handler -- constructed here
//! and handed to [`app::serve`].

#![no_std]

use light_app_crossfire as app;
use app::{ConsoleMod, LedMod, NavMod, OledMod, ProbationMod, UsbMod};
use light_core::hal::UpdateError;
use light_core::{info, log, ConstStaticCell, Idle, StaticCell};
use light_display::sh1107::Sh1107;
use light_display::{Display, FrameLayer};
use light_draw::PixelFormat;
use light_font::Font;
mod board;
use board::*;
use light_rp2::spi::Spi1Display;
use light_rp2::usb_host::UsbMidiHost;
use light_rp2::shell::{bootsel, panic_report, BootInfo, ShellInfo, UART_BAUD, UART_RX, UART_TX};
use light_rp2::uart::Uart;
use light_rp2::update::COMMIT_SCRATCH_WORDS;
use light_rp2::{now_us, Breathe, Clocks, SysClock};

/// 64x128 at 1 bpp: one kilobyte.
static FRAME: ConstStaticCell<[u8; PixelFormat::Mono1.buffer_len(OLED_WIDTH, OLED_HEIGHT)]> = ConstStaticCell::new([0; PixelFormat::Mono1.buffer_len(OLED_WIDTH, OLED_HEIGHT)]);
static FONT_BLOB: &[u8] = include_bytes!(env!("LIGHT_FONT_LGF"));
/// The look-and-feel and the interface, as data: the framework MONO default with this board's
/// rounding, and the crossfire status page -- both compiled to blobs the app embeds.
static THEME_BLOB: &[u8] = include_bytes!(env!("LIGHT_THEME_LTH"));
static UI_BLOB: &[u8] = include_bytes!(env!("LIGHT_UI_LUI"));

/// The boot ROM's flag for an image started on approval that has not yet bought itself
/// (`BOOT_TBYB_AND_UPDATE_FLAG_BUY_PENDING`): a candidate, discarded unless it commits.
const BUY_PENDING: u8 = 0x01;

/// Whether an image is on probation is the ROM's to say, and keeping it is the ROM's to do: this
/// is the RP2350's answer to both, for the application's probation checks.
struct RomProbation {
        candidate: bool,
}

impl RomProbation {
        fn new(boot: Option<BootInfo>) -> Self {
                Self { candidate: boot.is_some_and(|b| b.tbyb_and_update & BUY_PENDING != 0) }
        }
}

impl app::Probation for RomProbation {
        fn candidate(&self) -> bool {
                self.candidate
        }

        fn commit(&mut self) -> Result<(), UpdateError> {
                //   the scratch the commit borrows: two sectors, word-aligned, in .bss because it is
                // twice core 0's whole stack -- see COMMIT_SCRATCH_WORDS for why neither half is
                // negotiable. Taken once; the application commits at most once
                static SCRATCH: ConstStaticCell<[u32; COMMIT_SCRATCH_WORDS]> = ConstStaticCell::new([0; COMMIT_SCRATCH_WORDS]);
                let Some(scratch) = SCRATCH.try_take() else {
                        return Err(UpdateError::Refused);
                };
                //   THE COMMIT REWRITES THE FLASH THIS FIRMWARE RUNS FROM, and for its duration
                // nothing may fetch from it. Core 1 runs the console out of flash, so it is parked in
                // RAM through the SDK's lockout -- the mechanism the shell's BOOTSEL read already
                // uses on this board -- and core 0's interrupts are held off, because every handler
                // it has (the host stack's, the display's DMA, the timer's) is in flash too. The
                // lockout comes FIRST: taken inside the critical section it could find core 1
                // waiting on the same lock with its interrupts off, never answering.
                //   A pause of tens of milliseconds, once, a few seconds after an update.
                // SAFETY: the SDK's lockout pair, which core 1 answers (the shell made it a victim
                // before launching it); balanced within this call
                unsafe { multicore_lockout_start_blocking() };
                let result = critical_section::with(|_| light_rp2::update::commit(scratch));
                unsafe { multicore_lockout_end_blocking() };
                result
        }
}

unsafe extern "C" {
        fn multicore_lockout_start_blocking();
        fn multicore_lockout_end_blocking();
}

/// The application's signs of life, with this port's console-core pulse.
fn vitals() -> app::Vitals {
        app::vitals(light_rp2::shell::core1_ticks())
}

/// Core 1: the port's console loop on the UART alone -- the native USB port is the MIDI host,
/// core 0's, so this build has no CDC console and light-rp2 carries no device stack.
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
        let p = take(&clocks).expect("the board's peripherals are taken once");
        let frame: &'static mut [u8] = FRAME.take();
        static LAYER: ConstStaticCell<FrameLayer> = ConstStaticCell::new(FrameLayer::new(OLED_WIDTH, OLED_HEIGHT, PixelFormat::Mono1));
        let layer: &'static mut FrameLayer = LAYER.take();
        let display = Display::new(Sh1107::new(p.oled_bus), frame, OLED_WIDTH, OLED_HEIGHT, PixelFormat::Mono1, now_us);
        let font = match Font::parse(FONT_BLOB) {
                Ok(f) => f,
                Err(e) => panic!("the embedded font does not parse: {e:?}"),
        };
        info!("crossfire (pico2): sys {} Hz; host stack on core 0, console on the UART", clocks.sys_hz);
        //   which image the ROM chose, and what it made of the slot it was asked about: on a
        // board with an A/B pair this is the only answer to "which image am I running?"
        let boot = light_rp2::shell::boot_info();
        if let Some(b) = boot {
                info!("boot: type {}, partition {:?}, probation {:#x}; diagnosing {:?} -> {:#010x}", b.boot_type, b.partition, b.tbyb_and_update, b.diagnostic_partition, b.diagnostic);
        }
        // the host stack, on THIS core -- see the shell
        let host = UsbMidiHost::init();
        info!("USB host stack up: {} MIDI slots, hub aware", app::USB_SLOTS);

        //   the big modules live in .bss, never on core 0's small stack
        static USB_MOD: StaticCell<UsbMod<UsbMidiHost>> = StaticCell::new();
        let usb_mod = USB_MOD.init(UsbMod::new(host));
        static OLED_MOD: StaticCell<OledMod<Spi1Display, SysClock>> = StaticCell::new();
        let oled_mod = OLED_MOD.init(OledMod::new(display, layer, font, THEME_BLOB, UI_BLOB, SysClock, OLED_DISPLAY_OFFSET));
        let mut led_mod = LedMod::new(p.led);
        let mut console_mod = ConsoleMod::new();
        let mut nav_mod = NavMod::new(bootsel);
        //   an image started on approval keeps itself only once the application has shown it
        // works; until then a reset, or the ROM's own probation watchdog, brings the old one back
        let mut probation_mod = ProbationMod::new(RomProbation::new(boot), vitals);
        let _ = (p.key0, p.key1);

        let mut idle = Breathe;
        app::serve(usb_mod, oled_mod, &mut led_mod, &mut console_mod, &mut nav_mod, &mut probation_mod, move || idle.idle())
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
        panic_report(info)
}
