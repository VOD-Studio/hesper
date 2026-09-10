//! Cycle positions cross-checked with Visual6502 revD d8ecc129 (see references).
use hesper_cpu6502::{Bus, Cpu, CpuError, Ram, Registers, Status, StepKind};
use serde::Deserialize;
use sha2::{Digest, Sha256};

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

fn compare_reference_cycles(cases: &[Reference], cycles_per_case: usize) -> usize {
    let mut compared = 0;
    for case in cases {
        let state = &case.initial;
        let expected_registers = Registers {
            pc: state.pc,
            a: state.a,
            x: state.x,
            y: state.y,
            sp: state.s,
            status: Status::from_bits(state.p),
        };
        let mut cpu = Cpu::from_registers(Registers {
            pc: 0x0200,
            ..Registers::default()
        });
        let mut ram = Ram::new();
        ram.load(0, &[0xea; 65536]).unwrap();
        for &(address, value) in &state.ram {
            ram.write(address, value);
        }
        // Match the oracle's real bootstrap: registers alone omit residual
        // datapath latches that can become observable during physical RESET.
        for index in 0..32 {
            cpu.step(&mut ram)
                .unwrap_or_else(|e| panic!("{} bootstrap step {index}: {e}", case.name));
            if cpu.at_instruction_boundary() && cpu.registers().pc == state.pc {
                break;
            }
        }
        assert!(
            cpu.at_instruction_boundary() && cpu.registers().pc == state.pc,
            "{} bootstrap did not reach ${:04x} within 32 steps; state={:?}",
            case.name,
            state.pc,
            cpu.debug_state()
        );
        assert_eq!(
            cpu.registers(),
            expected_registers,
            "{} bootstrap",
            case.name
        );
        // The reference applies scenario RAM overrides after boot. Restore the
        // complete captured image, without reconstructing CPU internal latches.
        ram.load(0, &[0xea; 65536]).unwrap();
        for &(address, value) in &state.ram {
            ram.write(address, value);
        }
        assert_eq!(case.cycles.len(), cycles_per_case, "{}", case.name);
        let mut dispatched = 0;
        for (index, &(address, data, ref direction, sync)) in case.cycles.iter().enumerate() {
            let mut result = None;
            for phase in 0..2 {
                for (half, pin, asserted) in &case.events {
                    if *half == index * 2 + phase {
                        dispatched += 1;
                        match pin.as_str() {
                            "irq" => cpu.set_irq_line(*asserted),
                            "nmi" => cpu.set_nmi_line(*asserted),
                            "rdy" => cpu.set_ready(!asserted),
                            "so" => cpu.set_so_line(*asserted),
                            "res" => cpu.set_reset_line(*asserted),
                            _ => panic!("{} half {half}: invalid pin {pin}", case.name),
                        }
                    }
                }
                result = cpu
                    .half_cycle(&mut ram)
                    .unwrap_or_else(|e| panic!("{} cycle {index} phase {phase}: {e}", case.name));
                assert_eq!(
                    result.is_some(),
                    phase == 1,
                    "{} cycle {index} phase {phase}",
                    case.name
                );
            }
            let result = result.unwrap();
            let read = result.bus.direction == hesper_cpu6502::Direction::Read;
            assert!(direction == "read" || direction == "write");
            assert_eq!(
                (result.bus.address, result.bus.data, read, result.bus.sync),
                (address, data, direction == "read", sync),
                "{} cycle {index}; state={:?}",
                case.name,
                cpu.debug_state()
            );
            compared += 1;
        }
        assert_eq!(
            dispatched,
            case.events.len(),
            "unreplayed event {}",
            case.name
        );
    }
    compared
}

#[test]
fn fixed_visual6502_revd_pin_traces_match_actual_bus_cycles() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("data/visual6502/manifest.json")).unwrap();
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(include_bytes!("data/visual6502/pins.json"))
        ),
        manifest["fixture_sha256"].as_str().unwrap()
    );
    let cases: Vec<Reference> =
        serde_json::from_str(include_str!("data/visual6502/pins.json")).unwrap();
    assert_eq!(cases.len(), 246);
    assert_eq!(compare_reference_cycles(&cases, 24), 5904);
}

