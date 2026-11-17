//! The timer's schedule and the figures it keeps.
//!
//! One run, from power-up to [`phase::DONE`]:
//!
//! 1. **Connect.** A message every 100 ms until the first echo comes back: the path through the
//!    board is up.
//! 2. **Latency.** One message at a time. Each is timed to the echo board's wire changing (one
//!    way) and to its echo (round trip), and the next goes 2 to 3 ms later, at a random point in
//!    the 1 ms USB frame, so the figures are a distribution over the frame rather than one phase
//!    of it.
//! 3. **Rates.** Messages at each rate in turn for two seconds, then time for the last echoes. The
//!    echoes are counted, their sequence checked, and each timed back to the moment its message
//!    was handed to the endpoint.
//!
//! Everything lands in [`Results`], a block of plain words that a debugger can read off the
//! running board.

use crate::stats::{Histogram, SeqCheck, Summary};
use crate::{probe, probe_seq, Packet, PACKETS_PER_TRANSFER, SEQ_MOD};

/// The USB-MIDI device the timer sends through.
pub trait Link {
        /// Hand messages to the IN endpoint; how many it took (none while it is busy).
        fn send(&mut self, packets: &[Packet]) -> usize;
        /// Messages the host sent, if any have arrived.
        fn receive(&mut self, out: &mut [Packet; PACKETS_PER_TRANSFER]) -> usize;
}

/// How a run is shaped.
#[derive(Clone, Copy, Debug)]
pub struct Config {
        pub probes: u32,
        pub probe_timeout_us: u64,
        /// Messages per second, one step each. At most [`MAX_RATES`].
        pub rates: &'static [u32],
        pub rate_us: u64,
        /// After a rate stops sending: how long its last echoes are waited for.
        pub drain_us: u64,
        /// Between rates: arrivals are ignored, so a straggler is not counted against the next.
        pub settle_us: u64,
}

pub const MAX_RATES: usize = 8;

pub const DEFAULT: Config = Config {
        probes: 2000,
        probe_timeout_us: 50_000,
        rates: &[500, 1000, 2000, 4000, 8000, 12_000, 16_000, 24_000],
        rate_us: 2_000_000,
        drain_us: 300_000,
        settle_us: 200_000,
};

const CONNECT_EVERY_US: u64 = 100_000;
const GAP_MIN_US: u64 = 2_000;
const GAP_SPREAD_US: u32 = 1_000;

/// "XFBN", read as a little-endian word: the block is this one, and it is initialised.
pub const MAGIC: u32 = 0x4E42_4658;
/// Bumped whenever the layout of [`Results`] changes; the reading script checks it.
pub const VERSION: u32 = 1;

pub mod phase {
        pub const CONNECTING: u32 = 0;
        pub const LATENCY: u32 = 1;
        pub const RATES: u32 = 2;
        pub const DONE: u32 = 3;
}

/// One rate's figures.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RateResult {
        /// Messages per second asked for.
        pub rate: u32,
        /// Messages due in the run at that rate.
        pub offered: u32,
        /// Messages the IN endpoint took: fewer than offered when the board did not collect them
        /// as fast as they were made.
        pub sent: u32,
        /// Echoes that came back.
        pub echoed: u32,
        /// Sequence numbers skipped over in the echoes.
        pub skipped: u32,
        /// Echoes that arrived behind a later one.
        pub behind: u32,
        pub round_trip: Summary,
}

/// The whole run's figures, in plain words a debugger reads off the running board. The layout is
/// [`VERSION`]ed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Results {
        pub magic: u32,
        pub version: u32,
        /// Where the run is: a [`phase`] constant.
        pub phase: u32,
        /// The longest pass of the firmware's loop: the resolution of every time here.
        pub loop_worst_us: u32,
        /// Latency probes finished.
        pub probes: u32,
        /// Probes whose echo never came.
        pub no_echo: u32,
        /// Probes the wire never answered.
        pub no_edge: u32,
        pub one_way: Summary,
        pub round_trip: Summary,
        /// Rates finished, filled in order.
        pub rates_done: u32,
        pub rates: [RateResult; MAX_RATES],
}

const NO_SUMMARY: Summary = Summary { count: 0, min: 0, p50: 0, p90: 0, p99: 0, max: 0 };

