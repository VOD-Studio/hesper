use crate::{
    DRAM_BASE, PrivilegeMode, atomic,
    board::{Board, read_le},
    compressed,
    decode::alu,
    dram_range,
    machine::Trap,
    mmu,
    pmp::Access,
    privilege,
};
use core::ops::Range;

#[derive(Debug)]
pub(crate) struct Hart {
    pub(crate) pc: u64,
    pub(crate) x: [u64; 32],
    pub(crate) last_trap: Option<Trap>,
    pub(crate) privilege: privilege::State,
    pub(crate) waiting: bool,
    reservation: Option<(u64, usize)>,
}

impl Hart {
    pub(crate) fn new() -> Self {
        Self {
            pc: DRAM_BASE,
            x: [0; 32],
            last_trap: None,
            privilege: privilege::State::new(),
            waiting: false,
            reservation: None,
        }
    }

    pub(crate) fn step(&mut self, board: &mut Board, cycle: u64, retired: u64) -> Option<bool> {
        if self.waiting {
            if !self.privilege.locally_pending() {
                return None;
            }
            self.waiting = false;
        }
        if let Some(cause) = self.privilege.pending_interrupt() {
            self.enter_trap(self.trap(cause, 0));
            return Some(false);
        }
        Some(match self.execute(board, [cycle, board.time(), retired]) {
            Ok(()) => true,
            Err(trap) => {
                self.enter_trap(trap);
                false
            }
        })
    }

    fn enter_trap(&mut self, trap: Trap) {
        self.last_trap = Some(trap);
        self.pc = self.privilege.take_trap(trap);
    }

    fn trap(&self, cause: u64, value: u64) -> Trap {
        Trap {
            epc: self.pc,
            cause,
            value,
        }
    }

