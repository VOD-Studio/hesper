const MSIP: u64 = 1 << 3;
const MTIP: u64 = 1 << 7;
const MEIP: u64 = 1 << 11;
const SEIP: u64 = 1 << 9;

#[derive(Debug)]
pub(crate) struct Clint {
    msip: bool,
    mtimecmp: u64,
    mtime_offset: u64,
}

impl Clint {
    pub(crate) fn new() -> Self {
        Self {
            msip: false,
            mtimecmp: u64::MAX,
            mtime_offset: 0,
        }
    }

    pub(crate) fn read(&self, offset: u64, bytes: usize, ticks: u64) -> Option<u64> {
        Some(match (offset, bytes) {
            (0, 4) => u64::from(self.msip),
            (0x4000, 8) => self.mtimecmp,
            (0x4000, 4) => u64::from(self.mtimecmp as u32),
            (0x4004, 4) => self.mtimecmp >> 32,
            (0xbff8, 8) => self.time(ticks),
            (0xbff8, 4) => u64::from(self.time(ticks) as u32),
            (0xbffc, 4) => self.time(ticks) >> 32,
            _ => return None,
        })
    }

    pub(crate) fn write(
        &mut self,
        offset: u64,
        bytes: usize,
        value: u64,
        ticks: u64,
    ) -> Option<()> {
        match (offset, bytes) {
            (0, 4) => self.msip = value & 1 != 0,
            (0x4000, 8) => self.mtimecmp = value,
            (0x4000, 4) => self.mtimecmp = replace_word(self.mtimecmp, value, 0),
            (0x4004, 4) => self.mtimecmp = replace_word(self.mtimecmp, value, 32),
            (0xbff8, 8) => self.mtime_offset = value.wrapping_sub(ticks),
            (0xbff8, 4) => {
                self.mtime_offset = replace_word(self.time(ticks), value, 0).wrapping_sub(ticks);
            }
            (0xbffc, 4) => {
                self.mtime_offset = replace_word(self.time(ticks), value, 32).wrapping_sub(ticks);
            }
            _ => return None,
        }
        Some(())
    }

    pub(crate) fn time(&self, ticks: u64) -> u64 {
        ticks.wrapping_add(self.mtime_offset)
    }

    pub(crate) fn pending(&self, ticks: u64) -> u64 {
        (if self.msip { MSIP } else { 0 })
            | if self.time(ticks) >= self.mtimecmp {
                MTIP
            } else {
                0
            }
    }

    pub(crate) fn next_event(&self, ticks: u64) -> Option<u64> {
        // Firmware uses MAX to disarm the timer. Keep normal MTIP comparisons
        // if time actually reaches MAX, but never fast-forward WFI there.
        if self.mtimecmp == u64::MAX {
            return None;
        }
        let delta = self.mtimecmp.checked_sub(self.time(ticks))?;
        if delta == 0 {
            return None;
        }
        ticks.checked_add(delta)
    }
}

fn replace_word(register: u64, value: u64, shift: u32) -> u64 {
    let mask = u64::from(u32::MAX) << shift;
    (register & !mask) | ((value << shift) & mask)
}

pub(crate) const BLOCK: u32 = 1 << 1;
const UART: u32 = 1 << 10;
const SOURCES: u32 = BLOCK | UART;
const PRIORITY_MASK: u32 = 7;

#[derive(Debug, Default)]
struct PlicContext {
    enabled: u32,
    threshold: u32,
}

#[derive(Debug)]
pub(crate) struct Plic {
    block_priority: u32,
    uart_priority: u32,
    contexts: [PlicContext; 2],
    lines: u32,
    pending: u32,
    in_service: u32,
}

impl Plic {
    pub(crate) fn new() -> Self {
        Self {
            block_priority: 0,
            uart_priority: 0,
            contexts: Default::default(),
            lines: 0,
            pending: 0,
            in_service: 0,
        }
    }

