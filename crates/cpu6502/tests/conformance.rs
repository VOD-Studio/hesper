use hesper_cpu6502::{Bus, Cpu, CpuError, LoadError, RAM_SIZE, Ram, Registers, Status, Step};

// Expectations are hand-written from MOS 6500-50A, chapters 5/7/8/9 and appendix B.
// See docs/references.md for the NMOS assumptions and evidence limits.
fn initial() -> Registers {
    Registers {
        a: 0x55,
        x: 0x33,
        y: 0x66,
        sp: 0xfd,
        pc: 0x8000,
        status: Status::from_bits(0xef),
    }
}

fn check_step(cpu: &mut Cpu, bus: &mut dyn Bus, after: Registers, cycles: u8) -> Step {
    let before = cpu.registers();
    let step = cpu.step(bus).expect("supported instruction");
    assert_eq!(cpu.registers(), after);
    assert_eq!(step.before, before);
    assert_eq!(step.after, after);
    assert_eq!(step.address, before.pc);
    assert_eq!(step.cycles, cycles);
    step
}

#[derive(Debug, PartialEq, Eq)]
enum Access {
    Read(u16),
    Write(u16, u8),
}

#[derive(Default)]
struct RecordingBus {
    ram: Ram,
    accesses: Vec<Access>,
}

impl Bus for RecordingBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.accesses.push(Access::Read(addr));
        self.ram.read(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.accesses.push(Access::Write(addr, value));
        self.ram.write(addr, value);
    }
}

#[test]
fn deterministic_construction_is_separate_from_reset() {
    let cpu = Cpu::new();
    let state = cpu.registers();
    assert_eq!(
        (state.a, state.x, state.y, state.sp, state.pc),
        (0, 0, 0, 0, 0)
    );
    assert_eq!(state.status.bits(), 0x20);
    let mut ram = Ram::new();
    ram.load(0xfffc, &[0xcd, 0xab]).unwrap();
    let mut cpu = cpu;
    assert_eq!(cpu.reset(&mut ram), 7);
    assert_eq!(cpu.registers().pc, 0xabcd);
    assert_eq!(cpu.registers().sp, 0xfd);
    assert_eq!(cpu.registers().status.bits(), 0x24);
}

#[test]
fn reset_reads_little_endian_vector_and_preserves_nmos_state() {
    for (sp, expected_sp) in [(0x00, 0xfd), (0x02, 0xff), (0x80, 0x7d), (0xff, 0xfc)] {
        for (flags, expected_flags) in [(0x20, 0x24), (0xeb, 0xef)] {
            let before = Registers {
                sp,
                status: Status::from_bits(flags),
                ..initial()
            };
            let mut cpu = Cpu::from_registers(before);
            let mut bus = RecordingBus::default();
            bus.ram.load(0xfffc, &[0x34, 0x12]).unwrap();
            bus.ram.load(0x0100, &[0xa5; 256]).unwrap();
            assert_eq!(cpu.reset(&mut bus), 7);
            assert_eq!(
                cpu.registers(),
                Registers {
                    pc: 0x1234,
                    sp: expected_sp,
                    status: Status::from_bits(expected_flags),
                    ..before
                }
            );
            // M0's documented sparse reset accesses, not a cycle-exact bus trace.
            assert_eq!(bus.accesses, [Access::Read(0xfffc), Access::Read(0xfffd)]);
            assert_eq!(&bus.ram.as_slice()[0x0100..0x0200], &[0xa5; 256]);
        }
    }
}

#[test]
fn repeated_reset_decrements_current_sp_instead_of_reinitializing_registers() {
    let mut cpu = Cpu::new();
    let mut ram = Ram::new();
    ram.load(0xfffc, &[0x00, 0x80]).unwrap();
    ram.load(0x8000, &[0xa2, 0x10, 0x9a, 0xa9, 0x7f]).unwrap();
    cpu.reset(&mut ram);
    for _ in 0..3 {
        cpu.step(&mut ram).unwrap();
    }
    assert_eq!(cpu.reset(&mut ram), 7);
    let state = cpu.registers();
    assert_eq!(
        (state.a, state.x, state.sp, state.pc),
        (0x7f, 0x10, 0x0d, 0x8000)
    );
}

