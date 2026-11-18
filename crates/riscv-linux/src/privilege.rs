use crate::Trap;
use crate::pmp::Pmp;

/// Current RISC-V hart privilege, encoded as in `mstatus.MPP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PrivilegeMode {
    User = 0,
    Supervisor = 1,
    Machine = 3,
}

const SIE: u64 = 1 << 1;
const MIE: u64 = 1 << 3;
const SPIE: u64 = 1 << 5;
const MPIE: u64 = 1 << 7;
const SPP: u64 = 1 << 8;
const MPP_SHIFT: u32 = 11;
const MPP: u64 = 3 << MPP_SHIFT;
const MPRV: u64 = 1 << 17;
const SUM: u64 = 1 << 18;
const MXR: u64 = 1 << 19;
pub(crate) const MSTATUS_TVM: u64 = 1 << 20;
const TW: u64 = 1 << 21;
const TSR: u64 = 1 << 22;
const UXL_64: u64 = 2 << 32;
const SXL_64: u64 = 2 << 34;
// Both lower modes stay RV64 and little endian; no floating-point, vector or
// other extension state is tracked (FS, VS, XS and SD are hardwired zero).
const SSTATUS_WRITABLE: u64 = SIE | SPIE | SPP | SUM | MXR;
const MSTATUS_WRITABLE: u64 = SSTATUS_WRITABLE | MIE | MPIE | MPP | MPRV | MSTATUS_TVM | TW | TSR;
const MISA: u64 = (2 << 62) | (1 << 20) | (1 << 18) | (1 << 12) | (1 << 8) | (1 << 2) | 1;

const INTERRUPT: u64 = 1 << 63;
const SSIP: u64 = 1 << 1;
const MSIP: u64 = 1 << 3;
const STIP: u64 = 1 << 5;
const MTIP: u64 = 1 << 7;
const SEIP: u64 = 1 << 9;
const MEIP: u64 = 1 << 11;
const SUPERVISOR_INTERRUPTS: u64 = SSIP | STIP | SEIP;
const BOARD_INTERRUPT_LINES: u64 = MSIP | MTIP | MEIP | SEIP;
const IMPLEMENTED_INTERRUPTS: u64 = SUPERVISOR_INTERRUPTS | BOARD_INTERRUPT_LINES;
const INTERRUPT_PRIORITY: [u64; 6] = [MEIP, MSIP, MTIP, SEIP, SSIP, STIP];

// Standard synchronous causes from 0 through 9, plus the page-fault causes.
// Page-fault delegation is programmable even though translation is not yet implemented.
const DELEGATABLE_EXCEPTIONS: u64 = 0x03ff | (1 << 12) | (1 << 13) | (1 << 15);
const COUNTER_MASK: u64 = 0b111;
// FIOM is supported by the board's synchronous, ordered accesses. Environment
// configuration for absent extensions (e.g. Sstc, Svadu, Svpbmt and CMO) reads zero.
const FIOM: u64 = 1;

const SSTATUS: u32 = 0x100;
const SIE_CSR: u32 = 0x104;
const STVEC: u32 = 0x105;
const SCOUNTEREN: u32 = 0x106;
const SENVCFG: u32 = 0x10a;
const SSCRATCH: u32 = 0x140;
const SEPC: u32 = 0x141;
const SCAUSE: u32 = 0x142;
const STVAL: u32 = 0x143;
const SIP: u32 = 0x144;
const SATP: u32 = 0x180;
const MSTATUS: u32 = 0x300;
const MISA_CSR: u32 = 0x301;
const MEDELEG: u32 = 0x302;
const MIDELEG: u32 = 0x303;
const MIE_CSR: u32 = 0x304;
const MTVEC: u32 = 0x305;
const MCOUNTEREN: u32 = 0x306;
const MENVCFG: u32 = 0x30a;
const MSCRATCH: u32 = 0x340;
const MEPC: u32 = 0x341;
const MCAUSE: u32 = 0x342;
const MTVAL: u32 = 0x343;
const MIP: u32 = 0x344;
const MCYCLE: u32 = 0xb00;
const MINSTRET: u32 = 0xb02;
const CYCLE: u32 = 0xc00;
const TIME: u32 = 0xc01;
const INSTRET: u32 = 0xc02;
const MVENDORID: u32 = 0xf11;
const MCONFIGPTR: u32 = 0xf15;

