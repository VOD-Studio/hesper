use hesper_cpu6502::{Bus, Cpu, Ram, Registers, Status, Step, StepKind};

#[derive(Debug, PartialEq, Eq)]
enum Access {
    Read(u16),
    Write(u16, u8),
}

#[derive(Default)]
struct Spy {
    ram: Ram,
    accesses: Vec<Access>,
}

impl Bus for Spy {
    fn read(&mut self, addr: u16) -> u8 {
        self.accesses.push(Access::Read(addr));
        self.ram.read(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.accesses.push(Access::Write(addr, value));
        self.ram.write(addr, value);
    }
}

fn setup(program: &[u8], flags: u8) -> (Cpu, Spy) {
    let mut bus = Spy::default();
    bus.ram.load(0x8000, program).unwrap();
    bus.ram
        .load(0xfffa, &[0x00, 0xa0, 0x00, 0x80, 0x00, 0x90])
        .unwrap();
    bus.ram.write(0x9000, 0x40); // IRQ: RTI
    bus.ram.write(0xa000, 0x40); // NMI: RTI
    let cpu = Cpu::from_registers(Registers {
        a: 0x12,
        x: 0x34,
        y: 0x56,
        sp: 0xfd,
        pc: 0x8000,
        status: Status::from_bits(flags),
    });
    (cpu, bus)
}

fn step(cpu: &mut Cpu, bus: &mut Spy, kind: StepKind, pc: u16, cycles: u8) -> Step {
    let before = cpu.registers();
    let result = cpu.step(bus).unwrap();
    assert_eq!(result.kind, kind);
    assert_eq!(result.address, before.pc);
    assert_eq!(result.before, before);
    assert_eq!(result.after, cpu.registers());
    assert_eq!(result.after.pc, pc);
    assert_eq!(result.cycles, u64::from(cycles));
    result
}

fn instruction(opcode: u8) -> StepKind {
    StepKind::Instruction { opcode }
}

#[test]
fn brk_pushes_pc_plus_two_and_old_flags_with_b_and_preserves_decimal() {
    for raw_flags in 0..=u8::MAX {
        let (cpu, mut bus) = setup(&[0x00, 0xab], raw_flags);
        let before = Registers {
            sp: 0,
            ..cpu.registers()
        };
        let mut cpu = Cpu::from_registers(before);
        let result = step(&mut cpu, &mut bus, instruction(0x00), 0x9000, 7);
        assert_eq!(
            result.after,
            Registers {
                pc: 0x9000,
                sp: 0xfd,
                status: Status::from_bits(raw_flags | 0x04),
                ..before
            }
        );
        // BRK's seven accesses now correspond one-for-one to bus cycles.
        assert_eq!(
            bus.accesses,
            [
                Access::Read(0x8000),
                Access::Read(0x8001),
                Access::Write(0x0100, 0x80),
                Access::Write(0x01ff, 0x02),
                Access::Write(0x01fe, raw_flags | 0x30),
                Access::Read(0xfffe),
                Access::Read(0xffff),
            ]
        );
        step(&mut cpu, &mut bus, instruction(0x40), 0x8002, 6);
        assert_eq!(
            cpu.registers(),
            Registers {
                pc: 0x8002,
                ..before
            }
        );
    }
}

#[test]
fn brk_padding_and_return_address_wrap_at_ffff() {
    let (cpu, mut bus) = setup(&[], 0x28);
    let mut cpu = Cpu::from_registers(Registers {
        pc: 0xffff,
        ..cpu.registers()
    });
    // Opcode at $FFFF is also the IRQ vector high byte.
    bus.ram.write(0xffff, 0x00);
    bus.ram.write(0xfffe, 0x78);
    bus.ram.write(0x0000, 0xab);
    step(&mut cpu, &mut bus, instruction(0x00), 0x0078, 7);
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0xffff),
            Access::Read(0x0000),
            Access::Write(0x01fd, 0x00),
            Access::Write(0x01fc, 0x01),
            Access::Write(0x01fb, 0x38),
            Access::Read(0xfffe),
            Access::Read(0xffff),
        ]
    );
}