#[test]
fn status_encoding_has_six_flags_and_no_persistent_break_bit() {
    let all = Status {
        carry: true,
        zero: true,
        interrupt_disable: true,
        decimal: true,
        overflow: true,
        negative: true,
    };
    assert_eq!(all.bits(), 0xef);
    assert_eq!(Status::from_bits(0xff), all);
    assert_eq!(Status::from_bits(0x30), Status::default());
    assert_eq!(
        Status {
            carry: true,
            ..Status::default()
        }
        .bits(),
        0x21
    );
    assert_eq!(
        Status {
            zero: true,
            ..Status::default()
        }
        .bits(),
        0x22
    );
    assert_eq!(
        Status {
            interrupt_disable: true,
            ..Status::default()
        }
        .bits(),
        0x24
    );
    assert_eq!(
        Status {
            decimal: true,
            ..Status::default()
        }
        .bits(),
        0x28
    );
    assert_eq!(
        Status {
            overflow: true,
            ..Status::default()
        }
        .bits(),
        0x60
    );
    assert_eq!(
        Status {
            negative: true,
            ..Status::default()
        }
        .bits(),
        0xa0
    );
}

#[test]
fn ram_load_checks_ranges_without_wrapping_or_partial_writes() {
    let mut ram = Ram::new();
    assert_eq!(ram.as_slice().len(), RAM_SIZE);
    assert!(ram.as_slice().iter().all(|&value| value == 0));
    ram.load(0, &vec![0xa5; 65_536]).unwrap();
    ram.load(0xffff, &[]).unwrap();
    ram.load(0xffff, &[0x12]).unwrap();
    assert_eq!(ram.read(0xffff), 0x12);
    assert_eq!(
        ram.load(0xffff, &[1, 2]),
        Err(LoadError {
            start: 0xffff,
            len: 2
        })
    );
    assert_eq!(ram.read(0xffff), 0x12);
    assert_eq!(ram.read(0), 0xa5);
    assert_eq!(
        ram.load(0, &vec![0; 65_537]),
        Err(LoadError {
            start: 0,
            len: 65_537
        })
    );
    assert!(ram.as_slice()[..0xffff].iter().all(|&value| value == 0xa5));
    ram.write(0, 0x56);
    assert_eq!(ram.read(0), 0x56);
}

#[test]
fn lda_modes_and_ldx_set_nz_and_preserve_other_flags() {
    for (opcode, length, cycles) in [(0xa9, 2, 2), (0xa5, 2, 3), (0xad, 3, 4), (0xa2, 2, 2)] {
        for (value, flags, expected_flags) in [
            (0x00, 0xed, 0x6f),
            (0x7f, 0xef, 0x6d),
            (0x80, 0x6f, 0xed),
            (0x00, 0xa0, 0x22),
            (0x7f, 0xa2, 0x20),
            (0x80, 0x22, 0xa0),
        ] {
            let mut ram = Ram::new();
            let program = match opcode {
                0xa5 => vec![opcode, 0xff],
                0xad => vec![opcode, 0x34, 0x12],
                _ => vec![opcode, value],
            };
            ram.load(0x8000, &program).unwrap();
            ram.write(0x00ff, value);
            ram.write(0x1234, value);
            ram.write(0x3412, 0x42); // Catch reversed absolute operand bytes.
            let before = Registers {
                status: Status::from_bits(flags),
                ..initial()
            };
            let mut cpu = Cpu::from_registers(before);
            let mut after = Registers {
                pc: 0x8000 + length,
                status: Status::from_bits(expected_flags),
                ..before
            };
            if opcode == 0xa2 {
                after.x = value;
            } else {
                after.a = value;
            }
            check_step(&mut cpu, &mut ram, after, cycles);
        }
    }
}

