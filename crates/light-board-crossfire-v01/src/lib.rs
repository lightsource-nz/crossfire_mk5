//! The board-support crate for crossfire revision 0.1, the production board: everything
//! specific to this board but independent of which firmware runs on it. [`board`] is the pin
//! wiring and the peripheral hand-over.
//!
//! There is no power mechanism here: the board has no backlight, no battery and no power
//! button, and runs from its USB-C input alone. The shell ABI glue is framework-level, in
//! [`light_rp2::shell`], so this crate holds only what is genuinely the board's.

#![no_std]

pub mod board;
