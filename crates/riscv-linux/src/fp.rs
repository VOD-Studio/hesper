//! RV64 F/D execution. Register representation and instruction legality are
//! handled here; the pinned RISC-V SoftFloat specialization supplies rounded
//! arithmetic, canonical NaNs, integer saturation and the architectural flags.

use softfloat_pure::softfloat as sf;

const S_SIGN: u32 = 1 << 31;
const D_SIGN: u64 = 1 << 63;
const TININESS: u8 = sf::softfloat_tininess_afterRounding;

pub(crate) enum Value {
    Float(u64),
    Integer(u64),
}

#[derive(Clone, Copy)]
enum Format {
    Single,
    Double,
}

impl Format {
    fn read(self, bits: u64) -> u64 {
        match self {
            Self::Single => u64::from(unbox_single(bits)),
            Self::Double => bits,
        }
    }

    fn write(self, bits: u64) -> Value {
        Value::Float(match self {
            Self::Single => box_single(bits as u32),
            Self::Double => bits,
        })
    }

    fn sign(self) -> u64 {
        match self {
            Self::Single => u64::from(S_SIGN),
            Self::Double => D_SIGN,
        }
    }

    fn nan_thresholds(self) -> (u64, u64) {
        match self {
            Self::Single => (0x7f80_0000, 1 << 22),
            Self::Double => (0x7ff0_0000_0000_0000, 1 << 51),
        }
    }
}

pub(crate) fn box_single(bits: u32) -> u64 {
    0xffff_ffff_0000_0000 | u64::from(bits)
}

fn unbox_single(bits: u64) -> u32 {
    if bits >> 32 == u64::from(u32::MAX) {
        bits as u32
    } else {
        sf::defaultNaNF32UI
    }
}

fn single(bits: u64) -> sf::float32_t {
    sf::float32_t::from_bits(unbox_single(bits))
}

fn single_result((value, flags): (sf::float32_t, u8)) -> (Value, u8) {
    (Value::Float(box_single(value.to_bits())), flags)
}

fn double_result((value, flags): (sf::float64_t, u8)) -> (Value, u8) {
    (Value::Float(value.to_bits()), flags)
}

fn word_to_x(bits: u32) -> u64 {
    bits as i32 as i64 as u64
}

pub(crate) fn execute(
    instruction: u32,
    f: &[u64; 32],
    integer_lhs: u64,
    frm: u8,
) -> Option<(Value, u8)> {
    let format = match (instruction >> 25) & 3 {
        0 => Format::Single,
        1 => Format::Double,
        _ => return None,
    };
    let field = ((instruction >> 12) & 7) as u8;
    // RNE/RTZ/RDN/RUP/RMM and all five SoftFloat exception bits have the
    // same encodings as RISC-V. Validate rm only on instructions with rm.
    let rounding = || {
        let rm = if field == 7 { frm } else { field };
        (rm <= 4).then_some(rm)
    };
    let rs2 = ((instruction >> 20) & 31) as usize;
    let lhs = f[((instruction >> 15) & 31) as usize];
    let rhs = || f[rs2];
    let sa = || single(lhs);
    let sb = || single(rhs());
    let da = || sf::float64_t::from_bits(lhs);
    let db = || sf::float64_t::from_bits(rhs());

    if instruction & 0x7f != 0x53 {
        let rm = rounding()?;
        let c = f[(instruction >> 27) as usize];
        // Opcode bits 3 and 2 negate the product and addend respectively.
        // Apply the signs before rounding, including for a signed-zero result.
        let sign = format.sign();
        let negate_a = if instruction & 8 != 0 { sign } else { 0 };
        let negate_c = if instruction & 4 != 0 { sign } else { 0 };
        return Some(match format {
            Format::Single => single_result(sf::f32_mulAdd(
                single(lhs ^ negate_a),
                sb(),
                single(c ^ negate_c),
                rm,
                TININESS,
            )),
            Format::Double => double_result(sf::f64_mulAdd(
                sf::float64_t::from_bits(lhs ^ negate_a),
                db(),
                sf::float64_t::from_bits(c ^ negate_c),
                rm,
                TININESS,
            )),
        });
    }

    let operation = instruction >> 27;
    Some(match operation {
        0x00..=0x03 => {
            let rm = rounding()?;
            match format {
                Format::Single => single_result(match operation {
                    0x00 => sf::f32_add(sa(), sb(), rm, TININESS),
                    0x01 => sf::f32_sub(sa(), sb(), rm, TININESS),
                    0x02 => sf::f32_mul(sa(), sb(), rm, TININESS),
                    _ => sf::f32_div(sa(), sb(), rm, TININESS),
                }),
                Format::Double => double_result(match operation {
                    0x00 => sf::f64_add(da(), db(), rm, TININESS),
                    0x01 => sf::f64_sub(da(), db(), rm, TININESS),
                    0x02 => sf::f64_mul(da(), db(), rm, TININESS),
                    _ => sf::f64_div(da(), db(), rm, TININESS),
                }),
            }
        }
        0x04 if field <= 2 => {
            // FSGNJ, FSGNJN, FSGNJX operate on boxed operands without flag changes.
            let a = format.read(lhs);
            let b = format.read(rhs());
            let sign = format.sign();
            let injected = match field {
                0 => b,
                1 => !b,
                _ => a ^ b,
            };
            (format.write((a & !sign) | (injected & sign)), 0)
        }
        0x05 if field <= 1 => min_max(format, lhs, rhs(), field == 1),
        0x08 => {
            let rm = rounding()?;
            match (format, rs2) {
                (Format::Single, 1) => single_result(sf::f64_to_f32(da(), rm, TININESS)),
                (Format::Double, 0) => double_result(sf::f32_to_f64(sa())),
                _ => return None,
            }
        }
        0x0b if rs2 == 0 => {
            let rm = rounding()?;
            match format {
                Format::Single => single_result(sf::f32_sqrt(sa(), rm, TININESS)),
                Format::Double => double_result(sf::f64_sqrt(da(), rm, TININESS)),
            }
        }
        0x14 if field <= 2 => {
            // FLE/FLT signal on either NaN; FEQ signals only on signaling NaNs.
            let (value, flags) = match format {
                Format::Single => match field {
                    0 => sf::f32_le(sa(), sb()),
                    1 => sf::f32_lt(sa(), sb()),
                    _ => sf::f32_eq(sa(), sb()),
                },
                Format::Double => match field {
                    0 => sf::f64_le(da(), db()),
                    1 => sf::f64_lt(da(), db()),
                    _ => sf::f64_eq(da(), db()),
                },
            };
            (Value::Integer(u64::from(value)), flags)
        }
        0x18 if rs2 <= 3 => convert_to_int(format, lhs, rs2, rounding()?),
        0x1a if rs2 <= 3 => {
            let rm = rounding()?;
            let x = integer_lhs;
            match format {
                Format::Single => single_result(match rs2 {
                    0 => sf::i32_to_f32(x as i32, rm, TININESS),
                    1 => sf::ui32_to_f32(x as u32, rm, TININESS),
                    2 => sf::i64_to_f32(x as i64, rm, TININESS),
                    _ => sf::ui64_to_f32(x, rm, TININESS),
                }),
                Format::Double => double_result(match rs2 {
                    0 => (sf::i32_to_f64(x as i32), 0),
                    1 => (sf::ui32_to_f64(x as u32), 0),
                    2 => sf::i64_to_f64(x as i64, rm, TININESS),
                    _ => sf::ui64_to_f64(x, rm, TININESS),
                }),
            }
        }
        0x1c if rs2 == 0 && field <= 1 => {
            let value = match (format, field) {
                (Format::Single, 0) => word_to_x(lhs as u32), // FMV.X.W ignores boxing.
                (Format::Double, 0) => lhs,
                (_, _) => classify(format, lhs),
            };
            (Value::Integer(value), 0)
        }
        0x1e if rs2 == 0 && field == 0 => (format.write(integer_lhs), 0),
        _ => return None,
    })
}

