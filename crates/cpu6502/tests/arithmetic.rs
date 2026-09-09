use hesper_cpu6502::{Bus, Cpu, Ram, Registers, Status};

fn state(a: u8, carry: bool, decimal: bool) -> Registers {
    Registers {
        a,
        x: 0x12,
        y: 0x34,
        sp: 0xfd,
        pc: 0x8000,
        status: Status {
            carry,
            decimal,
            ..Status::from_bits(0xe6)
        },
    }
}

fn binary_exhaustive(opcode: u8) {
    let mut ram = Ram::new();
    ram.write(0x8000, opcode);
    for a in 0..=u8::MAX {
        for operand in 0..=u8::MAX {
            for carry in [false, true] {
                ram.write(0x8001, operand);
                let before = state(a, carry, false);
                let step = Cpu::from_registers(before).step(&mut ram).unwrap();
                // A wider signed-integer oracle; overflow is a range check, not
                // the bit-expression used by the implementation.
                let (unsigned, signed, expected_carry) = if opcode == 0x69 {
                    let sum = i32::from(a) + i32::from(operand) + i32::from(carry);
                    (
                        sum,
                        i32::from(a as i8) + i32::from(operand as i8) + i32::from(carry),
                        sum > 255,
                    )
                } else {
                    let difference = i32::from(a) - i32::from(operand) - i32::from(!carry);
                    (
                        difference,
                        i32::from(a as i8) - i32::from(operand as i8) - i32::from(!carry),
                        difference >= 0,
                    )
                };
                let value = unsigned.rem_euclid(256) as u8;
                let flags = 0x24
                    | u8::from(expected_carry)
                    | (u8::from(value == 0) << 1)
                    | (u8::from(!(-128..=127).contains(&signed)) << 6)
                    | (value & 0x80);
                assert_eq!(
                    step.after,
                    Registers {
                        a: value,
                        pc: 0x8002,
                        status: Status::from_bits(flags),
                        ..before
                    },
                    "op={opcode:02X} a={a:02X} m={operand:02X} c={carry}"
                );
                assert_eq!(step.cycles, 2);
            }
        }
    }
}

#[test]
fn adc_binary_exhaustive_results_and_all_flags() {
    binary_exhaustive(0x69);
}

#[test]
fn sbc_binary_exhaustive_results_and_all_flags() {
    binary_exhaustive(0xe9);
}

#[test]
fn valid_decimal_adc_sbc_exhaustive_results_and_carry() {
    let mut ram = Ram::new();
    for opcode in [0x69, 0xe9] {
        ram.write(0x8000, opcode);
        for a in 0..100_u8 {
            for operand in 0..100_u8 {
                for carry in [false, true] {
                    let packed_a = (a / 10) * 16 + a % 10;
                    let packed_operand = (operand / 10) * 16 + operand % 10;
                    ram.write(0x8001, packed_operand);
                    let before = state(packed_a, carry, true);
                    let step = Cpu::from_registers(before).step(&mut ram).unwrap();
                    // Ordinary base-ten arithmetic is independent of the core's
                    // nibble correction and also catches a NES-style ignored D.
                    let (decimal, expected_carry) = if opcode == 0x69 {
                        let sum = i16::from(a) + i16::from(operand) + i16::from(carry);
                        (sum, sum > 99)
                    } else {
                        let difference = i16::from(a) - i16::from(operand) - i16::from(!carry);
                        (difference, difference >= 0)
                    };
                    let result = decimal.rem_euclid(100) as u8;
                    assert_eq!(step.after.a, (result / 10) * 16 + result % 10);
                    assert_eq!(step.after.status.carry, expected_carry);
                    assert!(step.after.status.decimal && step.after.status.interrupt_disable);
                    assert_eq!(step.cycles, 2); // No CMOS decimal-cycle penalty.
                }
            }
        }
    }
}

#[test]
fn decimal_nmos_flag_stages_and_invalid_bcd_digits() {
    // Hand-worked NMOS cases, using the stages described by Bruce Clark's
    // original decimal test (linked in docs/references.md). Expected P is literal.
    for (opcode, a, operand, carry, result, flags) in [
        (0x69, 0x00, 0x00, false, 0x00, 0x2e),
        (0x69, 0x00, 0x00, true, 0x01, 0x2c),
        (0x69, 0x50, 0x50, false, 0x00, 0xed),
        (0x69, 0x99, 0x00, true, 0x00, 0xad),
        (0x69, 0x79, 0x00, true, 0x80, 0xec),
        (0x69, 0x80, 0x80, false, 0x60, 0x6f),
        (0x69, 0xff, 0x00, true, 0x66, 0x2f),
        (0x69, 0xff, 0xff, true, 0x55, 0xad),
        (0x69, 0x0f, 0x0f, true, 0x15, 0x2c),
        (0xe9, 0x00, 0x00, true, 0x00, 0x2f),
        (0xe9, 0x00, 0x01, true, 0x99, 0xac),
        (0xe9, 0x10, 0x01, true, 0x09, 0x2d),
        (0xe9, 0x50, 0x49, true, 0x01, 0x2d),
        (0xe9, 0x80, 0x01, true, 0x79, 0x6d),
        (0xe9, 0x00, 0x80, true, 0x20, 0xec),
        (0xe9, 0x10, 0x0f, true, 0x0b, 0x2d),
        (0xe9, 0x00, 0x0f, true, 0x9b, 0xac),
    ] {
        let mut ram = Ram::new();
        ram.load(0x8000, &[opcode, operand]).unwrap();
        let before = state(a, carry, true);
        let step = Cpu::from_registers(before).step(&mut ram).unwrap();
        assert_eq!(
            step.after,
            Registers {
                a: result,
                pc: 0x8002,
                status: Status::from_bits(flags),
                ..before
            },
            "op={opcode:02X} a={a:02X} m={operand:02X} c={carry}"
        );
        assert_eq!(step.cycles, 2);
    }
}

#[test]
fn selected_nmos_decimal_vectors() {
    for line in include_str!("data/singlestep-decimal.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 7);
        let values: Vec<_> = fields[..6]
            .iter()
            .map(|field| u8::from_str_radix(field, 16).unwrap())
            .collect();
        let mut ram = Ram::new();
        ram.load(0x8000, &[values[0], values[2]]).unwrap();
        let before = state(values[1], values[3] != 0, true);
        let step = Cpu::from_registers(before).step(&mut ram).unwrap();
        assert_eq!(step.after.a, values[4], "{line}");
        assert_eq!(step.after.status.bits(), 0x2c | values[5], "{line}");
        assert_eq!(step.cycles, 2);
    }
}
