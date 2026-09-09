//! Explicit host-side replay of the same fixtures exercised by cargo test.

#[path = "../tests/support/singlestep.rs"]
mod singlestep;

use singlestep::{FIXTURES, Options};
use std::{env, path::Path, process::ExitCode};

fn run() -> Result<(), String> {
    let mut options = Options::default();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--opcode" => {
                let value = args.next().ok_or("--opcode requires a hex byte")?;
                options.opcode = Some(
                    u8::from_str_radix(&value, 16).map_err(|_| "--opcode requires a hex byte")?,
                );
            }
            "--case-index" => {
                let value = args
                    .next()
                    .ok_or("--case-index requires an unsigned index")?;
                options.case_index = Some(
                    value
                        .parse()
                        .map_err(|_| "--case-index requires an unsigned index")?,
                );
            }
            "--help" | "-h" => {
                println!(
                    "Usage: singlestep [--opcode HEX [--case-index N]]\nReplay pinned NMOS fixtures. Case indexes start at zero."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let report = singlestep::run(Path::new(FIXTURES), options)?;
    println!(
        "NMOS 6502/v1 @ {}\nSelection: {}\nPassed: {} cases across {} opcode files\nRegisters/memory: passed; cycle counts: passed; bus sequences: not checked",
        report.revision, report.selection, report.cases, report.files
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("singlestep: {error}");
            ExitCode::FAILURE
        }
    }
}
