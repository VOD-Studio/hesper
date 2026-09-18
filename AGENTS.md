# Repository Guidelines

## Project Overview

Hesper is a Rust workspace for a cycle-accurate, machine-independent NMOS 6502 core, an Apple I machine layer above it, and several hosts: a CLI/TUI emulator center, Wasm bindings, and a React web workbench.

- `hesper-cpu6502` models official NMOS instructions, bus cycles, interrupts, RDY/SO, host reset, and physical RESET. It stays machine-independent and dependency-free.
- `hesper-apple1` owns the Apple I text system: address decoding and bus, PIA keyboard/display handshake, 8 KiB RAM plus optional `$1000–$1FFF` expansion RAM, the 40×24 character screen, and the digital video chain (2504 data tracks → C3/2519 six 40-stage row buffers → D2/2513 font ROM → D1/74166 pixel serialiser → composite sync).
- `hesper` (CLI) runs the built-in count demo, the interactive TUI launcher/config editor, and the `apple1` session with the bundled Woz Monitor ROM and 42 built-in program presets.
- `hesper-cpu6502-wasm` and `hesper-apple1-wasm` expose the same cores to JavaScript; `examples/web` is a private React + Vite workbench that drives the real Wasm in a Worker and is deployed to GitHub Pages by the release workflow.
- Current compatibility claims are deliberately narrow: fixed NMOS behavior, documented Visual6502 revD observations, and the Apple I subset documented in `docs/roadmap.md`. Do not silently add 65C02, NES 2A03, unofficial opcodes, Apple II (M4), or the still-open M3.4 electrical/power-on items.

## Architecture & Data Flow

Dependency direction is:

`crates/cli/src/main.rs` → `crates/cli/src/{lib,apple1,tui}.rs` → `crates/apple1/src/{machine,video}.rs` → `crates/cpu6502/src/lib.rs` → private decoder/cycle engine → host-owned `Bus`.

- `main.rs` dispatches `demo` / `tui` / `apple1`, prints bounded trace/output data, and handles exit. With no arguments, a usable TTY opens the TUI; otherwise the demo runs.
- The CLI library loads `DEMO_PROGRAM` into `Ram`, writes the reset vector, drives `Cpu` cycle by cycle, and stops at a host-defined address or step budget.
- `crates/cli/src/apple1.rs` owns machine *policy* (ROM/preset/local-program selection, load address, expansion RAM, cycle budget, terminal lifecycle, trace formatting); `hesper-apple1` owns the *machine* and never reads host files, terminals, or environment.
- `Cpu` does not own memory or devices. Every emulated access goes through `Bus::read`/`Bus::write`; reads may have side effects.
- `crates/cpu6502/src/instruction.rs` is the explicit private opcode/addressing/cycle table. `crates/cpu6502/src/cpu/cycle.rs` is the single cycle state machine used by `half_cycle`, `cycle`, and `step`.
- One sequencer phase means one real bus access. Preserve dummy reads, repeated RDY reads, RMW writes, pin-sampling points, and phase transitions; final-register equivalence alone is insufficient.
- `Cycle` exposes bus-level activity and optional completion. `Step` captures instruction/IRQ/NMI/RESET before/after state. `DebugState` and `Registers` are observations, not full savestates.
- `begin_reset`/`reset` are host-requested seven-cycle entry APIs. `set_reset_line` models sampled physical RESET. Never merge these semantics.
- The core returns data; it does not log or retain trace history. Host callbacks (`FnMut`) provide observation, and `&mut dyn Bus` is the device-injection seam. There is no async runtime, global state, or dependency-injection framework.
- Wasm hosts convert Rust observations to JS values through the internal `crates/wasm-support/`; they add no CPU or machine behavior. `tests/wasm/checks.mjs` is the single check list reused by Bun, Node.js, and Chrome, and `crates/apple1-wasm/examples/reference.rs` supplies native reference observations.
- The web workbench never reimplements emulation state: `crates/cli/examples/web_assets.rs` exports demo bytes and the preset catalogue, `examples/web/scripts/prepare.ts` writes them to ignored `src/generated/` and `public/machine/`, and the emulator runs in a Worker against the built Wasm packages.

## Key Directories

