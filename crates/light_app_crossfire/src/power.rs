//! What the supply can give, and what that leaves for the instruments.
//!
//! A board powered over USB-C learns its budget from the connector's CC lines: the source
//! pulls one of them up through a resistor whose value says how much current it will supply,
//! and the board's own termination turns that into a voltage in one of three bands. This module
//! reads those bands and does the arithmetic -- the source's offer, less what the board itself
//! takes, divided among the ports -- so that what the indicators show and what the console
//! reports come from one place, tested on the host.
//!
//! THE BUDGET IS ADVISORY. The hub switches each port's power on request and the port switches
//! limit the current in hardware; nothing here turns a port off. What the budget decides is
//! what to TELL: a source that offers the USB default cannot run four instruments at full
//! current, and a user plugging in a fourth should be able to see that on the board rather than
//! discover it as a hub resetting under load.

/// What a USB-C source advertises on the CC line, by the band the sink's termination puts it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advertisement {
        /// Neither line pulled up: no source attached, or not a USB-C one.
        NoSource,
        /// The USB default: 500 mA, or 900 mA from a USB 3 port, and no way to tell which.
        Default,
        /// 1.5 A.
        Amp1_5,
        /// 3 A.
        Amp3,
}

/// Which CC line the source pulled up: the cable's orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CcLine {
        Cc1,
        Cc2,
}

/// The specification's three bands across a 5.1 kΩ sink termination, in millivolts: 0.25 to
/// 0.61 V for the default, 0.70 to 1.16 V for 1.5 A, 1.31 to 2.04 V for 3 A. The thresholds
/// sit midway between the bands; below the lowest band the line has no pull-up on it at all.
pub const NO_PULLUP_BELOW_MV: u32 = 200;
pub const DEFAULT_BELOW_MV: u32 = 655;
pub const AMP1_5_BELOW_MV: u32 = 1_235;
pub const AMP3_BELOW_MV: u32 = 2_200;

impl Advertisement {
        /// Read one line's voltage.
        pub fn from_mv(mv: u32) -> Self {
                match mv {
                        v if v < NO_PULLUP_BELOW_MV => Self::NoSource,
                        v if v < DEFAULT_BELOW_MV => Self::Default,
                        v if v < AMP1_5_BELOW_MV => Self::Amp1_5,
                        v if v < AMP3_BELOW_MV => Self::Amp3,
                        // above every band: not a USB-C source talking USB-C
                        _ => Self::NoSource,
                }
        }

        /// Read both lines: the source pulls up the one its cable orients to, and the other
        /// carries the cable's own identification or nothing, so the higher reading is the
        /// advertisement and says which way round the cable is.
        pub fn from_cc(cc1_mv: u32, cc2_mv: u32) -> (Self, CcLine) {
                if cc1_mv >= cc2_mv {
                        (Self::from_mv(cc1_mv), CcLine::Cc1)
                } else {
                        (Self::from_mv(cc2_mv), CcLine::Cc2)
                }
        }

        /// What the source will supply, in milliamps. The default is taken at its USB 2.0
        /// value: a USB 3 port gives more, but says so no differently.
        pub fn source_ma(self) -> u32 {
                match self {
                        Self::NoSource => 0,
                        Self::Default => 500,
                        Self::Amp1_5 => 1_500,
                        Self::Amp3 => 3_000,
                }
        }

        pub fn describe(self) -> &'static str {
                match self {
                        Self::NoSource => "no USB-C source",
                        Self::Default => "USB default (500 mA)",
                        Self::Amp1_5 => "1.5 A",
                        Self::Amp3 => "3 A",
                }
        }
}

/// What the board takes for itself before any port: the processor, the hub and the
/// regulator's loss, at the top of their budget.
pub const BOARD_MA: u32 = 250;
/// A port's full USB 2.0 allowance.
pub const PORT_FULL_MA: u32 = 500;
/// What a typical instrument actually draws: the figure the "typical instruments" count uses.
pub const INSTRUMENT_TYPICAL_MA: u32 = 300;
pub const PORTS: u32 = 4;

/// The budget: the source's offer divided up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
        pub advertisement: Advertisement,
        pub line: CcLine,
        /// What is left for the ports after the board's own share.
        pub ports_ma: u32,
        /// How many ports could draw their full allowance at once.
        pub full_ports: u8,
        /// How many typical instruments could run at once.
        pub typical_instruments: u8,
}

impl Budget {
        pub fn from_cc(cc1_mv: u32, cc2_mv: u32) -> Self {
                let (advertisement, line) = Advertisement::from_cc(cc1_mv, cc2_mv);
                let ports_ma = advertisement.source_ma().saturating_sub(BOARD_MA);
                Self { advertisement, line, ports_ma, full_ports: (ports_ma / PORT_FULL_MA).min(PORTS) as u8, typical_instruments: (ports_ma / INSTRUMENT_TYPICAL_MA).min(PORTS) as u8 }
        }

        /// Whether the supply is short of what four ports could ask for: what the red indicator
        /// says.
        pub fn limited(&self) -> bool {
                self.full_ports < PORTS as u8
        }

        /// Whether there is any supply to speak of. A board running on its own USB port's
        /// power with the source's advertisement unreadable still runs; it is only short of
        /// knowing.
        pub fn known(&self) -> bool {
                self.advertisement != Advertisement::NoSource
        }
}

#[cfg(test)]
mod tests {
        use super::*;

        #[test]
        fn the_bands_decode_to_the_three_offers() {
                assert_eq!(Advertisement::from_mv(0), Advertisement::NoSource);
                assert_eq!(Advertisement::from_mv(199), Advertisement::NoSource);
                assert_eq!(Advertisement::from_mv(250), Advertisement::Default);
                assert_eq!(Advertisement::from_mv(610), Advertisement::Default);
                assert_eq!(Advertisement::from_mv(700), Advertisement::Amp1_5);
                assert_eq!(Advertisement::from_mv(1160), Advertisement::Amp1_5);
                assert_eq!(Advertisement::from_mv(1310), Advertisement::Amp3);
                assert_eq!(Advertisement::from_mv(2040), Advertisement::Amp3);
                assert_eq!(Advertisement::from_mv(3300), Advertisement::NoSource, "a rail, not a source");
        }

        #[test]
        fn the_higher_line_is_the_advertisement_and_names_the_orientation() {
                assert_eq!(Advertisement::from_cc(1700, 50), (Advertisement::Amp3, CcLine::Cc1));
                assert_eq!(Advertisement::from_cc(60, 900), (Advertisement::Amp1_5, CcLine::Cc2));
                assert_eq!(Advertisement::from_cc(0, 0), (Advertisement::NoSource, CcLine::Cc1));
        }

        #[test]
        fn the_budget_divides_what_is_left_after_the_board() {
                let b = Budget::from_cc(1700, 0);
                assert_eq!(b.ports_ma, 2750);
                assert_eq!(b.full_ports, 4);
                assert_eq!(b.typical_instruments, 4);
                assert!(!b.limited() && b.known());

                let b = Budget::from_cc(0, 900);
                assert_eq!(b.ports_ma, 1250);
                assert_eq!(b.full_ports, 2);
                assert_eq!(b.typical_instruments, 4);
                assert!(b.limited());

                let b = Budget::from_cc(400, 0);
                assert_eq!(b.ports_ma, 250);
                assert_eq!(b.full_ports, 0);
                assert_eq!(b.typical_instruments, 0, "one at a push, and not promised");
                assert!(b.limited());

                let b = Budget::from_cc(0, 0);
                assert_eq!(b.ports_ma, 0);
                assert!(b.limited() && !b.known());
        }
}
