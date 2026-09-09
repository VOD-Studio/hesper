//! Cycle positions cross-checked with Visual6502 revD d8ecc129 (see references).
use hesper_cpu6502::{Bus, Cpu, Ram, Registers, Status, StepKind};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceState {
    pc: u16,
    a: u8,
    x: u8,
    y: u8,
    s: u8,
    p: u8,
    ram: Vec<(u16, u8)>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    name: String,
    initial: ReferenceState,
    events: Vec<(usize, String, bool)>,
    cycles: Vec<(u16, u8, String, bool)>,
}

#[test]
fn fixed_visual6502_revd_pin_traces_match_actual_bus_cycles() {
    let cases: Vec<Reference> =
        serde_json::from_str(include_str!("data/visual6502/pins.json")).unwrap();
    assert_eq!(cases.len(), 96);
    for case in cases {
        let state = case.initial;
        let mut cpu = Cpu::from_registers(Registers {
            pc: state.pc,
            a: state.a,
            x: state.x,
            y: state.y,
            sp: state.s,
            status: Status::from_bits(state.p),
        });
        let mut ram = Ram::new();
        ram.load(0, &[0xea; 65536]).unwrap();
        for (address, value) in state.ram {
            ram.write(address, value);
        }
        assert_eq!(case.cycles.len(), 24);
        assert!(case.events.iter().all(|(half, _, _)| half % 2 == 0));
        for (index, (address, data, direction, sync)) in case.cycles.into_iter().enumerate() {
            for (half, pin, asserted) in &case.events {
                if *half == index * 2 {
                    match pin.as_str() {
                        "irq" => cpu.set_irq_line(*asserted),
                        "nmi" => cpu.set_nmi_line(*asserted),
                        _ => panic!("invalid pin"),
                    }
                }
            }
            let result = cpu
                .cycle(&mut ram)
                .unwrap_or_else(|e| panic!("{} cycle {index}: {e}", case.name));
            let read = result.bus.direction == hesper_cpu6502::Direction::Read;
            assert!(direction == "read" || direction == "write");
            assert_eq!(
                (result.bus.address, result.bus.data, read, result.bus.sync),
                (address, data, direction == "read", sync),
                "{} cycle {index}; cpu={cpu:?}",
                case.name
            );
        }
    }
}

fn setup(pc: u16, program: &[u8], p: u8) -> (Cpu, Ram) {
    let mut ram = Ram::new();
    ram.load(0, &[0xea; 65536]).unwrap();
    ram.load(pc, program).unwrap();
    ram.load(0xfffa, &[0, 0xa0, 0, 0x80, 0, 0x90]).unwrap();
    (
        Cpu::from_registers(Registers {
            pc,
            sp: 0xfd,
            status: Status::from_bits(p),
            ..Registers::default()
        }),
        ram,
    )
}

#[test]
fn irq_and_nmi_poll_previous_cycle_not_the_last_cycle() {
    for nmi in [false, true] {
        for assertion in 0..2 {
            let (mut cpu, mut ram) = setup(0x8000, &[0xea, 0xea], 0x20);
            for index in 0..2 {
                if index == assertion {
                    if nmi {
                        cpu.set_nmi_line(true);
                    } else {
                        cpu.set_irq_line(true);
                    }
                }
                cpu.cycle(&mut ram).unwrap();
            }
            if assertion == 1 {
                assert_eq!(
                    cpu.step(&mut ram).unwrap().kind,
                    StepKind::Instruction { opcode: 0xea }
                );
            }
            let entry = cpu.step(&mut ram).unwrap();
            assert_eq!(entry.kind, if nmi { StepKind::Nmi } else { StepKind::Irq });
            assert_eq!(entry.after.pc, if nmi { 0xa000 } else { 0x9000 });
            assert_eq!(ram.as_slice()[0x1fc], if assertion == 0 { 1 } else { 2 });
        }
    }
}