impl Results {
        /// A block that says what it is and that nothing has been measured.
        pub const fn new() -> Self {
                Self {
                        magic: MAGIC,
                        version: VERSION,
                        phase: phase::CONNECTING,
                        loop_worst_us: 0,
                        probes: 0,
                        no_echo: 0,
                        no_edge: 0,
                        one_way: NO_SUMMARY,
                        round_trip: NO_SUMMARY,
                        rates_done: 0,
                        rates: [RateResult { rate: 0, offered: 0, sent: 0, echoed: 0, skipped: 0, behind: 0, round_trip: NO_SUMMARY }; MAX_RATES],
                }
        }
}

/// What a step finished, for the firmware to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
        Connected,
        LatencyDone,
        RateDone(usize),
        Finished,
}

#[derive(Clone, Copy, Debug)]
enum Phase {
        Connect { next_try: u64 },
        Gap { until: u64 },
        Probe { sent_at: Option<u64>, edge_us: Option<u32>, echo_us: Option<u32> },
        Rate { idx: usize, start: u64 },
        Settle { idx: usize, until: u64 },
        Done,
}

pub struct Timer {
        config: Config,
        phase: Phase,
        seq: u16,
        rng: u32,
        one_way: Histogram,
        round_trip: Histogram,
        rate_rtt: Histogram,
        rate_seq: SeqCheck,
        rate_sent: u32,
        rate_echoed: u32,
        /// When each sequence number was handed to the endpoint, low 32 bits of the clock.
        sent_at: [u32; SEQ_MOD as usize],
        pub results: Results,
}

impl Timer {
        pub const fn new(config: Config) -> Self {
                Self {
                        config,
                        phase: Phase::Connect { next_try: 0 },
                        seq: 0,
                        rng: 0x2545_F491,
                        one_way: Histogram::new(),
                        round_trip: Histogram::new(),
                        rate_rtt: Histogram::new(),
                        rate_seq: SeqCheck::new(),
                        rate_sent: 0,
                        rate_echoed: 0,
                        sent_at: [0; SEQ_MOD as usize],
                        results: Results::new(),
                }
        }

        fn random(&mut self) -> u32 {
                // xorshift32: the phase within a frame needs spreading, not cryptography
                let mut x = self.rng;
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                self.rng = x;
                x
        }

        fn next_seq(&mut self) -> u16 {
                let s = self.seq;
                self.seq = (self.seq + 1) % SEQ_MOD;
                s
        }

        fn gap(&mut self, now: u64) -> Phase {
                let spread = u64::from(self.random() % GAP_SPREAD_US);
                Phase::Gap { until: now + GAP_MIN_US + spread }
        }

        /// One pass: `now` is the clock in microseconds, and `edge_at` the time the echo board's
        /// wire was last seen to change, if it changed since the last pass.
        pub fn step(&mut self, now: u64, edge_at: Option<u64>, link: &mut impl Link) -> Option<Event> {
                let mut arrived = [[0u8; 4]; PACKETS_PER_TRANSFER];
                let n = link.receive(&mut arrived);
                let arrived = &arrived[..n];

                match self.phase {
                        Phase::Connect { next_try } => {
                                if arrived.iter().any(|p| probe_seq(p).is_some()) {
                                        self.results.phase = phase::LATENCY;
                                        self.phase = self.gap(now);
                                        return Some(Event::Connected);
                                }
                                if now >= next_try {
                                        let p = probe(self.seq);
                                        if link.send(&[p]) == 1 {
                                                self.next_seq();
                                        }
                                        self.phase = Phase::Connect { next_try: now + CONNECT_EVERY_US };
                                }
                                None
                        }
                        Phase::Gap { until } => {
                                if now >= until {
                                        self.phase = Phase::Probe { sent_at: None, edge_us: None, echo_us: None };
                                }
                                None
                        }
                        Phase::Probe { sent_at: None, .. } => {
                                if link.send(&[probe(self.seq)]) == 1 {
                                        self.phase = Phase::Probe { sent_at: Some(now), edge_us: None, echo_us: None };
                                }
                                None
                        }
                        Phase::Probe { sent_at: Some(sent), mut edge_us, mut echo_us } => {
                                if edge_us.is_none() {
                                        if let Some(at) = edge_at.filter(|&at| at >= sent) {
                                                edge_us = Some((at - sent) as u32);
                                        }
                                }
                                if echo_us.is_none() && arrived.iter().any(|p| probe_seq(p) == Some(self.seq)) {
                                        echo_us = Some((now - sent) as u32);
                                }
                                let timed_out = now - sent >= self.config.probe_timeout_us;
                                if !(timed_out || (edge_us.is_some() && echo_us.is_some())) {
                                        self.phase = Phase::Probe { sent_at: Some(sent), edge_us, echo_us };
                                        return None;
                                }
                                self.finish_probe(edge_us, echo_us);
                                if self.results.probes >= self.config.probes {
                                        self.results.phase = phase::RATES;
                                        self.phase = Phase::Settle { idx: 0, until: now + self.config.settle_us };
                                        return Some(Event::LatencyDone);
                                }
                                self.phase = self.gap(now);
                                None
                        }
                        Phase::Rate { idx, start } => self.rate_step(now, idx, start, arrived, link),
                        Phase::Settle { idx, until } => {
                                if now < until {
                                        return None;
                                }
                                if idx >= self.config.rates.len().min(MAX_RATES) {
                                        self.results.phase = phase::DONE;
                                        self.phase = Phase::Done;
                                        return Some(Event::Finished);
                                }
                                self.rate_rtt.clear();
                                self.rate_seq = SeqCheck::default();
                                self.rate_sent = 0;
                                self.rate_echoed = 0;
                                self.phase = Phase::Rate { idx, start: now };
                                None
                        }
                        Phase::Done => None,
                }
        }

