//! Two indicators as the whole of a status display.
//!
//! A board with no glass still has to answer the two questions a person standing over it asks:
//! is anything connected, and is anything happening. One indicator answers both -- lit while an
//! instrument is mounted, and interrupted by traffic, so it flickers with the notes the way a
//! network port's does with its packets. The other says the one thing worth a second colour:
//! that the supply is short of what the ports could ask for, which is the failure a person can
//! fix by changing the cable at the wall and cannot otherwise see.
//!
//! The decision is a pure function of what the bus has said, so it is tested on the host; the
//! module around it only remembers the last of each thing it heard.

use light_core::events::Subscription;
use light_core::{Module, OutputPin, Poll};

use crate::power::Budget;
use crate::{AppEvent, EVENTS};

/// What the bus has said so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Heard {
        pub mounted: bool,
        pub rx: bool,
        pub tx: bool,
        pub budget: Option<Budget>,
}

/// The two levels: `(green, red)`.
pub fn levels(h: &Heard) -> (bool, bool) {
        let traffic = h.rx || h.tx;
        //   lit while mounted, OFF while traffic flows: the interruption is the activity, and a
        // steady light with nothing mounted would say the opposite of the truth
        let green = h.mounted && !traffic;
        //   lit while the supply is known to be short of four full ports; an unknown supply is
        // not a short one, it is a board that cannot tell, and says so on the console
        let red = h.budget.is_some_and(|b| b.known() && b.limited());
        (green, red)
}

pub struct IndicatorMod<G: OutputPin, R: OutputPin> {
        green: G,
        red: R,
        events: Subscription,
        heard: Heard,
        shown: (bool, bool),
}

impl<G: OutputPin, R: OutputPin> IndicatorMod<G, R> {
        pub fn new(green: G, red: R) -> Self {
                Self { green, red, events: EVENTS.subscribe().expect("subscriber slot"), heard: Heard::default(), shown: (false, false) }
        }

        fn show(&mut self) -> bool {
                let want = levels(&self.heard);
                if want == self.shown {
                        return false;
                }
                if want.0 != self.shown.0 {
                        self.green.set(want.0);
                }
                if want.1 != self.shown.1 {
                        self.red.set(want.1);
                }
                self.shown = want;
                true
        }
}

impl<G: OutputPin, R: OutputPin> Module for IndicatorMod<G, R> {
        fn name(&self) -> &'static str {
                "indicators"
        }

        fn poll(&mut self) -> Poll {
                let mut changed = false;
                while let Some(ev) = EVENTS.poll(&self.events) {
                        match ev {
                                AppEvent::Mounted(on) => self.heard.mounted = on,
                                AppEvent::Indicators { rx, tx } => {
                                        self.heard.rx = rx;
                                        self.heard.tx = tx;
                                }
                                AppEvent::Budget(b) => self.heard.budget = Some(b),
                                _ => continue,
                        }
                        changed = true;
                }
                if changed && self.show() { Poll::Busy } else { Poll::Idle }
        }

        fn unload(&mut self) {
                self.green.set(false);
                self.red.set(false);
        }
}

#[cfg(test)]
mod tests {
        use super::*;

        #[test]
        fn green_means_mounted_and_is_interrupted_by_traffic() {
                assert_eq!(levels(&Heard::default()), (false, false));
                let mounted = Heard { mounted: true, ..Heard::default() };
                assert_eq!(levels(&mounted), (true, false));
                assert_eq!(levels(&Heard { rx: true, ..mounted }), (false, false));
                assert_eq!(levels(&Heard { tx: true, ..mounted }), (false, false));
                // traffic with nothing mounted cannot happen, and would not light anything
                assert_eq!(levels(&Heard { rx: true, ..Heard::default() }), (false, false));
        }

        #[test]
        fn red_means_the_supply_is_known_to_be_short() {
                let short = Heard { budget: Some(Budget::from_cc(400, 0)), ..Heard::default() };
                assert_eq!(levels(&short), (false, true));
                let ample = Heard { budget: Some(Budget::from_cc(1700, 0)), ..Heard::default() };
                assert_eq!(levels(&ample), (false, false));
                let unknown = Heard { budget: Some(Budget::from_cc(0, 0)), ..Heard::default() };
                assert_eq!(levels(&unknown), (false, false), "not knowing is not a shortage");
        }
}
