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

#[derive(Clone, Copy, Debug, Default)]
struct Region {
    start: u64,
    end: u64,
    cfg: u8,
}

#[derive(Debug, Default)]
pub(crate) struct Pmp {
    cfg: [u8; SLOTS],
    addr: [u64; SLOTS],
    regions: [Region; SLOTS],
    region_count: usize,
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
        // TOR can depend on the preceding PMPADDR, even across CSR banks
        // and when the preceding entry itself is OFF.
        self.update_regions();
        Some(())
    }

    fn update_regions(&mut self) {
        self.region_count = 0;
        for (index, cfg) in self.cfg.iter().copied().enumerate() {
            let encoded = self.addr[index];
            let (start, end) = match cfg & MODE {
                0 => continue,
                TOR => (
                    index.checked_sub(1).map_or(0, |i| self.addr[i] << 2),
                    encoded << 2,
                ),
                0x10 => (encoded << 2, (encoded + 1) << 2),
                _ => {
                    let bits = encoded.trailing_ones();
                    let mask = (1 << bits) - 1;
                    let start = (encoded & !mask) << 2;
                    (start, start + (8 << bits))
                }
            };
            // Keep enabled, empty TOR entries in the list for the overflow
            // check on the requested span.
            let end = if start < end { end } else { 0 };
            self.regions[self.region_count] = Region { start, end, cfg };
            self.region_count += 1;
        }
    }

    pub(crate) fn allows(
        &self,
        address: u64,
        bytes: usize,
        mode: PrivilegeMode,
        access: Access,
    ) -> bool {
        let machine = mode == PrivilegeMode::Machine;
        if self.region_count == 0 {
            return machine;
        }
        let Some(end) = address.checked_add(bytes as u64) else {
            return false;
        };
        for region in &self.regions[..self.region_count] {
            if address < region.end && region.start < end {
                return region.start <= address
                    && end <= region.end
                    && (machine && region.cfg & LOCK == 0
                        || region.cfg & access as u8 == access as u8);
            }
        }
        machine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tor_tracks_predecessor_and_config_writes_across_csr_banks() {
        let mut pmp = Pmp::default();
        let supervisor = PrivilegeMode::Supervisor;
        pmp.write(0x3b7, 0x1000 >> 2).unwrap();
        pmp.write(0x3b8, 0x1010 >> 2).unwrap();
        pmp.write(0x3a2, u64::from(TOR | Access::Read as u8))
            .unwrap();

        assert!(pmp.allows(0x1000, 8, supervisor, Access::Read));
        assert!(!pmp.allows(0x1000, 8, supervisor, Access::Write));
        pmp.write(0x3b7, 0x1004 >> 2).unwrap();
        assert!(!pmp.allows(0x1000, 8, supervisor, Access::Read));
        assert!(pmp.allows(0x1004, 8, supervisor, Access::Read));
        pmp.write(0x3b8, 0x1004 >> 2).unwrap();
        assert!(!pmp.allows(0x1004, 4, supervisor, Access::Read));

        pmp.write(0x3a2, 0x10 | Access::Execute as u64).unwrap();
        assert!(pmp.allows(0x1004, 4, supervisor, Access::Execute));
        assert!(!pmp.allows(0x1004, 4, supervisor, Access::Read));
        pmp.write(0x3a2, 0).unwrap();
        assert!(!pmp.allows(0x1004, 4, supervisor, Access::Execute));
    }
}
