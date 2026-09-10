mod apple1;

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

fn run_apple1_subcommand(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut rom: Option<String> = None;
    let mut program: Option<String> = None;
    let mut cycles_per_char: u64 = 1000;
    let mut max_cycles: Option<u64> = None;
    let mut trace = false;
    let mut bus_trace = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rom" => {
                rom = Some(args.next().ok_or("--rom requires a file path")?);
            }
            "--program" => {
                program = Some(args.next().ok_or("--program requires a file path")?);
            }
            "--cycles-per-char" => {
                cycles_per_char = args
                    .next()
                    .ok_or("--cycles-per-char requires a positive integer")?
                    .parse()
                    .map_err(|_| "--cycles-per-char requires a positive integer")?;
            }
            "--max-cycles" => {
                max_cycles = Some(
                    args.next()
                        .ok_or("--max-cycles requires a positive integer")?
                        .parse()
                        .map_err(|_| "--max-cycles requires a positive integer")?,
                );
            }
            "--trace" => trace = true,
            "--bus-trace" => bus_trace = true,
            "--help" | "-h" => {
                print_apple1_usage();
                return Ok(());
            }
            _ => return Err(format!("unknown apple1 argument: {arg}; use 'apple1 --help'").into()),
        }
    }

    let rom_path = rom.ok_or("missing required --rom <path>\nUse 'apple1 --help' for usage")?;

    apple1::run_apple1(
        &rom_path,
        program.as_deref(),
        cycles_per_char,
        max_cycles,
        trace,
        bus_trace,
    )
}

fn print_apple1_usage() {
    eprintln!(
        "\
Usage: hesper apple1 --rom <path> [OPTIONS]

Options:
  --rom <path>           Path to the 256-byte Woz Monitor ROM (required)
  --program <path>       Optional program file to load into RAM at $0000
  --cycles-per-char <N>  CPU cycles per display character (default: 1000)
  --max-cycles <N>       Maximum total cycles before the emulator exits
  --trace                Enable instruction trace (not yet implemented)
  --bus-trace             Enable bus-level trace (not yet implemented)
  --help, -h              Show this help message"
    );
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut trace = false;
    let mut bus_trace = false;
    let mut trace_limit = 64_usize;
    let mut max_steps = DEFAULT_MAX_STEPS;
    let args: Vec<String> = env::args().skip(1).collect();
    let mut args_iter = args.into_iter();

    let subcommand = args_iter.next();
    if let Some(ref cmd) = subcommand
        && cmd == "apple1"
    {
        return run_apple1_subcommand(args_iter);
    }

    // Existing demo behavior: treat first non-apple1 arg as a regular flag.
    // Put the subcommand arg back (if any) so the original switch parser sees it.
    let all_args: Vec<String> = subcommand.into_iter().chain(args_iter).collect();
    let mut args = all_args.into_iter();
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
