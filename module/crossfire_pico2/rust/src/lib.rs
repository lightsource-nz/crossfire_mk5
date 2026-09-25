//! crossfire on a Pico 2 with the Waveshare Pico-OLED-1.3 display board: the
//! hardware-bound instantiation. The application is `light_app_crossfire`, with no
//! hardware in it; this crate is everything tangible -- the board wiring, the RP2350 chip
//! feature, the USB host stack, the shell ABI, the panic handler -- constructed here
//! and handed to [`app::serve`].

#![no_std]

use light_app_crossfire as app;
use app::{ConsoleMod, NavMod, OledMod, UsbMod};
use light_assets::{Pack, PackError};
use light_core::{info, log, ConstStaticCell, Idle, StaticCell};
use light_display::sh1107::Sh1107;
use light_display::{Display, FrameLayer};
use light_draw::PixelFormat;
use light_font::Font;
mod board;
use board::*;
mod radio;
use radio::RadioMod;
mod update;
use update::UpdateMod;
use light_rp2::spi::Spi1Display;
use light_rp2::usb_host::UsbMidiHost;
use light_rp2::sha256::Sha256Hw;
use light_rp2::shell::{bootsel, panic_report, ShellInfo, UART_BAUD, UART_RX, UART_TX};
use light_rp2::uart::Uart;
use light_rp2::{now_us, Breathe, Clocks, SysClock};

/// 64x128 at 1 bpp: one kilobyte.
static FRAME: ConstStaticCell<[u8; PixelFormat::Mono1.buffer_len(OLED_WIDTH, OLED_HEIGHT)]> = ConstStaticCell::new([0; PixelFormat::Mono1.buffer_len(OLED_WIDTH, OLED_HEIGHT)]);
/// THE ASSETS ARE NOT IN THIS IMAGE. The font, the look-and-feel and the interface are written to
/// the region of storage the flash map sets aside for them and read from there at startup, so
/// restyling or re-lettering crossfire is a pack rewritten rather than a firmware release. What
/// the image carries instead is the digest of the pack it was built against: this image is signed,
/// so a pack that hashes to this is as trustworthy as the image naming it, and one that does not
/// is refused.
static ASSET_DIGEST: &[u8; 32] = include_bytes!(env!("LIGHT_ASSETS_SHA256"));

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
        info!("crossfire (pico2): sys {} Hz; host stack on core 0, console on the UART", clocks.sys_hz);

        //   the assets, out of the storage set aside for them and checked against the digest this
        // image was built with. There is deliberately no fallback: an interface with no font is
        // not an interface, and a second copy carried in the image would undo the reason the
        // assets were taken out of it. What a stop here means is that the pack was never written,
        // or belongs to another build -- write it and the board comes up
        let region = match light_rp2::assets::region() {
                Ok(region) => region,
                Err(e) => panic!("this board has nowhere to keep assets ({e:?})"),
        };
        //   how long checking the pack takes is a real part of how long this board takes to come
        // up, and the only way anyone would notice it growing is if it is said
        let began = now_us();
        let pack = match Pack::open(region, ASSET_DIGEST, Sha256Hw::new()) {
                Ok(pack) => pack,
                //   blank storage, which is a board whose assets were never written -- worth
                // telling apart from a pack that is there and is the wrong one
                Err(PackError::BadMagic) => panic!("no asset pack has been written to this board"),
                Err(e) => panic!("this board's asset pack is not the one the firmware was built with ({e:?})"),
        };
        let (font_blob, theme_blob, ui_blob) = match (pack.get("font"), pack.get("theme"), pack.get("ui")) {
                (Ok(f), Ok(t), Ok(u)) => (f, t, u),
                _ => panic!("the asset pack is missing one of font, theme or ui"),
        };
        //   the radio's firmware is an asset like the rest: this board's radio holds nothing of
        // its own, so a quarter of a megabyte is uploaded into it at every power-up
        let (radio_blob, radio_limits) = match (pack.get("radio"), pack.get("radio_limits")) {
                (Ok(f), Ok(l)) => (f, l),
                _ => panic!("the asset pack has no radio firmware in it"),
        };
        let font = match Font::parse(font_blob) {
                Ok(f) => f,
                Err(e) => panic!("the packed font does not parse: {e:?}"),
        };
        info!("assets: {} entries from the data region, {} bytes, checked in {} ms", pack.len(), pack.as_bytes().len(), (now_us() - began) / 1000);
        //   which image the ROM chose, and what it made of the slot it was asked about: on a
        // board with an A/B pair this is the only answer to "which image am I running?"
        if let Some(b) = light_rp2::shell::boot_info() {
                info!("boot: type {}, partition {:?}, probation {:#x}; diagnosing {:?} -> {:#010x}", b.boot_type, b.partition, b.tbyb_and_update, b.diagnostic_partition, b.diagnostic);
        }
        // the host stack, on THIS core -- see the shell
        let host = UsbMidiHost::init();
        info!("USB host stack up: {} MIDI slots, hub aware", app::USB_SLOTS);

        //   the big modules live in .bss, never on core 0's small stack
        static USB_MOD: StaticCell<UsbMod<UsbMidiHost>> = StaticCell::new();
        let usb_mod = USB_MOD.init(UsbMod::new(host));
        static OLED_MOD: StaticCell<OledMod<Spi1Display, SysClock>> = StaticCell::new();
        let oled_mod = OLED_MOD.init(OledMod::new(display, layer, font, theme_blob, ui_blob, SysClock, OLED_DISPLAY_OFFSET));
        let mut console_mod = ConsoleMod::new();
        let mut nav_mod = NavMod::new(bootsel);
        let _ = (p.key0, p.key1);

        let mut idle = Breathe;
        //   this board can replace its own firmware, which the portable application cannot know
        //   WORDS, not bytes: the boot facility wants this word-aligned, and a byte array is not
        static COMMIT_SCRATCH: ConstStaticCell<[u32; light_rp2::update::COMMIT_SCRATCH_WORDS]> = ConstStaticCell::new([0; light_rp2::update::COMMIT_SCRATCH_WORDS]);
        static UPDATE_MOD: StaticCell<UpdateMod> = StaticCell::new();
        let update_mod = UPDATE_MOD.init(UpdateMod::new(COMMIT_SCRATCH.take()));
        //   the radio, which on this board also carries the indicator -- see radio.rs for why
        // that is not a pin of the board's
        static RADIO_MOD: StaticCell<RadioMod> = StaticCell::new();
        let radio_mod = RADIO_MOD.init(RadioMod::new(clocks.sys_hz, radio_blob, radio_limits));
        app::serve(usb_mod, oled_mod, &mut console_mod, &mut nav_mod, &mut [update_mod, radio_mod], move || idle.idle())
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
        panic_report(info)
}
