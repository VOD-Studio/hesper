use std::{env, error::Error, fmt::Write as _, process::ExitCode};

use hesper::{DEFAULT_MAX_STEPS, run_demo};
use hesper_cpu6502::{Registers, StepKind};

fn registers(state: Registers) -> String {
    format!(
        "A={:02X} X={:02X} Y={:02X} SP={:02X} PC={:04X} P={:02X}",
        state.a,
        state.x,
        state.y,
        state.sp,
        state.pc,
        state.status.bits()
    )
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut trace = false;
    let mut max_steps = DEFAULT_MAX_STEPS;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--trace" => trace = true,
            "--max-steps" => {
                max_steps = args
                    .next()
                    .ok_or("--max-steps requires an unsigned integer")?
                    .parse()
                    .map_err(|_| "--max-steps requires an unsigned integer")?;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: hesper [--trace] [--max-steps N]\nRun the built-in NMOS 6502 count demo. Default limit: {DEFAULT_MAX_STEPS} instructions."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}; use --help").into()),
        }
    }
    let result = run_demo(max_steps, |step, total| {
        if trace {
            let event = match step.kind {
                StepKind::Instruction { opcode } => format!("{opcode:02X}"),
                StepKind::Irq => "IRQ".to_owned(),
                StepKind::Nmi => "NMI".to_owned(),
                StepKind::Reset => "RESET".to_owned(),
            };
            println!(
                "${:04X} {event} | {} -> {} | +{} cycles total={total}",
                step.address,
                registers(step.before),
                registers(step.after),
                step.cycles
            );
        }
    })?;
    let mut output = String::new();
    for value in &result.ram.as_slice()[0x0200..0x020a] {
        write!(output, " {value}")?;
    }
    println!("$0200..$0209:{output}");
    println!("{}", registers(result.registers));
    println!(
        "Completed: {} instructions, {} instruction cycles + {} reset cycles = {} total cycles",
        result.steps,
        result.instruction_cycles,
        result.reset_cycles,
        result.instruction_cycles + u64::from(result.reset_cycles)
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("hesper: {err}");
            ExitCode::FAILURE
        }
    }
}
