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

use light_app_crossfire::{AppEvent, Credentials, FetchTarget, EVENTS};
use light_core::events::Subscription;
use light_core::{info, warn};
use light_core::module::{Module, Poll};
use light_rp2::update::FlashSlot;
use light_rp2::wifi::Pins;
//   THE DRIVER IS NAMED DIRECTLY, not reached through the port: it belongs to the radio part rather
// than to the chip, so the chip supplies only the bus it runs over. A board built around a different
// radio part changes this line and leaves the port alone.
use light_cyw43::{Incoming, JoinError, Radio, ScanEntry};
use light_update::Update;

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
        /// The module's own calibration and identity, which the driver wants beside the image.
        nvram: &'static [u8],
        /// The short-range radio's own patch, uploaded beside the other two.
        bt_firmware: &'static [u8],
        events: Subscription,
}

impl RadioMod {
        /// Hold what the radio will need, without touching it.
        pub fn new(sys_hz: u32, firmware: &'static [u8], limits: &'static [u8], nvram: &'static [u8], bt_firmware: &'static [u8]) -> Self {
                Self { radio: None, sys_hz, firmware, limits, nvram, bt_firmware, events: EVENTS.subscribe().expect("subscriber slot") }
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
                info!("radio: starting on {} bytes of image, {} of limits and {} of short-range patch", self.firmware.len(), self.limits.len(), self.bt_firmware.len());
                //   the chip's two contributions, taken together so the wrong pair cannot be made
                let (bus, pwr) = light_rp2::wifi::radio_bus(PINS, self.sys_hz, DMA_CH);
                let mut radio = Radio::new_with_bluetooth(bus, pwr, self.firmware, self.limits, self.nvram, self.bt_firmware);
                let a = radio.address();
                //   READ OUT OF THE RUNNING RADIO, not out of the image: an address here is the
                // proof that the bus carried a quarter of a megabyte correctly and that what is
                // on the other end of it is now running
                info!("radio: up, address {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", a[0], a[1], a[2], a[3], a[4], a[5]);
                //   and the same proof for the other radio in the same part, which has had its own
                // image and answers on its own protocol: a reset it accepted and a question it
                // answered. Reported rather than insisted on -- the wireless side is what this
                // board already depends on, and it should not be held back by the new one
                match radio.bluetooth_address() {
                        Some(b) => info!("radio: short-range up, address {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", b[0], b[1], b[2], b[3], b[4], b[5]),
                        None => warn!("radio: the short-range controller did not answer; wireless is unaffected"),
                }
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
                match radio.join(c) {
                        Ok(()) => info!("radio: joined {}", c.ssid()),
                        Err(JoinError::NotFound) => warn!("radio: no network called {} was found -- check the name, and that it is in range", c.ssid()),
                        //   NOT "worth trying again", which it was until the retry was found to
                        // stop the board: the part counts the attempt, not the outcome, so a second
                        // join needs the radio taken down and back up -- which here means a reset
                        //   TWO LINES, AND THE SECOND ONE EARNS ITS SPACE. This used to blame the
                        // passphrase alone, which cost a long evening: the passphrase was right and
                        // the network was a mixed WPA2/WPA3 one, which this radio does not join --
                        // it offers the newer exchange, the access point refuses it, and from here
                        // that is indistinguishable from a typo. Naming the other cause is the
                        // difference between checking a setting and doubting what you typed.
                        Err(JoinError::Refused) => {
                                warn!("radio: {} refused us; reset the board to try again", c.ssid());
                                warn!("radio: check the passphrase -- and that the network is not WPA3 or mixed WPA2/WPA3");
                        }
                        Err(JoinError::Other(code)) => warn!("radio: {} refused the join, reason {code}", c.ssid()),
                        Err(JoinError::NoAnswer) => warn!("radio: no answer from {} -- check the name, and that it is in range", c.ssid()),
                        //   a join has been tried once already, which may or may not have worked:
                        // either way the part will refuse the commands a second one needs, and the
                        // driver treats that refusal as fatal
                        Err(JoinError::AlreadyAttempted) => warn!("radio: a join has already been attempted on this power-up; reset the board to try another"),
                }
                //   NO ARMS FOR A MALFORMED NAME OR PASSPHRASE, because there is no longer any way to
                // reach here with one: the console refuses to build a set of credentials it could not
                // send, so what arrives is always something the part will at least accept as a
                // question. The two messages that used to stand here were carrying a check the type
                // system now carries.
        }

        /// Ask the joined network for an address, and say what it gave.
        fn address(&mut self) {
                let Some(radio) = self.radio.as_mut() else {
                        warn!("radio: there is no radio up to ask a network for an address");
                        return;
                };
                match radio.configure() {
                        Ok(c) => {
                                let a = c.address;
                                info!("radio: address {}.{}.{}.{}/{}", a[0], a[1], a[2], a[3], c.prefix);
                                match c.gateway {
                                        Some(g) => {
                                                info!("radio: the way out is {}.{}.{}.{}", g[0], g[1], g[2], g[3]);
                                        }
                                        //   worth saying: an address without one reaches the
                                        // local network and nothing beyond it, which is a fetch
                                        // that fails later for a reason nobody would look for here
                                        None => warn!("radio: no way out of this network was given -- only local addresses are reachable"),
                                }
                        }
                        Err(e) => warn!("radio: no address ({e:?}) -- has this radio joined a network, and did that network offer one?"),
                }
        }

