//! The hub: held in reset until the application is ready for instruments, told when its
//! upstream is live, and read the supply's advertisement on the way.
//!
//! A board that hosts its instruments through a hub on the board itself has a hub with no
//! upstream cable: nothing supplies its upstream port, so the firmware tells it when to attach
//! -- an output that stands in for the VBUS it would otherwise detect -- and holds it in reset
//! until then. Both are plain outputs behind the pin trait, so this module is portable and its
//! sequence is tested on the host. A board whose hub is a real hub on a real cable brings no
//! such module.
//!
//! The order matters and is the part a datasheet spells out: out of reset first, so the part
//! samples its configuration straps and settles, and only then told that its upstream is
//! there, which is what makes it pull the line up and enumerate. Holding it and releasing it
//! again is how the firmware forces a fresh enumeration without a power cycle -- with the hold
//! kept for [`RESTART_HOLD_MS`], since a hub released in the same instant it was held never
//! notices it went away.

use light_core::events::Subscription;
use light_core::{info, log, warn, Module, OutputPin, Poll};

use crate::power::Budget;
use crate::{AppEvent, EVENTS};

/// The supply's two CC lines, read in millivolts. The board knows its ADC and its scale; this
/// module knows what the voltages mean.
pub trait CcSense {
        fn read_mv(&mut self) -> (u32, u32);
}

/// A board with no CC lines to read: the budget is unknown and said to be.
pub struct NoCcSense;

impl CcSense for NoCcSense {
        fn read_mv(&mut self) -> (u32, u32) {
                (0, 0)
        }
}

/// How long a restart holds the hub before releasing it: long enough for the host to see
/// every device behind it leave, so what comes back is enumerated afresh.
pub const RESTART_HOLD_MS: u32 = 100;

/// The two lines and the order they are driven in. Apart from the module so the sequence is
/// tested without a bus or a clock: time comes in as an argument.
pub struct HubLines<R: OutputPin, V: OutputPin> {
        reset_n: R,
        vbus_det: V,
        released: bool,
        /// When a restart in progress releases the hub again.
        restart_at: Option<u32>,
}

impl<R: OutputPin, V: OutputPin> HubLines<R, V> {
        /// `reset_n` and `vbus_det` as the board constructed them: low, the hub held.
        pub fn new(reset_n: R, vbus_det: V) -> Self {
                Self { reset_n, vbus_det, released: false, restart_at: None }
        }

        pub fn released(&self) -> bool {
                self.released
        }

        /// Out of reset first, then told its upstream is there: the part samples its straps on
        /// the way out of reset and attaches when the upstream rises.
        pub fn release(&mut self) {
                self.reset_n.set(true);
                self.vbus_det.set(true);
                self.released = true;
                self.restart_at = None;
        }

        /// The upstream taken away first, then the reset: the mirror of `release`. Cancels a
        /// restart in progress -- held means held.
        pub fn hold(&mut self) {
                self.vbus_det.set(false);
                self.reset_n.set(false);
                self.released = false;
                self.restart_at = None;
        }

        /// A fresh enumeration without a power cycle: held now, released by [`Self::service`]
        /// once [`RESTART_HOLD_MS`] have passed.
        pub fn restart(&mut self, now_ms: u32) {
                self.hold();
                self.restart_at = Some(now_ms.wrapping_add(RESTART_HOLD_MS));
        }

        pub fn restarting(&self) -> bool {
                self.restart_at.is_some()
        }

        /// Finish a restart whose hold has run its course; true when this call released the hub.
        pub fn service(&mut self, now_ms: u32) -> bool {
                match self.restart_at {
                        //   compared as a signed difference, so the millisecond count wrapping
                        // does not release early or never
                        Some(at) if now_ms.wrapping_sub(at) as i32 >= 0 => {
                                self.release();
                                true
                        }
                        _ => false,
                }
        }
}

/// Read the supply, say what it offers, and tell the bus.
pub fn read_budget(sense: &mut impl CcSense) -> Budget {
        let (cc1, cc2) = sense.read_mv();
        let b = Budget::from_cc(cc1, cc2);
        if b.known() {
                info!("power: source on {:?} advertises {}: {} mA for the ports -- {} at full current, {} typical instruments", b.line, b.advertisement.describe(), b.ports_ma, b.full_ports, b.typical_instruments);
        } else {
                warn!("power: no USB-C advertisement on either CC line ({cc1} mV, {cc2} mV); the budget is unknown");
        }
        let _ = EVENTS.publish(AppEvent::Budget(b));
        b
}

pub struct HubMod<R: OutputPin, V: OutputPin, S: CcSense> {
        lines: HubLines<R, V>,
        sense: S,
        events: Subscription,
}

impl<R: OutputPin, V: OutputPin, S: CcSense> HubMod<R, V, S> {
        /// `reset_n` and `vbus_det` as the board constructed them: low, the hub held.
        pub fn new(reset_n: R, vbus_det: V, sense: S) -> Self {
                Self { lines: HubLines::new(reset_n, vbus_det), sense, events: EVENTS.subscribe().expect("subscriber slot") }
        }
}

