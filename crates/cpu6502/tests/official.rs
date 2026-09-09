use hesper_cpu6502::{Bus, Cpu, Ram, Registers, Status};

struct Spec {
    opcode: u8,
    name: &'static str,
    mode: &'static str,
    cycles: u8,
}

fn specs() -> Vec<Spec> {
    include_str!("data/opcodes.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 4);
            Spec {
                opcode: u8::from_str_radix(fields[0], 16).unwrap(),
                name: fields[1],
                mode: fields[2],
                cycles: fields[3].parse().unwrap(),
            }
        })
        .collect()
}

fn initial() -> Registers {
    Registers {
        a: 0x55,
        x: 1,
        y: 2,
        sp: 0xfd,
        pc: 0x8000,
        status: Status::from_bits(0x65),
    }
}

#[derive(Default)]
struct Spy {
    ram: Ram,
    writes: Vec<(u16, u8)>,
    reads: Vec<u16>,
}
impl Bus for Spy {
    fn read(&mut self, addr: u16) -> u8 {
        self.reads.push(addr);
        self.ram.read(addr)
    }
    fn write(&mut self, addr: u16, value: u8) {
        self.writes.push((addr, value));
        self.ram.write(addr, value);
    }
}

// Independent fixture encoding: registers X=1/Y=2, operand=$80.
fn setup(spec: &Spec) -> (Spy, u16, u16) {
    let mut bus = Spy::default();
    bus.ram.write(0x0040, 0x80);
    bus.ram.write(0x2040, 0x80);
    bus.ram.load(0x01fe, &[0x80, 0x12]).unwrap();
    bus.ram.write(0x0100, 0x34);
    bus.ram.load(0xfffe, &[0x78, 0x56]).unwrap();
    let operands: &[u8] = match spec.mode {
        "imp" | "acc" => &[],
        "imm" => &[0x80],
        "zp" => &[0x40],
        "zpx" => &[0x3f],
        "zpy" => &[0x3e],
        "abs" => &[0x40, 0x20],
        "abx" => &[0x3f, 0x20],
        "aby" => &[0x3e, 0x20],
        "izx" => {
            bus.ram.load(0x0080, &[0x40, 0x20]).unwrap();
            &[0x7f]
        }
        "izy" => {
            bus.ram.load(0x0080, &[0x3e, 0x20]).unwrap();
            &[0x80]
        }
        "ind" => {
            bus.ram.load(0x2080, &[0x00, 0x50]).unwrap();
            &[0x80, 0x20]
        }
        "rel" => &[5],
        _ => panic!("unknown fixture mode"),
    };
    bus.ram.write(0x8000, spec.opcode);
    bus.ram.load(0x8001, operands).unwrap();
    let pc = match spec.mode {
        "imp" | "acc" => 0x8001,
        "abs" | "abx" | "aby" | "ind" => 0x8003,
        _ => 0x8002,
    };
    let addr = if matches!(spec.mode, "zp" | "zpx" | "zpy") {
        0x0040
    } else {
        0x2040
    };
    (bus, pc, addr)
}

