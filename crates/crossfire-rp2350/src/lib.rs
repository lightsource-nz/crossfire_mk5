//! What every crossfire on an RP2350 has in common, below the application and above the board:
//! the chip's boot facility as the application's [`Probation`], the module that replaces the
//! board's own firmware through the second application slot, and the signs of life with this
//! chip's console-core pulse in them. Two boards carry this chip -- the development board
//! with a display and a radio, and the production board with neither -- and neither owns any
//! of it.
//!
//! Nothing here names a pin. What a board wires is the board's crate; what the chip can do
//! is the port's; this is the application's use of the chip, written once.

#![no_std]

use light_app_crossfire::{AppEvent, Chunk, Probation, Receive, Vitals, EVENTS};
use light_core::events::Subscription;
use light_core::hal::UpdateError;
use light_core::module::{Module, Poll};
use light_core::{info, warn, ConstStaticCell};
use light_rp2::shell::BootInfo;
use light_rp2::update::{FlashSlot, COMMIT_SCRATCH_WORDS};
use light_update::wire::{ReceiveError, Receiver};
use light_update::{Update, UpdateTarget};

/// The boot ROM's flag for an image started on approval that has not yet bought itself
/// (`BOOT_TBYB_AND_UPDATE_FLAG_BUY_PENDING`): a candidate, discarded unless it commits.
const BUY_PENDING: u8 = 0x01;

/// Whether an image is on probation is the ROM's to say, and keeping it is the ROM's to do: this
/// is the RP2350's answer to both, for the application's probation checks.
pub struct RomProbation {
        candidate: bool,
}

impl RomProbation {
        pub fn new(boot: Option<BootInfo>) -> Self {
                Self { candidate: boot.is_some_and(|b| b.tbyb_and_update & BUY_PENDING != 0) }
        }
}

impl Probation for RomProbation {
        fn candidate(&self) -> bool {
                self.candidate
        }

        fn commit(&mut self) -> Result<(), UpdateError> {
                //   the scratch the commit borrows: two sectors, word-aligned, in .bss because it is
                // twice core 0's whole stack -- see COMMIT_SCRATCH_WORDS for why neither half is
                // negotiable. Taken once; the application commits at most once
                static SCRATCH: ConstStaticCell<[u32; COMMIT_SCRATCH_WORDS]> = ConstStaticCell::new([0; COMMIT_SCRATCH_WORDS]);
                let Some(scratch) = SCRATCH.try_take() else {
                        return Err(UpdateError::Refused);
                };
                //   THE COMMIT REWRITES THE FLASH THIS FIRMWARE RUNS FROM, and for its duration
                // nothing may fetch from it. Core 1 runs the console out of flash, so it is parked in
                // RAM through the SDK's lockout -- the mechanism the shell's BOOTSEL read already
                // uses on this chip -- and core 0's interrupts are held off, because every handler
                // it has (a host stack's, a display's DMA, the timer's) is in flash too. The
                // lockout comes FIRST: taken inside the critical section it could find core 1
                // waiting on the same lock with its interrupts off, never answering.
                //   A pause of tens of milliseconds, once, a few seconds after an update. A host
                // built out of PIO misses its frames for that long; the instruments on it see a
                // bus that went quiet and comes back, which is what a hub's upstream does on a
                // cable, and the stack re-enumerates what it must.
                // SAFETY: the SDK's lockout pair, which core 1 answers (the shell made it a victim
                // before launching it); balanced within this call
                unsafe { multicore_lockout_start_blocking() };
                let result = critical_section::with(|_| light_rp2::update::commit(scratch));
                unsafe { multicore_lockout_end_blocking() };
                result
        }
}

unsafe extern "C" {
        fn multicore_lockout_start_blocking();
        fn multicore_lockout_end_blocking();
}

/// The application's signs of life, with this chip's console-core pulse.
pub fn vitals() -> Vitals {
        light_app_crossfire::vitals(light_rp2::shell::core1_ticks())
}

/// Where the running image is addressed from once the bootloader has handed over: its slot,
/// rolled to the start of the execute-in-place window.
const RUNNING_IMAGE: *const u8 = 0x1000_0000 as *const u8;
/// How much is handed to the session at a time. Nothing to do with how storage is written -- the
/// session pages that itself -- only how much of the source is looked at per step.
const CHUNK: usize = 4096;