#[test]
fn sta_modes_preserve_flags_and_absolute_x_always_costs_five_cycles() {
    let cases: &[(&[u8], u8, u16, u16, u8)] = &[
        (&[0x85, 0xff], 0x00, 0x00ff, 0x8002, 3),
        (&[0x8d, 0x34, 0x12], 0x00, 0x1234, 0x8003, 4),
        (&[0x9d, 0x00, 0x12], 0x01, 0x1201, 0x8003, 5),
        (&[0x9d, 0xff, 0x12], 0x01, 0x1300, 0x8003, 5),
        (&[0x9d, 0xff, 0xff], 0x01, 0x0000, 0x8003, 5),
        (&[0x9d, 0xff, 0xff], 0xff, 0x00fe, 0x8003, 5),
    ];
    for &(program, x, target, pc, cycles) in cases {
        for flags in [0x20, 0xef] {
            let mut bus = RecordingBus::default();
            bus.ram.load(0x8000, program).unwrap();
            let before = Registers {
                x,
                status: Status::from_bits(flags),
                ..initial()
            };
            let mut cpu = Cpu::from_registers(before);
            check_step(&mut cpu, &mut bus, Registers { pc, ..before }, cycles);
            let writes: Vec<_> = bus
                .accesses
                .iter()
                .filter(|a| matches!(a, Access::Write(..)))
                .collect();
            assert_eq!(writes, [&Access::Write(target, 0x55)]);
            assert_eq!(bus.ram.read(target), 0x55);
        }
    }
}

#[test]
fn tax_txa_set_nz_but_txs_preserves_all_flags() {
    for opcode in [0xaa, 0x8a, 0x9a] {
        for (value, expected_flags) in [(0x00, 0x6f), (0x7f, 0x6d), (0x80, 0xed)] {
            let mut ram = Ram::new();
            ram.load(0x8000, &[opcode]).unwrap();
            let before = if opcode == 0xaa {
                Registers {
                    a: value,
                    ..initial()
                }
            } else {
                Registers {
                    x: value,
                    ..initial()
                }
            };
            let mut after = Registers {
                pc: 0x8001,
                ..before
            };
            match opcode {
                0xaa => after.x = value,
                0x8a => after.a = value,
                _ => after.sp = value,
            }
            if opcode != 0x9a {
                after.status = Status::from_bits(expected_flags);
            }
            check_step(&mut Cpu::from_registers(before), &mut ram, after, 2);
        }
    }
}

#[test]
fn inx_dex_wrap_and_update_only_nz() {
    for (opcode, x, expected_x, flags) in [
        (0xe8, 0x00, 0x01, 0x6d),
        (0xe8, 0x7f, 0x80, 0xed),
        (0xe8, 0xff, 0x00, 0x6f),
        (0xca, 0x00, 0xff, 0xed),
        (0xca, 0x80, 0x7f, 0x6d),
        (0xca, 0x01, 0x00, 0x6f),
    ] {
        let mut ram = Ram::new();
        ram.load(0x8000, &[opcode]).unwrap();
        let before = Registers { x, ..initial() };
        check_step(
            &mut Cpu::from_registers(before),
            &mut ram,
            Registers {
                x: expected_x,
                pc: 0x8001,
                status: Status::from_bits(flags),
                ..before
            },
            2,
        );
    }
}

#[test]
fn cpx_is_unsigned_compare_with_wrapped_subtraction_nz() {
    for (x, operand, expected_flags) in [
        (0x00, 0x00, 0x6f),
        (0x42, 0x42, 0x6f),
        (0xff, 0xff, 0x6f),
        (0x01, 0x00, 0x6d),
        (0x00, 0x01, 0xec),
        (0x80, 0x01, 0x6d),
        (0x00, 0xff, 0x6c),
        (0x7f, 0xff, 0xec),
        (0xff, 0x00, 0xed),
    ] {
        for flags in [0x6c, 0xef] {
            let mut ram = Ram::new();
            ram.load(0x8000, &[0xe0, operand]).unwrap();
            let before = Registers {
                x,
                status: Status::from_bits(flags),
                ..initial()
            };
            check_step(
                &mut Cpu::from_registers(before),
                &mut ram,
                Registers {
                    pc: 0x8002,
                    status: Status::from_bits(expected_flags),
                    ..before
                },
                2,
            );
        }
    }
}

