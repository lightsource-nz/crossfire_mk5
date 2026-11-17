//! Measures a crossfire board from the outside, the way an instrument would see it.
//!
//! Two small boards act as USB-MIDI instruments on two of crossfire's ports. The TIMER sends
//! numbered messages and times them. The ECHO sends every message straight back and toggles a wire
//! to the timer as each one arrives. With nothing else plugged in, crossfire forwards the timer's
//! messages only to the echo, and the echo's only to the timer. So the timer measures, on its own
//! clock:
//!
//! - **one-way latency**: from handing a message to its USB endpoint to the echo's wire changing;
//! - **round trip**: from the same moment to the echo arriving back;
//! - **throughput**: messages sent at a stepped rate, with the echoes counted and their sequence
//!   checked, so a loss in either direction shows as a gap.
//!
//! This crate is the part that names no chip: the USB-MIDI device class ([`class`]), the timer's
//! schedule and figures ([`timer`]), the echo ([`echo`]), and the statistics both use ([`stats`]).
//! The two firmwares that run them are `module/crossfire_bench_timer` and
//! `module/crossfire_bench_echo`.

#![no_std]

pub mod class;
pub mod echo;
pub mod stats;
pub mod timer;

/// A USB-MIDI event packet: byte 0 is `(cable << 4) | code_index_number`, bytes 1..4 the MIDI data.
pub type Packet = [u8; 4];

/// Event packets in one full-speed bulk packet.
pub const PACKETS_PER_TRANSFER: usize = 16;

/// What a bench message carries: a control change on channel 16 whose controller number and value
/// are the high and low seven bits of a 14-bit sequence number. Any message would do; this one is
/// ordinary MIDI that no instrument will play, should one be left plugged in.
pub fn probe(seq: u16) -> Packet {
        [0x0B, 0xBF, ((seq >> 7) & 0x7F) as u8, (seq & 0x7F) as u8]
}

/// The sequence number of a bench message, or `None` for anything else.
pub fn probe_seq(p: &Packet) -> Option<u16> {
        if p[0] & 0x0F == 0x0B && p[1] == 0xBF {
                Some((u16::from(p[2] & 0x7F) << 7) | u16::from(p[3] & 0x7F))
        } else {
                None
        }
}

/// Sequence numbers wrap at fourteen bits.
pub const SEQ_MOD: u16 = 1 << 14;

#[cfg(test)]
mod tests {
        use super::*;

        #[test]
        fn a_probe_carries_its_sequence_number_and_nothing_else_reads_as_one() {
                for seq in [0, 1, 127, 128, 0x2AAA, SEQ_MOD - 1] {
                        assert_eq!(probe_seq(&probe(seq)), Some(seq));
                }
                assert_eq!(probe_seq(&[0x09, 0x90, 0x3C, 0x64]), None);
                assert_eq!(probe_seq(&[0x0B, 0xB0, 0x01, 0x02]), None, "a control change on another channel");
        }
}
