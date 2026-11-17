//! A USB-MIDI 1.0 device: one cable each way, over any `usb_device` bus.
//!
//! The descriptors are the minimum a host takes for a MIDI instrument: an audio-control interface
//! that names the streaming one, and a MIDI-streaming interface with one embedded and one external
//! jack in each direction and a bulk endpoint each way. The OUT endpoint's size is a parameter,
//! because what a host does with an instrument that takes four bytes at a time rather than
//! sixty-four is one of the things the bench is for.

use usb_device::class_prelude::*;
use usb_device::Result;

use crate::{Packet, PACKETS_PER_TRANSFER};

const CLASS_AUDIO: u8 = 0x01;
const SUBCLASS_AUDIO_CONTROL: u8 = 0x01;
const SUBCLASS_MIDI_STREAMING: u8 = 0x03;
const CS_INTERFACE: u8 = 0x24;
const CS_ENDPOINT: u8 = 0x25;
const HEADER: u8 = 0x01;
const MS_GENERAL: u8 = 0x01;
const MIDI_IN_JACK: u8 = 0x02;
const MIDI_OUT_JACK: u8 = 0x03;
const EMBEDDED: u8 = 0x01;
const EXTERNAL: u8 = 0x02;

/// The jacks' ids: the host's OUT endpoint feeds embedded IN jack 1, which sounds on external OUT
/// jack 4; external IN jack 2 feeds embedded OUT jack 3, which the IN endpoint carries to the host.
const JACK_EMB_IN: u8 = 1;
const JACK_EXT_IN: u8 = 2;
const JACK_EMB_OUT: u8 = 3;
const JACK_EXT_OUT: u8 = 4;

/// The class-specific MIDI-streaming descriptors' total: its header (7), four jacks (6 + 6 + 9 +
/// 9), and both endpoints with their class-specific halves ((9 + 5) * 2).
const MS_TOTAL: u16 = 7 + 30 + 28;

/// The IN endpoint is always a full bulk packet.
const IN_MAX_PACKET: u16 = 64;

pub struct MidiClass<'a, B: UsbBus> {
        control: InterfaceNumber,
        streaming: InterfaceNumber,
        ep_out: EndpointOut<'a, B>,
        ep_in: EndpointIn<'a, B>,
        out_max_packet: u16,
}

impl<'a, B: UsbBus> MidiClass<'a, B> {
        /// `out_max_packet` is what the OUT endpoint declares: 64 for an ordinary instrument, 4 for
        /// one that takes a single event at a time. A multiple of four, at most 64.
        pub fn new(alloc: &'a UsbBusAllocator<B>, out_max_packet: u16) -> Self {
                let out_max_packet = (out_max_packet.clamp(4, 64)) & !3;
                Self { control: alloc.interface(), streaming: alloc.interface(), ep_out: alloc.bulk(out_max_packet), ep_in: alloc.bulk(IN_MAX_PACKET), out_max_packet }
        }

        pub fn out_max_packet(&self) -> u16 {
                self.out_max_packet
        }

        /// Hand up to one full bulk packet of events to the IN endpoint, for the host's next poll.
        /// Returns how many were taken: all of them, or none while the previous packet is still
        /// waiting to be collected.
        pub fn send(&mut self, packets: &[Packet]) -> usize {
                let n = packets.len().min(PACKETS_PER_TRANSFER);
                if n == 0 {
                        return 0;
                }
                let mut buf = [0u8; 64];
                for (i, p) in packets[..n].iter().enumerate() {
                        buf[i * 4..i * 4 + 4].copy_from_slice(p);
                }
                match self.ep_in.write(&buf[..n * 4]) {
                        Ok(_) => n,
                        Err(_) => 0,
                }
        }

        /// The events of the next packet the host sent, if one has arrived: `out` filled from the
        /// front, the count returned. Padding (code index 0) is left out.
        pub fn receive(&mut self, out: &mut [Packet; PACKETS_PER_TRANSFER]) -> usize {
                let mut buf = [0u8; 64];
                let Ok(len) = self.ep_out.read(&mut buf) else { return 0 };
                let mut n = 0;
                for chunk in buf[..len].chunks_exact(4) {
                        if chunk[0] & 0x0F != 0 {
                                out[n] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                                n += 1;
                        }
                }
                n
        }
}

impl<B: UsbBus> UsbClass<B> for MidiClass<'_, B> {
        fn get_configuration_descriptors(&self, w: &mut DescriptorWriter) -> Result<()> {
                w.interface(self.control, CLASS_AUDIO, SUBCLASS_AUDIO_CONTROL, 0)?;
                // the audio-control header: ADC 1.0, its own 9 bytes, one streaming interface
                w.write(CS_INTERFACE, &[HEADER, 0x00, 0x01, 0x09, 0x00, 0x01, u8::from(self.streaming)])?;

                w.interface(self.streaming, CLASS_AUDIO, SUBCLASS_MIDI_STREAMING, 0)?;
                w.write(CS_INTERFACE, &[HEADER, 0x00, 0x01, (MS_TOTAL & 0xFF) as u8, (MS_TOTAL >> 8) as u8])?;
                w.write(CS_INTERFACE, &[MIDI_IN_JACK, EMBEDDED, JACK_EMB_IN, 0])?;
                w.write(CS_INTERFACE, &[MIDI_IN_JACK, EXTERNAL, JACK_EXT_IN, 0])?;
                // an OUT jack names its one source: the jack on the other side of the device
                w.write(CS_INTERFACE, &[MIDI_OUT_JACK, EMBEDDED, JACK_EMB_OUT, 1, JACK_EXT_IN, 1, 0])?;
                w.write(CS_INTERFACE, &[MIDI_OUT_JACK, EXTERNAL, JACK_EXT_OUT, 1, JACK_EMB_IN, 1, 0])?;

                // audio-class endpoints are nine bytes: bRefresh and bSynchAddress, both zero
                w.endpoint_ex(&self.ep_out, |d| {
                        d[..2].fill(0);
                        Ok(2)
                })?;
                w.write(CS_ENDPOINT, &[MS_GENERAL, 1, JACK_EMB_IN])?;
                w.endpoint_ex(&self.ep_in, |d| {
                        d[..2].fill(0);
                        Ok(2)
                })?;
                w.write(CS_ENDPOINT, &[MS_GENERAL, 1, JACK_EMB_OUT])?;
                Ok(())
        }
}