impl<R: OutputPin, V: OutputPin, S: CcSense> Module for HubMod<R, V, S> {
        fn name(&self) -> &'static str {
                "hub"
        }

        fn deps(&self) -> &'static [&'static str] {
                //   after the host stack is up: the hub attaches the moment it is told to, and
                // what it attaches to must be listening
                &["usb"]
        }

        fn load(&mut self) -> Result<(), ()> {
                let _ = read_budget(&mut self.sense);
                self.lines.release();
                info!("hub: released from reset, upstream reported live");
                Ok(())
        }

        fn poll(&mut self) -> Poll {
                let mut busy = false;
                let now_ms = (log::now_us() / 1000) as u32;
                while let Some(ev) = EVENTS.poll(&self.events) {
                        match ev {
                                AppEvent::Hub(true) => {
                                        if self.lines.released() {
                                                self.lines.restart(now_ms);
                                                info!("hub: held for {} ms to re-enumerate", RESTART_HOLD_MS);
                                        } else if self.lines.restarting() {
                                                //   already on its way back; asking again changes nothing
                                        } else {
                                                self.lines.release();
                                                info!("hub: released from reset, upstream reported live");
                                        }
                                        busy = true;
                                }
                                AppEvent::Hub(false) => {
                                        self.lines.hold();
                                        info!("hub: held in reset");
                                        busy = true;
                                }
                                AppEvent::Power => {
                                        let _ = read_budget(&mut self.sense);
                                        busy = true;
                                }
                                _ => {}
                        }
                }
                if self.lines.service(now_ms) {
                        info!("hub: released from reset, upstream reported live");
                        busy = true;
                }
                if busy { Poll::Busy } else { Poll::Idle }
        }

        fn unload(&mut self) {
                self.lines.hold();
        }
}

#[cfg(test)]
mod tests {
        use super::*;
        use core::cell::RefCell;

        extern crate std;
        use std::rc::Rc;
        use std::vec::Vec;

        /// Every level written to either pin, in order, so the sequence can be read back.
        type Trace = Rc<RefCell<Vec<(&'static str, bool)>>>;

        struct Pin(&'static str, Trace);

        impl OutputPin for Pin {
                fn set(&mut self, high: bool) {
                        self.1.borrow_mut().push((self.0, high));
                }
        }

        fn lines() -> (HubLines<Pin, Pin>, Trace) {
                let t = Trace::default();
                (HubLines::new(Pin("reset_n", t.clone()), Pin("vbus_det", t.clone())), t)
        }

        #[test]
        fn release_lifts_the_reset_before_reporting_the_upstream() {
                let (mut l, t) = lines();
                assert!(!l.released());
                l.release();
                assert!(l.released());
                assert_eq!(*t.borrow(), [("reset_n", true), ("vbus_det", true)]);
        }

        #[test]
        fn hold_takes_the_upstream_away_before_the_reset() {
                let (mut l, t) = lines();
                l.release();
                t.borrow_mut().clear();
                l.hold();
                assert!(!l.released());
                assert_eq!(*t.borrow(), [("vbus_det", false), ("reset_n", false)]);
        }

        #[test]
        fn a_restart_holds_for_the_full_period_before_releasing() {
                let (mut l, t) = lines();
                l.release();
                t.borrow_mut().clear();
                l.restart(1_000);
                assert!(!l.released() && l.restarting());
                assert_eq!(*t.borrow(), [("vbus_det", false), ("reset_n", false)]);
                assert!(!l.service(1_000 + RESTART_HOLD_MS - 1));
                assert!(!l.released());
                assert!(l.service(1_000 + RESTART_HOLD_MS));
                assert!(l.released() && !l.restarting());
                assert_eq!(t.borrow()[2..], [("reset_n", true), ("vbus_det", true)]);
                assert!(!l.service(1_000 + RESTART_HOLD_MS + 1));
        }

        #[test]
        fn a_restart_across_the_millisecond_wrap_still_waits() {
                let (mut l, _) = lines();
                l.release();
                l.restart(u32::MAX - 10);
                assert!(!l.service(u32::MAX));
                assert!(!l.service(RESTART_HOLD_MS - 12));
                assert!(l.service(RESTART_HOLD_MS - 11));
        }

        #[test]
        fn a_hold_cancels_a_restart_in_progress() {
                let (mut l, _) = lines();
                l.release();
                l.restart(0);
                l.hold();
                assert!(!l.restarting());
                assert!(!l.service(10 * RESTART_HOLD_MS));
                assert!(!l.released());
        }

        struct Sense(u32, u32);

        impl CcSense for Sense {
                fn read_mv(&mut self) -> (u32, u32) {
                        (self.0, self.1)
                }
        }

        #[test]
        fn the_budget_is_read_from_the_lines_the_board_supplies() {
                let b = read_budget(&mut Sense(60, 900));
                assert_eq!(b.advertisement, crate::power::Advertisement::Amp1_5);
                assert_eq!(b.line, crate::power::CcLine::Cc2);
                let b = read_budget(&mut NoCcSense);
                assert!(!b.known());
        }
}
