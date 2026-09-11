//! Host-side adapter for pinned SingleStepTests data; never part of the CPU library.

use std::{collections::BTreeSet, fs, path::Path};

use hesper_cpu6502::{Bus, Cpu, Ram, Registers, Status, StepKind};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[path = "trace.rs"]
pub mod trace;

pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/singlestep");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    repository: String,
    revision: String,
    variant: String,
    selection: String,
    files: Vec<File>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    opcode: String,
    source_sha256: String,
    source_count: usize,
    fixture_sha256: String,
    selected_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub pc: u16,
    pub s: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,
    pub ram: Vec<(u16, u8)>,
}

impl State {
    fn registers(&self) -> Registers {
        Registers {
            a: self.a,
            x: self.x,
            y: self.y,
            sp: self.s,
            pc: self.pc,
            status: Status::from_bits(self.p),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Read,
    Write,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub initial: State,
    #[serde(rename = "final")]
    pub final_state: State,
    pub cycles: Vec<(u16, u8, Direction)>,
}

pub fn parse_cases(bytes: &[u8], expected_count: usize) -> Result<Vec<Case>, String> {
    let cases: Vec<Case> =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    if cases.is_empty() || cases.len() != expected_count {
        return Err(format!(
            "case count: expected {expected_count}, got {}",
            cases.len()
        ));
    }
    for (index, case) in cases.iter().enumerate() {
        if !(2..=7).contains(&case.cycles.len()) {
            return Err(format!("case {index}: invalid official NMOS cycle count"));
        }
        for state in [&case.initial, &case.final_state] {
            let mut addresses = BTreeSet::new();
            for &(addr, _) in &state.ram {
                if !addresses.insert(addr) {
                    return Err(format!("case {index}: duplicate RAM address ${addr:04X}"));
                }
            }
        }
        if !case
            .initial
            .ram
            .iter()
            .any(|&(addr, _)| addr == case.initial.pc)
        {
            return Err(format!("case {index}: missing opcode at initial PC"));
        }
    }
    Ok(cases)
}

pub fn verify_hash(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(format!("SHA-256: expected {expected}, got {actual}"))
    }
}

struct ObservedBus {
    ram: Ram,
    allowed: BTreeSet<u16>,
    writes: Vec<u16>,
    events: Vec<(u16, u8, Direction)>,
    unlisted: Option<u16>,
}

impl ObservedBus {
    fn check_address(&mut self, addr: u16) {
        if !self.allowed.contains(&addr) && self.unlisted.is_none() {
            self.unlisted = Some(addr);
        }
    }
}

impl Bus for ObservedBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.check_address(addr);
        let value = self.ram.read(addr);
        self.events.push((addr, value, Direction::Read));
        value
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.check_address(addr);
        self.writes.push(addr);
        self.events.push((addr, value, Direction::Write));
        self.ram.write(addr, value);
    }
}

