use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
};

use hesper_riscv_linux::{DRAM_BASE, HartIsa, Machine};

fn hex(value: &str) -> Result<u64, std::num::ParseIntError> {
    u64::from_str_radix(value.trim_start_matches("0x"), 16)
}

fn main() -> Result<(), Box<dyn Error>> {
    const USAGE: &str = "usage: isa_probe IMAGE FUEL STOP_HEX SIGNATURE_HEX BYTES [--fd [--step]]";
    let mut args = env::args().skip(1);
    let [
        Some(image),
        Some(fuel),
        Some(stop),
        Some(address),
        Some(bytes),
    ] = std::array::from_fn(|_| args.next())
    else {
        return Err(USAGE.into());
    };
    let (isa, step) = match (args.next().as_deref(), args.next().as_deref()) {
        (None, None) => (HartIsa::Imac, false),
        (Some("--fd"), None) => (HartIsa::Imafdc, false),
        (Some("--fd"), Some("--step")) if args.next().is_none() => (HartIsa::Imafdc, true),
        _ => return Err(USAGE.into()),
    };
    let fuel = fuel.parse()?;
    let mut board = Machine::try_with_isa(isa)?;
    board.load(DRAM_BASE, &fs::read(image)?)?;
    let mut retired = 0;
    while retired < fuel {
        let budget = if step { 1 } else { fuel - retired };
        let progress = board.run(budget);
        if progress.retired != budget {
            let state = board.debug_state();
            return Err(format!(
                "probe stopped after {retired} retired instructions: {progress:?}; {state:x?}"
            )
            .into());
        }
        retired += progress.retired;
    }
    let state = board.debug_state();
    if state.pc != hex(&stop)? || state.last_trap.is_some() {
        return Err(format!("unexpected probe stop: {state:x?}").into());
    }
    let mut output = io::stdout().lock();
    for byte in board
        .inspect(hex(&address)?, bytes.parse()?)
        .ok_or("probe signature is outside board DRAM")?
    {
        write!(output, "{byte:02x}")?;
    }
    writeln!(output)?;
    Ok(())
}