#[test]
fn branch_poll_retains_operand_poll_and_crossing_adds_a_second_opportunity() {
    for (pc, offset, duration, target) in [(0x8000, 0, 3, 0x8002), (0x80fd, 1, 4, 0x8100)] {
        for nmi in [false, true] {
            for assertion in 0..duration {
                let (mut cpu, mut ram) = setup(pc, &[0xd0, offset], 0x20);
                for cycle in 0..duration {
                    if cycle == assertion {
                        if nmi {
                            cpu.set_nmi_line(true);
                        } else {
                            cpu.set_irq_line(true);
                        }
                    }
                    cpu.cycle(&mut ram).unwrap();
                }
                assert_eq!(cpu.registers().pc, target);
                let accepted = if duration == 3 {
                    assertion == 0
                } else {
                    assertion <= 2
                };
                if !accepted {
                    assert_eq!(
                        cpu.step(&mut ram).unwrap().kind,
                        StepKind::Instruction { opcode: 0xea }
                    );
                }
                assert_eq!(
                    cpu.step(&mut ram).unwrap().kind,
                    if nmi { StepKind::Nmi } else { StepKind::Irq }
                );
            }
        }
    }
}

#[test]
fn nmi_vector_hijack_closes_before_status_push_and_preserves_brk_stack_image() {
    for hardware in [false, true] {
        for assertion in 0..7 {
            let (mut cpu, mut ram) =
                setup(0x8000, &[if hardware { 0xea } else { 0x00 }, 0xea], 0x28);
            if hardware {
                cpu.set_irq_line(true);
                cpu.step(&mut ram).unwrap();
                cpu.set_irq_line(false);
            }
            let mut completed = None;
            for cycle in 0..7 {
                if cycle == assertion {
                    cpu.set_nmi_line(true);
                }
                completed = cpu.cycle(&mut ram).unwrap().completed;
            }
            let entry = completed.unwrap();
            assert_eq!(
                entry.kind,
                if hardware {
                    StepKind::Irq
                } else {
                    StepKind::Instruction { opcode: 0x00 }
                }
            );
            assert_eq!(entry.after.pc, if assertion <= 3 { 0xa000 } else { 0x9000 });
            assert_eq!(
                &ram.as_slice()[0x1fb..=0x1fd],
                if hardware {
                    &[0x28, 1, 0x80]
                } else {
                    &[0x38, 2, 0x80]
                }
            );
            if assertion >= 4 {
                // A late edge survives vector selection, then polls in the handler.
                assert_eq!(
                    cpu.step(&mut ram).unwrap().kind,
                    StepKind::Instruction { opcode: 0xea }
                );
                assert_eq!(cpu.step(&mut ram).unwrap().kind, StepKind::Nmi);
            }
        }
    }
}

#[test]
fn pulses_entirely_between_sampling_points_are_not_hardware_edges() {
    let (mut cpu, mut ram) = setup(0x8000, &[0xea, 0xea], 0x20);
    cpu.set_nmi_line(true);
    cpu.set_nmi_line(false);
    cpu.set_irq_line(true);
    cpu.set_irq_line(false);
    for _ in 0..2 {
        assert_eq!(
            cpu.step(&mut ram).unwrap().kind,
            StepKind::Instruction { opcode: 0xea }
        );
    }
}

#[test]
fn reset_request_discards_partial_rmw_without_undoing_completed_bus_writes() {
    for prefix in 1..6 {
        let (mut cpu, mut ram) = setup(0x8000, &[0xee, 0x00, 0x20], 0x28);
        ram.write(0x2000, 0x7f);
        for _ in 0..prefix {
            assert!(cpu.cycle(&mut ram).unwrap().completed.is_none());
        }
        cpu.begin_reset();
        let reset = cpu.step(&mut ram).unwrap();
        assert_eq!(reset.kind, StepKind::Reset);
        assert_eq!(
            (reset.cycles, reset.after.pc, reset.after.sp),
            (7, 0x8000, 0xfa)
        );
        assert!(reset.after.status.decimal && reset.after.status.interrupt_disable);
        // The modified final write had not happened; the old-value write can have.
        assert_eq!(ram.as_slice()[0x2000], 0x7f);
    }
}
