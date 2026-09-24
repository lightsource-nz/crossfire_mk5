//! This board's wireless part, and the indicator that lives on it.
//!
//! The radio is a second chip: powered, handed its firmware, and only then a radio. The bus and
//! the driving of it are the port's ([`light_rp2::wifi`]); what is here is this board's answer to
//! "which pins, whose firmware, and what is it for?".
//!
//! THE INDICATOR IS ON THE RADIO, which is why it is in this file and not beside the other board
//! wiring. On a board of this kind the pin that carries an indicator when there is no radio is the
//! radio's chip select when there is one -- the same pin, a different job -- so a firmware that
//! drives it as an indicator is driving the radio's chip select, and the two cannot both be true.
//! The indicator this board actually has hangs off the radio's own pins, so answering the
//! application's indicator events means asking the radio, and that is this module's other job.

use light_app_crossfire::{AppEvent, Credentials, EVENTS};
use light_core::events::Subscription;
use light_core::{info, warn};
use light_core::module::{Module, Poll};
use light_rp2::wifi::{JoinError, Pins, Radio};

/// The radio's wiring on this board.
const PINS: Pins = light_rp2::wifi::ONBOARD;
/// The transfer channel the radio's bus uses. Both directions share it -- they never overlap --
/// and it is not the one the display streams frames on.
const DMA_CH: usize = 14;
/// Which of the radio's own pins the indicator is on.
const INDICATOR: u8 = 0;

pub struct RadioMod {
        /// Nothing until it is asked for -- see `AppEvent::Radio` for why this is on request.
        radio: Option<Radio>,
        sys_hz: u32,
        firmware: &'static [u8],
        limits: &'static [u8],
        events: Subscription,
}

impl RadioMod {
        /// Hold what the radio will need, without touching it.
        pub fn new(sys_hz: u32, firmware: &'static [u8], limits: &'static [u8]) -> Self {
                Self { radio: None, sys_hz, firmware, limits, events: EVENTS.subscribe().expect("subscriber slot") }
        }

        /// Power the radio up with the firmware out of this board's asset pack.
        ///
        /// Slow on purpose, and blocking: a quarter of a megabyte goes over the bus before there
        /// is a radio at all, and there is nothing useful to do in the meantime.
        fn bring_up(&mut self) {
                if self.radio.is_some() {
                        info!("radio: already up");
                        return;
                }
                info!("radio: starting on {} bytes of image and {} of limits", self.firmware.len(), self.limits.len());
                let radio = Radio::new(PINS, self.sys_hz, DMA_CH, self.firmware, self.limits);
                let a = radio.address();
                //   READ OUT OF THE RUNNING RADIO, not out of the image: an address here is the
                // proof that the bus carried a quarter of a megabyte correctly and that what is
                // on the other end of it is now running
                info!("radio: up, address {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", a[0], a[1], a[2], a[3], a[4], a[5]);
                self.radio = Some(radio);
                //   lit once the radio is up, which on this board is the only way to light
                // anything at all -- and so is worth doing as a sign of life
                self.set_indicator(true);
        }

        /// Join a network, bringing the radio up first if it is not already.
        ///
        /// Asking to join is asking for a radio, so there is nothing to be gained by refusing
        /// until someone has said `radio` first.
        fn join(&mut self, c: &Credentials) {
                self.bring_up();
                let Some(radio) = self.radio.as_mut() else {
                        return;
                };
                //   the name, never the passphrase -- see Credentials
                info!("radio: joining {}", c.ssid());
                match radio.join(c.ssid(), c.pass()) {
                        Ok(()) => info!("radio: joined {}", c.ssid()),
                        //   the radio says which of the two it was in its own line above this
                        // one, so this does not guess between them
                        Err(JoinError::Refused) => warn!("radio: the join was refused -- no network called {}, or the wrong passphrase for it", c.ssid()),
                        Err(JoinError::NoAnswer) => warn!("radio: no answer from {} -- check the name, and that it is in range", c.ssid()),
                }
        }

        fn set_indicator(&mut self, on: bool) {
                if let Some(radio) = self.radio.as_mut() {
                        radio.set_gpio(INDICATOR, on);
                }
        }
}

impl Module for RadioMod {
        fn name(&self) -> &'static str {
                "radio"
        }

        fn poll(&mut self) -> Poll {
                let mut busy = false;
                while let Some(ev) = EVENTS.poll(&self.events) {
                        match ev {
                                AppEvent::Radio => {
                                        self.bring_up();
                                        busy = true;
                                }
                                AppEvent::Join(c) => {
                                        self.join(&c);
                                        busy = true;
                                }
                                AppEvent::Mounted(on) => {
                                        self.set_indicator(on);
                                        busy = true;
                                }
                                _ => {}
                        }
                }
                //   the radio's own work: the stack is written for an executor, and this is the
                // one pass of it a cooperative runtime gives every module
                if let Some(radio) = self.radio.as_mut() {
                        radio.poll();
                }
                if busy { Poll::Busy } else { Poll::Idle }
        }

        fn unload(&mut self) {
                self.set_indicator(false);
        }
}