#[test]
fn rti_restores_all_flags_and_pc_without_increment_and_wraps_sp() {
    for raw_flags in 0..=u8::MAX {
        let (mut cpu, mut bus) = setup(&[0x40], 0x24);
        let before = cpu.registers();
        bus.ram.load(0x01fe, &[raw_flags, 0xff]).unwrap();
        bus.ram.write(0x0100, 0xff);
        step(&mut cpu, &mut bus, instruction(0x40), 0xffff, 6);
        assert_eq!(
            cpu.registers(),
            Registers {
                pc: 0xffff,
                sp: 0,
                status: Status::from_bits(raw_flags),
                ..before
            }
        );
        assert_eq!(cpu.registers().status.bits(), (raw_flags & 0xcf) | 0x20);
        assert_eq!(
            bus.accesses,
            [
                Access::Read(0x8000),
                Access::Read(0x8001),
                Access::Read(0x01fd),
                Access::Read(0x01fe),
                Access::Read(0x01ff),
                Access::Read(0x0100)
            ]
        );
    }
}

#[test]
fn irq_polls_a_level_and_latched_request_survives_deassertion() {
    let (mut cpu, mut bus) = setup(&[0xea, 0xea], 0xeb);
    let before = cpu.registers();
    cpu.set_irq_line(true);
    step(&mut cpu, &mut bus, instruction(0xea), 0x8001, 2);
    cpu.set_irq_line(false);
    bus.accesses.clear();
    let irq = step(&mut cpu, &mut bus, StepKind::Irq, 0x9000, 7);
    assert_eq!(
        irq.after,
        Registers {
            pc: 0x9000,
            sp: 0xfa,
            status: Status::from_bits(0xef),
            ..before
        }
    );
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0x8001),
            Access::Read(0x8001),
            Access::Write(0x01fd, 0x80),
            Access::Write(0x01fc, 0x01),
            Access::Write(0x01fb, 0xeb),
            Access::Read(0xfffe),
            Access::Read(0xffff),
        ]
    );
    let accesses = bus.accesses.len();
    let trace = format!(
        "{:?} {:?} {:?} {}",
        irq.kind, irq.before, irq.after, irq.cycles
    );
    assert!(trace.contains("Irq"));
    assert_eq!(bus.accesses.len(), accesses);
    step(&mut cpu, &mut bus, instruction(0x40), 0x8001, 6);
    assert_eq!(
        cpu.registers(),
        Registers {
            pc: 0x8001,
            ..before
        }
    );
    step(&mut cpu, &mut bus, instruction(0xea), 0x8002, 2);
}

#[test]
fn masked_or_unpolled_irq_is_not_latched() {
    let (mut cpu, mut bus) = setup(&[0xea, 0x58, 0xea], 0x24);
    cpu.set_irq_line(true);
    step(&mut cpu, &mut bus, instruction(0xea), 0x8001, 2);
    cpu.set_irq_line(false); // Masked request must not become an edge-triggered IRQ.
    step(&mut cpu, &mut bus, instruction(0x58), 0x8002, 2);
    cpu.set_irq_line(true);
    cpu.set_irq_line(false); // No instruction has polled this pulse.
    step(&mut cpu, &mut bus, instruction(0xea), 0x8003, 2);
}

#[test]
fn cli_and_plp_unmask_after_one_following_instruction() {
    for opcode in [0x58, 0x28] {
        let (mut cpu, mut bus) = setup(&[opcode, 0xea, 0xea], 0x24);
        bus.ram.write(0x01fe, 0x20); // PLP clears I.
        cpu.set_irq_line(true);
        step(
            &mut cpu,
            &mut bus,
            instruction(opcode),
            0x8001,
            if opcode == 0x58 { 2 } else { 4 },
        );
        assert!(!cpu.registers().status.interrupt_disable);
        step(&mut cpu, &mut bus, instruction(0xea), 0x8002, 2);
        step(&mut cpu, &mut bus, StepKind::Irq, 0x9000, 7);
    }
}

