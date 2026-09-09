//! Bounded host execution of the pinned Klaus Dormann NMOS functional image.

use std::{env, fs, path::Path, process::ExitCode};

use hesper_cpu6502::{Bus, Cpu, Ram, Registers, Status, Step};
use sha2::{Digest, Sha256};

#[path = "../tests/support/trace.rs"]
pub mod trace;

fn traced_step(trace: &mut trace::Trace, cpu: &mut Cpu, bus: &mut dyn Bus) -> Result<Step, String> {
    for _ in 0..7 {
        let cycle = cpu
            .cycle(bus)
            .map_err(|e| trace.failure(&e.to_string(), cpu))?;
        trace.record(cycle, cpu);
        if let Some(step) = cycle.completed {
            return Ok(step);
        }
    }
    Err(trace.failure("seven-cycle step budget exceeded", cpu))
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    let decimal = match args.as_slice() {
        [] => false,
        [arg] if arg == "--decimal" => true,
        [arg] if arg == "--help" || arg == "-h" => {
            println!(
                "Usage: functional [--decimal]\nPinned Klaus functional or Bruce Clark exhaustive NMOS decimal test."
            );
            return Ok(());
        }
        _ => return Err("expected no arguments or --decimal".into()),
    };
    let (name, filename, hash, load, entry, success) = if decimal {
        (
            "Bruce Clark NMOS decimal (all A/N/V/Z/C checks, invalid BCD included)",
            "decimal.bin",
            "03798ab778456cc350044fdbe28b4078278648892712b994cdbdda09018674e7",
            0x0200,
            0x0200,
            0x024b,
        )
    } else {
        (
            "Klaus NMOS functional",
            "bin_files/6502_functional_test.bin",
            "fa12bfc761e6f9057e4cc01a665a7b800ff01ae91f598af1e39a1201d01953fd",
            0,
            0x0400,
            0x3469,
        )
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.cache/cpu6502/klaus/7954e2dbb49c469ea286070bf46cdd71aeb29e4b")
        .join(filename);
    let image = fs::read(&path).map_err(|error| {
        format!(
            "{}: {error}; prepare with python3 tools/prepare_klaus.py",
            path.display()
        )
    })?;
    let digest = format!("{:x}", Sha256::digest(&image));
    if digest != hash {
        return Err(format!("{name} image SHA-256 mismatch: {digest}"));
    }
    let mut ram = Ram::new();
    ram.load(load, &image).map_err(|e| e.to_string())?;
    // The upstream monitor-entry contract starts at $0400, not through RESET.
    let mut cpu = Cpu::from_registers(Registers {
        pc: entry,
        sp: 0xfd,
        status: Status::from_bits(0x20),
        ..Registers::default()
    });
    let mut cycles = 0_u64;
    let mut trace = trace::Trace::default();
    for steps in 0..=100_000_000_u64 {
        if cpu.registers().pc == success {
            // Stop before the upstream end macro ($DB). It is not NMOS HALT.
            if decimal && ram.as_slice()[0x000b] != 0 {
                return Err(trace.failure(
                    &format!(
                        "decimal ERROR=$01 at DONE: {:?}; variables={:02X?}",
                        cpu.registers(),
                        &ram.as_slice()[..17]
                    ),
                    &cpu,
                ));
            }
            println!(
                "{name}: passed at ${success:04X} after {steps} instructions / {cycles} cycles"
            );
            return Ok(());
        }
        if steps == 100_000_000 || cycles >= 400_000_000 {
            break;
        }
        let step = traced_step(&mut trace, &mut cpu, &mut ram)?;
        cycles += step.cycles;
        if step.before.pc == step.after.pc {
            return Err(trace.failure(
                &format!(
                    "functional failure trap: {step:?}; instructions={steps}; cycles={cycles}"
                ),
                &cpu,
            ));
        }
    }
    Err(trace.failure("functional instruction/cycle budget exceeded", &cpu))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("functional: {error}");
            ExitCode::FAILURE
        }
    }
}