#[test]
fn beq_bne_signed_offsets_pages_and_address_wrap() {
    // PC, encoded displacement, target if taken, cycles if taken.
    let cases = [
        (0x8000, 0x00, 0x8002, 3),
        (0x8000, 0x01, 0x8003, 3),
        (0x8000, 0x7f, 0x8081, 3),
        (0x8080, 0x80, 0x8002, 3),
        (0x8000, 0xff, 0x8001, 3),
        (0x80fd, 0x01, 0x8100, 4),
        (0x8100, 0xfd, 0x80ff, 4),
        (0x80fe, 0x00, 0x8100, 3),
        (0x0000, 0xfd, 0xffff, 4),
        (0xfffd, 0x01, 0x0000, 4),
        (0xfffe, 0x00, 0x0000, 3),
        (0xfffe, 0x80, 0xff80, 4),
        (0xffff, 0xff, 0x0000, 3),
        (0xffff, 0xfe, 0xffff, 4),
    ];
    for opcode in [0xd0, 0xf0] {
        for (pc, offset, target, cycles) in cases {
            for taken in [false, true] {
                let zero = if opcode == 0xf0 { taken } else { !taken };
                let before = Registers {
                    pc,
                    status: Status {
                        zero,
                        ..initial().status
                    },
                    ..initial()
                };
                let mut ram = Ram::new();
                // Place separate bytes when the emulated instruction crosses $FFFF.
                ram.write(pc, opcode);
                ram.write(pc.wrapping_add(1), offset);
                let after = Registers {
                    pc: if taken { target } else { pc.wrapping_add(2) },
                    ..before
                };
                check_step(
                    &mut Cpu::from_registers(before),
                    &mut ram,
                    after,
                    if taken { cycles } else { 2 },
                );
            }
        }
    }
}

#[test]
fn operand_and_opcode_fetches_wrap_at_ffff() {
    struct Case {
        bytes: &'static [(u16, u8)],
        pc: u16,
        expected_pc: u16,
        cycles: u8,
    }
    let cases = [
        Case {
            bytes: &[(0xffff, 0xa9), (0x0000, 0x80)],
            pc: 0xffff,
            expected_pc: 0x0001,
            cycles: 2,
        },
        Case {
            bytes: &[
                (0xfffe, 0xad),
                (0xffff, 0x34),
                (0x0000, 0x12),
                (0x1234, 0x80),
            ],
            pc: 0xfffe,
            expected_pc: 0x0001,
            cycles: 4,
        },
        Case {
            bytes: &[
                (0xffff, 0xad),
                (0x0000, 0x34),
                (0x0001, 0x12),
                (0x1234, 0x80),
            ],
            pc: 0xffff,
            expected_pc: 0x0002,
            cycles: 4,
        },
    ];
    for Case {
        bytes,
        pc,
        expected_pc,
        cycles,
    } in cases
    {
        let mut ram = Ram::new();
        for &(addr, byte) in bytes {
            ram.write(addr, byte);
        }
        let before = Registers { pc, ..initial() };
        check_step(
            &mut Cpu::from_registers(before),
            &mut ram,
            Registers {
                a: 0x80,
                pc: expected_pc,
                status: Status::from_bits(0xed),
                ..before
            },
            cycles,
        );
    }
    let mut ram = Ram::new();
    ram.write(0xffff, 0xea);
    let before = Registers {
        pc: 0xffff,
        ..initial()
    };
    check_step(
        &mut Cpu::from_registers(before),
        &mut ram,
        Registers { pc: 0, ..before },
        2,
    );
}

