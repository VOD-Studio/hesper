use crate::{DRAM_BASE, dram_range, machine::Trap};

#[derive(Debug)]
pub(crate) struct Hart {
    pub(crate) pc: u64,
    pub(crate) x: [u64; 32],
    pub(crate) last_trap: Option<Trap>,
}

impl Hart {
    pub(crate) fn new() -> Self {
        Self {
            pc: DRAM_BASE,
            x: [0; 32],
            last_trap: None,
        }
    }

    pub(crate) fn step(&mut self, ram: &mut [u8]) -> bool {
        match self.execute(ram) {
            Ok(()) => true,
            Err(trap) => {
                self.last_trap = Some(trap);
                self.pc = 0; // Reset value of the M-mode trap vector.
                false
            }
        }
    }

    fn trap(&self, cause: u64, value: u64) -> Trap {
        Trap {
            epc: self.pc,
            cause,
            value,
        }
    }

    fn execute(&mut self, ram: &mut [u8]) -> Result<(), Trap> {
        let pc = self.pc;
        if pc & 1 != 0 {
            return Err(self.trap(0, pc));
        }
        let range = dram_range(pc, 2).ok_or_else(|| self.trap(1, pc))?;
        let start = range.start;
        let low = u16::from_le_bytes(ram[range].try_into().expect("two-byte fetch"));
        if low & 3 != 3 {
            return Err(self.trap(2, u64::from(low)));
        }
        let high = ram
            .get(start + 2..start + 4)
            .ok_or_else(|| self.trap(1, pc + 2))?;
        let high = u16::from_le_bytes(high.try_into().expect("two-byte fetch"));
        let instruction = u32::from(low) | (u32::from(high) << 16);

        let opcode = instruction & 0x7f;
        let funct3 = (instruction >> 12) & 7;
        let rd = ((instruction >> 7) & 31) as usize;
        let rs1 = ((instruction >> 15) & 31) as usize;
        match (opcode, funct3) {
            (0x13, 0) => {
                let offset = i64::from((instruction as i32) >> 20);
                self.write(rd, self.x[rs1].wrapping_add_signed(offset));
            }
            (0x17, _) => {
                let offset = i64::from((instruction & 0xffff_f000) as i32);
                self.write(rd, pc.wrapping_add_signed(offset));
            }
            (0x23, 2) => {
                let rs2 = ((instruction >> 20) & 31) as usize;
                let offset = (instruction & 0xfe00_0000) | ((instruction & 0x0f80) << 13);
                let address = self.x[rs1].wrapping_add_signed(i64::from((offset as i32) >> 20));
                if address & 3 != 0 {
                    return Err(self.trap(6, address));
                }
                let range = dram_range(address, 4).ok_or_else(|| self.trap(7, address))?;
                ram[range].copy_from_slice(&(self.x[rs2] as u32).to_le_bytes());
            }
            _ => return Err(self.trap(2, u64::from(instruction))),
        }
        self.pc = pc.wrapping_add(4);
        Ok(())
    }

    fn write(&mut self, rd: usize, value: u64) {
        if rd != 0 {
            self.x[rd] = value;
        }
    }
}
