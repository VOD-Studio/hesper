# Repository Guidelines

## Project Overview

Hesper is a Rust workspace for a cycle-accurate, machine-independent NMOS 6502 core plus a small host CLI demo.

- `hesper-cpu6502` models official NMOS instructions, bus cycles, interrupts, RDY/SO, host reset, and physical RESET.
- `hesper` loads and runs the built-in count demo, owns execution limits and tracing, and formats terminal output.
- Current compatibility claims are deliberately narrow: fixed NMOS behavior and documented Visual6502 revD observations. Do not silently add 65C02, NES 2A03, unofficial-opcode, or future Apple-machine behavior.

## Architecture & Data Flow

Dependency direction is:

`crates/cli/src/main.rs` → `crates/cli/src/lib.rs` → `crates/cpu6502/src/lib.rs` → private decoder/cycle engine → host-owned `Bus`.

- `main.rs` parses CLI arguments and prints bounded trace/output data.
- The CLI library loads `DEMO_PROGRAM` into `Ram`, writes the reset vector, drives `Cpu` cycle by cycle, and stops at a host-defined address or step budget.
- `Cpu` does not own memory or devices. Every emulated access goes through `Bus::read`/`Bus::write`; reads may have side effects.
- `crates/cpu6502/src/instruction.rs` is the explicit private opcode/addressing/cycle table. `crates/cpu6502/src/cpu/cycle.rs` is the single cycle state machine used by `half_cycle`, `cycle`, and `step`.
- One sequencer phase means one real bus access. Preserve dummy reads, repeated RDY reads, RMW writes, pin-sampling points, and phase transitions; final-register equivalence alone is insufficient.
- `Cycle` exposes bus-level activity and optional completion. `Step` captures instruction/IRQ/NMI/RESET before/after state. `DebugState` and `Registers` are observations, not full savestates.
- `begin_reset`/`reset` are host-requested seven-cycle entry APIs. `set_reset_line` models sampled physical RESET. Never merge these semantics.
- The core returns data; it does not log or retain trace history. Host callbacks (`FnMut`) provide observation, and `&mut dyn Bus` is the device-injection seam. There is no async runtime, global state, or dependency-injection framework.

## Key Directories

- `crates/cpu6502/src/`: dependency-free CPU library, bus abstraction, instruction decoder, and cycle engine.
- `crates/cpu6502/tests/`: integration tests grouped by arithmetic, official instructions, cycles, interrupts, external samples, and pin traces; shared host-only helpers live in `tests/support/`.
- `crates/cpu6502/tests/data/`: checked-in deterministic fixtures, manifests, provenance, licenses, and authoritative test commands.
- `crates/cpu6502/examples/`: bounded SingleStep, Klaus functional/decimal, and interrupt verification runners.
- `crates/cli/src/`: demo host library and binary presentation layer.
- `crates/cli/tests/`: library and real-binary CLI integration tests.
- `tools/`: Bun data preparation and Visual6502 reference replay. Prepared data belongs only under ignored `.cache/cpu6502/`.
- `docs/`: architecture contracts, opcode scope, source provenance, roadmap, and chronological verification evidence.

## Development Commands

Root Cargo defaults target only `crates/cli`; use `--workspace` for repository-wide work.

```sh
cargo run -p hesper                              # built-in demo
cargo run -p hesper -- --trace
cargo run -p hesper -- --bus-trace --trace-limit 4
cargo fmt --all                                  # rewrite formatting
cargo check --workspace --all-targets
cargo test --workspace
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
make verify                                      # complete routine local gate
```

Useful focused commands:

```sh
cargo test -p hesper-cpu6502 --test pins
cargo test -p hesper-cpu6502 --test external
cargo run -p hesper-cpu6502 --example singlestep -- --opcode 69 --case-index 0
cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown
```

`make full` is networked and expensive; reserve it for CPU semantic/timing, fixture, or release-validation changes. `make data` prepares its pinned inputs first.

## Code Conventions & Common Patterns

