use std::{
    error::Error,
    fs,
    io::{self, BufRead, Write},
};

use hesper_apple1::Apple1;

/// Run the Apple I with the given ROM and optional program.
pub fn run_apple1(
    rom_path: &str,
    program_path: Option<&str>,
    cycles_per_char: u64,
    max_cycles: Option<u64>,
    trace: bool,
    bus_trace: bool,
) -> Result<(), Box<dyn Error>> {
    if trace {
        eprintln!("[--trace not yet implemented for apple1]");
    }
    if bus_trace {
        eprintln!("[--bus-trace not yet implemented for apple1]");
    }

    // 1. Load ROM
    let rom = fs::read(rom_path).map_err(|e| format!("cannot read ROM file '{rom_path}': {e}"))?;
    let mut machine = Apple1::new(&rom, Some(cycles_per_char))?;

    // 2. Load optional program into RAM at $0000
    if let Some(path) = program_path {
        let program =
            fs::read(path).map_err(|e| format!("cannot read program file '{path}': {e}"))?;
        machine
            .bus_mut()
            .load_ram(0x0000, &program)
            .map_err(|e| format!("cannot load program: {e}"))?;
    }

    // 3. Physical RESET (assert/hold/release through the real reset
    // sequence, then runs ROM from $FF00)
    machine.reset()?;

    // 4. Interactive loop
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    // Initial boot: run enough cycles to reach the prompt
    let output = machine.run_cycles(50000)?;
    print_output(&output, &mut stdout)?;

    loop {
        // Check budget
        if let Some(max) = max_cycles
            && machine.total_cycles() >= max
        {
            eprintln!("\n[max cycles reached: {}]", machine.total_cycles());
            break;
        }

        // Read a line from stdin (blocking)
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                eprintln!("\n[EOF]");
                break;
            }
            Err(e) => {
                eprintln!("\n[input error: {e}]");
                break;
            }
            Ok(_) => {}
        }

        // Canonical-mode terminals translate the user's physical Enter key
        // (CR, 0x0D) to LF (0x0A) before `read_line` sees it (POSIX ICRNL).
        // Strip whatever line ending arrived and replay it as a single CR —
        // the byte the Woz Monitor ROM actually expects to terminate a line.
        let content = line.strip_suffix('\n').unwrap_or(line.as_str());
        let content = content.strip_suffix('\r').unwrap_or(content);
        for ch in content.chars() {
            let byte = ch as u8;
            machine.type_char(byte);
            let output = machine.run_cycles(2000)?;
            print_output(&output, &mut stdout)?;
        }
        machine.type_char(b'\r');
        let output = machine.run_cycles(2000)?;
        print_output(&output, &mut stdout)?;

        // After the line, run more cycles to allow processing to complete
        // (memory dumps, program execution, etc.)
        // Run until output stops for a while
        let mut idle_batches = 0;
        loop {
            let output = machine.run_cycles(10000)?;
            if output.is_empty() {
                idle_batches += 1;
                if idle_batches >= 3 {
                    break; // machine is idle (polling for input)
                }
            } else {
                idle_batches = 0;
                print_output(&output, &mut stdout)?;
            }

            // Check budget inside the post-line loop too
            if let Some(max) = max_cycles
                && machine.total_cycles() >= max
            {
                eprintln!("\n[max cycles reached: {}]", machine.total_cycles());
                return Ok(());
            }
        }
    }

    Ok(())
}

fn print_output(output: &[u8], stdout: &mut impl Write) -> io::Result<()> {
    for &ch in output {
        // Convert CR to LF for modern terminals
        if ch == b'\r' {
            writeln!(stdout)?;
        } else {
            write!(stdout, "{}", ch as char)?;
        }
    }
    stdout.flush()
}