#[test]
fn jmp_absolute_reads_little_endian_and_preserves_flags() {
    let mut ram = Ram::new();
    ram.load(0x8000, &[0x4c, 0xcd, 0xab]).unwrap();
    check_step(
        &mut Cpu::from_registers(initial()),
        &mut ram,
        Registers {
            pc: 0xabcd,
            ..initial()
        },
        3,
    );
}

#[test]
fn jmp_indirect_uses_nmos_same_page_high_byte() {
    for (pointer, high_addr, wrong_addr) in [
        (0x1234_u16, 0x1235, 0x1236),
        (0x12ff, 0x1200, 0x1300),
        (0x00ff, 0x0000, 0x0100),
        (0xffff, 0xff00, 0x0000),
    ] {
        let mut bus = RecordingBus::default();
        let [lo, hi] = pointer.to_le_bytes();
        bus.ram.load(0x8000, &[0x6c, lo, hi]).unwrap();
        bus.ram.write(pointer, 0x78);
        bus.ram.write(high_addr, 0x56);
        bus.ram.write(wrong_addr, 0x99);
        check_step(
            &mut Cpu::from_registers(initial()),
            &mut bus,
            Registers {
                pc: 0x5678,
                ..initial()
            },
            5,
        );
        assert_eq!(
            bus.accesses,
            [
                Access::Read(0x8000),
                Access::Read(0x8001),
                Access::Read(0x8002),
                Access::Read(pointer),
                Access::Read(high_addr),
            ]
        );
    }
}

#[test]
fn pha_pla_are_lifo_with_stack_page_and_sp_wrap() {
    let before = Registers {
        a: 0x80,
        sp: 0x00,
        ..initial()
    };
    let mut cpu = Cpu::from_registers(before);
    let mut ram = Ram::new();
    ram.load(0x8000, &[0x48, 0xa9, 0x00, 0x48, 0x68, 0x68])
        .unwrap();
    check_step(
        &mut cpu,
        &mut ram,
        Registers {
            sp: 0xff,
            pc: 0x8001,
            ..before
        },
        3,
    );
    assert_eq!(ram.read(0x0100), 0x80);
    let zero = Registers {
        a: 0,
        sp: 0xff,
        pc: 0x8003,
        status: Status::from_bits(0x6f),
        ..before
    };
    check_step(&mut cpu, &mut ram, zero, 2);
    check_step(
        &mut cpu,
        &mut ram,
        Registers {
            sp: 0xfe,
            pc: 0x8004,
            ..zero
        },
        3,
    );
    assert_eq!(ram.read(0x01ff), 0x00);
    check_step(&mut cpu, &mut ram, Registers { pc: 0x8005, ..zero }, 4);
    check_step(
        &mut cpu,
        &mut ram,
        Registers {
            pc: 0x8006,
            status: Status::from_bits(0xed),
            ..before
        },
        4,
    );
    assert_eq!(ram.read(0x0000), 0x00);
    assert_eq!(ram.read(0x0200), 0x00);
}

#[test]
fn pla_sets_and_clears_nz_without_changing_other_flags() {
    for (value, expected_flags) in [(0x00, 0x6f), (0x7f, 0x6d), (0x80, 0xed)] {
        let mut ram = Ram::new();
        ram.load(0x8000, &[0x68]).unwrap();
        ram.write(0x01fe, value);
        check_step(
            &mut Cpu::from_registers(initial()),
            &mut ram,
            Registers {
                a: value,
                sp: 0xfe,
                pc: 0x8001,
                status: Status::from_bits(expected_flags),
                ..initial()
            },
            4,
        );
    }
}

