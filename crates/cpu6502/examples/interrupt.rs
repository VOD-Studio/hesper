//! Pinned Klaus interrupt program with its documented open-collector feedback.
use hesper_cpu6502::{Bus, Cpu, Ram, Registers, Status};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path, process::ExitCode};

#[derive(Default)]
struct Feedback {
    ram: Ram,
    changed: Option<u8>,
}
impl Bus for Feedback {
    fn read(&mut self, address: u16) -> u8 {
        let value = self.ram.read(address);
        if address == 0xbffc {
            value & 0x7f
        } else {
            value
        }
    }
    fn write(&mut self, address: u16, value: u8) {
        self.ram.write(address, value);
        if address == 0xbffc {
            self.changed = Some(value);
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    let delay: u32 = match args.as_slice() {
        [] => 0,
        [flag, value] if flag == "--feedback-delay" => value
            .parse()
            .map_err(|_| "delay must be an unsigned cycle count")?,
        _ => return Err("Usage: interrupt [--feedback-delay 0..32]".into()),
    };
    if delay > 32 {
        return Err("feedback delay exceeds 32-cycle limit".into());
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.cache/cpu6502/klaus/7954e2dbb49c469ea286070bf46cdd71aeb29e4b/interrupt.bin");
    let image = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if format!("{:x}", Sha256::digest(&image))
        != "ecc829d494fd1f4b4262ac5c58e8cb3925f7aa7570ce815e9a3214b6f66a3c58"
    {
        return Err("interrupt image SHA-256 mismatch".into());
    }
    let mut bus = Feedback::default();
    bus.ram.load(0, &image).map_err(|e| e.to_string())?;
    let mut cpu = Cpu::from_registers(Registers {
        pc: 0x0400,
        sp: 0xfd,
        status: Status::from_bits(0x24),
        ..Registers::default()
    });
    let mut instructions = 0;
    let mut port = 0;
    let mut transitions = std::collections::VecDeque::new();
    let mut recent = std::collections::VecDeque::new();
    for cycles in 0..1_000_000 {
        if cpu.at_instruction_boundary() && cpu.registers().pc == 0x06f5 {
            println!(
                "Klaus NMOS interrupt: passed at $06F5 after {instructions} steps / {cycles} cycles; feedback delay={delay}; order NMI/IRQ/BRK={:?}",
                &bus.ram.as_slice()[0x200..0x203]
            );
            return Ok(());
        }
        while transitions.front().is_some_and(|&(due, _)| due <= cycles) {
            if let Some((_, value)) = transitions.pop_front() {
                port = value;
            }
        }
        cpu.set_irq_line(port & 1 != 0);
        cpu.set_nmi_line(port & 2 != 0);
        let result = cpu
            .cycle(&mut bus)
            .map_err(|e| format!("{e}; recent={recent:?}"))?;
        if let Some(value) = bus.changed.take() {
            transitions.push_back((cycles + 1 + delay, value));
        }
        if let Some(step) = result.completed {
            instructions += 1;
            if recent.len() == 16 {
                recent.pop_front();
            }
            recent.push_back(step);
            if step.before.pc == step.after.pc {
                return Err(format!(
                    "interrupt failure trap: {step:?}; cycles={cycles}; port={port:02X}; recent={recent:?}"
                ));
            }
        }
    }
    Err(format!(
        "interrupt cycle budget exceeded: {:?}",
        cpu.registers()
    ))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("interrupt: {error}");
            ExitCode::FAILURE
        }
    }
}