/// Compare externally supplied expectations; no CPU decoder generates an oracle.
pub fn execute_case(case: &Case, opcode: u8) -> Result<(), String> {
    if !case.initial.ram.contains(&(case.initial.pc, opcode)) {
        return Err("fixture opcode does not match its manifest".into());
    }
    let mut bus = ObservedBus {
        ram: Ram::new(),
        allowed: case
            .initial
            .ram
            .iter()
            .chain(&case.final_state.ram)
            .map(|&(a, _)| a)
            .collect(),
        writes: Vec::new(),
        events: Vec::new(),
        unlisted: None,
    };
    for &(addr, value) in &case.initial.ram {
        bus.ram.write(addr, value);
    }
    let mut cpu = Cpu::from_registers(case.initial.registers());
    let mut trace = trace::Trace::default();
    let result = (|| {
        // A finite cycle budget also catches accidental non-completing sequencers.
        let mut completed = None;
        for index in 0..7 {
            let cycle = cpu.cycle(&mut bus).map_err(|e| e.to_string())?;
            trace.record(cycle, &cpu);
            let direction = match cycle.bus.direction {
                hesper_cpu6502::Direction::Read => Direction::Read,
                hesper_cpu6502::Direction::Write => Direction::Write,
            };
            if bus.events.len() != index + 1
                || bus.events[index] != (cycle.bus.address, cycle.bus.data, direction)
            {
                return Err(format!(
                    "cycle {index}: returned transaction differs from actual Bus activity"
                ));
            }
            if cycle.completed.is_some() {
                completed = cycle.completed;
                break;
            }
        }
        let step = completed.ok_or("official instruction exceeded seven-cycle budget")?;
        if step.kind != (StepKind::Instruction { opcode }) {
            return Err(format!("unexpected step event: {:?}", step.kind));
        }
        if let Some(addr) = bus.unlisted {
            return Err(format!("access to RAM not listed by fixture: ${addr:04X}"));
        }
        let expected = &case.final_state;
        let actual = step.after;
        for (label, wanted, got) in [
            ("PC", expected.pc, actual.pc),
            ("A", u16::from(expected.a), u16::from(actual.a)),
            ("X", u16::from(expected.x), u16::from(actual.x)),
            ("Y", u16::from(expected.y), u16::from(actual.y)),
            ("SP", u16::from(expected.s), u16::from(actual.sp)),
            // Only the non-stored B/bit-5 representations are normalized.
            (
                "P (NV-DIZC)",
                u16::from(expected.p & 0xcf),
                u16::from(actual.status.bits() & 0xcf),
            ),
        ] {
            if wanted != got {
                return Err(format!("{label}: expected ${wanted:04X}, got ${got:04X}"));
            }
        }
        for &(addr, value) in &expected.ram {
            let actual = bus.ram.as_slice()[usize::from(addr)];
            if actual != value {
                return Err(format!(
                    "RAM ${addr:04X}: expected ${value:02X}, got ${actual:02X}"
                ));
            }
        }
        // Catch modifications omitted from final.ram, without scanning 64 KiB per case.
        for addr in bus.writes {
            let wanted = expected
                .ram
                .iter()
                .chain(&case.initial.ram)
                .find(|&&(a, _)| a == addr)
                .map(|&(_, value)| value)
                .unwrap_or(0);
            let got = bus.ram.as_slice()[usize::from(addr)];
            if wanted != got {
                return Err(format!(
                    "RAM ${addr:04X}: expected ${wanted:02X}, got ${got:02X}"
                ));
            }
        }
        if step.cycles != case.cycles.len() as u64 {
            return Err(format!(
                "cycles: expected {}, got {}",
                case.cycles.len(),
                step.cycles
            ));
        }
        if bus.events.len() != case.cycles.len() {
            return Err(format!(
                "bus event count: expected {}, got {}",
                case.cycles.len(),
                bus.events.len()
            ));
        }
        for (index, (expected, actual)) in case.cycles.iter().zip(&bus.events).enumerate() {
            if expected != actual {
                return Err(format!(
                    "bus cycle {index}: expected {expected:?}, got {actual:?}\nactual bus: {:?}",
                    bus.events
                ));
            }
        }
        Ok(())
    })();
    result.map_err(|reason: String| trace.failure(&reason, &cpu))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Options {
    pub full: bool,
    pub opcode: Option<u8>,
    pub case_index: Option<usize>,
}

#[derive(Debug)]
pub struct Report {
    pub revision: String,
    pub selection: String,
    pub files: usize,
    pub cases: usize,
}

pub fn run(directory: &Path, options: Options) -> Result<Report, String> {
    if options.case_index.is_some() && options.opcode.is_none() {
        return Err("--case-index requires --opcode".into());
    }
    let path = directory.join(if options.full {
        "full-manifest.json"
    } else {
        "manifest.json"
    });
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("manifest: {e}"))?;
    if manifest.repository != "https://github.com/SingleStepTests/65x02"
        || manifest.variant != "6502/v1"
        || manifest.revision.len() != 40
        || !manifest.revision.bytes().all(|b| b.is_ascii_hexdigit())
        || manifest.files.is_empty()
    {
        return Err("manifest must pin a nonempty NMOS 6502/v1 corpus to a full commit".into());
    }
    if options.full {
        let official: BTreeSet<_> = include_str!("../data/opcodes.txt")
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_ascii_lowercase)
            .collect();
        let listed: BTreeSet<_> = manifest
            .files
            .iter()
            .map(|file| file.opcode.clone())
            .collect();
        if official.len() != 151
            || listed != official
            || manifest.files.len() != 151
            || manifest.files.iter().any(|file| {
                file.source_count != 10_000
                    || file.selected_count != file.source_count
                    || file.fixture_sha256 != file.source_sha256
            })
        {
            return Err(
                "full manifest must contain all 151 official files, 10000 original cases each"
                    .into(),
            );
        }
    }
    let mut report = Report {
        revision: manifest.revision,
        selection: manifest.selection,
        files: 0,
        cases: 0,
    };
    let data_directory = if options.full {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.cache/cpu6502/singlestep")
            .join(&report.revision)
            .join("6502/v1")
    } else {
        directory.to_path_buf()
    };
    let replay_mode = if options.full { "--full " } else { "" };
    let mut seen = BTreeSet::new();
    for file in manifest.files {
        let opcode =
            u8::from_str_radix(&file.opcode, 16).map_err(|e| format!("manifest opcode: {e}"))?;
        if !seen.insert(opcode)
            || file.selected_count == 0
            || file.selected_count > file.source_count
            || file.source_sha256.len() != 64
            || !file.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(format!("invalid manifest entry for {opcode:02X}"));
        }
        if options.opcode.is_some_and(|selected| selected != opcode) {
            continue;
        }
        let path = data_directory.join(format!("{opcode:02x}.json"));
        let bytes = fs::read(&path).map_err(|e| {
            format!(
                "{}: {e}; prepare with bun tools/prepare_singlestep.ts --full",
                path.display()
            )
        })?;
        verify_hash(&bytes, &file.fixture_sha256)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let cases = parse_cases(&bytes, file.selected_count)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        report.files += 1;
        for (index, case) in cases.iter().enumerate() {
            if options.case_index.is_some_and(|selected| selected != index) {
                continue;
            }
            execute_case(case, opcode).map_err(|error| format!(
                "{} / {opcode:02X} case {index} {:?}: {error}\ninitial: {:?}\nreproduce: cargo run -p hesper-cpu6502 --example singlestep -- {replay_mode}--opcode {opcode:02X} --case-index {index}",
                report.revision, case.name, case.initial
            ))?;
            report.cases += 1;
        }
    }
    if report.cases == 0 {
        return Err(format!("selection matched zero cases: {options:?}"));
    }
    Ok(report)
}
