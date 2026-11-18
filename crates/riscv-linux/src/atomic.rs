#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Operation {
    LoadReserved,
    StoreConditional,
    Add,
    Swap,
    Xor,
    Or,
    And,
    Min,
    Max,
    MinUnsigned,
    MaxUnsigned,
}

pub(crate) fn decode(bits: u32) -> Option<(usize, Operation)> {
    if bits & 0x7f != 0x2f {
        return None;
    }
    let bytes = match (bits >> 12) & 7 {
        0b010 => 4,
        0b011 => 8,
        _ => return None,
    };
    // funct5 excludes aq and rl; neither changes the operation.
    let operation = match bits >> 27 {
        0b00000 => Operation::Add,
        0b00001 => Operation::Swap,
        0b00010 if (bits >> 20) & 31 == 0 => Operation::LoadReserved,
        0b00011 => Operation::StoreConditional,
        0b00100 => Operation::Xor,
        0b01000 => Operation::Or,
        0b01100 => Operation::And,
        0b10000 => Operation::Min,
        0b10100 => Operation::Max,
        0b11000 => Operation::MinUnsigned,
        0b11100 => Operation::MaxUnsigned,
        _ => return None,
    };
    Some((bytes, operation))
}

impl Operation {
    // This is the value written to memory, not the old value returned in rd.
    pub(crate) fn apply(self, bytes: usize, old: u64, rhs: u64) -> u64 {
        match bytes {
            4 => {
                let old = old as u32;
                let rhs = rhs as u32;
                u64::from(match self {
                    Self::Add => old.wrapping_add(rhs),
                    Self::Swap => rhs,
                    Self::Xor => old ^ rhs,
                    Self::Or => old | rhs,
                    Self::And => old & rhs,
                    Self::Min => (old as i32).min(rhs as i32) as u32,
                    Self::Max => (old as i32).max(rhs as i32) as u32,
                    Self::MinUnsigned => old.min(rhs),
                    Self::MaxUnsigned => old.max(rhs),
                    Self::LoadReserved | Self::StoreConditional => {
                        unreachable!("not an RMW operation")
                    }
                })
            }
            8 => match self {
                Self::Add => old.wrapping_add(rhs),
                Self::Swap => rhs,
                Self::Xor => old ^ rhs,
                Self::Or => old | rhs,
                Self::And => old & rhs,
                Self::Min => (old as i64).min(rhs as i64) as u64,
                Self::Max => (old as i64).max(rhs as i64) as u64,
                Self::MinUnsigned => old.min(rhs),
                Self::MaxUnsigned => old.max(rhs),
                Self::LoadReserved | Self::StoreConditional => {
                    unreachable!("not an RMW operation")
                }
            },
            _ => unreachable!("invalid atomic width"),
        }
    }
}
