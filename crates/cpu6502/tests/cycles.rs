use hesper_cpu6502::{
    Bus, BusCycle, ClockPhase, Cpu, CpuError, Direction, ExecutionPhase, Ram, Registers, Status,
    StepKind,
};

#[derive(Default)]
struct Device {
    ram: Ram,
    accesses: Vec<(u16, u8, Direction)>,
    side_effect: Option<u16>,
}

impl Bus for Device {
    fn read(&mut self, address: u16) -> u8 {
        let value = self.ram.read(address);
        self.accesses.push((address, value, Direction::Read));
        if self.side_effect == Some(address) {
            self.ram.write(address, value.wrapping_add(1));
        }
        value
    }
    fn write(&mut self, address: u16, value: u8) {
        self.accesses.push((address, value, Direction::Write));
        self.ram.write(address, value);
    }
}

fn cpu() -> Cpu {
    Cpu::from_registers(Registers {
        pc: 0x8000,
        sp: 0xfd,
        status: Status::from_bits(0x24),
        ..Registers::default()
    })
}

#[test]
fn cycles_are_real_transactions_and_dummy_reads_have_side_effects() {
    let mut cpu = cpu();
    let mut bus = Device {
        side_effect: Some(0x40),
        ..Device::default()
    };
    bus.ram.load(0x8000, &[0xb5, 0x40]).unwrap(); // LDA $40,X with X=0
    bus.ram.write(0x40, 0x10);
    for (index, (address, data)) in [(0x8000, 0xb5), (0x8001, 0x40), (0x40, 0x10), (0x40, 0x11)]
        .into_iter()
        .enumerate()
    {
        let cycle = cpu.cycle(&mut bus).unwrap();
        assert_eq!(
            cycle.bus,
            BusCycle {
                address,
                data,
                direction: Direction::Read,
                sync: index == 0
            }
        );
        assert_eq!(bus.accesses.len(), index + 1);
        assert_eq!(cycle.completed.is_some(), index == 3);
        if index != 3 {
            assert_eq!(cpu.registers().a, 0);
        }
        let _debug = format!("{cycle:?} {:?}", cpu.registers());
        assert_eq!(bus.accesses.len(), index + 1);
    }
    assert_eq!(cpu.registers().a, 0x11);
    assert_eq!(bus.ram.as_slice()[0x40], 0x12);
    assert!(cpu.at_instruction_boundary());
}

#[test]
fn step_can_finish_any_partial_jsr_and_keeps_original_snapshot_and_cycle_count() {
    for prefix in 0..6 {
        let mut cpu = cpu();
        let before = cpu.registers();
        let mut bus = Device::default();
        bus.ram.load(0x8000, &[0x20, 0x34, 0x12]).unwrap();
        for _ in 0..prefix {
            assert!(cpu.cycle(&mut bus).unwrap().completed.is_none());
        }
        assert_eq!(cpu.at_instruction_boundary(), prefix == 0);
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.before, before);
        assert_eq!(step.cycles, 6);
        assert_eq!(
            step.after,
            Registers {
                pc: 0x1234,
                sp: 0xfb,
                ..before
            }
        );
        assert_eq!(
            bus.accesses,
            [
                (0x8000, 0x20, Direction::Read),
                (0x8001, 0x34, Direction::Read),
                (0x01fd, 0, Direction::Read),
                (0x01fd, 0x80, Direction::Write),
                (0x01fc, 0x02, Direction::Write),
                (0x8002, 0x12, Direction::Read),
            ]
        );
    }
}

#[test]
fn rmw_latches_data_and_writes_old_then_modified_value_on_separate_cycles() {
    let mut cpu = cpu();
    let mut bus = Device::default();
    bus.ram.load(0x8000, &[0xee, 0x00, 0x20]).unwrap(); // INC $2000
    bus.ram.write(0x2000, 0xff);
    for _ in 0..4 {
        assert!(cpu.cycle(&mut bus).unwrap().completed.is_none());
    }
    // A host device changing storage after the operand read cannot change its latch.
    bus.ram.write(0x2000, 0x50);
    let old = cpu.cycle(&mut bus).unwrap();
    assert_eq!(
        old.bus,
        BusCycle {
            address: 0x2000,
            data: 0xff,
            direction: Direction::Write,
            sync: false
        }
    );
    assert!(old.completed.is_none());
    let new = cpu.cycle(&mut bus).unwrap();
    assert_eq!(new.bus.data, 0);
    assert_eq!(new.completed.unwrap().cycles, 6);
    assert_eq!(cpu.registers().status.bits(), 0x26);
    assert_eq!(bus.ram.as_slice()[0x2000], 0);
}