- Rust 2024; standard `rustfmt`; Clippy warnings are errors; workspace lint forbids `unsafe`.
- Keep the public facade narrow in `crates/cpu6502/src/lib.rs`. Decoder enums and in-flight execution state remain private unless an external contract genuinely requires them.
- Keep machine-independent CPU behavior in `hesper-cpu6502`; loading, devices, stop conditions, trace storage, formatting, and CLI policy belong in hosts.
- Use explicit hardware-width types: `u8` for bytes/registers, `u16` for addresses, and `u64` for cycle/step budgets. Use wrapping arithmetic where hardware wraps.
- Hardware naming is explicit (`ClockPhase::Phi1`, `StepKind::Nmi`, `Phase::VectorLow`). Pin setters take `asserted`; `true` means asserted even for electrically active-low pins.
- Use small typed errors implementing `Display` and `Error`; propagate with `Result` and `?`. Preserve resumable state on `CycleBudgetExceeded`.
- Use `&mut dyn Bus` for device substitution and `FnMut` callbacks for tracing. Do not add async, shared ownership, registries, or runtime dependencies without a demonstrated need.
- Never inspect emulated devices through direct RAM access. `Ram::as_slice` is only side-effect-free host inspection of concrete `Ram`.
- Preserve bounded execution and bounded diagnostics. Fail on unsupported opcodes, malformed data, missing hashes, zero matches, and exhausted budgets rather than skipping.

## Important Files

- `Cargo.toml`: workspace membership, CLI default member, Rust 2024 metadata, and unsafe-code policy.
- `rust-toolchain.toml`: stable minimal toolchain with `rustfmt` and `clippy`; no numeric MSRV is declared.
- `Makefile`: canonical routine and full-validation commands.
- `crates/cpu6502/src/lib.rs`: supported public API boundary.
- `crates/cpu6502/src/bus.rs`: `Bus`, fixed 64 KiB `Ram`, and transactional non-wrapping host loads.
- `crates/cpu6502/src/cpu.rs`: architectural state, pins/latches, ALU behavior, public cycle/step/error types.
- `crates/cpu6502/src/cpu/cycle.rs`: cycle/half-cycle sequencing, stalls, interrupts, RESET, and completion snapshots.
- `crates/cpu6502/src/instruction.rs`: explicit official-opcode decode table.
- `crates/cli/src/lib.rs`: demo bytes and bounded host runner shared by CLI tests.
- `crates/cli/src/main.rs`: binary entry point, argument parsing, trace buffering, and exit handling.
- `crates/cpu6502/tests/data/README.md`: current fixture scope, exact commands, hashes, licenses, and success criteria.
- `docs/architecture.md`: behavioral contracts and compatibility boundaries.
- `docs/verification.md`: chronological evidence; use the newest relevant section because older sections preserve historical limitations.

## Runtime/Tooling Preferences

- Use the checked-in stable Rust toolchain. Keep `Cargo.lock` current and prefer `--locked`; CI fetches once, then runs normal Rust checks offline.
- The CPU crate has no runtime dependencies. `serde`, `serde_json`, and `sha2` are test/example-only dependencies.
- Bun is a host verification tool, not an application runtime. Full CI uses Bun 1.4.0; Klaus preparation also needs `make` and a C compiler.
- No `package.json`, custom Cargo config, custom rustfmt/Clippy config, feature matrix, or declared MSRV exists. Do not invent one.
- `wasm32-unknown-unknown` is optional locally and checks only the CPU library; install the target before running `make wasm`.
- Do not create future machine/browser/plugin scaffolding unless the task explicitly enters that roadmap milestone.

## Testing & QA

- Tests use Rust’s built-in integration-test harness; follow existing targets and reuse `crates/cpu6502/tests/support/` rather than adding another framework.
- Routine regression is deterministic and offline after Cargo dependencies are fetched: run both debug and release workspace tests. Checked-in coverage includes arithmetic/boundary checks, official-opcode specification, sampled SingleStep cases, and CPU comparisons against both pin and physical-RESET traces.
- For changed behavior, run the focused integration target first, then `make verify`. CPU timing, pin, interrupt, or RESET changes also require the relevant full external layer.
- Full conformance:

```sh
make data
make full
# or run individual Make targets: singlestep functional decimal interrupt visual6502 pins
```

- Keep two proofs distinct: `bun tools/verify_visual6502.ts` reproduces tracked observations using the pinned upstream model; `cargo test -p hesper-cpu6502 --test pins --release` compares Hesper with those observations. Neither substitutes for the other.
- Diagnose narrowly with `--opcode ... --case-index ...` or `bun tools/verify_visual6502.ts --suite ... --case ...`; never report a targeted pass as full-corpus validation.
- Do not regenerate expected fixtures to hide mismatches. Preserve pinned revisions, SHA-256 checks, fixture counts, exact success addresses, cycle budgets, bus comparisons, and replay commands.
- Klaus interrupt conformance is verified with `--feedback-delay 4`. Zero delay reaches a documented NMOS trap and must remain a failure, not be suppressed or called passing.
- Report local runs and remote CI separately. Workflow presence is not evidence that CI passed.
