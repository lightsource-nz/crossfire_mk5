//! Keeping a new firmware, or letting it go.
//!
//! An update is started ON PROBATION: the hardware runs the new image once, and unless that image
//! commits itself it is discarded and the previous one boots again -- at the next reset, or when
//! the hardware's own probation watchdog runs out, whichever comes first. So a candidate that
//! panics, stops its loop or never gets that far is thrown away without anyone doing anything.
//! What is left for the firmware to decide is the other case: an image that runs but is WRONG.
//!
//! This module is that decision. At boot it asks the board whether the image is a candidate; if
//! it is, it watches the application for a few seconds and commits only once every check has
//! passed. A check that fails, or one that has not passed by the deadline, means no commit -- and
//! no commit means the old firmware comes back, which is the whole point of having put the new
//! one on probation.
//!
//! THE CHECKS ARE WHAT A CROSSFIRE CAN PROVE ON ITS OWN. Nobody is guaranteed to have plugged an
//! instrument in during the first seconds after an update, so a working MIDI path cannot be asked
//! for; what can be asked is that every module loaded, the main loop is turning over, the console
//! core is alive, and the display took a whole frame without its bus timing out.
//!
//! AND THEY HAVE TO BE QUICK. The probation window is on the order of seventeen seconds from the
//! boot, and a commit made after it closes does nothing while reporting success. So the checks
//! settle for [`SETTLE_MS`] and give up at [`COMMIT_DEADLINE_MS`] of uptime, which leaves the
//! hardware's window a margin of several seconds rather than a race.

use light_core::hal::UpdateError;
use light_core::{info, log, warn, Module, Poll};

/// What a board's boot facility says about the image it is running, and how that image keeps
/// itself. Implemented by the hardware module: which image is on probation, and what keeping it
/// takes, are facts about the chip and not about the application.
pub trait Probation {
        /// Whether the running image is a CANDIDATE: started on approval, and discarded unless it
        /// commits itself.
        fn candidate(&self) -> bool;

        /// Keep the running image from now on. Called at most once, and only on a candidate.
        fn commit(&mut self) -> Result<(), UpdateError>;
}

/// A board whose images are never on probation -- the RP2040, whose boot ROM runs whatever is at
/// the start of flash and has no second slot to fall back to. Every image is already kept.
pub struct Permanent;

impl Probation for Permanent {
        fn candidate(&self) -> bool {
                false
        }

        fn commit(&mut self) -> Result<(), UpdateError> {
                Ok(())
        }
}

/// How long a candidate must have run, from the moment every module was loaded, before it is
/// judged. Long enough for the display to have taken frames and the host stack to have been
/// serviced a great many times; short against the hardware's window.
pub const SETTLE_MS: u32 = 3_000;
/// How many passes the main loop must have made in that time. A loop that is turning over makes
/// this many in a few milliseconds; one that is stuck in a module makes none at all.
pub const MIN_PASSES: u32 = 100;
/// The uptime past which a candidate that has not yet passed every check is given up on. Uptime
/// and not time since load, because the hardware's window opened at the boot, before any of this
/// ran -- and well inside that window, so a commit is never attempted after it has closed.
pub const COMMIT_DEADLINE_MS: u32 = 10_000;

/// The application's signs of life, as the probation module reads them. Counters, so a check
/// compares two readings rather than trusting a flag something forgot to clear.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vitals {
        /// Milliseconds since the boot.
        pub uptime_ms: u32,
        /// Passes of the main loop (core 0).
        pub passes: u32,
        /// Passes of the console core's loop (core 1).
        pub console_ticks: u32,
        /// Frames the display has delivered to the glass in full.
        pub frames_shown: u32,
        /// Pushes to the display abandoned because its bus stopped answering.
        pub display_timeouts: u32,
}

/// What the checks make of a candidate so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
        /// Not yet: keep watching.
        Wait,
        /// Every check has passed: commit.
        Healthy,
        /// It is not to be kept, and this is why.
        Unfit(&'static str),
}

/// Judge a candidate by its vitals `now` against those it had when the runtime started.
///
/// A display timeout is final however early it comes: it is a fault, not a delay. Everything else
/// is something that has not happened YET, which becomes a failure only at the deadline.
pub fn judge(since: &Vitals, now: &Vitals) -> Verdict {
        if now.display_timeouts != 0 {
                return Verdict::Unfit("the display stopped answering");
        }
        let pending = if now.uptime_ms.wrapping_sub(since.uptime_ms) < SETTLE_MS {
                Some("it did not run long enough to be judged")
        } else if now.passes.wrapping_sub(since.passes) < MIN_PASSES {
                Some("the main loop is not turning over")
        } else if now.console_ticks == since.console_ticks {
                Some("the console core is not running")
        } else if now.frames_shown == 0 {
                Some("the display never showed a frame")
        } else {
                None
        };
        match pending {
                None => Verdict::Healthy,
                Some(why) if now.uptime_ms >= COMMIT_DEADLINE_MS => Verdict::Unfit(why),
                Some(_) => Verdict::Wait,
        }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
        /// Not a candidate, or already decided: nothing more to do, ever.
        Settled,
        /// A candidate, watched since these vitals.
        Watching(Vitals),
}