#[test]
fn fixed_visual6502_revd_reset_traces_match_actual_bus_cycles() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("data/visual6502/manifest.json")).unwrap();
    let bytes = include_bytes!("data/visual6502/reset.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        manifest["reset_fixture_sha256"].as_str().unwrap()
    );
    let cases: Vec<Reference> = serde_json::from_slice(bytes).unwrap();
    assert_eq!(
        cases.len(),
        manifest["reset_cases"].as_u64().unwrap() as usize
    );
    assert_eq!(
        cases.len(),
        419,
        "the RESET reference corpus must not shrink"
    );
    let mut names = std::collections::HashSet::new();
    let mut cycles = 0;
    for case in &cases {
        assert!(case.name.starts_with("reset-"));
        assert!(names.insert(&case.name), "duplicate case {}", case.name);
        assert_eq!(case.cycles.len(), 64, "{}", case.name);
        cycles += case.cycles.len();
        let mut events = std::collections::HashSet::new();
        assert!(case.events.iter().any(|(_, pin, low)| pin == "res" && *low));
        for (half, pin, _) in &case.events {
            assert!(*half < case.cycles.len() * 2, "{}", case.name);
            assert!(matches!(pin.as_str(), "res" | "irq" | "nmi" | "rdy" | "so"));
            assert!(events.insert((half, pin)), "ambiguous event {}", case.name);
        }
        let mut addresses = std::collections::HashSet::new();
        for (address, _) in &case.initial.ram {
            assert!(addresses.insert(address), "duplicate RAM {}", case.name);
        }
        for (_, _, direction, _) in &case.cycles {
            assert!(matches!(direction.as_str(), "read" | "write"));
        }
    }
    assert_eq!(cycles, manifest["reset_cycles"].as_u64().unwrap() as usize);
    assert_eq!(cycles, 26816);
    assert_eq!(compare_reference_cycles(&cases, 64), cycles);
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
    cpu.set_reset_line(true);
    cpu.set_reset_line(false);
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

#[test]
fn physical_reset_completes_at_vector_high_after_the_fixed_nop_release_delay() {
    let (mut cpu, mut ram) = setup(0x8000, &[0xea], 0x29);
    ram.load(0xfffc, &[0x34, 0xb1]).unwrap();
    let before = cpu.registers();
    cpu.set_reset_line(true);
    assert_eq!(cpu.registers(), before);

    // reset-nop-0-8: low before half 0, high before half 8. The
    // vector-high read is cycle 12, not a host begin_reset's seventh cycle.
    for index in 0..13 {
        if index == 4 {
            cpu.set_reset_line(false);
        }
        let cycle = cpu.cycle(&mut ram).unwrap();
        if index == 0 {
            assert_eq!((cycle.bus.address, cycle.bus.sync), (0x8000, true));
        }
        if (2..12).contains(&index) {
            assert!(
                cycle.completed.is_none(),
                "physical reset fabricated a completion at cycle {index}"
            );
        }
        if index == 12 {
            assert_eq!(
                (
                    cycle.bus.address,
                    cycle.bus.data,
                    cycle.bus.direction,
                    cycle.bus.sync
                ),
                (0xfffd, 0xb1, hesper_cpu6502::Direction::Read, false)
            );
            let reset = cycle
                .completed
                .expect("vector-high read must complete RESET");
            assert_eq!(reset.kind, StepKind::Reset);
            assert_eq!((reset.after.pc, reset.after.sp), (0xb134, 0xfa));
            assert!(reset.after.status.decimal && reset.after.status.interrupt_disable);
        }
    }
    let fetch = cpu.cycle(&mut ram).unwrap();
    assert_eq!((fetch.bus.address, fetch.bus.sync), (0xb134, true));
    assert!(fetch.completed.is_none());
}