        fn finish_probe(&mut self, edge_us: Option<u32>, echo_us: Option<u32>) {
                self.next_seq();
                self.results.probes += 1;
                match edge_us {
                        Some(us) => self.one_way.add(us),
                        None => self.results.no_edge += 1,
                }
                match echo_us {
                        Some(us) => self.round_trip.add(us),
                        None => self.results.no_echo += 1,
                }
                self.results.one_way = self.one_way.summary();
                self.results.round_trip = self.round_trip.summary();
        }

        fn rate_step(&mut self, now: u64, idx: usize, start: u64, arrived: &[Packet], link: &mut impl Link) -> Option<Event> {
                for p in arrived {
                        let Some(seq) = probe_seq(p) else { continue };
                        self.rate_seq.see(seq);
                        self.rate_echoed += 1;
                        self.rate_rtt.add((now as u32).wrapping_sub(self.sent_at[usize::from(seq)]));
                }

                let rate = self.config.rates[idx];
                let elapsed = now - start;
                let offered = (u64::from(rate) * self.config.rate_us / 1_000_000) as u32;
                if elapsed < self.config.rate_us {
                        //   message k is due at k / rate, the first at the start
                        let due = ((u64::from(rate) * elapsed / 1_000_000) as u32 + 1).min(offered);
                        let want = (due - self.rate_sent.min(due)) as usize;
                        if want > 0 {
                                let mut batch = [[0u8; 4]; PACKETS_PER_TRANSFER];
                                let n = want.min(PACKETS_PER_TRANSFER);
                                for (i, slot) in batch[..n].iter_mut().enumerate() {
                                        *slot = probe((self.seq + i as u16) % SEQ_MOD);
                                }
                                let taken = link.send(&batch[..n]);
                                for _ in 0..taken {
                                        let seq = self.next_seq();
                                        self.sent_at[usize::from(seq)] = now as u32;
                                }
                                self.rate_sent += taken as u32;
                        }
                        return None;
                }
                if elapsed < self.config.rate_us + self.config.drain_us {
                        return None;
                }
                self.results.rates[idx] = RateResult { rate, offered, sent: self.rate_sent, echoed: self.rate_echoed, skipped: self.rate_seq.skipped, behind: self.rate_seq.behind, round_trip: self.rate_rtt.summary() };
                self.results.rates_done = idx as u32 + 1;
                self.phase = Phase::Settle { idx: idx + 1, until: now + self.config.settle_us };
                Some(Event::RateDone(idx))
        }
}

#[cfg(test)]
mod tests {
        use super::*;
        extern crate std;
        use std::boxed::Box;
        use std::collections::VecDeque;

        /// The two boards and the one between them, simulated: a message takes `delay` to reach the
        /// echo, whose wire changes on arrival, and `delay` again to come back. Every `drop_every`th
        /// message to the echo is lost on the way.
        struct Board {
                delay: u64,
                drop_every: u32,
                carried: u32,
                to_echo: VecDeque<(u64, Packet)>,
                to_timer: VecDeque<(u64, Packet)>,
                edge: Option<u64>,
                now: u64,
        }

        impl Board {
                fn new(delay: u64, drop_every: u32) -> Self {
                        Self { delay, drop_every, carried: 0, to_echo: VecDeque::new(), to_timer: VecDeque::new(), edge: None, now: 0 }
                }