#[derive(Debug, Default)]
struct TrapCsrs {
    vector: u64,
    scratch: u64,
    epc: u64,
    cause: u64,
    value: u64,
}

impl TrapCsrs {
    fn enter(&mut self, trap: Trap) -> u64 {
        self.epc = trap.epc & !1; // IALIGN is fixed at 16 by C.
        self.cause = trap.cause;
        self.value = trap.value;
        let base = self.vector & !3;
        if self.vector & 3 == 1 && trap.cause & INTERRUPT != 0 {
            base.wrapping_add((trap.cause & !INTERRUPT).wrapping_mul(4))
        } else {
            base
        }
    }
}

#[derive(Debug)]
pub(crate) struct State {
    pub(crate) mode: PrivilegeMode,
    pub(crate) mstatus: u64,
    pub(crate) satp: u64,
    pub(crate) pmp: Pmp,
    supervisor: TrapCsrs,
    machine: TrapCsrs,
    medeleg: u64,
    mideleg: u64,
    mie: u64,
    mip_software: u64,
    interrupt_lines: u64,
    mcounteren: u64,
    scounteren: u64,
    menvcfg: u64,
    senvcfg: u64,
    cycle_offset: u64,
    instret_offset: u64,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            mode: PrivilegeMode::Machine,
            mstatus: SXL_64 | UXL_64,
            satp: 0,
            pmp: Pmp::default(),
            supervisor: TrapCsrs::default(),
            machine: TrapCsrs::default(),
            medeleg: 0,
            mideleg: 0,
            mie: 0,
            mip_software: 0,
            interrupt_lines: 0,
            mcounteren: 0,
            scounteren: 0,
            menvcfg: 0,
            senvcfg: 0,
            cycle_offset: 0,
            instret_offset: 0,
        }
    }

    /// Execute a CSR instruction with the pre-instruction cycle, time and instret counts.
    pub(crate) fn csr(
        &mut self,
        instruction: u32,
        operand: u64,
        counters: [u64; 3],
    ) -> Option<u64> {
        let operation = (instruction >> 12) & 3;
        if instruction & 0x7f != 0x73 || operation == 0 {
            return None;
        }
        let csr = instruction >> 20;
        let write = operation == 1 || (instruction >> 15) & 31 != 0;
        if (csr >> 8) & 3 > self.mode as u32
            || (write && csr >> 10 == 3)
            || (csr == SATP
                && self.mode == PrivilegeMode::Supervisor
                && self.mstatus & MSTATUS_TVM != 0)
        {
            return None;
        }

        let old = if operation == 1 && (instruction >> 7) & 31 == 0 {
            0 // CSRRW[I] with rd=x0 suppresses the CSR read.
        } else {
            self.read_csr(csr, counters)?
        };
        if write {
            // For mip.SEIP, the physical line participates only in readback,
            // never in a CSR read-modify-write of the software pending bit.
            let stored = if csr == MIP { self.mip_software } else { old };
            let value = match operation {
                1 => operand,
                2 => stored | operand,
                _ => stored & !operand,
            };
            self.write_csr(csr, value, counters)?;
        }
        Some(old)
    }

    fn read_csr(&self, csr: u32, counters: [u64; 3]) -> Option<u64> {
        Some(match csr {
            SSTATUS => self.mstatus & (SSTATUS_WRITABLE | UXL_64),
            SIE_CSR => self.mie & self.mideleg,
            STVEC => self.supervisor.vector,
            SCOUNTEREN => self.scounteren,
            SENVCFG => self.senvcfg,
            SSCRATCH => self.supervisor.scratch,
            SEPC => self.supervisor.epc,
            SCAUSE => self.supervisor.cause,
            STVAL => self.supervisor.value,
            SIP => self.pending() & self.mideleg,
            SATP => self.satp,
            MSTATUS => self.mstatus,
            MISA_CSR => MISA,
            MEDELEG => self.medeleg,
            MIDELEG => self.mideleg,
            MIE_CSR => self.mie,
            MTVEC => self.machine.vector,
            MCOUNTEREN => self.mcounteren,
            MENVCFG => self.menvcfg,
            MSCRATCH => self.machine.scratch,
            MEPC => self.machine.epc,
            MCAUSE => self.machine.cause,
            MTVAL => self.machine.value,
            MIP => self.pending(),
            MCYCLE => counters[0].wrapping_add(self.cycle_offset),
            MINSTRET => counters[2].wrapping_add(self.instret_offset),
            CYCLE..=INSTRET => {
                let index = csr - CYCLE;
                let bit = 1 << index;
                if (self.mode != PrivilegeMode::Machine && self.mcounteren & bit == 0)
                    || (self.mode == PrivilegeMode::User && self.scounteren & bit == 0)
                {
                    return None;
                }
                match csr {
                    CYCLE => counters[0].wrapping_add(self.cycle_offset),
                    TIME => counters[1],
                    _ => counters[2].wrapping_add(self.instret_offset),
                }
            }
            // The single hart is hart 0; there are no assigned IDs or config structure.
            MVENDORID..=MCONFIGPTR => 0,
            // No HPM or counter inhibition yet; unimplemented CSRs trap.
            _ => return self.pmp.read(csr),
        })
    }

    fn write_csr(&mut self, csr: u32, value: u64, counters: [u64; 3]) -> Option<()> {
        match csr {
            SSTATUS => {
                self.mstatus = (self.mstatus & !SSTATUS_WRITABLE) | (value & SSTATUS_WRITABLE)
            }
            SIE_CSR => self.mie = (self.mie & !self.mideleg) | (value & self.mideleg),
            STVEC => self.supervisor.vector = trap_vector(value),
            SCOUNTEREN => self.scounteren = value & COUNTER_MASK,
            SENVCFG => self.senvcfg = value & FIOM,
            SSCRATCH => self.supervisor.scratch = value,
            SEPC => self.supervisor.epc = value & !1,
            SCAUSE => self.supervisor.cause = value,
            STVAL => self.supervisor.value = value,
            SIP => {
                let writable = self.mideleg & SSIP;
                self.mip_software = (self.mip_software & !writable) | (value & writable);
            }
            SATP => match value >> 60 {
                0 => self.satp = 0, // Bare has no ASID or PPN.
                8 => self.satp = value,
                _ => {} // An unsupported MODE leaves the entire CSR unchanged.
            },
            MSTATUS => {
                self.mstatus = (value & MSTATUS_WRITABLE) | SXL_64 | UXL_64;
                if self.mstatus & MPP == 2 << MPP_SHIFT {
                    self.mstatus &= !MPP; // Reserved encoding coerces to U-mode.
                }
            }
            MISA_CSR => {} // Fixed RV64IMACSU is the only legal configuration.
            MEDELEG => self.medeleg = value & DELEGATABLE_EXCEPTIONS,
            MIDELEG => self.mideleg = value & SUPERVISOR_INTERRUPTS,
            MIE_CSR => self.mie = value & IMPLEMENTED_INTERRUPTS,
            MTVEC => self.machine.vector = trap_vector(value),
            MCOUNTEREN => self.mcounteren = value & COUNTER_MASK,
            MENVCFG => self.menvcfg = value & FIOM,
            MSCRATCH => self.machine.scratch = value,
            MEPC => self.machine.epc = value & !1,
            MCAUSE => self.machine.cause = value,
            MTVAL => self.machine.value = value,
            // Without Sstc, M-mode software (e.g. OpenSBI) injects STIP here.
            MIP => self.mip_software = value & SUPERVISOR_INTERRUPTS,
            // Explicit writes replace this instruction's automatic counter increments.
            MCYCLE => self.cycle_offset = value.wrapping_sub(counters[0].wrapping_add(1)),
            MINSTRET => self.instret_offset = value.wrapping_sub(counters[2].wrapping_add(1)),
            _ => return self.pmp.write(csr, value),
        }
        Some(())
    }

    pub(crate) fn data_mode(&self) -> PrivilegeMode {
        if self.mode != PrivilegeMode::Machine || self.mstatus & MPRV == 0 {
            return self.mode;
        }
        match (self.mstatus & MPP) >> MPP_SHIFT {
            0 => PrivilegeMode::User,
            1 => PrivilegeMode::Supervisor,
            _ => PrivilegeMode::Machine,
        }
    }

    pub(crate) fn take_trap(&mut self, trap: Trap) -> u64 {
        let interrupt = trap.cause & INTERRUPT != 0;
        let code = trap.cause & !INTERRUPT;
        let deleg = if interrupt {
            self.mideleg
        } else {
            self.medeleg
        };
        if self.mode != PrivilegeMode::Machine && code < 64 && deleg & (1 << code) != 0 {
            self.push_interrupt_enable(SIE, SPIE);
            self.mstatus &= !SPP;
            if self.mode == PrivilegeMode::Supervisor {
                self.mstatus |= SPP;
            }
            self.mode = PrivilegeMode::Supervisor;
            self.supervisor.enter(trap)
        } else {
            self.push_interrupt_enable(MIE, MPIE);
            self.mstatus = (self.mstatus & !MPP) | ((self.mode as u64) << MPP_SHIFT);
            self.mode = PrivilegeMode::Machine;
            self.machine.enter(trap)
        }
    }

    pub(crate) fn return_from(&mut self, machine: bool) -> Option<u64> {
        let (mode, pc) = if machine {
            if self.mode != PrivilegeMode::Machine {
                return None;
            }
            let mode = match self.mstatus & MPP {
                0 => PrivilegeMode::User,
                x if x == 1 << MPP_SHIFT => PrivilegeMode::Supervisor,
                _ => PrivilegeMode::Machine,
            };
            self.pop_interrupt_enable(MIE, MPIE);
            self.mstatus &= !MPP;
            (mode, self.machine.epc)
        } else {
            if self.mode == PrivilegeMode::User
                || (self.mode == PrivilegeMode::Supervisor && self.mstatus & TSR != 0)
            {
                return None;
            }
            let mode = if self.mstatus & SPP != 0 {
                PrivilegeMode::Supervisor
            } else {
                PrivilegeMode::User
            };
            self.pop_interrupt_enable(SIE, SPIE);
            self.mstatus &= !SPP;
            (mode, self.supervisor.epc)
        };
        if mode != PrivilegeMode::Machine {
            self.mstatus &= !MPRV;
        }
        self.mode = mode;
        Some(pc)
    }

    fn push_interrupt_enable(&mut self, ie: u64, pie: u64) {
        let was_enabled = self.mstatus & ie != 0;
        self.mstatus &= !(ie | pie);
        if was_enabled {
            self.mstatus |= pie;
        }
    }

    fn pop_interrupt_enable(&mut self, ie: u64, pie: u64) {
        let was_enabled = self.mstatus & pie != 0;
        self.mstatus = (self.mstatus & !ie) | pie;
        if was_enabled {
            self.mstatus |= ie;
        }
    }

    pub(crate) fn set_interrupt_lines(&mut self, mask: u64) {
        self.interrupt_lines = mask & BOARD_INTERRUPT_LINES;
    }

    fn pending(&self) -> u64 {
        self.mip_software | self.interrupt_lines
    }

    pub(crate) fn pending_interrupt(&self) -> Option<u64> {
        let pending = self.pending() & self.mie;
        if pending == 0 {
            return None;
        }
        let machine = if self.mode != PrivilegeMode::Machine || self.mstatus & MIE != 0 {
            pending & !self.mideleg
        } else {
            0
        };
        let eligible = if machine != 0 {
            machine
        } else if self.mode == PrivilegeMode::User
            || (self.mode == PrivilegeMode::Supervisor && self.mstatus & SIE != 0)
        {
            pending & self.mideleg
        } else {
            return None;
        };
        INTERRUPT_PRIORITY
            .into_iter()
            .find(|bit| eligible & bit != 0)
            .map(|bit| INTERRUPT | u64::from(bit.trailing_zeros()))
    }

    pub(crate) fn locally_pending(&self) -> bool {
        // WFI wakeup ignores global interrupt enables and delegation.
        self.pending() & self.mie != 0
    }

    pub(crate) fn can_wfi(&self) -> bool {
        // Implement the allowed immediate timeout for U-mode and for S-mode with TW=1.
        self.mode == PrivilegeMode::Machine
            || (self.mode == PrivilegeMode::Supervisor && self.mstatus & TW == 0)
    }
}

fn trap_vector(value: u64) -> u64 {
    // Direct and Vectored both use four-byte BASE alignment; reserved modes
    // fall back to Direct without changing the supplied BASE.
    if value & 3 <= 1 { value } else { value & !3 }
}
