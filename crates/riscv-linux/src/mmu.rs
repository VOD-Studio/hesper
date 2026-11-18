use crate::{PrivilegeMode, dram_range, pmp::Access, privilege::State};

const PAGE_SHIFT: u32 = 12;
const VPN_BITS: u32 = 9;
const VPN_MASK: u64 = (1 << VPN_BITS) - 1;
const PPN_MASK: u64 = (1 << 44) - 1;
const PTE_BYTES: usize = 8;

const V: u64 = 1 << 0;
const R: u64 = 1 << 1;
const W: u64 = 1 << 2;
const X: u64 = 1 << 3;
const U: u64 = 1 << 4;
const A: u64 = 1 << 6;
const D: u64 = 1 << 7;
const SUM: u64 = 1 << 18;
const MXR: u64 = 1 << 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    Page,
    Access,
}

#[derive(Debug)]
pub(crate) struct Translation {
    pub(crate) address: u64,
    update: Option<(usize, u64)>,
}

impl Translation {
    /// Apply the checked PTE update once the explicit access is permitted.
    pub(crate) fn commit(self, ram: &mut [u8]) {
        if let Some((offset, pte)) = self.update {
            ram[offset..offset + PTE_BYTES].copy_from_slice(&pte.to_le_bytes());
        }
    }
}

pub(crate) fn translate(
    state: &State,
    ram: &[u8],
    virtual_address: u64,
    access: Access,
) -> Result<Translation, Fault> {
    let mode = if access == Access::Execute {
        state.mode
    } else {
        state.data_mode()
    };
    if mode == PrivilegeMode::Machine || state.satp >> 60 == 0 {
        return Ok(Translation {
            address: virtual_address,
            update: None,
        });
    }

    // SATP's WARL handling accepts only Bare and Sv39.
    if ((virtual_address as i64) << 25 >> 25) as u64 != virtual_address {
        return Err(Fault::Page);
    }

    let mut table = (state.satp & PPN_MASK) << PAGE_SHIFT;
    for level in (0..3).rev() {
        let shift = PAGE_SHIFT + level * VPN_BITS;
        let index = (virtual_address >> shift) & VPN_MASK;
        let pte_address = table + index * PTE_BYTES as u64;
        if !state.pmp.allows(
            pte_address,
            PTE_BYTES,
            PrivilegeMode::Supervisor,
            Access::Read,
        ) {
            return Err(Fault::Access);
        }
        let range = dram_range(pte_address, PTE_BYTES).ok_or(Fault::Access)?;
        let offset = range.start;
        let bytes = ram.get(range).ok_or(Fault::Access)?;
        let pte = u64::from_le_bytes(bytes.try_into().unwrap());
        // Base Sv39 reserves every bit above the PPN, including N and PBMT.
        if pte >> 54 != 0 || pte & V == 0 || (pte & (R | W) == W) {
            return Err(Fault::Page);
        }

        let page = ((pte >> 10) & PPN_MASK) << PAGE_SHIFT;
        if pte & (R | X) == 0 {
            if level == 0 || pte & (U | A | D) != 0 {
                return Err(Fault::Page);
            }
            table = page;
            continue;
        }

        let user_page = pte & U != 0;
        let mode_permitted = if mode == PrivilegeMode::User {
            user_page
        } else {
            !user_page || (access != Access::Execute && state.mstatus & SUM != 0)
        };
        let access_permitted = match access {
            Access::Read => pte & R != 0 || (state.mstatus & MXR != 0 && pte & X != 0),
            Access::Write => pte & W != 0,
            Access::Atomic => pte & (R | W) == (R | W),
            Access::Execute => pte & X != 0,
        };
        let offset_mask = (1 << shift) - 1;
        if !mode_permitted || !access_permitted || page & offset_mask != 0 {
            return Err(Fault::Page);
        }

        let updated_pte = pte
            | A
            | if matches!(access, Access::Write | Access::Atomic) {
                D
            } else {
                0
            };
        let update = if updated_pte == pte {
            None
        } else {
            if !state.pmp.allows(
                pte_address,
                PTE_BYTES,
                PrivilegeMode::Supervisor,
                Access::Write,
            ) {
                return Err(Fault::Access);
            }
            Some((offset, updated_pte))
        };
        return Ok(Translation {
            address: page | (virtual_address & offset_mask),
            update,
        });
    }
    Err(Fault::Page)
}