/// The module: detect a candidate at load, judge it while the application runs, commit or not.
///
/// Added to the runtime LAST, so its load comes after every other module's -- which makes a
/// successful load of this one the proof that all the others loaded -- and its first reading is
/// taken with the application already up.
pub struct ProbationMod<P: Probation> {
        probation: P,
        /// The application's signs of life, read by the caller: the hardware module supplies the
        /// console core's counter, and the application its own.
        vitals: fn() -> Vitals,
        state: State,
}

impl<P: Probation> ProbationMod<P> {
        pub fn new(probation: P, vitals: fn() -> Vitals) -> Self {
                Self { probation, vitals, state: State::Settled }
        }

        /// Whether a candidate is still being judged.
        pub fn watching(&self) -> bool {
                matches!(self.state, State::Watching(_))
        }

        /// One look at the candidate, given its vitals now. Separate from `poll` so the decision
        /// can be driven with made-up readings.
        fn step(&mut self, now: Vitals) -> Poll {
                let State::Watching(since) = self.state else {
                        return Poll::Idle;
                };
                match judge(&since, &now) {
                        Verdict::Wait => Poll::Idle,
                        Verdict::Healthy => {
                                self.state = State::Settled;
                                info!("probation: every check passed at {} ms; committing this firmware", now.uptime_ms);
                                match self.probation.commit() {
                                        Ok(()) => info!("probation: committed -- this firmware is kept from now on"),
                                        Err(e) => warn!("probation: the commit failed ({e:?}); the previous firmware comes back at the next reset"),
                                }
                                Poll::Busy
                        }
                        Verdict::Unfit(why) => {
                                self.state = State::Settled;
                                warn!("probation: not keeping this firmware -- {why}; the hardware restores the previous one within seconds");
                                Poll::Busy
                        }
                }
        }
}

impl<P: Probation> Module for ProbationMod<P> {
        fn name(&self) -> &'static str {
                "probation"
        }

        fn deps(&self) -> &'static [&'static str] {
                //   the modules whose health is judged: they load first, so the first reading is
                // of an application that is already up
                &["usb", "oled", "led", "console", "nav"]
        }

        fn load(&mut self) -> Result<(), ()> {
                if self.probation.candidate() {
                        let since = (self.vitals)();
                        info!("probation: this firmware is a candidate; checking it for {} ms before keeping it", SETTLE_MS);
                        self.state = State::Watching(since);
                }
                Ok(())
        }

        fn poll(&mut self) -> Poll {
                if !self.watching() {
                        return Poll::Idle;
                }
                self.step((self.vitals)())
        }
}

/// Milliseconds since the boot, from the log's clock.
pub(crate) fn uptime_ms() -> u32 {
        (log::now_us() / 1000) as u32
}

#[cfg(test)]
mod tests {
        use super::*;

        /// A boot facility that remembers being asked.
        struct Fake {
                candidate: bool,
                commits: u32,
                answer: Result<(), UpdateError>,
        }

        impl Probation for &mut Fake {
                fn candidate(&self) -> bool {
                        self.candidate
                }
                fn commit(&mut self) -> Result<(), UpdateError> {
                        self.commits += 1;
                        self.answer
                }
        }

        fn fake(candidate: bool) -> Fake {
                Fake { candidate, commits: 0, answer: Ok(()) }
        }

        fn no_vitals() -> Vitals {
                Vitals::default()
        }

        /// What the application looks like at load: up, but with no time behind it.
        const AT_LOAD: Vitals = Vitals { uptime_ms: 500, passes: 10, console_ticks: 1_000, frames_shown: 1, display_timeouts: 0 };

        /// The same application a while later, doing everything it should.
        fn healthy(uptime_ms: u32) -> Vitals {
                Vitals { uptime_ms, passes: AT_LOAD.passes + 50_000, console_ticks: AT_LOAD.console_ticks + 90_000, frames_shown: 3, display_timeouts: 0 }
        }

        fn watching(p: &mut Fake) -> ProbationMod<&mut Fake> {
                let mut m = ProbationMod::new(p, no_vitals);
                m.load().unwrap();
                m.state = State::Watching(AT_LOAD);
                m
        }