#[test]
fn jsr_rts_return_address_stack_order_and_sp_wrap() {
    let before = Registers {
        pc: 0x4000,
        sp: 0x00,
        ..initial()
    };
    let mut cpu = Cpu::from_registers(before);
    let mut bus = RecordingBus::default();
    bus.ram.load(0x4000, &[0x20, 0x34, 0x12]).unwrap();
    bus.ram.write(0x1234, 0x60);
    check_step(
        &mut cpu,
        &mut bus,
        Registers {
            pc: 0x1234,
            sp: 0xfe,
            ..before
        },
        6,
    );
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0x4000),
            Access::Read(0x4001),
            Access::Write(0x0100, 0x40),
            Access::Write(0x01ff, 0x02),
            Access::Read(0x4002),
        ]
    );
    bus.accesses.clear();
    check_step(
        &mut cpu,
        &mut bus,
        Registers {
            pc: 0x4003,
            ..before
        },
        6,
    );
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0x1234),
            Access::Read(0x01ff),
            Access::Read(0x0100)
        ]
    );
}

#[test]
fn jsr_late_operand_fetch_handles_code_overlapping_stack() {
    let mut ram = Ram::new();
    ram.load(0x01fd, &[0x20, 0x34, 0x12]).unwrap();
    let before = Registers {
        pc: 0x01fd,
        sp: 0xff,
        ..initial()
    };
    // Saved PC $01FF overwrites the high operand at $01FF before it is read.
    check_step(
        &mut Cpu::from_registers(before),
        &mut ram,
        Registers {
            pc: 0x0134,
            sp: 0xfd,
            ..before
        },
        6,
    );
    assert_eq!(ram.read(0x01ff), 0x01);
    assert_eq!(ram.read(0x01fe), 0xff);
}

#[test]
fn jsr_operand_fetch_and_rts_increment_wrap_program_counter() {
    let mut ram = Ram::new();
    ram.write(0xfffe, 0x20);
    ram.write(0xffff, 0x34);
    ram.write(0x0000, 0x12);
    ram.write(0x1234, 0x60);
    let before = Registers {
        pc: 0xfffe,
        ..initial()
    };
    let mut cpu = Cpu::from_registers(before);
    check_step(
        &mut cpu,
        &mut ram,
        Registers {
            pc: 0x1234,
            sp: 0xfb,
            ..before
        },
        6,
    );
    assert_eq!((ram.read(0x01fd), ram.read(0x01fc)), (0x00, 0x00));
    check_step(
        &mut cpu,
        &mut ram,
        Registers {
            pc: 0x0001,
            ..before
        },
        6,
    );

    ram.load(0x8000, &[0x60]).unwrap();
    ram.load(0x01fe, &[0xff, 0xff]).unwrap();
    check_step(
        &mut Cpu::from_registers(initial()),
        &mut ram,
        Registers {
            pc: 0x0000,
            sp: 0xff,
            ..initial()
        },
        6,
    );
}

#[test]
fn nested_subroutines_restore_stack_and_caller() {
    let mut ram = Ram::new();
    ram.load(0x8000, &[0x20, 0x00, 0x90]).unwrap();
    ram.load(0x9000, &[0x20, 0x00, 0xa0, 0x60]).unwrap();
    ram.load(0xa000, &[0x48, 0x68, 0x60]).unwrap();
    let mut cpu = Cpu::from_registers(initial());
    let mut cycles = 0;
    for _ in 0..6 {
        cycles += u16::from(cpu.step(&mut ram).unwrap().cycles);
    }
    assert_eq!(cycles, 31); // JSR + JSR + PHA + PLA + RTS + RTS
    assert_eq!(
        cpu.registers(),
        Registers {
            pc: 0x8003,
            status: Status::from_bits(0x6d),
            ..initial()
        }
    );
}