#[test]
fn indexed_store_always_performs_intermediate_read_even_without_crossing() {
    for (base, intermediate, target) in [
        (0x20ff, 0x2000, 0x2100),
        (0xffff, 0xff00, 0x0000),
        (0x2000, 0x2001, 0x2001),
    ] {
        let mut cpu = Cpu::from_registers(Registers {
            a: 0x55,
            x: 1,
            ..cpu().registers()
        });
        let mut bus = Device::default();
        let [lo, hi] = u16::to_le_bytes(base);
        bus.ram.load(0x8000, &[0x9d, lo, hi]).unwrap();
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.cycles, 5);
        assert_eq!(
            &bus.accesses[3..],
            &[
                (intermediate, 0, Direction::Read),
                (target, 0x55, Direction::Write)
            ]
        );
    }
}

#[test]
fn reset_is_seven_observable_cycles_with_stack_reads_and_a_separate_event() {
    let before = Registers {
        pc: 0x4567,
        sp: 0,
        a: 0x12,
        x: 0x34,
        y: 0x56,
        status: Status::from_bits(0xeb),
    };
    let mut cpu = Cpu::from_registers(before);
    let mut bus = Device::default();
    bus.ram.load(0xfffc, &[0x00, 0x80]).unwrap();
    cpu.begin_reset();
    for (index, address) in [0x4567, 0x4567, 0x0100, 0x01ff, 0x01fe, 0xfffc, 0xfffd]
        .into_iter()
        .enumerate()
    {
        let cycle = cpu.cycle(&mut bus).unwrap();
        assert_eq!(
            (cycle.bus.address, cycle.bus.direction),
            (address, Direction::Read)
        );
        if index < 6 {
            assert!(cycle.completed.is_none());
        } else {
            let step = cycle.completed.unwrap();
            assert_eq!(step.kind, StepKind::Reset);
            assert_eq!(step.cycles, 7);
            assert_eq!(step.before, before);
            assert_eq!(
                step.after,
                Registers {
                    pc: 0x8000,
                    sp: 0xfd,
                    status: Status::from_bits(0xef),
                    ..before
                }
            );
        }
    }
    assert!(cpu.at_instruction_boundary());
}

#[test]
fn rdy_repeats_real_reads_and_long_waits_are_included_without_u8_overflow() {
    let mut cpu = cpu();
    let before = cpu.registers();
    let mut bus = Device::default();
    bus.ram.write(0x8000, 0xea);
    cpu.set_ready(false);
    for _ in 0..1000 {
        let cycle = cpu.cycle(&mut bus).unwrap();
        assert!(cycle.stalled && cycle.completed.is_none() && cycle.bus.sync);
        assert_eq!(cycle.bus.address, 0x8000);
        assert_eq!(cpu.registers(), before);
    }
    cpu.set_ready(true);
    let step = cpu.step(&mut bus).unwrap();
    assert_eq!(step.before, before);
    assert_eq!(step.cycles, 1002);
    assert_eq!(bus.accesses.len(), 1002);
    assert_eq!(step.after.pc, 0x8001);
}

#[test]
fn rdy_does_not_stop_either_write_of_a_nmos_rmw() {
    let mut cpu = cpu();
    let mut bus = Device::default();
    bus.ram.load(0x8000, &[0xe6, 0x40, 0xea]).unwrap();
    bus.ram.write(0x40, 0x7f);
    for _ in 0..3 {
        cpu.cycle(&mut bus).unwrap();
    }
    cpu.set_ready(false);
    for value in [0x7f, 0x80] {
        let cycle = cpu.cycle(&mut bus).unwrap();
        assert!(!cycle.stalled);
        assert_eq!(
            (cycle.bus.address, cycle.bus.data, cycle.bus.direction),
            (0x40, value, Direction::Write)
        );
        if value == 0x80 {
            assert_eq!(cycle.completed.unwrap().cycles, 5);
        }
    }
    assert!(cpu.cycle(&mut bus).unwrap().stalled);
    assert_eq!(cpu.registers().pc, 0x8002);
}

#[test]
fn read_side_effects_continue_during_rdy_and_only_resumed_data_is_latched() {
    let mut cpu = cpu();
    let mut bus = Device {
        side_effect: Some(0x40),
        ..Device::default()
    };
    bus.ram.load(0x8000, &[0xa5, 0x40]).unwrap();
    bus.ram.write(0x40, 0x10);
    for _ in 0..2 {
        cpu.cycle(&mut bus).unwrap();
    }
    cpu.set_ready(false);
    for expected in [0x10, 0x11, 0x12] {
        assert_eq!(cpu.cycle(&mut bus).unwrap().bus.data, expected);
        assert_eq!(cpu.registers().a, 0);
    }
    cpu.set_ready(true);
    let step = cpu.step(&mut bus).unwrap();
    assert_eq!((step.after.a, step.cycles), (0x13, 6));
}