        #[test]
        fn a_healthy_candidate_is_committed_once_it_has_settled() {
                assert_eq!(judge(&AT_LOAD, &healthy(AT_LOAD.uptime_ms + SETTLE_MS - 1)), Verdict::Wait);
                assert_eq!(judge(&AT_LOAD, &healthy(AT_LOAD.uptime_ms + SETTLE_MS)), Verdict::Healthy);
        }

        #[test]
        fn a_display_timeout_is_final_at_once() {
                let mut now = healthy(AT_LOAD.uptime_ms + 10);
                now.display_timeouts = 1;
                assert!(matches!(judge(&AT_LOAD, &now), Verdict::Unfit(_)));
        }

        #[test]
        fn each_missing_sign_of_life_waits_and_then_fails_at_the_deadline() {
                let stalls: [fn(&mut Vitals); 3] = [
                        |v| v.passes = AT_LOAD.passes + MIN_PASSES - 1,
                        |v| v.console_ticks = AT_LOAD.console_ticks,
                        |v| v.frames_shown = 0,
                ];
                for stall in stalls {
                        let mut early = healthy(AT_LOAD.uptime_ms + SETTLE_MS);
                        stall(&mut early);
                        assert_eq!(judge(&AT_LOAD, &early), Verdict::Wait);
                        let mut late = healthy(COMMIT_DEADLINE_MS);
                        stall(&mut late);
                        assert!(matches!(judge(&AT_LOAD, &late), Verdict::Unfit(_)));
                }
        }

        #[test]
        fn the_deadline_is_uptime_not_time_since_load() {
                //   loaded late: the window opened at the boot, so settling past the deadline is
                // too late even though the settle time itself was honoured
                let since = Vitals { uptime_ms: COMMIT_DEADLINE_MS - 1_000, ..AT_LOAD };
                let mut now = healthy(COMMIT_DEADLINE_MS + 1);
                now.frames_shown = 0;
                assert!(matches!(judge(&since, &now), Verdict::Unfit(_)));
        }

        #[test]
        fn an_image_that_is_not_a_candidate_is_never_committed() {
                let mut p = fake(false);
                {
                        let mut m = ProbationMod::new(&mut p, || healthy(COMMIT_DEADLINE_MS - 1));
                        m.load().unwrap();
                        assert!(!m.watching());
                        assert_eq!(m.poll(), Poll::Idle);
                }
                assert_eq!(p.commits, 0);
        }

        #[test]
        fn a_candidate_is_watched_from_load() {
                let mut p = fake(true);
                let m = {
                        let mut m = ProbationMod::new(&mut p, no_vitals);
                        m.load().unwrap();
                        m.state
                };
                assert_eq!(m, State::Watching(Vitals::default()));
        }

        #[test]
        fn a_healthy_candidate_commits_exactly_once() {
                let mut p = fake(true);
                {
                        let mut m = watching(&mut p);
                        assert_eq!(m.step(healthy(AT_LOAD.uptime_ms + 100)), Poll::Idle);
                        assert_eq!(m.step(healthy(AT_LOAD.uptime_ms + SETTLE_MS)), Poll::Busy);
                        assert!(!m.watching());
                        assert_eq!(m.step(healthy(AT_LOAD.uptime_ms + SETTLE_MS + 100)), Poll::Idle);
                }
                assert_eq!(p.commits, 1);
        }

        #[test]
        fn an_unfit_candidate_is_never_committed() {
                let mut p = fake(true);
                {
                        let mut m = watching(&mut p);
                        let mut now = healthy(AT_LOAD.uptime_ms + SETTLE_MS);
                        now.display_timeouts = 2;
                        assert_eq!(m.step(now), Poll::Busy);
                        assert!(!m.watching());
                        assert_eq!(m.step(healthy(AT_LOAD.uptime_ms + SETTLE_MS)), Poll::Idle);
                }
                assert_eq!(p.commits, 0);
        }

        #[test]
        fn a_refused_commit_is_not_retried() {
                //   the facility's answer is the answer: asking again inside a window that may
                // already be closing is a second rewrite of the image's own storage for nothing
                let mut p = Fake { answer: Err(UpdateError::Storage), ..fake(true) };
                {
                        let mut m = watching(&mut p);
                        m.step(healthy(AT_LOAD.uptime_ms + SETTLE_MS));
                        m.step(healthy(AT_LOAD.uptime_ms + SETTLE_MS + 1_000));
                        assert!(!m.watching());
                }
                assert_eq!(p.commits, 1);
        }
}