#[test]
fn sei_and_plp_do_not_cancel_irq_polled_using_old_i() {
    for opcode in [0x78, 0x28] {
        let (mut cpu, mut bus) = setup(&[opcode, 0xea], 0x20);
        bus.ram.write(0x01fe, 0x24); // PLP sets I.
        cpu.set_irq_line(true);
        step(
            &mut cpu,
            &mut bus,
            instruction(opcode),
            0x8001,
            if opcode == 0x78 { 2 } else { 4 },
        );
        assert!(cpu.registers().status.interrupt_disable);
        let sp = cpu.registers().sp;
        step(&mut cpu, &mut bus, StepKind::Irq, 0x9000, 7);
        assert_eq!(
            bus.ram.as_slice()[usize::from(0x0100 | u16::from(sp.wrapping_sub(2)))],
            0x24
        );
    }
}

#[test]
fn cli_then_sei_can_dispatch_irq_with_i_set_on_stack() {
    let (mut cpu, mut bus) = setup(&[0x58, 0x78, 0xea], 0x24);
    cpu.set_irq_line(true);
    step(&mut cpu, &mut bus, instruction(0x58), 0x8001, 2);
    step(&mut cpu, &mut bus, instruction(0x78), 0x8002, 2);
    step(&mut cpu, &mut bus, StepKind::Irq, 0x9000, 7);
    assert_eq!(&bus.ram.as_slice()[0x01fb..=0x01fd], &[0x24, 0x02, 0x80]);
}

#[test]
fn rti_polls_restored_i_without_cli_delay() {
    for (pulled_flags, next_kind, next_pc, cycles) in [
        (0x20, StepKind::Irq, 0x9000, 7),
        (0x24, instruction(0xea), 0x8002, 2),
    ] {
        let (mut cpu, mut bus) = setup(&[0x40, 0xea], 0x24);
        bus.ram.load(0x01fe, &[pulled_flags, 0x01]).unwrap();
        bus.ram.write(0x0100, 0x80);
        cpu.set_irq_line(true);
        step(&mut cpu, &mut bus, instruction(0x40), 0x8001, 6);
        step(&mut cpu, &mut bus, next_kind, next_pc, cycles);
    }
}

#[test]
fn nmi_latches_an_edge_ignores_i_and_requires_release_to_retrigger() {
    for flags in [0xeb, 0xef] {
        let (mut cpu, mut bus) = setup(&[0xea, 0xea, 0xea, 0xea], flags);
        cpu.set_nmi_line(true);
        step(&mut cpu, &mut bus, instruction(0xea), 0x8001, 2);
        let before = cpu.registers();
        step(&mut cpu, &mut bus, StepKind::Nmi, 0xa000, 7);
        assert_eq!(&bus.ram.as_slice()[0x01fb..=0x01fd], &[flags, 1, 0x80]);
        step(&mut cpu, &mut bus, instruction(0x40), 0x8001, 6);
        assert_eq!(cpu.registers(), before);
        cpu.set_nmi_line(true); // Held asserted: no second edge.
        step(&mut cpu, &mut bus, instruction(0xea), 0x8002, 2);
        cpu.set_nmi_line(false);
        step(&mut cpu, &mut bus, instruction(0xea), 0x8003, 2);
        cpu.set_nmi_line(true);
        step(&mut cpu, &mut bus, instruction(0xea), 0x8004, 2);
        cpu.set_nmi_line(false); // Sampled pulse remains latched after release.
        step(&mut cpu, &mut bus, StepKind::Nmi, 0xa000, 7);
    }
}