#[test]
fn every_documented_opcode_has_independent_result_length_flags_and_cycle_expectations() {
    let cases = specs();
    assert_eq!(cases.len(), 149);
    let mut seen = [false; 256];
    for spec in cases {
        assert!(!seen[usize::from(spec.opcode)], "duplicate specification");
        seen[usize::from(spec.opcode)] = true;
        let (mut bus, pc, addr) = setup(&spec);
        let mut expected = Registers { pc, ..initial() };
        let mut flags = 0x65;
        let mut writes = Vec::new();
        let mut cycles = spec.cycles;
        match spec.name {
            "ADC" => {
                expected.a = 0xd6;
                flags = 0xa4;
            }
            "SBC" => {
                expected.a = 0xd5;
                flags = 0xe4;
            }
            "LDA" => {
                expected.a = 0x80;
                flags = 0xe5;
            }
            "LDX" => {
                expected.x = 0x80;
                flags = 0xe5;
            }
            "LDY" => {
                expected.y = 0x80;
                flags = 0xe5;
            }
            "AND" => {
                expected.a = 0;
                flags = 0x67;
            }
            "ORA" | "EOR" => {
                expected.a = 0xd5;
                flags = 0xe5;
            }
            "CMP" | "CPX" | "CPY" => flags = 0xe4,
            "BIT" => flags = 0xa7,
            "STA" => writes.push((addr, 0x55)),
            "STX" => writes.push((addr, 1)),
            "STY" => writes.push((addr, 2)),
            "ASL" | "LSR" | "ROL" | "ROR" | "INC" | "DEC" => {
                let (result, status) = match (spec.name, spec.mode == "acc") {
                    ("ASL", true) => (0xaa, 0xe4),
                    ("LSR", true) => (0x2a, 0x65),
                    ("ROL", true) => (0xab, 0xe4),
                    ("ROR", true) => (0xaa, 0xe5),
                    ("ASL", false) => (0, 0x67),
                    ("LSR", false) => (0x40, 0x64),
                    ("ROL", false) => (1, 0x65),
                    ("ROR", false) => (0xc0, 0xe4),
                    ("INC", false) => (0x81, 0xe5),
                    ("DEC", false) => (0x7f, 0x65),
                    _ => panic!("invalid modify fixture"),
                };
                flags = status;
                if spec.mode == "acc" {
                    expected.a = result;
                } else {
                    writes.extend([(addr, 0x80), (addr, result)]);
                }
            }
            "TAX" => expected.x = 0x55,
            "TAY" => expected.y = 0x55,
            "TSX" => {
                expected.x = 0xfd;
                flags = 0xe5;
            }
            "TXA" => expected.a = 1,
            "TYA" => expected.a = 2,
            "TXS" => expected.sp = 1,
            "INX" => expected.x = 2,
            "INY" => expected.y = 3,
            "DEX" => {
                expected.x = 0;
                flags = 0x67;
            }
            "DEY" => expected.y = 1,
            "PHA" | "PHP" => {
                expected.sp = 0xfc;
                writes.push((0x01fd, if spec.name == "PHA" { 0x55 } else { 0x75 }));
            }
            "PLA" => {
                expected.sp = 0xfe;
                expected.a = 0x80;
                flags = 0xe5;
            }
            "PLP" => {
                expected.sp = 0xfe;
                flags = 0xa0;
            }
            "CLC" => flags = 0x64,
            "CLI" => flags = 0x61,
            "CLV" => flags = 0x25,
            "SED" => flags = 0x6d,
            "CLD" | "SEC" | "SEI" | "NOP" => {}
            "BPL" | "BNE" | "BCS" | "BVS" => {
                expected.pc = 0x8007;
                cycles = 3;
            }
            "BMI" | "BEQ" | "BCC" | "BVC" => {}
            "JMP" => expected.pc = if spec.mode == "ind" { 0x5000 } else { 0x2040 },
            "JSR" => {
                expected.pc = 0x2040;
                expected.sp = 0xfb;
                writes.extend([(0x01fd, 0x80), (0x01fc, 0x02)]);
            }
            "RTS" => {
                expected.pc = 0x1281;
                expected.sp = 0xff;
            }
            _ => panic!("missing independent expectation for {}", spec.name),
        }
        expected.status = Status::from_bits(flags);
        let mut cpu = Cpu::from_registers(initial());
        let step = cpu.step(&mut bus).unwrap();
        assert_eq!(step.before, initial());
        assert_eq!(
            step.after, expected,
            "{:02X} {} {}",
            spec.opcode, spec.name, spec.mode
        );
        assert_eq!(step.cycles, cycles, "opcode {:02X}", spec.opcode);
        assert_eq!(bus.writes, writes, "opcode {:02X}", spec.opcode);
    }
}