- `crates/cpu6502/src/`: dependency-free CPU library, bus abstraction, instruction decoder, and cycle engine.
- `crates/cpu6502/tests/`: integration tests grouped by arithmetic, official instructions, conformance, cycles, interrupts, external samples, and pin traces; shared host-only helpers live in `tests/support/`.
- `crates/cpu6502/tests/data/`: checked-in deterministic fixtures, manifests, provenance, licenses, and authoritative test commands.
- `crates/cpu6502/examples/`: bounded SingleStep, Klaus functional/decimal, and interrupt verification runners.
- `crates/apple1/src/`: machine, bus, PIA, keyboard, display, timing, and the `video/` digital chain.
- `crates/apple1/tests/`: `machine`, `timing`, `video`, and real-ROM `wozmon` integration tests; provenance and hashes in `tests/data/README.md`.
- `crates/cli/src/`: `lib.rs` demo host, `main.rs` dispatch, `apple1.rs` session policy, `tui.rs` launcher and config form, `terminal.rs` raw-mode/mouse lifecycle, `config.rs` and `presets.rs` host data.
- `crates/cli/assets/`: bundled `wozmon.bin` and the `programs/` preset library; `assets/README.md` carries the per-program compatibility matrix.
- `crates/cli/tests/`: library and real-binary CLI integration tests, including the Apple I session lifecycle.
- `crates/cli/examples/web_assets.rs`: asset export consumed by the web workbench.
- `crates/cpu6502-wasm/`, `crates/apple1-wasm/`: independent JS/Wasm hosts; shared JS conversion lives in internal `crates/wasm-support/`.
- `tests/wasm/`: public JS binding checks shared by Bun, Node.js and a real browser; native reference observations come from `apple1-wasm/examples/reference.rs`.
- `examples/web/`: private React + Vite + TypeScript workbench (`bun` scripts, `src/`, Worker, shadcn/ui components). Generated assets and `dist/` are ignored.
- `tools/`: Bun data preparation, Visual6502 reference replay, Wasm packaging/browser checks, and `release_notes.py` (Python standard library only). Prepared data belongs only under ignored `.cache/`.
- `tests/release/`: unittest regression for the release-notes extractor, run inside regular CI.
- `docs/`: architecture contracts, opcode scope, Apple I notes (`docs/apple1/`), Wasm guide, roadmap, source provenance, and chronological verification evidence.

## Development Commands

Root Cargo defaults target only `crates/cli`; use `--workspace` for repository-wide work.

```sh
cargo run -p hesper                              # 可用终端下进入 TUI；重定向时运行 demo
cargo run -p hesper -- demo                      # 显式内置演示，适合脚本/CI
cargo run -p hesper -- demo --trace
cargo run -p hesper -- demo --bus-trace --trace-limit 4
cargo run -p hesper -- apple1                     # Apple I 会话（内置 Woz Monitor）
cargo run -p hesper -- apple1 --preset <id>       # 载入内置程序；--preset 与 --program/--program-address 互斥
cargo run -p hesper -- apple1 --program <path> --program-address 0xE000 --expansion-ram
cargo run -p hesper -- apple1 --list-presets
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
cargo test -p hesper-apple1 --test wozmon
cargo run -p hesper-cpu6502 --example singlestep -- --opcode 69 --case-index 0
cargo run -p hesper-apple1 --release --example video -- /tmp/hesper-video-capture   # 目录须不存在
cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown
make tools-test                                  # bun test tools/ (Bun regression for the verification scripts)
python3 -m unittest discover -s tests/release    # release-notes extractor regression
```

`make full` is networked and expensive; reserve it for CPU semantic/timing, fixture, or release-validation changes. `make data` prepares its pinned inputs first.

`make tools-test` covers only `tools/*.test.ts` (4 files); it is not part of `make verify` because a cold cache performs real pinned downloads. `cargo test --workspace` does not run those Bun tests, but includes the real Woz Monitor tests using `crates/cli/assets/wozmon.bin`; no external ROM or download is required.

Wasm and web work have their own gates (see `docs/wasm.md` and `examples/web/README.md`):