#[test]
fn multiple_unserviced_nmi_edges_coalesce() {
    let (mut cpu, mut bus) = setup(&[0xee, 0x00, 0x40, 0xea], 0x20);
    // Three sampled edges during one six-cycle INC, before any can be serviced.
    for index in 0..6 {
        cpu.set_nmi_line(index % 2 == 0);
        assert_eq!(cpu.cycle(&mut bus).unwrap().completed.is_some(), index == 5);
    }
    step(&mut cpu, &mut bus, StepKind::Nmi, 0xa000, 7);
    step(&mut cpu, &mut bus, instruction(0x40), 0x8003, 6);
    step(&mut cpu, &mut bus, instruction(0xea), 0x8004, 2);
}

#[test]
fn nmi_wins_over_queued_irq_and_held_irq_is_resampled_after_rti() {
    let (mut cpu, mut bus) = setup(&[0xea, 0xea], 0x20);
    cpu.set_irq_line(true);
    cpu.set_nmi_line(true);
    step(&mut cpu, &mut bus, instruction(0xea), 0x8001, 2);
    step(&mut cpu, &mut bus, StepKind::Nmi, 0xa000, 7);
    // No stale queued IRQ may preempt the handler before RTI restores I=0.
    step(&mut cpu, &mut bus, instruction(0x40), 0x8001, 6);
    step(&mut cpu, &mut bus, StepKind::Irq, 0x9000, 7);
}

#[test]
fn nmi_can_nest_inside_irq_and_both_returns_restore_their_contexts() {
    let (mut cpu, mut bus) = setup(&[0xea, 0xea], 0x28);
    bus.ram.load(0x9000, &[0xea, 0x40]).unwrap();
    let before = cpu.registers();
    cpu.set_irq_line(true);
    step(&mut cpu, &mut bus, instruction(0xea), 0x8001, 2);
    cpu.set_irq_line(false);
    step(&mut cpu, &mut bus, StepKind::Irq, 0x9000, 7);
    cpu.set_nmi_line(true);
    step(&mut cpu, &mut bus, instruction(0xea), 0x9001, 2);
    let handler = cpu.registers();
    step(&mut cpu, &mut bus, StepKind::Nmi, 0xa000, 7);
    assert_eq!(cpu.registers().sp, 0xf7);
    assert_eq!(&bus.ram.as_slice()[0x01f8..=0x01fa], &[0x2c, 0x01, 0x90]);
    step(&mut cpu, &mut bus, instruction(0x40), 0x9001, 6);
    assert_eq!(cpu.registers(), handler);
    step(&mut cpu, &mut bus, instruction(0x40), 0x8001, 6);
    assert_eq!(
        cpu.registers(),
        Registers {
            pc: 0x8001,
            ..before
        }
    );
}

#[test]
fn reset_discards_pending_interrupts_without_inventing_nmi_edges() {
    let (mut cpu, mut bus) = setup(&[0x58, 0xea, 0xea], 0x20);
    bus.ram.load(0x9000, &[0xea, 0x40]).unwrap();
    cpu.set_irq_line(true);
    step(&mut cpu, &mut bus, instruction(0x58), 0x8001, 2);
    cpu.set_nmi_line(true);
    assert_eq!(cpu.reset(&mut bus).unwrap().cycles, 7);
    assert_eq!(cpu.registers().sp, 0xfa);
    step(&mut cpu, &mut bus, instruction(0x58), 0x8001, 2);
    step(&mut cpu, &mut bus, instruction(0xea), 0x8002, 2);
    // The external IRQ level survived reset and was polled again after CLI.
    step(&mut cpu, &mut bus, StepKind::Irq, 0x9000, 7);
    cpu.set_nmi_line(false);
    step(&mut cpu, &mut bus, instruction(0xea), 0x9001, 2);
    cpu.set_nmi_line(true);
    step(&mut cpu, &mut bus, instruction(0x40), 0x8002, 6);
    step(&mut cpu, &mut bus, StepKind::Nmi, 0xa000, 7);
}
