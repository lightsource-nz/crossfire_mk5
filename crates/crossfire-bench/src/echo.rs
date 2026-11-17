//! The echo: every message that arrives goes back the way it came, in arrival order.
//!
//! Arrivals queue here until the IN endpoint takes them, which it does a full bulk packet at a
//! time once the host has collected the one before. A queue that fills is counted, not waited on:
//! the timer sees the loss as a gap in its sequence, and this count says it happened here rather
//! than on the board being measured.

use crate::{Packet, PACKETS_PER_TRANSFER};

const QUEUE: usize = 512;

pub struct Echo {
        queue: [Packet; QUEUE],
        head: usize,
        len: usize,
        /// Messages that arrived.
        pub received: u32,
        /// Messages handed back to the IN endpoint.
        pub echoed: u32,
        /// Messages that arrived to a full queue and were not echoed.
        pub overflowed: u32,
}

impl Default for Echo {
        fn default() -> Self {
                Self::new()
        }
}

impl Echo {
        pub const fn new() -> Self {
                Self { queue: [[0; 4]; QUEUE], head: 0, len: 0, received: 0, echoed: 0, overflowed: 0 }
        }

        /// Messages that arrived, queued to go back.
        pub fn arrived(&mut self, packets: &[Packet]) {
                for p in packets {
                        self.received = self.received.wrapping_add(1);
                        if self.len == QUEUE {
                                self.overflowed = self.overflowed.wrapping_add(1);
                                continue;
                        }
                        self.queue[(self.head + self.len) % QUEUE] = *p;
                        self.len += 1;
                }
        }

        /// The next messages to send back, oldest first, at most a bulk packet's worth: `out` filled
        /// from the front, the count returned. Nothing leaves the queue until [`Echo::sent`] says so.
        pub fn next(&self, out: &mut [Packet; PACKETS_PER_TRANSFER]) -> usize {
                let n = self.len.min(PACKETS_PER_TRANSFER);
                for (i, slot) in out[..n].iter_mut().enumerate() {
                        *slot = self.queue[(self.head + i) % QUEUE];
                }
                n
        }

        /// The first `n` of what [`Echo::next`] offered went back.
        pub fn sent(&mut self, n: usize) {
                let n = n.min(self.len);
                self.head = (self.head + n) % QUEUE;
                self.len -= n;
                self.echoed = self.echoed.wrapping_add(n as u32);
        }

        pub fn pending(&self) -> usize {
                self.len
        }
}

#[cfg(test)]
mod tests {
        use super::*;
        use crate::probe;

        #[test]
        fn messages_go_back_in_order_a_bulk_packet_at_a_time() {
                let mut e = Echo::new();
                let sent: [Packet; 20] = core::array::from_fn(|i| probe(i as u16));
                e.arrived(&sent);
                let mut out = [[0; 4]; PACKETS_PER_TRANSFER];
                assert_eq!(e.next(&mut out), 16);
                assert_eq!(out[0], probe(0));
                assert_eq!(out[15], probe(15));
                e.sent(16);
                assert_eq!(e.next(&mut out), 4);
                assert_eq!(out[0], probe(16));
                e.sent(4);
                assert_eq!((e.received, e.echoed, e.pending()), (20, 20, 0));
        }

        #[test]
        fn nothing_leaves_until_the_endpoint_took_it() {
                let mut e = Echo::new();
                e.arrived(&[probe(7)]);
                let mut out = [[0; 4]; PACKETS_PER_TRANSFER];
                assert_eq!(e.next(&mut out), 1);
                e.sent(0);
                assert_eq!(e.next(&mut out), 1, "offered again");
                assert_eq!(out[0], probe(7));
        }

        #[test]
        fn a_full_queue_counts_what_it_could_not_keep() {
                let mut e = Echo::new();
                for i in 0..(QUEUE + 3) {
                        e.arrived(&[probe(i as u16)]);
                }
                assert_eq!((e.received, e.overflowed, e.pending()), (QUEUE as u32 + 3, 3, QUEUE));
        }
}
