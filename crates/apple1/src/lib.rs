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
//! # RESET is not clear‑screen
//!
//! On real Apple I hardware the RESET pushbutton only asserts the shared
//! system reset line, which the 6502's `RES` pin and the MC6820/6821 PIA's
//! own active‑low RESET pin (tied to the same line on the Apple I board)
//! both sample. Asserting it clears the PIA's control/data‑direction
//! registers (so both ports come up as inputs and interrupts disabled) and
//! restarts the CPU from `$FFFC/D` — it does **not** clear the video
//! screen or the external keyboard's already‑queued keys; the Apple I has
//! no separate clear‑screen hardware input at all. `Apple1::reset` models
//! the CPU+PIA reset tied to the physical `RES`/PIA‑reset line; a host
//! wanting to present a cleared terminal does so at the presentation layer,
//! not as a machine input (see `crates/cli/src/apple1.rs`).
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

pub use bus::{Apple1Bus, RomSizeError};
pub use display::Display;
pub use keyboard::Keyboard;
pub use machine::Apple1;
pub use pia::Pia6821;