                /// The echo's side, up to `now`: arrivals toggle the wire and go back.
                fn advance(&mut self, now: u64) {
                        self.now = now;
                        while let Some(&(at, p)) = self.to_echo.front() {
                                if at > now {
                                        break;
                                }
                                self.to_echo.pop_front();
                                self.edge = Some(at);
                                self.to_timer.push_back((at + self.delay, p));
                        }
                }
        }

        impl Link for Board {
                fn send(&mut self, packets: &[Packet]) -> usize {
                        for p in packets {
                                self.carried += 1;
                                if self.drop_every != 0 && self.carried % self.drop_every == 0 {
                                        continue;
                                }
                                self.to_echo.push_back((self.now + self.delay, *p));
                        }
                        packets.len()
                }
                fn receive(&mut self, out: &mut [Packet; PACKETS_PER_TRANSFER]) -> usize {
                        let mut n = 0;
                        while n < PACKETS_PER_TRANSFER {
                                match self.to_timer.front() {
                                        Some(&(at, p)) if at <= self.now => {
                                                self.to_timer.pop_front();
                                                out[n] = p;
                                                n += 1;
                                        }
                                        _ => break,
                                }
                        }
                        n
                }
        }

        const QUICK: Config = Config { probes: 50, probe_timeout_us: 50_000, rates: &[1000, 8000], rate_us: 200_000, drain_us: 50_000, settle_us: 20_000 };

        fn run(board: &mut Board) -> (Box<Timer>, std::vec::Vec<Event>) {
                let mut t = Box::new(Timer::new(QUICK));
                let mut events = std::vec::Vec::new();
                let mut now = 0;
                while t.results.phase != phase::DONE {
                        board.advance(now);
                        let edge = board.edge.take();
                        if let Some(e) = t.step(now, edge, board) {
                                events.push(e);
                        }
                        now += 10;
                        assert!(now < 60_000_000, "the run never finished");
                }
                (t, events)
        }

        #[test]
        fn a_clean_board_is_timed_one_way_and_round_trip_and_loses_nothing() {
                let mut board = Board::new(1000, 0);
                let (t, events) = run(&mut board);
                assert_eq!(events, [Event::Connected, Event::LatencyDone, Event::RateDone(0), Event::RateDone(1), Event::Finished]);
                let r = &t.results;
                assert_eq!((r.magic, r.version), (MAGIC, VERSION));
                assert_eq!((r.probes, r.no_echo, r.no_edge), (50, 0, 0));
                //   the simulation steps in 10 us, so a time is the delay to within one step
                assert!((1000..=1010).contains(&r.one_way.p50), "one way {:?}", r.one_way);
                assert!((2000..=2010).contains(&r.round_trip.p50), "round trip {:?}", r.round_trip);
                assert_eq!(r.rates_done, 2);
                for (i, rate) in [1000u32, 8000].iter().enumerate() {
                        let x = &r.rates[i];
                        assert_eq!(x.rate, *rate);
                        assert_eq!(x.offered, rate / 5, "a fifth of a second");
                        assert_eq!((x.sent, x.echoed, x.skipped, x.behind), (x.offered, x.offered, 0, 0), "{x:?}");
                        assert!((2000..=2020).contains(&x.round_trip.p50), "{x:?}");
                }
        }

        #[test]
        fn a_lossy_board_shows_as_missing_echoes_and_skipped_numbers() {
                let mut board = Board::new(500, 20);
                let (t, _) = run(&mut board);
                let r = &t.results;
                assert!(r.no_echo > 0 && r.no_edge == r.no_echo, "a lost probe has neither: {r:?}");
                let fast = &r.rates[1];
                assert_eq!(fast.sent, fast.offered);
                assert!(fast.echoed < fast.sent);
                //   every loss is a gap, except one at the very end, which nothing arrives after
                let lost = fast.sent - fast.echoed;
                assert!(fast.skipped == lost || fast.skipped + 1 == lost, "{fast:?}");
        }

        #[test]
        fn a_silent_board_never_leaves_connecting() {
                struct Nothing;
                impl Link for Nothing {
                        fn send(&mut self, p: &[Packet]) -> usize {
                                p.len()
                        }
                        fn receive(&mut self, _: &mut [Packet; PACKETS_PER_TRANSFER]) -> usize {
                                0
                        }
                }
                let mut t = Box::new(Timer::new(QUICK));
                for now in (0..2_000_000).step_by(100) {
                        assert_eq!(t.step(now, None, &mut Nothing), None);
                }
                assert_eq!(t.results.phase, phase::CONNECTING);
        }
}