#[test]
fn clc_sec_cld_and_nop_change_only_their_documented_flags() {
    for (opcode, flags, expected_flags) in [
        (0x18, 0xef, 0xee),
        (0x18, 0x20, 0x20),
        (0x38, 0xee, 0xef),
        (0x38, 0x21, 0x21),
        (0xd8, 0xef, 0xe7),
        (0xd8, 0x20, 0x20),
        (0xea, 0xef, 0xef),
        (0xea, 0x20, 0x20),
    ] {
        let mut ram = Ram::new();
        ram.load(0x8000, &[opcode]).unwrap();
        let before = Registers {
            status: Status::from_bits(flags),
            ..initial()
        };
        check_step(
            &mut Cpu::from_registers(before),
            &mut ram,
            Registers {
                pc: 0x8001,
                status: Status::from_bits(expected_flags),
                ..before
            },
            2,
        );
    }
}

#[test]
fn every_supported_opcode_preserves_unaffected_flags_when_set_or_clear() {
    // Masks specify the flags this instruction MUST leave alone (MOS appendix B).
    let cases = [
        (0xa9, 0x6d),
        (0xa5, 0x6d),
        (0xad, 0x6d),
        (0xa2, 0x6d),
        (0x85, 0xef),
        (0x8d, 0xef),
        (0x9d, 0xef),
        (0xaa, 0x6d),
        (0x8a, 0x6d),
        (0x9a, 0xef),
        (0xe8, 0x6d),
        (0xca, 0x6d),
        (0xe0, 0x6c),
        (0xd0, 0xef),
        (0xf0, 0xef),
        (0x4c, 0xef),
        (0x6c, 0xef),
        (0x20, 0xef),
        (0x60, 0xef),
        (0x48, 0xef),
        (0x68, 0x6d),
        (0x18, 0xee),
        (0x38, 0xee),
        (0xd8, 0xe7),
        (0xea, 0xef),
    ];
    for (opcode, preserved) in cases {
        for flags in [0x20, 0xef] {
            let mut ram = Ram::new();
            ram.load(0x8000, &[opcode, 0x00, 0x20]).unwrap();
            let mut cpu = Cpu::from_registers(Registers {
                status: Status::from_bits(flags),
                ..initial()
            });
            let step = cpu.step(&mut ram).unwrap();
            assert_eq!(
                step.after.status.bits() & preserved,
                flags & preserved,
                "opcode ${opcode:02X}"
            );
        }
    }
}

#[test]
fn unsupported_opcodes_including_brk_return_context_without_register_changes() {
    // Independent specifications, never obtained from the CPU decoder.
    let supported: Vec<u8> = include_str!("data/opcodes.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| u8::from_str_radix(line.split_whitespace().next().unwrap(), 16).unwrap())
        .collect();
    for opcode in 0..=u8::MAX {
        if supported.contains(&opcode) {
            continue;
        }
        let before = Registers {
            pc: 0xffff,
            ..initial()
        };
        let mut cpu = Cpu::from_registers(before);
        let mut bus = RecordingBus::default();
        bus.ram.write(0xffff, opcode);
        let err = cpu.step(&mut bus).unwrap_err();
        assert_eq!(
            err,
            CpuError::UnsupportedOpcode {
                address: 0xffff,
                opcode
            }
        );
        assert_eq!(cpu.registers(), before);
        assert_eq!(bus.accesses, [Access::Read(0xffff)]);
        assert_eq!(
            err.to_string(),
            format!("unsupported opcode ${opcode:02X} at $FFFF")
        );
    }
}

#[test]
fn step_trace_keeps_fetched_opcode_even_if_instruction_overwrites_itself() {
    let mut bus = RecordingBus::default();
    bus.ram.load(0x8000, &[0x8d, 0x00, 0x80]).unwrap();
    let before = Registers {
        a: 0xea,
        ..initial()
    };
    let step = check_step(
        &mut Cpu::from_registers(before),
        &mut bus,
        Registers {
            pc: 0x8003,
            ..before
        },
        4,
    );
    assert_eq!(step.opcode, 0x8d);
    assert_eq!(bus.ram.as_slice()[0x8000], 0xea);
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0x8000),
            Access::Read(0x8001),
            Access::Read(0x8002),
            Access::Write(0x8000, 0xea),
        ]
    );
}