    fn execute(&mut self, board: &mut Board, counters: [u64; 3]) -> Result<(), Trap> {
        let pc = self.pc;
        if pc & 1 != 0 {
            return Err(self.trap(0, pc));
        }
        let (translated, range) = self.map_ram(&board.ram, pc, 2, Access::Execute)?;
        let physical = translated.address;
        translated.commit(&mut board.ram);
        let low = u16::from_le_bytes(board.ram[range].try_into().expect("two-byte fetch"));
        let (instruction, length) = if low & 3 != 3 {
            (
                compressed::expand(low).ok_or_else(|| self.trap(2, u64::from(low)))?,
                2,
            )
        } else {
            let next_half = pc.wrapping_add(2);
            let range = if pc & 0xfff == 0xffe {
                let (translated, range) =
                    self.map_ram(&board.ram, next_half, 2, Access::Execute)?;
                translated.commit(&mut board.ram);
                range
            } else {
                // Translation is shared within the page, but a four-byte
                // instruction at a halfword boundary can straddle a PMP entry.
                if pc & 3 != 0
                    && !self
                        .privilege
                        .pmp
                        .allows(physical, 4, self.privilege.mode, Access::Execute)
                {
                    return Err(self.trap(1, next_half));
                }
                dram_range(physical + 2, 2).ok_or_else(|| self.trap(1, next_half))?
            };
            let high = u16::from_le_bytes(board.ram[range].try_into().expect("two-byte fetch"));
            (u32::from(low) | (u32::from(high) << 16), 4)
        };

        let opcode = instruction & 0x7f;
        let funct3 = (instruction >> 12) & 7;
        let rd = ((instruction >> 7) & 31) as usize;
        let lhs = self.x[((instruction >> 15) & 31) as usize];
        let rhs = self.x[((instruction >> 20) & 31) as usize];
        let immediate = i64::from((instruction as i32) >> 20);
        let mut next = pc.wrapping_add(length);
        let illegal = || self.trap(2, u64::from(instruction));
        match (opcode, funct3) {
            (0x13 | 0x1b | 0x33 | 0x3b, _) => {
                self.write(rd, alu(instruction, lhs, rhs).ok_or_else(illegal)?);
            }
            (0x37, _) => self.write(rd, (instruction & 0xffff_f000) as i32 as i64 as u64),
            (0x17, _) => {
                let offset = i64::from((instruction & 0xffff_f000) as i32);
                self.write(rd, pc.wrapping_add_signed(offset));
            }
            (0x03, 0..=6) => {
                let bytes = 1 << (funct3 & 3);
                let address = lhs.wrapping_add_signed(immediate);
                let raw = self.read_data(board, address, bytes)?;
                let value = if funct3 < 4 {
                    sign_extend(raw, bytes * 8) as u64
                } else {
                    raw
                };
                self.write(rd, value);
            }
            (0x23, 0..=3) => {
                let offset = (instruction & 0xfe00_0000) | ((instruction & 0x0f80) << 13);
                let address = lhs.wrapping_add_signed(i64::from((offset as i32) >> 20));
                self.write_data(board, address, 1 << funct3, rhs)?;
            }
            (0x6f, _) => {
                let offset = ((instruction >> 31) << 20)
                    | (instruction & 0xff000)
                    | ((instruction >> 9) & 0x800)
                    | ((instruction >> 20) & 0x7fe);
                self.write(rd, next);
                next = pc.wrapping_add_signed(sign_extend(u64::from(offset), 21));
            }
            (0x67, 0) => {
                self.write(rd, next);
                next = lhs.wrapping_add_signed(immediate) & !1;
            }
            (0x63, _) => {
                let take = match funct3 {
                    0 => lhs == rhs,
                    1 => lhs != rhs,
                    4 => (lhs as i64) < (rhs as i64),
                    5 => (lhs as i64) >= (rhs as i64),
                    6 => lhs < rhs,
                    7 => lhs >= rhs,
                    _ => return Err(illegal()),
                };
                if take {
                    let offset = ((instruction >> 31) << 12)
                        | ((instruction & 0x80) << 4)
                        | ((instruction >> 20) & 0x7e0)
                        | ((instruction >> 7) & 0x1e);
                    next = pc.wrapping_add_signed(sign_extend(u64::from(offset), 13));
                }
            }
            (0x2f, _) => {
                let (bytes, op) = atomic::decode(instruction).ok_or_else(illegal)?;
                self.execute_atomic(&mut board.ram, lhs, bytes, rhs, rd, op)?;
            }
            // FENCE orders an already in-order bus; FENCE.I has no cached
            // instructions to discard. The ISA requires ignoring reserved
            // register fields of these two forms, not other funct3 encodings.
            (0x0f, 0 | 1) => {}
            (0x73, 0) => match instruction {
                0x0000_0073 => return Err(self.trap(8 + self.privilege.mode as u64, 0)),
                0x0010_0073 => return Err(self.trap(3, pc)),
                0x1020_0073 | 0x3020_0073 => {
                    let invalid = illegal();
                    next = self
                        .privilege
                        .return_from(instruction == 0x3020_0073)
                        .ok_or(invalid)?;
                }
                0x1050_0073 if self.privilege.can_wfi() => self.waiting = true,
                _ if instruction & 0xfe00_7fff == 0x1200_0073
                    && self.privilege.mode != PrivilegeMode::User
                    && (self.privilege.mode == PrivilegeMode::Machine
                        || self.privilege.mstatus & privilege::MSTATUS_TVM == 0) =>
                {
                    // SFENCE.VMA: this implementation keeps no translations.
                }
                _ => return Err(illegal()),
            },
            (0x73, 1..=3 | 5..=7) => {
                let operand = if funct3 & 4 == 0 {
                    lhs
                } else {
                    u64::from((instruction >> 15) & 31)
                };
                let invalid = illegal();
                let previous = self
                    .privilege
                    .csr(instruction, operand, counters)
                    .ok_or(invalid)?;
                self.write(rd, previous);
            }
            _ => return Err(illegal()),
        }
        self.pc = next;
        Ok(())
    }

    fn read_data(&self, board: &mut Board, address: u64, bytes: usize) -> Result<u64, Trap> {
        if address & (bytes as u64 - 1) != 0 {
            return Err(self.trap(4, address));
        }
        let translation = self.translate(&board.ram, address, bytes, Access::Read)?;
        let physical = translation.address;
        if let Some(range) = dram_range(physical, bytes) {
            translation.commit(&mut board.ram);
            Ok(read_le(&board.ram[range]))
        } else {
            let value = board
                .read_mmio(physical, bytes)
                .ok_or_else(|| self.trap(5, address))?;
            translation.commit(&mut board.ram);
            Ok(value)
        }
    }

    fn read_ram(&self, ram: &mut [u8], address: u64, bytes: usize) -> Result<(u64, u64), Trap> {
        if address & (bytes as u64 - 1) != 0 {
            return Err(self.trap(4, address));
        }
        let (translation, range) = self.map_ram(ram, address, bytes, Access::Read)?;
        let physical = translation.address;
        translation.commit(ram);
        Ok((physical, read_le(&ram[range])))
    }

