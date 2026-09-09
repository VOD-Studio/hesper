use std::{collections::VecDeque, env, error::Error, fmt::Write as _, process::ExitCode};

use hesper::{DEFAULT_MAX_STEPS, DemoEvent, run_demo_with_trace};
use hesper_cpu6502::{Direction, Registers, StepKind};

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
    let mut bus_trace = false;
    let mut trace_limit = 64_usize;
    let mut max_steps = DEFAULT_MAX_STEPS;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--trace" => trace = true,
            "--bus-trace" => bus_trace = true,
            "--trace-limit" => {
                trace_limit = args
                    .next()
                    .ok_or("--trace-limit requires 1..4096")?
                    .parse()
                    .map_err(|_| "--trace-limit requires 1..4096")?;
                if !(1..=4096).contains(&trace_limit) {
                    return Err("--trace-limit requires 1..4096".into());
                }
            }
            "--max-steps" => {
                max_steps = args
                    .next()
                    .ok_or("--max-steps requires an unsigned integer")?
                    .parse()
                    .map_err(|_| "--max-steps requires an unsigned integer")?;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: hesper [--trace] [--bus-trace] [--trace-limit 1..4096] [--max-steps N]\nRun the built-in NMOS 6502 count demo. Default limit: {DEFAULT_MAX_STEPS} instructions; keep the last 64 trace records."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}; use --help").into()),
        }
    }
    let mut recent = VecDeque::new();
    let result = run_demo_with_trace(max_steps, |event, total| {
        let line = match event {
            DemoEvent::Instruction(step) if trace => {
                let event = match step.kind {
                    StepKind::Instruction { opcode } => format!("{opcode:02X}"),
                    StepKind::Irq => "IRQ".to_owned(),
                    StepKind::Nmi => "NMI".to_owned(),
                    StepKind::Reset => "RESET".to_owned(),
                };
                Some(format!(
                    "${:04X} {event} | {} -> {} | +{} cycles total={total}",
                    step.address,
                    registers(step.before),
                    registers(step.after),
                    step.cycles
                ))
            }
            DemoEvent::Cycle { cycle, state } if bus_trace => {
                let direction = if cycle.bus.direction == Direction::Read {
                    'R'
                } else {
                    'W'
                };
                Some(format!(
                    "C{total:06} {direction} ${:04X}={:02X} SYNC={} stalled={} | next={:?}/{:?} pins={:?} latches={:?}",
                    cycle.bus.address,
                    cycle.bus.data,
                    cycle.bus.sync,
                    cycle.stalled,
                    state.next_clock_phase,
                    state.execution.map(|e| e.phase),
                    state.pins,
                    state.latches
                ))
            }
            _ => None,
        };
        if let Some(line) = line {
            if recent.len() == trace_limit {
                recent.pop_front();
            }
            recent.push_back(line);
        }
    });
    for line in recent {
        println!("{line}");
    }
    let result = result?;
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
        result.instruction_cycles + result.reset_cycles
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