#[test]
fn all_indexed_reads_add_one_cycle_on_page_cross_but_stores_and_rmw_do_not() {
    for spec in specs()
        .into_iter()
        .filter(|s| matches!(s.mode, "abx" | "aby" | "izy"))
    {
        for base in [0x20ff_u16, 0xffff] {
            let (mut bus, _, _) = setup(&spec);
            let [lo, hi] = base.to_le_bytes();
            if spec.mode == "izy" {
                bus.ram.load(0x0080, &[lo, hi]).unwrap();
            } else {
                bus.ram.load(0x8001, &[lo, hi]).unwrap();
            }
            let target = match (base, spec.mode) {
                (0x20ff, "abx") => 0x2100,
                (0x20ff, _) => 0x2101,
                (_, "abx") => 0,
                _ => 1,
            };
            bus.ram.write(target, 0x80);
            let step = Cpu::from_registers(initial()).step(&mut bus).unwrap();
            let extra = u8::from(matches!(
                spec.name,
                "LDA" | "LDX" | "LDY" | "AND" | "ORA" | "EOR" | "CMP" | "ADC" | "SBC"
            ));
            assert_eq!(
                step.cycles,
                spec.cycles + extra,
                "opcode {:02X}",
                spec.opcode
            );
            if extra == 1 {
                assert_eq!(bus.reads.last(), Some(&target));
            }
            if extra == 0 {
                assert!(bus.writes.iter().all(|&(addr, _)| addr == target));
            }
        }
    }
}

#[test]
fn all_branches_preserve_flags_and_use_signed_displacement_and_post_operand_page() {
    for (opcode, bit, set_takes) in [
        (0x10, 0x80, false),
        (0x30, 0x80, true),
        (0x50, 0x40, false),
        (0x70, 0x40, true),
        (0x90, 1, false),
        (0xb0, 1, true),
        (0xd0, 2, false),
        (0xf0, 2, true),
    ] {
        for (pc, offset, target, taken_cycles) in [
            (0x8000, 0x7f, 0x8081, 3),
            (0x8080, 0x80, 0x8002, 3),
            (0x8000, 0xfd, 0x7fff, 4),
            (0xffff, 0xfe, 0xffff, 4),
        ] {
            for set in [false, true] {
                let before = Registers {
                    pc,
                    status: Status::from_bits(if set { 0x20 | bit } else { 0x20 }),
                    ..initial()
                };
                let mut ram = Ram::new();
                ram.write(pc, opcode);
                ram.write(pc.wrapping_add(1), offset);
                let taken = set == set_takes;
                let step = Cpu::from_registers(before).step(&mut ram).unwrap();
                assert_eq!(
                    step.after,
                    Registers {
                        pc: if taken { target } else { pc.wrapping_add(2) },
                        ..before
                    }
                );
                assert_eq!(step.cycles, if taken { taken_cycles } else { 2 });
            }
        }
    }
}

#[test]
fn zero_page_indexes_and_both_indirect_pointers_wrap_in_zero_page() {
    for (program, x, y, memory, cycles) in [
        (vec![0xb5, 0xff], 1, 0, vec![(0, 0x80)], 4),
        (vec![0xb6, 0xff], 0, 1, vec![(0, 0x80)], 4),
        (
            vec![0xa1, 0xfe],
            1,
            0,
            vec![(0xff, 0x34), (0, 0x12), (0x1234, 0x80)],
            6,
        ),
        (
            vec![0xa1, 0xff],
            1,
            0,
            vec![(0, 0x34), (1, 0x12), (0x1234, 0x80)],
            6,
        ),
        (vec![0xb1, 0xff], 0, 1, vec![(0xff, 0xff), (0, 0xff)], 6),
    ] {
        let mut ram = Ram::new();
        ram.load(0x8000, &program).unwrap();
        for (addr, value) in memory {
            ram.write(addr, value);
        }
        let step = Cpu::from_registers(Registers { x, y, ..initial() })
            .step(&mut ram)
            .unwrap();
        let value = if program[0] == 0xb6 {
            step.after.x
        } else {
            step.after.a
        };
        assert_eq!(value, if program[0] == 0xb1 { 0xff } else { 0x80 });
        assert_eq!(step.cycles, cycles);
        assert_eq!(step.after.pc, 0x8002);
    }
}