```sh
make wasm-setup                                  # pinned wasm32 target + wasm-bindgen
make wasm                                        # CPU / Apple I / both binding libraries
make wasm-test                                   # build packages, then Bun + Node.js checks
make wasm-browser-test                           # wasm-test + TypeScript + real headless Chrome
cd examples/web && bun install --frozen-lockfile
bun run prepare:assets                           # regenerate ignored assets after host changes
bun run dev                                      # Vite dev server
bun run check                                    # typecheck + oxlint + real Wasm session tests
bun run build                                    # production site into dist/
```

## Code Conventions & Common Patterns

- Rust 2024; standard `rustfmt`; Clippy warnings are errors; workspace lint forbids `unsafe`.
- Keep the public facade narrow in `crates/cpu6502/src/lib.rs`. Decoder enums and in-flight execution state remain private unless an external contract genuinely requires them.
- Keep machine-independent CPU behavior in `hesper-cpu6502`; machine topology in `hesper-apple1`; loading, stop conditions, trace storage, formatting, terminal and CLI policy in hosts.
- Use explicit hardware-width types: `u8` for bytes/registers, `u16` for addresses, and `u64` for cycle/step budgets. Use wrapping arithmetic where hardware wraps.
- Hardware naming is explicit (`ClockPhase::Phi1`, `StepKind::Nmi`, `Phase::VectorLow`). Pin setters take `asserted`; `true` means asserted even for electrically active-low pins.
- Use small typed errors implementing `Display` and `Error`; propagate with `Result` and `?`. Preserve resumable state on `CycleBudgetExceeded`.
- Use `&mut dyn Bus` for device substitution and `FnMut` callbacks for tracing. Do not add async, shared ownership, registries, or runtime dependencies without a demonstrated need.
- Never inspect emulated devices through direct RAM access. `Ram::as_slice` is only side-effect-free host inspection of concrete `Ram`.
- Preserve bounded execution and bounded diagnostics. Fail on unsupported opcodes, malformed data, missing hashes, zero matches, and exhausted budgets rather than skipping.
- The Apple I screen surface is a fixed 40×24 character grid (1976 Operation Manual). Borders, titles, fonts, and mouse menus are emulator chrome, not hardware; digital-video-chain output is a separate proof from TUI rendering.
- Keyboard/display character flow must go through the CPU polling PIA; never forward host I/O directly to the display.
- Web changes keep the emulator in the Worker, treat `src/generated/` and `public/machine/` as build output, and preserve existing keyboard interactions when adding mouse/UI affordances.

## Important Files

- `Cargo.toml`: workspace membership, CLI default member, Rust 2024 metadata, pinned Wasm deps, and unsafe-code policy.
- `rust-toolchain.toml`: stable minimal toolchain with `rustfmt` and `clippy`; no numeric MSRV is declared.
- `Makefile`: canonical routine, Wasm, and full-validation commands.
- `crates/cpu6502/src/lib.rs`: supported public API boundary.
- `crates/cpu6502/src/bus.rs`: `Bus`, fixed 64 KiB `Ram`, and transactional non-wrapping host loads.
- `crates/cpu6502/src/cpu.rs`: architectural state, pins/latches, ALU behavior, public cycle/step/error types.
- `crates/cpu6502/src/cpu/cycle.rs`: cycle/half-cycle sequencing, stalls, interrupts, RESET, and completion snapshots.
- `crates/cpu6502/src/instruction.rs`: explicit official-opcode decode table.
- `crates/apple1/src/lib.rs` and `crates/apple1/src/machine.rs`: machine entry point and reset/clock-driven orchestration.
- `crates/cli/src/lib.rs`: demo bytes and bounded host runner shared by CLI tests.
- `crates/cli/src/main.rs`: binary entry point, subcommand parsing, trace buffering, and exit handling.
- `crates/cli/src/tui.rs`: launcher, focused config form, mouse/keyboard interaction, and session controls.
- `crates/cli/assets/README.md`: preset provenance, hashes, licences, and the per-program compatibility matrix.
- `crates/cpu6502/tests/data/README.md` and `crates/apple1/tests/data/README.md`: current fixture scope, exact commands, hashes, licenses, and success criteria.
- `tools/release_notes.py`: changelog-section extractor that authors the Release notes.
- `docs/architecture.md`: behavioral contracts and compatibility boundaries, including the Apple I layer.
- `docs/roadmap.md`: milestone status — which Apple I items are accepted and which remain open (M3.4, M4, full browser acceptance).
- `docs/wasm.md`: Wasm build/package and multi-runtime verification contracts.
- `docs/verification.md`: chronological evidence; use the newest relevant section because older sections preserve historical limitations.

