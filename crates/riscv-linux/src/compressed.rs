const NOP: u32 = 0x0000_0013;

/// Expands an RV64 compressed instruction (or HINT) to its full-width equivalent.
/// The caller retains the original two-byte instruction length.
pub(crate) fn expand(bits: u16) -> Option<u32> {
    let bits = u32::from(bits);
    let field = |lsb: u32, width: u32| (bits >> lsb) & ((1 << width) - 1);
    let rd = field(7, 5);
    let rs2 = field(2, 5);
    let rd_prime = 8 + field(2, 3);
    let rs1_prime = 8 + field(7, 3);
    let high = field(12, 1);
    let ci = high << 5 | rs2;
    let funct3 = bits >> 13;

    Some(match (bits & 3, funct3) {
        (0, 0) => {
            // C.ADDI4SPN
            let imm = field(7, 4) << 6 | field(11, 2) << 4 | field(5, 1) << 3 | field(6, 1) << 2;
            if imm == 0 {
                return None;
            }
            encode_i(0x13, 0, rd_prime, 2, imm)
        }
        (0, 1..=3 | 5..=7) => {
            // The compact floating-point doubleword forms use the LD/SD offsets.
            let float = funct3 & 3 == 1;
            let width = if float { 3 } else { funct3 & 3 };
            let offset = field(10, 3) << 3
                | if width == 2 {
                    field(5, 1) << 6 | field(6, 1) << 2
                } else {
                    field(5, 2) << 6
                };
            if funct3 < 4 {
                let opcode = if float { 0x07 } else { 0x03 };
                encode_i(opcode, width, rd_prime, rs1_prime, offset)
            } else {
                let opcode = if float { 0x27 } else { 0x23 };
                encode_store(opcode, width, rs1_prime, rd_prime, offset)
            }
        }
        (1, 0) => {
            // C.ADDI, C.NOP, and their HINTs.
            if rd == 0 || ci == 0 {
                NOP
            } else {
                encode_i(0x13, 0, rd, rd, sign_extend(ci, 6))
            }
        }
        (1, 1) if rd != 0 => encode_i(0x1b, 0, rd, rd, sign_extend(ci, 6)), // C.ADDIW
        (1, 2) => {
            // C.LI
            if rd == 0 {
                NOP
            } else {
                encode_i(0x13, 0, rd, 0, sign_extend(ci, 6))
            }
        }
        (1, 3) if ci != 0 => match rd {
            0 => NOP,
            2 => {
                // C.ADDI16SP
                let imm = high << 9
                    | field(3, 2) << 7
                    | field(5, 1) << 6
                    | field(2, 1) << 5
                    | field(6, 1) << 4;
                encode_i(0x13, 0, 2, 2, sign_extend(imm, 10))
            }
            _ => sign_extend(ci, 6) << 12 | rd << 7 | 0x37, // C.LUI
        },
        (1, 4) => match field(10, 2) {
            shift @ (0 | 1) => {
                // C.SRLI/C.SRAI: bit 5 of the shift amount is valid on RV64.
                if ci == 0 {
                    NOP
                } else {
                    encode_i(0x13, 5, rs1_prime, rs1_prime, shift << 10 | ci)
                }
            }
            2 => encode_i(0x13, 7, rs1_prime, rs1_prime, sign_extend(ci, 6)), // C.ANDI
            _ => {
                let (opcode, funct3, funct7) = match (high, field(5, 2)) {
                    (0, 0) => (0x33, 0, 0x20), // C.SUB
                    (0, 1) => (0x33, 4, 0),    // C.XOR
                    (0, 2) => (0x33, 6, 0),    // C.OR
                    (0, 3) => (0x33, 7, 0),    // C.AND
                    (1, 0) => (0x3b, 0, 0x20), // C.SUBW
                    (1, 1) => (0x3b, 0, 0),    // C.ADDW
                    _ => return None,
                };
                encode_r(opcode, funct3, funct7, rs1_prime, rs1_prime, rd_prime)
            }
        },
        (1, 5) => {
            // C.J
            let imm = high << 11
                | field(8, 1) << 10
                | field(9, 2) << 8
                | field(6, 1) << 7
                | field(7, 1) << 6
                | field(2, 1) << 5
                | field(11, 1) << 4
                | field(3, 3) << 1;
            let imm = sign_extend(imm, 12);
            (imm & 0x0010_0000) << 11
                | (imm & 0x7fe) << 20
                | (imm & 0x800) << 9
                | (imm & 0x000f_f000)
                | 0x6f
        }
        (1, 6 | 7) => {
            // C.BEQZ/C.BNEZ
            let imm = high << 8
                | field(5, 2) << 6
                | field(2, 1) << 5
                | field(10, 2) << 3
                | field(3, 2) << 1;
            let imm = sign_extend(imm, 9);
            (imm & 0x1000) << 19
                | (imm & 0x7e0) << 20
                | rs1_prime << 15
                | (funct3 & 1) << 12
                | (imm & 0x1e) << 7
                | (imm & 0x800) >> 4
                | 0x63
        }
        (2, 0) => {
            // C.SLLI
            if rd == 0 || ci == 0 {
                NOP
            } else {
                encode_i(0x13, 1, rd, rd, ci)
            }
        }
        (2, 1..=3) if rd != 0 || funct3 == 1 => {
            // C.FLDSP/C.LWSP/C.LDSP. Unlike x0, f0 is a writable destination.
            let offset = high << 5
                | if funct3 == 2 {
                    field(2, 2) << 6 | field(4, 3) << 2
                } else {
                    field(2, 3) << 6 | field(5, 2) << 3
                };
            if funct3 == 1 {
                encode_i(0x07, 3, rd, 2, offset)
            } else {
                encode_i(0x03, funct3, rd, 2, offset)
            }
        }
        (2, 4) => match (rd, rs2, high) {
            (0, 0, 0) => return None,
            (0, 0, 1) => 0x0010_0073,                    // C.EBREAK
            (_, 0, _) => encode_i(0x67, 0, high, rd, 0), // C.JR/C.JALR
            (0, _, _) => NOP,
            (_, _, 0) => encode_r(0x33, 0, 0, rd, 0, rs2), // C.MV
            _ => encode_r(0x33, 0, 0, rd, rd, rs2),        // C.ADD
        },
        (2, 5..=7) => {
            // C.FSDSP/C.SWSP/C.SDSP.
            let offset = if funct3 == 6 {
                field(7, 2) << 6 | field(9, 4) << 2
            } else {
                field(7, 3) << 6 | field(10, 3) << 3
            };
            if funct3 == 5 {
                encode_store(0x27, 3, 2, rs2, offset)
            } else {
                encode_store(0x23, funct3 & 3, 2, rs2, offset)
            }
        }
        // Reserved encodings and wider instructions.
        _ => return None,
    })
}

fn sign_extend(value: u32, width: u32) -> u32 {
    let shift = 32 - width;
    (((value << shift) as i32) >> shift) as u32
}

fn encode_i(opcode: u32, funct3: u32, rd: u32, rs1: u32, imm: u32) -> u32 {
    imm << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
}

fn encode_r(opcode: u32, funct3: u32, funct7: u32, rd: u32, rs1: u32, rs2: u32) -> u32 {
    encode_i(opcode, funct3, rd, rs1, funct7 << 5 | rs2)
}

fn encode_store(opcode: u32, funct3: u32, rs1: u32, rs2: u32, imm: u32) -> u32 {
    (imm & 0xfe0) << 20 | rs2 << 20 | rs1 << 15 | funct3 << 12 | (imm & 0x1f) << 7 | opcode
}