    fn write_data(
        &mut self,
        board: &mut Board,
        address: u64,
        bytes: usize,
        value: u64,
    ) -> Result<(), Trap> {
        if address & (bytes as u64 - 1) != 0 {
            return Err(self.trap(6, address));
        }
        let translation = self.translate(&board.ram, address, bytes, Access::Write)?;
        let physical = translation.address;
        if let Some(range) = dram_range(physical, bytes) {
            translation.commit(&mut board.ram);
            board.ram[range].copy_from_slice(&value.to_le_bytes()[..bytes]);
            self.invalidate_reservation(physical, bytes);
        } else {
            board
                .write_mmio(physical, bytes, value)
                .ok_or_else(|| self.trap(7, address))?;
            translation.commit(&mut board.ram);
        }
        Ok(())
    }

    pub(crate) fn invalidate_reservation(&mut self, address: u64, bytes: usize) {
        if let Some((start, width)) = self.reservation
            && bytes != 0
            && address < start + width as u64
            && start < address + bytes as u64
        {
            self.reservation = None;
        }
    }

    fn execute_atomic(
        &mut self,
        ram: &mut [u8],
        address: u64,
        bytes: usize,
        rhs: u64,
        rd: usize,
        op: atomic::Operation,
    ) -> Result<(), Trap> {
        use atomic::Operation::{LoadReserved, StoreConditional};

        let value = match op {
            LoadReserved => {
                let (physical, old) = self.read_ram(ram, address, bytes)?;
                self.reservation = Some((physical, bytes));
                sign_extend(old, bytes * 8) as u64
            }
            StoreConditional => {
                let reservation = self.reservation.take();
                let (translation, range) = self.store_range(ram, address, bytes, Access::Write)?;
                let reserved = reservation == Some((translation.address, bytes));
                if reserved {
                    translation.commit(ram);
                    ram[range].copy_from_slice(&rhs.to_le_bytes()[..bytes]);
                }
                u64::from(!reserved)
            }
            _ => {
                let (translation, range) = self.store_range(ram, address, bytes, Access::Atomic)?;
                let physical = translation.address;
                translation.commit(ram);
                let old = read_le(&ram[range.clone()]);
                let new = op.apply(bytes, old, rhs);
                ram[range].copy_from_slice(&new.to_le_bytes()[..bytes]);
                self.invalidate_reservation(physical, bytes);
                sign_extend(old, bytes * 8) as u64
            }
        };
        self.write(rd, value);
        Ok(())
    }

    fn store_range(
        &self,
        ram: &[u8],
        address: u64,
        bytes: usize,
        access: Access,
    ) -> Result<(mmu::Translation, Range<usize>), Trap> {
        if address & (bytes as u64 - 1) != 0 {
            return Err(self.trap(6, address));
        }
        self.map_ram(ram, address, bytes, access)
    }

    fn map_ram(
        &self,
        ram: &[u8],
        address: u64,
        bytes: usize,
        access: Access,
    ) -> Result<(mmu::Translation, Range<usize>), Trap> {
        let translation = self.translate(ram, address, bytes, access)?;
        let range = dram_range(translation.address, bytes)
            .ok_or_else(|| self.trap(access_fault(access), address))?;
        Ok((translation, range))
    }

    fn translate(
        &self,
        ram: &[u8],
        address: u64,
        bytes: usize,
        access: Access,
    ) -> Result<mmu::Translation, Trap> {
        let (mode, page_fault) = match access {
            Access::Execute => (self.privilege.mode, 12),
            Access::Read => (self.privilege.data_mode(), 13),
            Access::Write | Access::Atomic => (self.privilege.data_mode(), 15),
        };
        let translation =
            mmu::translate(&self.privilege, ram, address, access).map_err(|fault| {
                self.trap(
                    match fault {
                        mmu::Fault::Access => access_fault(access),
                        mmu::Fault::Page => page_fault,
                    },
                    address,
                )
            })?;
        let physical = translation.address;
        if !self.privilege.pmp.allows(physical, bytes, mode, access) {
            return Err(self.trap(access_fault(access), address));
        }
        Ok(translation)
    }

    fn write(&mut self, rd: usize, value: u64) {
        if rd != 0 {
            self.x[rd] = value;
        }
    }
}

fn sign_extend(value: u64, width: usize) -> i64 {
    let shift = 64 - width;
    ((value << shift) as i64) >> shift
}

fn access_fault(access: Access) -> u64 {
    match access {
        Access::Execute => 1,
        Access::Read => 5,
        Access::Write | Access::Atomic => 7,
    }
}
