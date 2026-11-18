use crate::PrivilegeMode;

const SLOTS: usize = 16;
const ADDR_MASK: u64 = (1 << 54) - 1;
const LOCK: u8 = 0x80;
const MODE: u8 = 0x18;
const TOR: u8 = 0x08;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Read = 1,
    Write = 2,
    Atomic = 3,
    Execute = 4,
}

#[derive(Debug, Default)]
pub(crate) struct Pmp {
    cfg: [u8; SLOTS],
    addr: [u64; SLOTS],
    active: u16,
}

impl Pmp {
    pub(crate) fn read(&self, csr: u32) -> Option<u64> {
        Some(match csr {
            0x3a0 => u64::from_le_bytes(self.cfg[..8].try_into().unwrap()),
            0x3a2 => u64::from_le_bytes(self.cfg[8..].try_into().unwrap()),
            0x3b0..=0x3bf => self.addr[(csr - 0x3b0) as usize],
            _ => return None,
        })
    }

    pub(crate) fn write(&mut self, csr: u32, value: u64) -> Option<()> {
        match csr {
            0x3a0 | 0x3a2 => {
                let base = (csr - 0x3a0) as usize * 4;
                for (offset, new) in value.to_le_bytes().into_iter().enumerate() {
                    let index = base + offset;
                    if self.cfg[index] & LOCK == 0 {
                        let mut cfg = new & 0x9f;
                        if cfg & 3 == 2 {
                            cfg &= !2; // Reserved R=0/W=1 coerces to no access.
                        }
                        self.cfg[index] = cfg;
                        let bit = 1 << index;
                        self.active = (self.active & !bit) | if cfg & MODE != 0 { bit } else { 0 };
                    }
                }
            }
            0x3b0..=0x3bf => {
                let index = (csr - 0x3b0) as usize;
                let locked_tor = self
                    .cfg
                    .get(index + 1)
                    .is_some_and(|cfg| cfg & (LOCK | MODE) == LOCK | TOR);
                if self.cfg[index] & LOCK == 0 && !locked_tor {
                    self.addr[index] = value & ADDR_MASK;
                }
            }
            _ => return None,
        }
        Some(())
    }

    pub(crate) fn allows(
        &self,
        address: u64,
        bytes: usize,
        mode: PrivilegeMode,
        access: Access,
    ) -> bool {
        let machine = mode == PrivilegeMode::Machine;
        if self.active == 0 {
            return machine;
        }
        let Some(end) = address.checked_add(bytes as u64) else {
            return false;
        };
        let mut active = self.active;
        while active != 0 {
            let index = active.trailing_zeros() as usize;
            active &= active - 1;
            let cfg = self.cfg[index];
            let encoded = self.addr[index];
            let (lo, hi) = match cfg & MODE {
                TOR => (
                    index.checked_sub(1).map_or(0, |i| self.addr[i] << 2),
                    encoded << 2,
                ),
                0x10 => (encoded << 2, (encoded + 1) << 2),
                _ => {
                    let bits = encoded.trailing_ones();
                    let mask = (1 << bits) - 1;
                    let lo = (encoded & !mask) << 2;
                    (lo, lo + (8 << bits))
                }
            };
            if lo < hi && address < hi && lo < end {
                return lo <= address
                    && end <= hi
                    && (machine && cfg & LOCK == 0 || cfg & access as u8 == access as u8);
            }
        }
        machine
    }
}
