use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
};

use hesper_riscv_linux::{DRAM_BASE, Machine};

fn hex(value: &str) -> Result<u64, std::num::ParseIntError> {
    u64::from_str_radix(value.trim_start_matches("0x"), 16)
}

fn main() -> Result<(), Box<dyn Error>> {
    const USAGE: &str = "usage: isa_probe IMAGE FUEL STOP_HEX SIGNATURE_HEX BYTES";
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
    if args.next().is_some() {
        return Err(USAGE.into());
    }
    let fuel = fuel.parse()?;
    let mut board = Machine::new();
    board.load(DRAM_BASE, &fs::read(image)?)?;
    let progress = board.run(fuel);
    let state = board.debug_state();
    if progress.retired != fuel || state.pc != hex(&stop)? || state.last_trap.is_some() {
        return Err(format!("unexpected probe stop: {progress:?}; {state:x?}").into());
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