        /// Fetch an image over the network and stage it into the slot that is not running.
        ///
        ///   NOTHING IS HELD ANYWHERE IN BETWEEN. The image is a megabyte and this part has no
        /// room for one, so each piece goes from the network into storage as it arrives and is
        /// not kept. That is also why the size has to be known before the first byte: the slot
        /// is erased to fit, and there is no second chance to ask.
        fn fetch(&mut self, t: &FetchTarget) {
                let Some(radio) = self.radio.as_mut() else {
                        warn!("radio: there is no radio up to fetch with");
                        return;
                };
                info!("radio: fetching {} from {}.{}.{}.{}:{}", t.path(), t.ip[0], t.ip[1], t.ip[2], t.ip[3], t.port);

                let started = light_core::log::now_us();
                let mut session: Option<Update<FlashSlot>> = None;
                let mut failed: Option<&'static str> = None;
                let mut sink = |incoming: Incoming| match incoming {
                        Incoming::Length(n) => {
                                let slot = match FlashSlot::inactive() {
                                        Ok(s) => s,
                                        Err(_) => {
                                                failed = Some("this board has no second slot to stage into");
                                                return false;
                                        }
                                };
                                //   ON APPROVAL, as `update` does: an image that arrives whole
                                // and then cannot do its job still has to expire on its own
                                match Update::begin_on_approval(slot, n) {
                                        Ok(u) => {
                                                session = Some(u);
                                                true
                                        }
                                        Err(_) => {
                                                failed = Some("the slot will not take an image that size");
                                                false
                                        }
                                }
                        }
                        Incoming::Body(b) => match session.as_mut() {
                                Some(u) => u.write(b).is_ok(),
                                None => false,
                        },
                };

                let outcome = radio.fetch(t.ip, t.port, t.path(), &mut sink);
                if let Some(why) = failed {
                        warn!("radio: {why}");
                        return;
                }
                let got = match outcome {
                        Ok(n) => n,
                        Err(e) => {
                                warn!("radio: the fetch failed ({e:?}) -- the slot is left unbootable, which is what stops it being started by mistake");
                                return;
                        }
                };

                let Some(session) = session else {
                        warn!("radio: nothing was staged");
                        return;
                };
                let staged = match session.finish() {
                        Ok(s) => s,
                        Err(e) => {
                                warn!("radio: what arrived is not usable ({e:?})");
                                return;
                        }
                };
                //   the number that decides whether any of this is fast enough to be worth
                // having, said out loud rather than guessed at
                let ms = (light_core::log::now_us() - started) / 1000;
                let rate = if ms > 0 { got as u64 * 1000 / ms / 1024 } else { 0 };
                info!("radio: {got} bytes fetched and staged in {ms} ms ({rate} KiB/s); starting it");

                let e = staged.boot();
                warn!("radio: the hardware would not start it ({e:?})");
        }

        /// List what the radio can see. Brings it up first, because asking an unpowered radio what
        /// it can see is not a question.
        ///
        ///   NEEDS NO PASSPHRASE, which is the point: it separates "that network is not there" from
        /// "this radio is not receiving", and those are the two explanations for a join that comes
        /// back saying it found nothing.
        fn scan(&mut self) {
                self.bring_up();
                let Some(radio) = self.radio.as_mut() else {
                        return;
                };
                info!("radio: looking for networks");
                let seen = radio.scan(|e: &ScanEntry| {
                        //   the signal and the channel as well as the name: a network that is there
                        // but barely audible looks identical to one that is absent if only the name
                        // is reported, and the channel says which band answered
                        //   and the address of the radio last, which is what tells two entries of
                        // the same name apart -- one network carried by two radios, not one network
                        // listed twice
                        let a = e.bssid;
                        let name = if e.hidden() { "(hidden)" } else { e.ssid() };
                        info!("radio:   {}, {} dBm, channel {}, {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", name, e.rssi, e.channel, a[0], a[1], a[2], a[3], a[4], a[5]);
                });
                match seen {
                        0 => warn!("radio: nothing at all was heard -- this radio is not receiving, whatever is in the room"),
                        //   RADIOS, NOT NETWORKS, because they are not the same count and the
                        // difference is visible right above: one name can be served by several
                        // radios, and one radio can serve several names
                        n => info!("radio: {} radios heard -- one network may be carried by several", n),
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
                                AppEvent::Address => {
                                        self.address();
                                        busy = true;
                                }
                                AppEvent::Scan => {
                                        self.scan();
                                        busy = true;
                                }
                                AppEvent::Fetch(t) => {
                                        self.fetch(&t);
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
