pub(crate) fn alu(instruction: u32, lhs: u64, rhs: u64) -> Option<u64> {
    let funct3 = (instruction >> 12) & 7;
    let funct7 = instruction >> 25;
    match instruction & 0x7f {
        0x13 => {
            let imm = i64::from((instruction as i32) >> 20);
            let shamt = (instruction >> 20) & 63;
            Some(match funct3 {
                0 => lhs.wrapping_add_signed(imm),
                1 if instruction >> 26 == 0 => lhs << shamt,
                2 => u64::from((lhs as i64) < imm),
                3 => u64::from(lhs < imm as u64),
                4 => lhs ^ imm as u64,
                5 if instruction >> 26 == 0 => lhs >> shamt,
                5 if instruction >> 26 == 0x10 => ((lhs as i64) >> shamt) as u64,
                6 => lhs | imm as u64,
                7 => lhs & imm as u64,
                _ => return None,
            })
        }
        0x1b => {
            let lhs = lhs as u32;
            let shamt = (instruction >> 20) & 31;
            Some(sign_extend_word(match (funct7, funct3) {
                (_, 0) => lhs.wrapping_add_signed((instruction as i32) >> 20),
                (0, 1) => lhs << shamt,
                (0, 5) => lhs >> shamt,
                (0x20, 5) => ((lhs as i32) >> shamt) as u32,
                _ => return None,
            }))
        }
        0x33 => {
            let shamt = rhs & 63;
            Some(match (funct7, funct3) {
                (0, 0) => lhs.wrapping_add(rhs),
                (0x20, 0) => lhs.wrapping_sub(rhs),
                (0, 1) => lhs << shamt,
                (0, 2) => u64::from((lhs as i64) < (rhs as i64)),
                (0, 3) => u64::from(lhs < rhs),
                (0, 4) => lhs ^ rhs,
                (0, 5) => lhs >> shamt,
                (0x20, 5) => ((lhs as i64) >> shamt) as u64,
                (0, 6) => lhs | rhs,
                (0, 7) => lhs & rhs,
                (1, 0) => lhs.wrapping_mul(rhs),
                (1, 1) => ((i128::from(lhs as i64) * i128::from(rhs as i64)) >> 64) as u64,
                (1, 2) => ((i128::from(lhs as i64) * i128::from(rhs)) >> 64) as u64,
                (1, 3) => ((u128::from(lhs) * u128::from(rhs)) >> 64) as u64,
                // Signed wrapping operations also implement MIN / -1 and MIN % -1.
                (1, 4) if rhs == 0 => u64::MAX,
                (1, 4) => (lhs as i64).wrapping_div(rhs as i64) as u64,
                (1, 5) => lhs.checked_div(rhs).unwrap_or(u64::MAX),
                (1, 6) if rhs == 0 => lhs,
                (1, 6) => (lhs as i64).wrapping_rem(rhs as i64) as u64,
                (1, 7) => lhs.checked_rem(rhs).unwrap_or(lhs),
                _ => return None,
            })
        }
        0x3b => {
            let lhs = lhs as u32;
            let rhs = rhs as u32;
            let shamt = rhs & 31;
            Some(sign_extend_word(match (funct7, funct3) {
                (0, 0) => lhs.wrapping_add(rhs),
                (0x20, 0) => lhs.wrapping_sub(rhs),
                (0, 1) => lhs << shamt,
                (0, 5) => lhs >> shamt,
                (0x20, 5) => ((lhs as i32) >> shamt) as u32,
                (1, 0) => lhs.wrapping_mul(rhs),
                (1, 4) if rhs == 0 => u32::MAX,
                (1, 4) => (lhs as i32).wrapping_div(rhs as i32) as u32,
                (1, 5) => lhs.checked_div(rhs).unwrap_or(u32::MAX),
                (1, 6) if rhs == 0 => lhs,
                (1, 6) => (lhs as i32).wrapping_rem(rhs as i32) as u32,
                (1, 7) => lhs.checked_rem(rhs).unwrap_or(lhs),
                _ => return None,
            }))
        }
        _ => None,
    }
}

fn sign_extend_word(value: u32) -> u64 {
    i64::from(value as i32) as u64
}
