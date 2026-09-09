use hesper_cpu6502::{Bus, BusCycle, Cpu, Direction, Ram, Registers, Status, StepKind};

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