#[test]
fn stalled_step_has_a_finite_budget_and_can_resume_without_resetting_state() {
    let mut cpu = cpu();
    let mut bus = Device::default();
    bus.ram.write(0x8000, 0xea);
    assert_eq!(
        cpu.step_with_cycle_budget(&mut bus, 0),
        Err(CpuError::CycleBudgetExceeded {
            address: 0x8000,
            budget: 0
        })
    );
    assert!(bus.accesses.is_empty());
    cpu.set_ready(false);
    assert_eq!(
        cpu.step(&mut bus),
        Err(CpuError::CycleBudgetExceeded {
            address: 0x8000,
            budget: 7
        })
    );
    assert_eq!(bus.accesses.len(), 7);
    cpu.set_ready(true);
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 9);
}

#[test]
fn reset_uses_the_same_bounded_engine_and_can_resume_after_rdy() {
    let mut cpu = cpu();
    let mut bus = Device::default();
    bus.ram.load(0xfffc, &[0x34, 0x12]).unwrap();
    cpu.set_ready(false);
    assert!(matches!(
        cpu.reset(&mut bus),
        Err(CpuError::CycleBudgetExceeded { budget: 7, .. })
    ));
    assert_eq!(bus.accesses.len(), 7);
    assert!(
        bus.accesses
            .iter()
            .all(|&(addr, _, direction)| addr == 0x8000 && direction == Direction::Read)
    );
    cpu.set_ready(true);
    let step = cpu.step(&mut bus).unwrap();
    assert_eq!(
        (step.kind, step.after.pc, step.cycles),
        (StepKind::Reset, 0x1234, 14)
    );
}

#[test]
fn clv_overlaps_so_and_a_held_low_so_does_not_retrigger() {
    // RevD: an SO edge at CLV's second phi2 loses to its overlapping V write.
    // Holding SO low through subsequent NOPs must not create another edge.
    let mut cpu = cpu();
    let mut bus = Device::default();
    bus.ram
        .load(0x8000, &[0xb8, 0xea, 0xea, 0xea, 0xea])
        .unwrap();
    for half in 0..16 {
        if half == 3 {
            cpu.set_so_line(true);
        }
        cpu.half_cycle(&mut bus).unwrap();
    }
    assert!(!cpu.registers().status.overflow);
    cpu.set_so_line(false);
    cpu.cycle(&mut bus).unwrap();
    cpu.set_so_line(true);
    cpu.cycle(&mut bus).unwrap();
    cpu.cycle(&mut bus).unwrap();
    assert!(cpu.registers().status.overflow);
}

#[test]
fn debug_snapshot_observes_pending_pins_and_rmw_latches_without_bus_access() {
    let mut cpu = cpu();
    let mut bus = Device::default();
    bus.ram.load(0x8000, &[0xe6, 0x40]).unwrap();
    bus.ram.write(0x40, 0x7f);
    for _ in 0..3 {
        cpu.cycle(&mut bus).unwrap();
    }
    let state = cpu.debug_state();
    let execution = state.execution.unwrap();
    assert_eq!(execution.phase, ExecutionPhase::RmwOld);
    assert_eq!(execution.instruction_address, 0x8000);
    assert_eq!(
        (
            execution.effective_address,
            execution.data,
            execution.cycles
        ),
        (0x40, 0x7f, 3)
    );
    cpu.set_nmi_line(true);
    cpu.set_ready(false);
    cpu.half_cycle(&mut bus).unwrap();
    let state = cpu.debug_state();
    assert_eq!(state.next_clock_phase, ClockPhase::Phi2);
    assert!(state.pins.nmi && !state.pins.ready && !state.latches.nmi_edge);
    assert_eq!(bus.accesses.len(), 3);
    cpu.half_cycle(&mut bus).unwrap();
    assert!(cpu.debug_state().latches.nmi_edge);
    assert_eq!(bus.accesses.len(), 4);
}

#[test]
fn so_edges_in_adjacent_half_cycles_can_change_a_branch_decision() {
    for edge in [4, 5] {
        let mut cpu = cpu();
        let mut bus = Device::default();
        bus.ram.load(0x8000, &[0xea, 0x50, 1, 0xea, 0xea]).unwrap();
        for half in 0..8 {
            assert_eq!(
                cpu.next_clock_phase(),
                if half % 2 == 0 {
                    ClockPhase::Phi1
                } else {
                    ClockPhase::Phi2
                }
            );
            if half == edge {
                cpu.set_so_line(true);
            }
            let result = cpu.half_cycle(&mut bus).unwrap();
            assert_eq!(result.is_some(), half % 2 == 1);
        }
        if edge == 4 {
            assert!(cpu.at_instruction_boundary());
            assert_eq!(cpu.registers().pc, 0x8003);
        } else {
            assert!(!cpu.at_instruction_boundary());
            assert_eq!(cpu.step(&mut bus).unwrap().after.pc, 0x8004);
        }
    }
}
