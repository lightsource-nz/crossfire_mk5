//! Board wiring for crossfire on a wireless Pico 2 with the Waveshare Pico-OLED-1.3 display
//! board: the OLED (SH1107, 64x128 portrait glass, 1 bpp) on SPI1 as the status display, and its
//! two keys. The pin assignments are the Pico-OLED-1.3's as it plugs onto the Pico header.
//!
//! THERE IS NO INDICATOR PIN HERE, and its absence is the point. On the wireless board the pin
//! that carries the indicator on the plain one is the RADIO'S CHIP SELECT -- the same pin, a
//! different job -- so a firmware that drives it as an indicator is quietly driving the radio's
//! chip select, which is harmless exactly as long as the radio is held in reset and no longer.
//! The indicator this board does have hangs off the radio's own pins, so it belongs to the
//! radio's module.
//!
//! This lives with the HARDWARE MODULE, not in the port crate or the application:
//! `light-rp2` knows the chip and nothing about what anyone soldered to it, and
//! `light_app_crossfire` knows neither. A crossfire on different hardware is another module
//! with another copy of this file.

use light_core::atomic::{AtomicBool, Ordering};
use light_rp2::gpio::Input;
use light_rp2::spi::Spi1Display;
use light_rp2::Clocks;

/// The Pico-OLED-1.3's two keys, active low.
pub const PIN_KEY0: usize = 15;
pub const PIN_KEY1: usize = 17;

pub const PIN_OLED_DC: usize = 8;
pub const PIN_OLED_CS: usize = 9;
pub const PIN_OLED_SCK: usize = 10;
pub const PIN_OLED_MOSI: usize = 11;
pub const PIN_OLED_RESET: usize = 12;
/// The glass is physically portrait: 64 wide, 128 tall.
pub const OLED_WIDTH: u16 = 64;
pub const OLED_HEIGHT: u16 = 128;
/// The controller's RAM offset the panel sits at (SH1107 command 0xD3), verified on the
/// glass: without it the image lands wrapped around the controller's 128-row RAM.
pub const OLED_DISPLAY_OFFSET: u8 = 96;
/// The rate this panel is qualified at; it has not been pushed further.
pub const OLED_SPI_HZ: u32 = 10_000_000;
/// The display bus's DMA channel: the top of the RP2350's 16, where pico-sdk's
/// dma_claim_unused_channel (counting up from 0) will not reach.
pub const OLED_DMA_CH: usize = 15;

pub struct Peripherals {
        pub key0: Input,
        pub key1: Input,
        pub oled_bus: Spi1Display,
}

static TAKEN: AtomicBool = AtomicBool::new(false);

/// Configure and hand over the board's peripherals. Once.
pub fn take(clocks: &Clocks) -> Option<Peripherals> {
        if TAKEN.swap(true, Ordering::AcqRel) {
                return None;
        }
        // SAFETY: the flag above makes this the one construction of each peripheral;
        // the shell's USB and timer blocks are not in this set
        unsafe {
                Some(Peripherals {
                        key0: Input::new_pull_up(PIN_KEY0),
                        key1: Input::new_pull_up(PIN_KEY1),
                        oled_bus: Spi1Display::new(
                                clocks.peri_hz,
                                PIN_OLED_SCK,
                                PIN_OLED_MOSI,
                                PIN_OLED_CS,
                                PIN_OLED_DC,
                                Some(PIN_OLED_RESET),
                                OLED_SPI_HZ,
                                OLED_DMA_CH,
                        ),
                })
        }
}
