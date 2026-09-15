//! Native reference observations consumed by tests/wasm. No ROM downloads.
use hesper_apple1::Apple1;
use hesper_cpu6502::{Cpu, Cycle, Direction, Ram, Registers, Step, StepKind};
use serde_json::{Value, json};

fn registers(r: Registers) -> Value {
    json!({"a":r.a,"x":r.x,"y":r.y,"sp":r.sp,"pc":r.pc,"status":r.status.bits()})
}

fn step(s: Step) -> Value {
    let (kind, opcode) = match s.kind {
        StepKind::Instruction { opcode } => ("instruction", Some(opcode)),
        StepKind::Irq => ("irq", None),
        StepKind::Nmi => ("nmi", None),
        StepKind::Reset => ("reset", None),
    };
    json!({"address":s.address,"kind":kind,"opcode":opcode,"before":registers(s.before),
        "after":registers(s.after),"cycles":s.cycles.to_string()})
}

fn cycle(c: Cycle) -> Value {
    json!({"bus":{"address":c.bus.address,"data":c.bus.data,
        "direction":match c.bus.direction { Direction::Read => "read", Direction::Write => "write" },
        "sync":c.bus.sync},"stalled":c.stalled,"completed":c.completed.map(step)})
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let program = [0xa9, 0x2a, 0x8d, 0, 2, 0xee, 0, 2, 0x4c, 5, 0x80];
    let vectors = [0, 0x80, 0, 0x80, 0, 0x80];
    let events = [
        (12, "setReady", false),
        (15, "setReady", true),
        (20, "setSoLine", true),
        (21, "setSoLine", false),
        (30, "setIrqLine", true),
        (40, "setIrqLine", false),
        (45, "setNmiLine", true),
        (46, "setNmiLine", false),
        (55, "setResetLine", true),
        (60, "setResetLine", false),
    ];
    let mut cpu = Cpu::new();
    let mut ram = Ram::new();
    ram.load(0x8000, &program)?;
    ram.load(0xfffa, &vectors)?;
    cpu.begin_reset();
    let mut trace = Vec::new();
    for at in 0..100 {
        for (_, pin, value) in events.iter().filter(|(index, _, _)| *index == at) {
            match *pin {
                "setReady" => cpu.set_ready(*value),
                "setSoLine" => cpu.set_so_line(*value),
                "setIrqLine" => cpu.set_irq_line(*value),
                "setNmiLine" => cpu.set_nmi_line(*value),
                "setResetLine" => cpu.set_reset_line(*value),
                _ => unreachable!(),
            }
        }
        trace.push(cycle(cpu.cycle(&mut ram)?));
    }
    let cpu_reference = json!({"program":program,"vectors":vectors,"events":events,
        "trace":trace,"registers":registers(cpu.registers()),"memory":ram.as_slice()[0x200]});

    // Original keyboard/display polling program, also documented in apple1/tests/machine.rs.
    let echo = [
        0xa9, 0x7f, 0x8d, 0x12, 0xd0, 0xa9, 0x27, 0x8d, 0x13, 0xd0, 0xa9, 0x07, 0x8d, 0x11, 0xd0,
        0x2c, 0x11, 0xd0, 0x10, 0xfb, 0xad, 0x10, 0xd0, 0x29, 0x7f, 0x8d, 0x12, 0xd0, 0x2c, 0x12,
        0xd0, 0x30, 0xfb, 0x4c, 0x0f, 0x00,
    ];
    let rom = [0; 256]; // Synthetic reset vector to $0000, no copyrighted firmware.
    let mut machine = Apple1::new(&rom)?;
    machine.bus_mut().load_ram(0, &echo)?;
    machine.reset()?;
    machine.type_str("a\rB");
    let mut video_hash = 2166136261u32;
    for index in 0..1_000_000 {
        let tick = machine.tick()?;
        if index < 2048 {
            let v = tick.video;
            let bits = u32::from(v.luminance)
                | (u32::from(v.sync) << 1)
                | (u32::from(v.hsync) << 2)
                | (u32::from(v.vsync) << 3)
                | (u32::from(v.dot_edge) << 4);
            video_hash = (video_hash ^ bits).wrapping_mul(16777619);
        }
    }
    let (row, column) = machine.display().cursor();
    let screen: Vec<u8> = machine
        .display()
        .screen()
        .iter()
        .flatten()
        .copied()
        .collect();
    let apple_reference = json!({"program":echo.as_slice(),"rom":rom.as_slice(),"text":"a\rB",
        "ticks":1_000_000,"videoPrefix":2048,"videoHash":video_hash,
        "screen":screen,"cursor":{"row":row,"column":column,"visible":row<24},
        "output":machine.drain_output(),"registers":registers(machine.cpu().registers()),
        "masterTicks":machine.master_ticks().to_string(),"cpuCycles":machine.cpu_cycles().to_string(),
        "videoFrames":machine.video_frames().to_string(),"ioPending":machine.io_pending()});
    println!("{}", json!({"cpu":cpu_reference,"apple1":apple_reference}));
    Ok(())
}
