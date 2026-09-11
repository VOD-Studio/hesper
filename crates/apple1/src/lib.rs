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
//! with the reset vector at `$FFFC/D` pointing to `$FF00`). The original
//! Operation Manual schematics and printed pages have been page‑checked for
//! the address decode, RESET/CLEAR SCREEN wiring, and PIA register selection;
//! see `docs/references.md#apple-i` and `docs/apple1/hardware-evidence.md`
//! for the full source‑to‑implementation correlation matrix. The specific
//! four‑address PIA range omits documented address aliases (e.g. `$D014`
//! selects Port A on real hardware but returns open bus here — see H04).
//! Treat the RAM/ROM/PIA address ranges as a documented, reproducible
//! configuration; the PIA alias gap and open‑bus convention are known
//! simulation differences, not from‑schematic guarantees for every board
//! revision.
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
//! Board time is counted in *master ticks*, one 14.31818 MHz crystal period
//! each (see [`timing`]). D11 divides by 14 to produce the ~1.023 MHz
//! character clock; the D6/D7 horizontal counters give the 65-slot line
//! whose `H6 && H10` decode selects four refresh clocks. Like
//! `hesper-cpu6502` this crate does not throttle to wall‑clock time, but it
//! does not invent display timing either: a character is only taken when
//! the video board's carousel reaches the cursor's slot, so a display write
//! costs about a frame of board time.
//!
//! Every fourth of those 65 clocks suppresses the CPU's Φ2 output: the CPU
//! holds Φ2, the PIA sees no enable, and no bus access happens, while the
//! board clock, the video counters, and the B3 one-shot keep running.
//! `tick().cpu` is therefore `None` on those clocks.
//!
//! # RDY scope
//!
//! This fixed text-system configuration has no single-step or slow-ROM
//! expansion driving RDY. The CPU therefore stays ready; keyboard and
//! display waits are PIA handshakes polled by software, not CPU RDY stalls.
//!
//! The Apple-1 Operation Manual, Section III (DMA), documents RDY for
//! single-stepping and slow ROM applications. Its REFRESH section instead
//! describes suppressing Φ2 while holding Φ1, which is what [`timing`] and
//! [`machine`] model. Refresh must not be replaced by RDY stalls: RDY stops
//! the CPU for a device, refresh stops only Φ2 for the DRAM.
//!
//! The PIA model implements the register file (DDR, control, data, and interrupt
//! flags) per the MC6820/MC6821 datasheets, plus Port B's CB2 output
//! handshake — the write-strobe mode the Apple I terminal uses, in which an
//! ORB write pulls CB2 low on the next enable and CB1's active edge releases
//! it. Port A and Port B readback still share a symmetric
//! `(OR & DDR) | (pins & !DDR)` formula where the datasheets specify Port A
//! reads actual pins always and Port B reads the output latch in output mode
//! (see [`docs/apple1/hardware-evidence.md`] H12). External I/O (keyboard
//! data, the terminal's DA line) is wired in by [`machine`]; the host drives
//! keys through `Keyboard` rather than touching the PIA directly.
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
pub mod timing;

pub use bus::{Apple1Bus, RamLoadError, RomSizeError};
pub use display::Display;
pub use keyboard::Keyboard;
pub use machine::{Apple1, Tick};
pub use pia::Pia6821;
