//! Apple I machine: MC6821 PIA device model and address‑decoded machine Bus.
//!
//! # Fixed configuration
//!
//! The real Apple I motherboard has a jumper area that lets a builder
//! assign each 4 KiB memory bank to RAM, ROM, or I/O; this crate fixes one
//! baseline configuration instead of modeling arbitrary jumpering:
//!
//! | Range           | Size   | Device                            |
//! |-----------------|--------|-----------------------------------|
//! | `$0000–$0FFF`   | 4 KiB  | RAM                               |
//! | `$D010–$D013`   | 4 B    | MC6821 PIA (keyboard + display)   |
//! | `$FF00–$FFFF`   | 256 B  | Woz Monitor ROM                   |
//! | everything else | —      | open bus                          |
//!
//! This is the commonly documented Apple I baseline (first 4 KiB RAM bank
//! at `$0000`, PIA at `$D010–$D013`, 256‑byte monitor ROM at `$FF00–$FFFF`
//! with the reset vector at `$FFFC/D` pointing to `$FF00`); see
//! `docs/references.md#apple-i` in this repository for the secondary
//! sources consulted. A from‑schematic primary‑source page citation has
//! not been completed; treat the specific addresses as a documented,
//! reproducible convention rather than a from‑schematic guarantee for
//! every board revision.
//!
//! ROM writes are silently ignored (read‑only ROM). Open bus deterministically
//! returns the last byte driven on the data bus — a **simulation
//! convention** chosen for reproducibility; real unmapped Apple I addresses
//! float and can return noise, which this crate does not model.
//!
//! # RESET and CLEAR SCREEN are separate inputs
//!
//! The Apple I keyboard has two pushbuttons, RESET and CLEAR SCREEN
//! (Apple-1 Operation Manual, Section I / Keyboard; see
//! `docs/references.md#apple-i`). They are unrelated hardware inputs:
//!
//! - RESET asserts the shared system reset line, which the 6502's `RES`
//!   pin and the MC6820/6821 PIA's own active‑low RESET pin (tied to the
//!   same line on the Apple I board) both sample. It clears the PIA's
//!   output/data‑direction/control registers (so both ports come up as
//!   inputs with interrupts disabled) and restarts the CPU from `$FFFC/D`.
//!   It does **not** clear the video screen, and it does not erase keys
//!   the user already typed ahead: neither the video board's
//!   shift‑register memory nor the external keyboard encoder is wired to
//!   that line. `Apple1::set_reset_line` models the pin; `Apple1::reset`
//!   is the synchronous convenience wrapper around it.
//! - CLEAR SCREEN is a video‑board input that blanks the 40x24 screen and
//!   homes the cursor, running no CPU cycle and changing no other machine
//!   state. `Apple1::clear_screen` models it as one functional action, not
//!   as a button pulse of a particular width.
//!
//! # Clock
//!
//! The real board derives a nominal 1.023 MHz CPU clock from a 14.31818 MHz
//! crystal (four times the NTSC color‑burst frequency) divided by 14. This
//! crate is purely cycle‑counted like `hesper-cpu6502`; it does not throttle
//! to wall‑clock time, and `cycles_per_char` approximates the video shift
//! register's data‑dependent ready timing with a fixed cycle count rather
//! than modeling the shift‑register position directly.
//!
//! The PIA model is a correct register file; external I/O (keyboard data,
//! display timing) is wired in by the host through `Pia6821` setter methods.
//!
//! The host is responsible for acquiring and loading the Woz Monitor ROM.
//! This crate never downloads, embeds, or ships that 256‑byte image; see
//! `crates/apple1/tests/data/README.md` for the resource and licensing
//! notes and the tests that require it.

pub mod bus;
pub mod display;
pub mod keyboard;
pub mod machine;
pub mod pia;

pub use bus::{Apple1Bus, RamLoadError, RomSizeError};
pub use display::Display;
pub use keyboard::Keyboard;
pub use machine::Apple1;
pub use pia::Pia6821;