/// Replacing this board's firmware, on the hardware that can.
///
/// The application asks -- a console command, and one day a download that finished -- and this
/// answers, because only the hardware-bound side knows that this chip has a second application
/// slot and how to write it. The staging itself is `light_update`'s and has nothing to do with
/// where the image came from; what is here is the chip's answer to "with what?".
///
/// TWO SOURCES. `update` copies the image this board is running, read through the window the
/// bootloader rolled onto it, into the other slot -- a real thing to want (a fallback that is known
/// to boot), and an exercise of every step an arriving image takes. `receive` takes an image that
/// arrives over the console a line at a time, sent by the framework's update script: the wired
/// route, which is how a board with no radio is given new firmware without a debugger or a button.
/// Both end at the same session, the same slot and the same hand-over.
pub struct UpdateMod {
        events: Subscription,
        /// Lent to the boot ROM when this board keeps an image it was trying out: clearing the
        /// on-approval mark means rewriting the sector the mark sits in, and it wants the room.
        scratch: &'static mut [u32],
        /// An image arriving over the console, while one is.
        receiving: Option<Receiver<FlashSlot>>,
}

impl UpdateMod {
        pub fn new(scratch: &'static mut [u32]) -> Self {
                Self { events: EVENTS.subscribe().expect("subscriber slot"), scratch, receiving: None }
        }

        //   WHAT THE SCRIPT READS. Every answer to a step that leaves the session open is
        // `receive: next <offset> of <length>`, so the sender has one line to wait for whatever it
        // said; everything that ends a session without starting an image says `receive: failed`.
        // A person watching the console reads the same lines
        fn receive(&mut self, step: Receive) {
                match step {
                        Receive::Begin { len, crc } => self.receive_begin(len, crc),
                        Receive::Chunk(c) => self.receive_chunk(&c),
                        Receive::Status => match &self.receiving {
                                Some(rx) => {
                                        let (next, total) = rx.progress();
                                        info!("receive: next {next} of {total}");
                                }
                                None => {
                                        let running = light_rp2::shell::boot_info().and_then(|b| b.partition);
                                        info!("receive: idle; running partition {running:?}");
                                }
                        },
                        Receive::Cancel => match self.receiving.take() {
                                Some(rx) => {
                                        let (next, total) = rx.progress();
                                        drop(rx.abandon());
                                        info!("receive: cancelled at {next} of {total}; the slot is left unbootable");
                                }
                                None => info!("receive: idle; nothing to cancel"),
                        },
                }
        }

        fn receive_begin(&mut self, len: u32, crc: u32) {
                //   a new image replaces one half-sent: the sender that started that one has gone
                if let Some(rx) = self.receiving.take() {
                        let (next, total) = rx.progress();
                        info!("receive: giving up the image at {next} of {total} for a new one");
                        drop(rx.abandon());
                }
                let slot = match FlashSlot::inactive() {
                        Ok(slot) => slot,
                        Err(e) => {
                                warn!("receive: failed: no slot to receive into ({e:?}) -- is this board running through its bootloader?");
                                return;
                        }
                };
                //   the erase is the slow part, seconds rather than milliseconds, and the console is
                // quiet while it runs -- said first so that the quiet has an explanation
                info!("receive: erasing the slot for {len} bytes");
                //   ON APPROVAL, as `update` does: the image arrives whole and is started once, and
                // keeps itself only when the application has seen it work
                match Receiver::begin_on_approval(slot, len, crc) {
                        Ok(rx) => {
                                info!("receive: next 0 of {len}");
                                self.receiving = Some(rx);
                        }
                        Err(e) => warn!("receive: failed: could not start ({e:?})"),
                }
        }

        fn receive_chunk(&mut self, chunk: &Chunk) {
                let Some(rx) = self.receiving.as_mut() else {
                        warn!("receive: failed: no image is being received -- `receive <length> <crc32>` first");
                        return;
                };
                let total = rx.progress().1;
                match rx.take(chunk) {
                        Ok(next) if next < total => info!("receive: next {next} of {total}"),
                        Ok(_) => self.receive_finish(),
                        Err(e) => {
                                let next = rx.next();
                                warn!("receive: failed at {next} of {total} ({e:?}); the slot is left unbootable");
                                self.receiving = None;
                        }
                }
        }

        fn receive_finish(&mut self) {
                let Some(rx) = self.receiving.take() else { return };
                let total = rx.progress().1;
                let staged = match rx.finish() {
                        Ok(s) => s,
                        Err(ReceiveError::Checksum { expected, got }) => {
                                warn!("receive: failed: what arrived has CRC {got:08x}, not the {expected:08x} that was sent; nothing was started");
                                return;
                        }
                        Err(ReceiveError::Update(e)) => {
                                warn!("receive: failed: what was staged is not usable ({e:?})");
                                return;
                        }
                };
                info!("receive: {total} bytes staged and read back, CRC matches; starting it");
                //   the hardware verifies the image before it runs it and comes back to this one if
                // it does not like what it finds, so the worst outcome is that nothing changes
                let e = staged.boot();
                warn!("receive: failed: the hardware would not start it ({e:?})");
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
                                        //   one writer to the other slot at a time
                                        if self.receiving.is_some() {
                                                warn!("update: an image is being received over the console; `receive cancel` first");
                                        } else {
                                                self.stage();
                                        }
                                        busy = true;
                                }
                                AppEvent::Receive(step) => {
                                        self.receive(step);
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
