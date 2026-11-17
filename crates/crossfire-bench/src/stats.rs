//! Latency distributions and sequence checking, with no allocation.

use crate::SEQ_MOD;

/// The histogram's resolution: one bucket per this many microseconds.
pub const BUCKET_US: u32 = 10;
/// Buckets: 4096 of 10 us spans 40.96 ms. A time past that lands in the last bucket; the
/// maximum is kept exactly regardless.
pub const BUCKETS: usize = 4096;

/// A latency distribution: what `min`, `max` and the percentiles are read from. The percentiles
/// are bucket starts, so they are good to [`BUCKET_US`]; `min` and `max` are exact.
pub struct Histogram {
        buckets: [u32; BUCKETS],
        count: u32,
        min: u32,
        max: u32,
}

/// What a distribution says, in microseconds. All zero when it is empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
        pub count: u32,
        pub min: u32,
        pub p50: u32,
        pub p90: u32,
        pub p99: u32,
        pub max: u32,
}

impl Default for Histogram {
        fn default() -> Self {
                Self::new()
        }
}

impl Histogram {
        pub const fn new() -> Self {
                Self { buckets: [0; BUCKETS], count: 0, min: u32::MAX, max: 0 }
        }

        pub fn clear(&mut self) {
                self.buckets = [0; BUCKETS];
                self.count = 0;
                self.min = u32::MAX;
                self.max = 0;
        }

        pub fn add(&mut self, us: u32) {
                let b = ((us / BUCKET_US) as usize).min(BUCKETS - 1);
                self.buckets[b] = self.buckets[b].saturating_add(1);
                self.count += 1;
                self.min = self.min.min(us);
                self.max = self.max.max(us);
        }

        /// The value below which `per_mille` thousandths of the samples fall.
        fn percentile(&self, per_mille: u32) -> u32 {
                //   the rank of the sample asked for, counted from one: the 500th thousandth of
                // ten samples is the fifth
                let rank = (u64::from(self.count) * u64::from(per_mille)).div_ceil(1000).max(1);
                let mut seen = 0u64;
                for (i, &n) in self.buckets.iter().enumerate() {
                        seen += u64::from(n);
                        if seen >= rank {
                                return (i as u32 * BUCKET_US).clamp(self.min, self.max);
                        }
                }
                self.max
        }

        pub fn summary(&self) -> Summary {
                if self.count == 0 {
                        return Summary::default();
                }
                Summary { count: self.count, min: self.min, p50: self.percentile(500), p90: self.percentile(900), p99: self.percentile(990), max: self.max }
        }
}

/// Checks a stream of 14-bit sequence numbers against the order they were sent in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeqCheck {
        next: Option<u16>,
        /// Numbers skipped over: sent, and not seen before a later one arrived.
        pub skipped: u32,
        /// Numbers that arrived behind one already seen: late, or twice.
        pub behind: u32,
}

impl SeqCheck {
        pub const fn new() -> Self {
                Self { next: None, skipped: 0, behind: 0 }
        }

        pub fn see(&mut self, seq: u16) {
                let Some(next) = self.next else {
                        self.next = Some((seq + 1) % SEQ_MOD);
                        return;
                };
                let ahead = seq.wrapping_sub(next) % SEQ_MOD;
                if ahead < SEQ_MOD / 2 {
                        self.skipped += u32::from(ahead);
                        self.next = Some((seq + 1) % SEQ_MOD);
                } else {
                        self.behind += 1;
                }
        }
}

#[cfg(test)]
mod tests {
        use super::*;
        extern crate std;
        use std::boxed::Box;

        #[test]
        fn percentiles_are_read_to_the_bucket_and_the_ends_are_exact() {
                let mut h = Box::new(Histogram::new());
                for us in 1..=100 {
                        h.add(us * 10 + 3);
                }
                let s = h.summary();
                assert_eq!(s.count, 100);
                assert_eq!(s.min, 13);
                assert_eq!(s.max, 1003);
                assert_eq!(s.p50, 500, "the 50th of 100 is 503, in the bucket starting at 500");
                assert_eq!(s.p90, 900);
                assert_eq!(s.p99, 990);
        }

        #[test]
        fn a_time_past_the_range_keeps_its_exact_maximum() {
                let mut h = Box::new(Histogram::new());
                h.add(100);
                h.add(1_000_000);
                let s = h.summary();
                assert_eq!(s.max, 1_000_000);
                assert_eq!(s.p99, (BUCKETS as u32 - 1) * BUCKET_US);
        }

        #[test]
        fn an_empty_histogram_says_nothing() {
                assert_eq!(Box::new(Histogram::new()).summary(), Summary::default());
        }

        #[test]
        fn one_sample_is_every_percentile() {
                let mut h = Box::new(Histogram::new());
                h.add(1234);
                let s = h.summary();
                assert_eq!((s.min, s.p50, s.p99, s.max), (1234, 1234, 1234, 1234));
        }

        #[test]
        fn the_sequence_check_counts_gaps_and_stragglers_across_the_wrap() {
                let mut c = SeqCheck::default();
                for seq in [SEQ_MOD - 2, SEQ_MOD - 1, 0, 1] {
                        c.see(seq);
                }
                assert_eq!((c.skipped, c.behind), (0, 0), "in order across the wrap");
                c.see(4);
                assert_eq!(c.skipped, 2, "2 and 3 never came");
                c.see(3);
                assert_eq!(c.behind, 1, "3 came after 4");
                c.see(5);
                assert_eq!((c.skipped, c.behind), (2, 1));
        }
}