#[test]
fn forced_reset_ir_is_reported_as_reset_not_a_fabricated_brk_instruction() {
    let (mut cpu, mut ram) = setup(0x8000, &[0xea, 0xea], 0x29);
    cpu.cycle(&mut ram).unwrap();
    cpu.set_reset_line(true);
    let nop = cpu.cycle(&mut ram).unwrap().completed.unwrap();
    assert_eq!(nop.kind, StepKind::Instruction { opcode: 0xea });
    let forced = cpu.cycle(&mut ram).unwrap();
    assert_eq!(
        (forced.bus.address, forced.bus.data, forced.bus.sync),
        (0x8001, 0xea, true)
    );
    assert!(forced.completed.is_none());
    assert_eq!(cpu.debug_state().execution.unwrap().kind, StepKind::Reset);
}

#[test]
fn short_reset_preserves_distinct_pc_and_address_latches_after_a_suppressed_push() {
    let (mut cpu, mut ram) = setup(0x8345, &[0x00, 0xea], 0x29);
    ram.load(0xfffc, &[0x34, 0xb1]).unwrap();
    ram.write(0x01fd, 0x5c);
    cpu.set_reset_line(true);
    // Independent revD variant: PCL=$47 contends with DL=$5C at ADH.
    // SYNC sees $44FC, while the following dummy read sees stored PC=$47FC.
    let expected = [
        (0x8345, 0x00, true),
        (0x8346, 0xea, false),
        (0x01fd, 0x5c, false),
        (0x44fc, 0xea, true),
        (0x47fc, 0xea, false),
        (0x01fd, 0x5c, false),
        (0x01fc, 0xea, false),
        (0x01fb, 0xea, false),
        (0xfffc, 0x34, false),
        (0xfffd, 0xb1, false),
    ];
    for (index, &(address, data, sync)) in expected.iter().enumerate() {
        if index == 1 {
            cpu.set_reset_line(false);
        }
        let cycle = cpu.cycle(&mut ram).unwrap();
        assert_eq!(
            (cycle.bus.address, cycle.bus.data, cycle.bus.sync),
            (address, data, sync),
            "cycle {index}"
        );
        assert_eq!(cycle.bus.direction, hesper_cpu6502::Direction::Read);
        if index == expected.len() - 1 {
            let reset = cycle.completed.unwrap();
            assert_eq!(reset.kind, StepKind::Reset);
            assert_eq!((reset.after.pc, reset.after.sp), (0xb134, 0xfa));
        }
    }
    assert_eq!(ram.as_slice()[0x01fd], 0x5c);
}

#[test]
fn held_physical_reset_exhausts_a_finite_step_budget_and_resumes_after_release() {
    let (mut cpu, mut ram) = setup(0x8000, &[0xea], 0x29);
    ram.load(0xfffc, &[0x34, 0xb1]).unwrap();
    cpu.set_reset_line(true);
    // Pass the original NOP and assertion synchronizer before asking for a step.
    for _ in 0..4 {
        cpu.cycle(&mut ram).unwrap();
    }
    assert!(matches!(
        cpu.step(&mut ram),
        Err(CpuError::CycleBudgetExceeded { budget: 7, .. })
    ));
    cpu.set_reset_line(false);
    // The fixed NOP trace takes nine cycles from release through vector high.
    // A timeout must preserve this physical entry, not restart a host reset.
    assert!(matches!(
        cpu.step_with_cycle_budget(&mut ram, 8),
        Err(CpuError::CycleBudgetExceeded { budget: 8, .. })
    ));
    let reset = cpu.step_with_cycle_budget(&mut ram, 1).unwrap();
    assert_eq!(reset.kind, StepKind::Reset);
    assert_eq!((reset.after.pc, reset.after.sp), (0xb134, 0xfa));
    let instruction = cpu.step(&mut ram).unwrap();
    assert_eq!(instruction.kind, StepKind::Instruction { opcode: 0xea });
    assert_eq!(
        (instruction.address, instruction.after.pc),
        (0xb134, 0xb135)
    );
}
