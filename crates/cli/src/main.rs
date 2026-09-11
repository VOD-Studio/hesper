use std::{
    collections::VecDeque,
    env,
    error::Error,
    fmt::Write as _,
    io::{self, IsTerminal},
    path::PathBuf,
    process::ExitCode,
};

use hesper::{
    DEFAULT_MAX_STEPS, DemoEvent,
    apple1::run_apple1,
    format_bus_trace, format_instruction_trace, format_registers, run_demo_with_trace,
    tui::{self, Apple1Launch},
};

const DEFAULT_TRACE_LIMIT: usize = 64;

fn print_demo_usage() {
    println!(
        "Usage: hesper demo [--trace] [--bus-trace] [--trace-limit 1..4096] [--max-steps N]\nRun the built-in NMOS 6502 count demo. Default limit: {DEFAULT_MAX_STEPS} instructions; keep the last {DEFAULT_TRACE_LIMIT} trace records."
    );
}

fn print_tui_usage() {
    println!(
        "Usage: hesper tui\nOpen the interactive Hesper emulator center. stdin and stdout must both be usable terminals."
    );
}

fn print_apple1_usage() {
    eprintln!(
        "\
Usage: hesper apple1 [--rom <path>] [OPTIONS]

On a usable terminal this opens the Apple-1 TUI. Without --rom it opens the
configuration page. In text/pipe mode --rom remains required.

Options:
  --rom <path>           Path to the 256-byte Woz Monitor ROM
  --program <path>       Optional program file to load into RAM at $0000
  --max-cycles <N>       Maximum total CPU cycles before the emulator exits
  --trace                Enable instruction trace
  --bus-trace            Enable bus-level trace
  --trace-limit <N>      Keep the last N trace records, 1..4096 (default: 64)
  --help, -h             Show this help message"
    );
}

fn print_usage() {
    println!(
        "Usage: hesper [demo options] | hesper <demo|tui|apple1> [options]\n\nWith no arguments Hesper opens the TUI on a usable terminal, otherwise it runs the built-in demo. Use 'hesper demo --help' for demo options."
    );
}

fn run_demo(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut trace = false;
    let mut bus_trace = false;
    let mut trace_limit = DEFAULT_TRACE_LIMIT;
    let mut max_steps = DEFAULT_MAX_STEPS;
    let mut args = args;
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
                print_demo_usage();
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}; use --help").into()),
        }
    }
    let mut recent = VecDeque::new();
    let result = run_demo_with_trace(max_steps, |event, total| {
        let line = match event {
            DemoEvent::Instruction(step) if trace => Some(format_instruction_trace(&step, total)),
            DemoEvent::Cycle { cycle, state } if bus_trace => {
                Some(format_bus_trace(&cycle, &state, total, None))
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
    println!("{}", format_registers(result.registers));
    println!(
        "Completed: {} instructions, {} instruction cycles + {} reset cycles = {} total cycles",
        result.steps,
        result.instruction_cycles,
        result.reset_cycles,
        result.instruction_cycles + result.reset_cycles
    );
    Ok(())
}

fn parse_apple1(
    mut args: impl Iterator<Item = String>,
) -> Result<Option<Apple1Launch>, Box<dyn Error>> {
    let mut launch = Apple1Launch {
        trace_limit: DEFAULT_TRACE_LIMIT,
        ..Apple1Launch::default()
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rom" => {
                launch.rom = Some(PathBuf::from(
                    args.next().ok_or("--rom requires a file path")?,
                ))
            }
            "--program" => {
                launch.program = Some(PathBuf::from(
                    args.next().ok_or("--program requires a file path")?,
                ));
            }
            "--max-cycles" => {
                launch.max_cycles = Some(
                    args.next()
                        .ok_or("--max-cycles requires an unsigned integer")?
                        .parse()
                        .map_err(|_| "--max-cycles requires an unsigned integer")?,
                );
            }
            "--trace-limit" => {
                launch.trace_limit = args
                    .next()
                    .ok_or("--trace-limit requires 1..4096")?
                    .parse()
                    .map_err(|_| "--trace-limit requires 1..4096")?;
                if !(1..=4096).contains(&launch.trace_limit) {
                    return Err("--trace-limit requires 1..4096".into());
                }
            }
            "--trace" => launch.trace = true,
            "--bus-trace" => launch.bus_trace = true,
            "--help" | "-h" => {
                print_apple1_usage();
                return Ok(None);
            }
            _ => return Err(format!("unknown apple1 argument: {arg}; use 'apple1 --help'").into()),
        }
    }
    launch.direct = true;
    Ok(Some(launch))
}

fn usable_tui_terminal() -> bool {
    io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && env::var("TERM").ok().as_deref() != Some("dumb")
}

fn run_apple1_subcommand(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let Some(launch) = parse_apple1(args)? else {
        return Ok(());
    };
    if usable_tui_terminal() {
        return tui::run(Some(launch));
    }
    let rom = launch
        .rom
        .as_ref()
        .ok_or("missing required --rom <path>\nUse 'apple1 --help' for usage")?;
    let rom = rom.to_str().ok_or("ROM path is not valid UTF-8")?;
    let program = launch
        .program
        .as_ref()
        .map(|path| path.to_str().ok_or("program path is not valid UTF-8"))
        .transpose()?;
    run_apple1(
        rom,
        program,
        launch.max_cycles,
        launch.trace,
        launch.bus_trace,
        launch.trace_limit,
    )
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    match args.next() {
        None if usable_tui_terminal() => tui::run(None),
        None => run_demo(std::iter::empty()),
        Some(command) if command == "demo" => run_demo(args),
        Some(command) if command == "tui" => match args.next() {
            None => tui::run(None),
            Some(arg) if arg == "--help" || arg == "-h" => {
                print_tui_usage();
                Ok(())
            }
            Some(arg) => Err(format!("unknown tui argument: {arg}; use 'tui --help'").into()),
        },
        Some(command) if command == "apple1" => run_apple1_subcommand(args),
        Some(command) if command == "--help" || command == "-h" => {
            print_usage();
            Ok(())
        }
        Some(first_demo_arg) => run_demo(std::iter::once(first_demo_arg).chain(args)),
    }
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
