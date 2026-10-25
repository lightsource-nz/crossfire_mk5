//! Board wiring for crossfire revision 0.1: every pin read off the board's own schematic, as
//! recorded in its release specification. Nothing here has been confirmed on hardware yet;
//! the first boards are in production.
//!
//! This lives with the BOARD, not in the port crate or the application: `light-rp2` knows the
//! chip and nothing about what anyone soldered to it, and the application knows neither. This
//! module is the one place that knows which pin carries what, and [`take`] configures every
//! peripheral the board wires up and hands them over once -- the firmware receives owned
//! values, so nothing can reach a peripheral the board did not wire.
//!
//! What the board has, from the processor's side:
//!
//! - the hardware USB pair goes to the USB-C receptacle: the DEVICE role, the console and the
//!   board's entire power supply, and nothing in this module -- the port's shell owns it;
//! - a PIO host pair goes to the hub's upstream port, on [`PIN_HOST_DP`] and [`PIN_HOST_DM`];
//! - the hub is held in reset until the firmware says otherwise, and told when its upstream is
//!   live, on two plain outputs;
//! - two indicators, one button, and the two CC lines of the receptacle on the ADC.
//!
//! The console UART is not constructed here. It is core 1's, built by the firmware's core-1
//! entry on the port's default pair, which is the pair the board wires to the debug header.

use light_core::atomic::{AtomicBool, Ordering};
use light_rp2::adc::Adc;
use light_rp2::gpio::{Input, Output};
use light_rp2::Clocks;

/// The hardware revision this wiring describes, as the board's silkscreen carries it.
pub const REVISION: &str = "0.1";

/// The console, on UART0's default pair, which is also the port's
/// (`light_rp2::shell::UART_TX` / `UART_RX`); brought out on the debug header beside SWD.
pub const PIN_CONSOLE_TX: usize = 0;
pub const PIN_CONSOLE_RX: usize = 1;

/// The hub's active-low reset, with a 10 kΩ pull-down on the board: the hub is held in reset
/// from power-up until this pin is driven high, so a firmware that never touches it has no hub.
pub const PIN_HUB_RESET_N: usize = 7;

/// The PIO USB host pair to the hub's upstream port, 27 Ω in series and 15 kΩ to ground on
/// each line -- the host's pull-downs, so the hub's attach pull-up is seen against them. D+ is
/// the base and D- the next pin up, because the PIO program addresses the pair as a base and
/// an offset.
pub const PIN_HOST_DP: usize = 8;
pub const PIN_HOST_DM: usize = 9;

/// The two indicators, anode to the pin through 330 Ω and cathode to ground: driven high to
/// light, about 4 mA each. The schematic names them LED_1 and LED_2; here they are named by
/// what they look like, since that is what a firmware chooses between.
pub const PIN_LED_RED: usize = 14;
pub const PIN_LED_GREEN: usize = 15;

/// The USER button, to ground, with no pull-up on the board: the pin's own pull-up is what
/// makes it readable, and pressed is low. The other two buttons are not GPIO -- BOOT pulls the
/// flash's chip select and RESET pulls RUN -- so BOOT is read through the port's flash-safe
/// `light_rp2::shell::bootsel`, never as a pin.
pub const PIN_BUTTON: usize = 16;

/// The hub's VBUS_DET input, with a 100 kΩ pull-down on the board: driven high to tell the hub
/// its upstream is live, which is what lets it attach; taken low and high again to make it
/// re-enumerate.
pub const PIN_HUB_VBUS_DET: usize = 18;

/// The receptacle's two CC lines, each terminated with [`CC_RD_OHMS`] to ground at the
/// connector and read here through 10 kΩ and 100 nF: ADC channels 0 and 1. The source's
/// pull-up on whichever line its cable orients to sets the voltage, and that voltage is what
/// the source advertises it can supply.
pub const PIN_CC1_SENSE: usize = 26;
pub const PIN_CC2_SENSE: usize = 27;
/// The termination the board presents on each CC line: the sink's Rd.
pub const CC_RD_OHMS: u32 = 5_100;
/// The ADC's full scale, in millivolts: its reference is the 3.3 V pad supply, through the
/// board's filtered analogue rail.
pub const ADC_FULL_SCALE_MV: u32 = 3_300;

/// The system clock the board is designed for: twenty times its 12 MHz crystal, which gives
/// the PIO host twenty cycles a bit and the flash its fastest legal clock through the divider
/// of 2. The SDK's default is 150 MHz; the firmware's build chooses whether to ask for this.
pub const DESIGN_SYS_HZ: u32 = 240_000_000;

pub struct Peripherals {
        /// Off until lit.
        pub led_red: Output,
        pub led_green: Output,
        /// Pulled up; pressed is low.
        pub button: Input,
        /// Low: the hub is held in reset, as the board's pull-down leaves it from power-up.
        pub hub_reset_n: Output,
        /// Low: the hub has not been told its upstream is live.
        pub hub_vbus_det: Output,
        /// One-shot reads, 12 bits over [`ADC_FULL_SCALE_MV`].
        pub cc1: Adc,
        pub cc2: Adc,
}

static TAKEN: AtomicBool = AtomicBool::new(false);

/// Configure and hand over the board's peripherals. Once.
///
/// Nothing wired today needs a clock rate; the host pair's PIO will, and takes it from here.
pub fn take(clocks: &Clocks) -> Option<Peripherals> {
        if TAKEN.swap(true, Ordering::AcqRel) {
                return None;
        }
        let _ = clocks;
        //   the hub stays exactly where the board's pull-downs put it at power-up -- in reset,
        // with no upstream -- until a firmware decides otherwise: an output constructed low
        // changes nothing on the wire
        Some(Peripherals {
                led_red: Output::new(PIN_LED_RED, false),
                led_green: Output::new(PIN_LED_GREEN, false),
                button: Input::new_pull_up(PIN_BUTTON),
                hub_reset_n: Output::new(PIN_HUB_RESET_N, false),
                hub_vbus_det: Output::new(PIN_HUB_VBUS_DET, false),
                cc1: Adc::new(PIN_CC1_SENSE),
                cc2: Adc::new(PIN_CC2_SENSE),
        })
}
