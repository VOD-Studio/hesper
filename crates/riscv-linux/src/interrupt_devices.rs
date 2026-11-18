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

const UART_ID: u32 = 10;
const UART_MASK: u32 = 1 << UART_ID;
const PRIORITY_MASK: u32 = 7;

#[derive(Debug, Default)]
struct PlicContext {
    enabled: bool,
    threshold: u32,
}

#[derive(Debug)]
pub(crate) struct Plic {
    priority: u32,
    contexts: [PlicContext; 2],
    uart_line: bool,
    uart_pending: bool,
    uart_in_service: bool,
}

impl Plic {
    pub(crate) fn new() -> Self {
        Self {
            priority: 0,
            contexts: Default::default(),
            uart_line: false,
            uart_pending: false,
            uart_in_service: false,
        }
    }

    pub(crate) fn read(&mut self, offset: u64) -> u32 {
        match offset {
            0x28 => self.priority,
            0x1000 => u32::from(self.uart_pending) * UART_MASK,
            0x2000 => u32::from(self.contexts[0].enabled) * UART_MASK,
            0x2080 => u32::from(self.contexts[1].enabled) * UART_MASK,
            0x20_0000 => self.contexts[0].threshold,
            0x20_1000 => self.contexts[1].threshold,
            0x20_0004 => self.claim(0),
            0x20_1004 => self.claim(1),
            _ => 0,
        }
    }

    pub(crate) fn write(&mut self, offset: u64, value: u32) {
        match offset {
            0x28 => self.priority = value & PRIORITY_MASK,
            0x2000 => self.contexts[0].enabled = value & UART_MASK != 0,
            0x2080 => self.contexts[1].enabled = value & UART_MASK != 0,
            0x20_0000 => self.contexts[0].threshold = value & PRIORITY_MASK,
            0x20_1000 => self.contexts[1].threshold = value & PRIORITY_MASK,
            0x20_0004 => self.complete(0, value),
            0x20_1004 => self.complete(1, value),
            _ => {}
        }
    }

    pub(crate) fn set_uart_line(&mut self, asserted: bool) {
        self.uart_line = asserted;
        self.latch_uart();
    }

    pub(crate) fn pending(&self) -> u64 {
        if !self.uart_pending {
            return 0;
        }
        let notified = |ctx: &PlicContext| ctx.enabled && self.priority > ctx.threshold;
        (if notified(&self.contexts[0]) { MEIP } else { 0 })
            | if notified(&self.contexts[1]) { SEIP } else { 0 }
    }

    fn latch_uart(&mut self) {
        // Deassertion cannot withdraw a request already forwarded to the PLIC core.
        if self.uart_line && !self.uart_in_service {
            self.uart_pending = true;
        }
    }

    fn claim(&mut self, context: usize) -> u32 {
        // The threshold gates notifications, not explicit claims.
        if self.uart_pending && self.contexts[context].enabled && self.priority != 0 {
            self.uart_pending = false;
            self.uart_in_service = true;
            UART_ID
        } else {
            0
        }
    }

    fn complete(&mut self, context: usize, id: u32) {
        if id == UART_ID && self.contexts[context].enabled && self.uart_in_service {
            self.uart_in_service = false;
            self.latch_uart();
        }
    }
}