## Runtime/Tooling Preferences

- Use the checked-in stable Rust toolchain. Keep `Cargo.lock` current and prefer `--locked`; CI fetches once, then runs normal Rust checks offline.
- The CPU and Apple I crates have no runtime dependencies. `serde`, `serde_json`, and `sha2` are test/example-only dependencies.
- Bun is the driver for `tools/` and `tests/wasm/` verification and the dev/build/test toolchain for `examples/web`; it is not an application runtime for the emulation cores. Full CI uses Bun 1.4.0; Klaus preparation also needs `make` and a C compiler.
- The Rust workspace has no `package.json` and must not gain one. The only npm-style project is the private `examples/web` app, pinned by `examples/web/bun.lock`. No custom Cargo config, no rustfmt/Clippy config files, no feature matrix, and no declared MSRV exist — do not invent one.
- `wasm32-unknown-unknown` is optional locally. `make wasm` checks the CPU, Apple I and both binding libraries; `make wasm-setup` installs the target and pinned binding CLI. `make wasm-browser-test` builds the actual JS/Wasm packages and runs Bun/Node.js, TypeScript and Chrome checks.
- Saved TUI configuration is fixed at `~/.config/hesper/config.toml` and deliberately does not follow `XDG_CONFIG_HOME`.
- Do not create scaffolding for milestones the task has not entered (Apple II, disk/cartridge interfaces, machine savestates, plugin systems). The Apple I layer, Wasm bindings, and web workbench already exist — extend them in place.

## Testing & QA

- All Rust crates share `workspace.package.version`. Record user-visible changes under `Unreleased` in root `CHANGELOG.md`, including affected components and compatibility changes. Follow README's tag-driven release checklist; `.github/workflows/release.yml` validates tag/changelog agreement, runs regular CI plus full CPU conformance, builds six-platform CLI and four Wasm packages, publishes the Release with SHA-256 sums, and then deploys `examples/web` to GitHub Pages. The final job deploys in the `github-pages` environment, so its deployment-protection policy must allow the pushed `v*` tag; a policy failure there is fixed and rerun without moving the tag. Manual dispatch is build-only and never touches a Release; a published Release is refused rather than overwritten. Never move published tags. Validation evidence belongs in `docs/verification.md`, not in place of a changelog.

- Tests use Rust’s built-in integration-test harness; follow existing targets and reuse `crates/cpu6502/tests/support/` and `crates/apple1/tests/support/` rather than adding another framework.
- Routine regression is deterministic and offline after Cargo dependencies are fetched: run both debug and release workspace tests. Checked-in coverage includes arithmetic/boundary checks, official-opcode specification, sampled SingleStep cases, CPU comparisons against pin and physical-RESET traces, Apple I memory map/PIA handshake/refresh/screen/RESET regressions, real Woz Monitor interaction, and CLI/TUI input, layout, preset-selection, and session-lifecycle tests.
- Terminal QA is layered and the layers do not substitute for each other: `ratatui` TestBackend and unit tests, content-ready PTY assertions (wait for `HESPER`, a full prompt, or a complete dump line — never "output paused for a while"), and real-terminal interaction at the layouts already evidenced in `docs/verification.md` (44×30 minimum, 80×30, 120×40, 180×50, plus shrink-and-restore). A library-API or non-TTY-pipe pass is not real-terminal acceptance.
- Regular CI (`.github/workflows/ci.yml`) runs the release-metadata unittest, the workspace checks, `make wasm`, and `make wasm-browser-test`; it does **not** typecheck or build `examples/web`, which only the release `pages` job builds. Web changes must therefore be validated locally with `bun run check` and `bun run build`.
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
- Bundling 42 presets is not compatibility: cite `crates/cli/assets/README.md` for what each program is actually proven to do, and keep loading-range validation separate from functional acceptance.
- Report local runs and remote CI separately. Workflow presence is not evidence that CI passed, and a pushed tag is not evidence that a Release or Pages deployment succeeded.