fn convert_to_int(format: Format, a: u64, kind: usize, rm: u8) -> (Value, u8) {
    // exact=true reports NX for inexact valid conversions. Both W and WU
    // sign-extend the 32-bit result into the RV64 integer register.
    let (value, flags) = match format {
        Format::Single => {
            let a = single(a);
            match kind {
                0 => {
                    let (value, flags) = sf::f32_to_i32(a, rm, true);
                    (word_to_x(value as u32), flags)
                }
                1 => {
                    let (value, flags) = sf::f32_to_ui32(a, rm, true);
                    (word_to_x(value), flags)
                }
                2 => {
                    let (value, flags) = sf::f32_to_i64(a, rm, true);
                    (value as u64, flags)
                }
                _ => sf::f32_to_ui64(a, rm, true),
            }
        }
        Format::Double => {
            let a = sf::float64_t::from_bits(a);
            match kind {
                0 => {
                    let (value, flags) = sf::f64_to_i32(a, rm, true);
                    (word_to_x(value as u32), flags)
                }
                1 => {
                    let (value, flags) = sf::f64_to_ui32(a, rm, true);
                    (word_to_x(value), flags)
                }
                2 => {
                    let (value, flags) = sf::f64_to_i64(a, rm, true);
                    (value as u64, flags)
                }
                _ => sf::f64_to_ui64(a, rm, true),
            }
        }
    };
    (Value::Integer(value), flags)
}

fn classify(format: Format, bits: u64) -> u64 {
    let bits = format.read(bits);
    let sign = format.sign();
    let negative = bits & sign != 0;
    let magnitude = bits & !sign;
    let (infinity, quiet) = format.nan_thresholds();
    let class = if magnitude > infinity {
        8 + u32::from(magnitude & quiet != 0)
    } else if magnitude == infinity {
        if negative { 0 } else { 7 }
    } else if magnitude == 0 {
        if negative { 3 } else { 4 }
    } else if magnitude < quiet << 1 {
        if negative { 2 } else { 5 }
    } else if negative {
        1
    } else {
        6
    };
    1 << class
}

fn min_max(format: Format, a: u64, b: u64, max: bool) -> (Value, u8) {
    let a = format.read(a);
    let b = format.read(b);
    let (infinity, quiet) = format.nan_thresholds();
    let sign = format.sign();
    let a_nan = a & !sign > infinity;
    let b_nan = b & !sign > infinity;
    let flags = if (a_nan && a & quiet == 0) || (b_nan && b & quiet == 0) {
        sf::softfloat_flag_invalid
    } else {
        0
    };
    let value = match (a_nan, b_nan) {
        (true, true) => infinity | quiet,
        (true, false) => b,
        (false, true) => a,
        (false, false) => {
            // IEEE magnitude order, reversed for negatives; -0 precedes +0.
            let a_negative = a & sign != 0;
            let less = if (a ^ b) & sign != 0 {
                a_negative
            } else if a_negative {
                a > b
            } else {
                a < b
            };
            if less != max { a } else { b }
        }
    };
    (format.write(value), flags)
}