#[test]
fn memory_modify_wraps_and_rotates_use_old_carry_and_preserve_vdi() {
    for (opcode, value, carry, result, flags) in [
        (0x06, 0x80, false, 0x00, 0x6f),
        (0x46, 0x01, false, 0x00, 0x6f),
        (0x26, 0x80, false, 0x00, 0x6f),
        (0x26, 0x00, true, 0x01, 0x6c),
        (0x66, 0x01, false, 0x00, 0x6f),
        (0x66, 0x00, true, 0x80, 0xec),
        (0xe6, 0xff, true, 0x00, 0x6f),
        (0xc6, 0x00, false, 0xff, 0xec),
    ] {
        let mut bus = Spy::default();
        bus.ram.load(0x8000, &[opcode, 0x40]).unwrap();
        bus.ram.write(0x40, value);
        let before = Registers {
            status: Status {
                carry,
                ..Status::from_bits(0xec)
            },
            ..initial()
        };
        let step = Cpu::from_registers(before).step(&mut bus).unwrap();
        assert_eq!(bus.writes, [(0x40, value), (0x40, result)]);
        assert_eq!(step.after.status.bits(), flags);
        assert_eq!(step.cycles, 5);
    }
}

#[test]
fn every_opcode_preserves_unaffected_flags_both_set_and_clear() {
    for spec in specs() {
        let mask = match spec.name {
            "LDA" | "LDX" | "LDY" | "AND" | "ORA" | "EOR" | "TAX" | "TAY" | "TSX" | "TXA"
            | "TYA" | "INX" | "INY" | "DEX" | "DEY" | "INC" | "DEC" | "PLA" => 0x6d,
            "ASL" | "LSR" | "ROL" | "ROR" | "CMP" | "CPX" | "CPY" => 0x6c,
            "ADC" | "SBC" => 0x2c,
            "BIT" => 0x2d,
            "PLP" | "RTI" => 0x20,
            "CLC" | "SEC" => 0xee,
            "CLD" | "SED" => 0xe7,
            "CLI" | "SEI" | "BRK" => 0xeb,
            "CLV" => 0xaf,
            _ => 0xef,
        };
        for flags in [0x20, 0xef] {
            let (mut bus, _, _) = setup(&spec);
            let before = Registers {
                status: Status::from_bits(flags),
                ..initial()
            };
            let step = Cpu::from_registers(before).step(&mut bus).unwrap();
            assert_eq!(
                step.after.status.bits() & mask,
                flags & mask,
                "opcode {:02X}",
                spec.opcode
            );
        }
    }
}

#[test]
fn bit_copies_nv_from_memory_and_tests_a_and_memory_for_zero() {
    for (a, value, flags) in [
        (0xff, 0x00, 0x2f),
        (0xff, 0x40, 0x6d),
        (0xff, 0x80, 0xad),
        (0x01, 0xc0, 0xef),
        (0x01, 0xc1, 0xed),
    ] {
        for program in [vec![0x24, 0x40], vec![0x2c, 0x40, 0x00]] {
            let mut ram = Ram::new();
            ram.load(0x8000, &program).unwrap();
            ram.write(0x40, value);
            let before = Registers {
                a,
                status: Status::from_bits(0xef),
                ..initial()
            };
            let step = Cpu::from_registers(before).step(&mut ram).unwrap();
            assert_eq!(step.after.status.bits(), flags);
            assert_eq!(step.after.a, a);
        }
    }
}

#[test]
fn php_plp_preserve_all_six_flags_with_synthesized_b_and_wrapped_sp() {
    for flags in 0..=u8::MAX {
        let before = Registers {
            sp: 0,
            status: Status::from_bits(flags),
            ..initial()
        };
        let mut cpu = Cpu::from_registers(before);
        let mut ram = Ram::new();
        ram.load(0x8000, &[0x08, 0x28]).unwrap();
        assert_eq!(cpu.step(&mut ram).unwrap().cycles, 3);
        assert_eq!(ram.read(0x0100), flags | 0x30);
        assert_eq!(cpu.registers().sp, 0xff);
        assert_eq!(cpu.step(&mut ram).unwrap().cycles, 4);
        assert_eq!(
            cpu.registers(),
            Registers {
                pc: 0x8002,
                ..before
            }
        );
        assert_eq!(cpu.registers().status.bits(), (flags & 0xcf) | 0x20);
    }
}
