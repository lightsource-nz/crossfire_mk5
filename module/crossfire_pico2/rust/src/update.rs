//! Replacing this board's firmware, on the hardware that can.
//!
//! The application asks -- a console command, and one day a download that finished -- and this
//! answers, because only the hardware-bound side knows that this chip has a second application
//! slot and how to write it. The staging itself is `light_update`'s and has nothing to do with
//! where the image came from; what is here is the board's answer to "with what?".
//!
//! WHAT IT COPIES TODAY IS ITSELF. There is no transport yet: the source is the image this board
//! is running, read through the window the bootloader rolled onto it, and the result is the same
//! firmware in the other slot -- which is a real thing to want (a fallback that is known to boot)
//! and, more to the point, exercises every step an arriving image will take. When a transport
//! arrives it replaces these few lines and nothing else: the session, the slot and the hand-over
//! are already what they will be.

use light_app_crossfire::{AppEvent, EVENTS};
use light_core::events::Subscription;
use light_core::module::{Module, Poll};
use light_core::{info, warn};
use light_rp2::update::FlashSlot;
use light_update::{Update, UpdateTarget};

/// Where the running image is addressed from once the bootloader has handed over: its slot,
/// rolled to the start of the execute-in-place window.
const RUNNING_IMAGE: *const u8 = 0x1000_0000 as *const u8;
/// How much is handed to the session at a time. Nothing to do with how storage is written -- the
/// session pages that itself -- only how much of the source is looked at per step.
const CHUNK: usize = 4096;

pub struct UpdateMod {
        events: Subscription,
        /// Lent to the boot ROM when this board keeps an image it was trying out: clearing the
        /// on-approval mark means rewriting the sector the mark sits in, and it wants the room.
        scratch: &'static mut [u32],
}

impl UpdateMod {
        pub fn new(scratch: &'static mut [u32]) -> Self {
                Self { events: EVENTS.subscribe().expect("subscriber slot"), scratch }
        }

        fn commit(&mut self) {
                match light_rp2::update::commit(self.scratch) {
                        Ok(()) => info!("update: this firmware is kept; a reset no longer goes back"),
                        Err(e) => warn!("update: could not keep this firmware ({e:?})"),
                }
        }

        fn stage(&mut self) {
                let slot = match FlashSlot::inactive() {
                        Ok(slot) => slot,
                        Err(e) => {
                                warn!("no slot to update into ({e:?}) -- is this board running through its bootloader?");
                                return;
                        }
                };
                let len = slot.capacity();
                info!("update: staging {} bytes into the slot that is not running", len);

                //   the source is flash, and the session's page buffer is RAM -- which matters:
                // storage cannot be read while it is being written, so what is handed to a write
                // must already be out of it
                let source = unsafe { core::slice::from_raw_parts(RUNNING_IMAGE, len as usize) };
                //   ON APPROVAL: the staged image gets one run, and the next reset goes back to
                // this one unless it keeps itself. That is the whole point of doing it here --
                // an image that boots and then cannot do its job is the failure the hardware's own
                // verification does not catch, and an image on approval answers it by expiring.
                // The keeping is `commit` below, and it is a console command rather than something
                // this does on its own, because "it started" is not yet "it works".
                let mut session = match Update::begin_on_approval(slot, len) {
                        Ok(s) => s,
                        Err(e) => {
                                warn!("update: could not start ({e:?})");
                                return;
                        }
                };
                let started = light_core::log::now_us();
                for chunk in source.chunks(CHUNK) {
                        if let Err(e) = session.write(chunk) {
                                let (done, total) = session.progress();
                                warn!("update: failed at {done} of {total} bytes ({e:?}) -- the slot is left unbootable, which is what stops it being started by mistake");
                                return;
                        }
                }
                let staged = match session.finish() {
                        Ok(s) => s,
                        Err(e) => {
                                warn!("update: what was staged is not usable ({e:?})");
                                return;
                        }
                };
                info!("update: {} bytes staged and read back in {} ms; starting it", staged.len(), (light_core::log::now_us() - started) / 1000);

                //   the hardware verifies the image before it runs it and falls back to the slot
                // this message came from if it does not like what it finds, so the worst outcome
                // here is that nothing changes
                let e = staged.boot();
                warn!("update: the hardware would not start it ({e:?})");
        }
}

impl Module for UpdateMod {
        fn name(&self) -> &'static str {
                "update"
        }

        fn poll(&mut self) -> Poll {
                let mut busy = false;
                while let Some(ev) = EVENTS.poll(&self.events) {
                        match ev {
                                AppEvent::Update => {
                                        self.stage();
                                        busy = true;
                                }
                                AppEvent::Commit => {
                                        self.commit();
                                        busy = true;
                                }
                                _ => {}
                        }
                }
                if busy { Poll::Busy } else { Poll::Idle }
        }
}