    pub(crate) fn read(&mut self, offset: u64) -> u32 {
        match offset {
            0x04 => self.block_priority,
            0x28 => self.uart_priority,
            0x1000 => self.pending,
            0x2000 => self.contexts[0].enabled,
            0x2080 => self.contexts[1].enabled,
            0x20_0000 => self.contexts[0].threshold,
            0x20_1000 => self.contexts[1].threshold,
            0x20_0004 => self.claim(0),
            0x20_1004 => self.claim(1),
            _ => 0,
        }
    }

    pub(crate) fn write(&mut self, offset: u64, value: u32) {
        match offset {
            0x04 => self.block_priority = value & PRIORITY_MASK,
            0x28 => self.uart_priority = value & PRIORITY_MASK,
            0x2000 => self.contexts[0].enabled = value & SOURCES,
            0x2080 => self.contexts[1].enabled = value & SOURCES,
            0x20_0000 => self.contexts[0].threshold = value & PRIORITY_MASK,
            0x20_1000 => self.contexts[1].threshold = value & PRIORITY_MASK,
            0x20_0004 => self.complete(0, value),
            0x20_1004 => self.complete(1, value),
            _ => {}
        }
    }

    pub(crate) fn set_uart_line(&mut self, asserted: bool) {
        self.set_line(UART, asserted);
    }

    pub(crate) fn set_line(&mut self, mask: u32, asserted: bool) {
        // A source cannot withdraw a pending request by deasserting its line.
        if asserted {
            self.lines |= mask;
            self.pending |= mask & !self.in_service;
        } else {
            self.lines &= !mask;
        }
    }

    pub(crate) fn pending(&self) -> u64 {
        if self.pending == 0 {
            return 0;
        }
        let notified = |ctx: &PlicContext| self.best(ctx.enabled, ctx.threshold) != 0;
        (if notified(&self.contexts[0]) { MEIP } else { 0 })
            | if notified(&self.contexts[1]) { SEIP } else { 0 }
    }

    fn best(&self, enabled: u32, threshold: u32) -> u32 {
        let pending = self.pending & enabled;
        let block = pending & BLOCK != 0 && self.block_priority > threshold;
        let uart = pending & UART != 0 && self.uart_priority > threshold;
        if block && (!uart || self.block_priority >= self.uart_priority) {
            BLOCK
        } else if uart {
            UART
        } else {
            0
        }
    }

    fn claim(&mut self, context: usize) -> u32 {
        // The threshold gates notifications, not explicit claims.
        let mask = self.best(self.contexts[context].enabled, 0);
        if mask == 0 {
            return 0;
        }
        self.pending &= !mask;
        self.in_service |= mask;
        mask.trailing_zeros()
    }

    fn complete(&mut self, context: usize, id: u32) {
        let mask = 1u32.checked_shl(id).unwrap_or(0);
        if self.in_service & self.contexts[context].enabled & mask != 0 {
            self.in_service &= !mask;
            self.pending |= self.lines & mask;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plic_prioritizes_independent_sources_and_masks_each_context() {
        let mut plic = Plic::new();
        plic.write(0x04, 1);
        plic.write(0x28, 2);
        plic.write(0x2000, BLOCK);
        plic.write(0x2080, BLOCK | UART);
        plic.write(0x20_0000, 1);
        plic.set_line(BLOCK, true);
        plic.set_uart_line(true);
        assert_eq!(plic.read(0x1000), BLOCK | UART);
        assert_eq!(plic.pending(), SEIP);
        assert_eq!(plic.read(0x20_1004), 10);
        assert_eq!(plic.pending(), SEIP);
        assert_eq!(plic.read(0x20_0004), 1);
        assert_eq!(plic.pending(), 0);
        plic.write(0x20_0004, 10); // UART wasn't enabled in M.
        assert_eq!(plic.read(0x1000), 0);
        plic.set_line(BLOCK, false);
        plic.write(0x20_0004, 1);
        plic.write(0x20_1004, 10);
        assert_eq!(plic.read(0x1000), UART);
        plic.set_line(BLOCK, true);
        plic.write(0x04, 2);
        assert_eq!(plic.pending(), MEIP | SEIP);
        assert_eq!(plic.read(0x20_1004), 1); // Lower source ID wins the tie.
        assert_eq!(plic.read(0x20_1004), 10);
    }
}
