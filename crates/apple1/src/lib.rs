//! Apple I machine: MC6821 PIA device model and address‑decoded machine Bus.
//!
//! # Memory map
//!
//! | Range           | Size   | Device                            |
//! |-----------------|--------|-----------------------------------|
//! | `$0000–$0FFF`   | 4 KiB  | RAM                               |
//! | `$D010–$D013`   | 4 B    | MC6821 PIA (keyboard + display)   |
//! | `$FF00–$FFFF`   | 256 B  | Woz Monitor ROM                   |
//!
//! Everything else is open bus.  ROM writes are silently ignored.
//! The PIA model is a correct register file; external I/O (keyboard data,
//! display timing) is wired in by the host through `Pia6821` setter methods.
//!
//! The host is responsible for acquiring and loading the Woz Monitor ROM
//! (256 bytes, not included in this crate).

pub mod bus;
pub mod pia;

pub use bus::{Apple1Bus, RomSizeError};
pub use pia::Pia6821;
