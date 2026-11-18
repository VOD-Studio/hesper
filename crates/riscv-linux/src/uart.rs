const TICKS_PER_SECOND: u64 = 10_000_000;
const UART_CLOCK: u64 = 3_686_400;
const FIFO_DEPTH: usize = 16;
const HOST_QUEUE_DEPTH: usize = 4096;

const IER_RX: u8 = 0x01;
const IER_THRE: u8 = 0x02;
const IER_LINE: u8 = 0x04;
const IER_MODEM: u8 = 0x08;

const IIR_MODEM: u8 = 0x00;
const IIR_IDLE: u8 = 0x01;
const IIR_THRE: u8 = 0x02;
const IIR_RX: u8 = 0x04;
const IIR_LINE: u8 = 0x06;
const IIR_TIMEOUT: u8 = 0x0c;
const IIR_FIFO: u8 = 0xc0;

const FCR_ENABLE: u8 = 0x01;
const FCR_RESET_RX: u8 = 0x02;
const FCR_RESET_TX: u8 = 0x04;
const LCR_DLAB: u8 = 0x80;
const MCR_LOOP: u8 = 0x10;
const LSR_DATA: u8 = 0x01;
const LSR_OVERRUN: u8 = 0x02;
const LSR_THRE: u8 = 0x20;
const LSR_TEMT: u8 = 0x40;

/// Observe serial registers and queues without acknowledging an interrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UartDiagnostics {
    pub ier: u8,
    pub iir: u8,
    pub lcr: u8,
    pub lsr: u8,
    pub mcr: u8,
    pub divisor: u16,
    pub input_queued: u16,
    pub output_queued: u16,
}

#[derive(Debug)]
pub(crate) struct Uart {
    divisor: u16,
    ier: u8,
    fcr: u8,
    lcr: u8,
    mcr: u8,
    scratch: u8,
    modem_delta: u8,
    overrun: bool,
    thre_pending: bool,
    frame_ticks: u64,
    rx: ByteQueue<FIFO_DEPTH>,
    tx: ByteQueue<FIFO_DEPTH>,
    input: ByteQueue<HOST_QUEUE_DEPTH>,
    output: ByteQueue<HOST_QUEUE_DEPTH>,
    rx_at: Option<u64>,
    timeout_at: Option<u64>,
    timed_out: bool,
    sending: Option<Frame>,
}

#[derive(Debug)]
struct Frame {
    at: u64,
    byte: u8,
    loopback: bool,
}

impl Uart {
    pub(crate) fn new() -> Self {
        let mut uart = Self {
            divisor: 24, // 9600 baud with the board clock.
            ier: 0,
            fcr: 0,
            lcr: 0,
            mcr: 0x08,
            scratch: 0,
            modem_delta: 0,
            overrun: false,
            thre_pending: false,
            frame_ticks: 0,
            rx: ByteQueue::new(),
            tx: ByteQueue::new(),
            input: ByteQueue::new(),
            output: ByteQueue::new(),
            rx_at: None,
            timeout_at: None,
            timed_out: false,
            sending: None,
        };
        uart.update_frame_ticks();
        uart
    }

    pub(crate) fn read(&mut self, offset: u64, now: u64) -> u8 {
        self.advance(now);
        match offset {
            0 if self.lcr & LCR_DLAB != 0 => self.divisor.to_le_bytes()[0],
            0 => {
                let byte = self.rx.pop().unwrap_or(0);
                self.restart_timeout(now);
                self.arm_input(now);
                byte
            }
            1 if self.lcr & LCR_DLAB != 0 => self.divisor.to_le_bytes()[1],
            1 => self.ier,
            2 => {
                let id = self.interrupt_id();
                if id == IIR_THRE {
                    self.thre_pending = false;
                }
                self.interrupt_status(id)
            }
            3 => self.lcr,
            4 => self.mcr,
            5 => {
                let status = self.line_status();
                self.overrun = false;
                status
            }
            6 => self.modem_status() | core::mem::take(&mut self.modem_delta),
            7 => self.scratch,
            _ => 0,
        }
    }

    pub(crate) fn diagnostics(&self) -> UartDiagnostics {
        UartDiagnostics {
            ier: self.ier,
            iir: self.interrupt_status(self.interrupt_id()),
            lcr: self.lcr,
            lsr: self.line_status(),
            mcr: self.mcr,
            divisor: self.divisor,
            input_queued: (self.input.len + self.rx.len) as u16,
            output_queued: (self.output.len + self.tx.len + usize::from(self.sending.is_some()))
                as u16,
        }
    }

    fn line_status(&self) -> u8 {
        let mut status = 0;
        if !self.rx.is_empty() {
            status |= LSR_DATA;
        }
        if self.overrun {
            status |= LSR_OVERRUN;
        }
        if self.tx.is_empty() {
            status |= LSR_THRE;
            if self.sending.is_none() {
                status |= LSR_TEMT;
            }
        }
        status
    }

    fn interrupt_status(&self, id: u8) -> u8 {
        id | if self.fifo_enabled() { IIR_FIFO } else { 0 }
    }

    pub(crate) fn write(&mut self, offset: u64, value: u8, now: u64) {
        self.advance(now);
        match offset {
            0 if self.lcr & LCR_DLAB != 0 => {
                self.divisor = (self.divisor & 0xff00) | u16::from(value);
                self.update_frame_ticks();
            }
            0 => {
                self.thre_pending = false;
                // Writes into a full hardware FIFO are lost. Drivers use THRE to
                // submit at most one FIFO's worth; host backpressure is separate.
                if self.tx.len < self.fifo_depth() {
                    self.tx.push(value);
                }
                self.start_tx(now);
            }
            1 if self.lcr & LCR_DLAB != 0 => {
                self.divisor = (self.divisor & 0x00ff) | (u16::from(value) << 8);
                self.update_frame_ticks();
            }
            1 => {
                if (self.ier ^ value) & IER_THRE != 0 {
                    self.thre_pending = value & IER_THRE != 0 && self.tx.is_empty();
                }
                self.ier = value & 0x0f;
            }
            2 => self.write_fcr(value, now),
            3 => {
                self.lcr = value;
                self.update_frame_ticks();
            }
            4 => self.write_mcr(value, now),
            7 => self.scratch = value,
            _ => {}
        }
    }

    pub(crate) fn advance(&mut self, now: u64) {
        while let Some(at) = self.event_matching(|at| at <= now) {
            if self.rx_at == Some(at) {
                self.rx_at = None;
                let byte = self.input.pop().expect("scheduled UART input");
                self.receive(byte, at);
                self.arm_input(at);
            }
            if self.tx_at() == Some(at) {
                let frame = self.sending.take().expect("scheduled UART transmission");
                if frame.loopback {
                    self.receive(frame.byte, at);
                } else {
                    self.output.push(frame.byte);
                }
                self.start_tx(at);
            }
            if self.timeout_at == Some(at) {
                self.timeout_at = None;
                self.timed_out = true;
            }
        }
    }

    pub(crate) fn next_event(&self, now: u64) -> Option<u64> {
        self.event_matching(|at| at > now)
    }

    pub(crate) fn interrupt(&self) -> bool {
        self.interrupt_id() != IIR_IDLE
    }

    /// Accept a prefix; the host retains the suffix for a later call.
    pub(crate) fn queue_input(&mut self, bytes: &[u8], now: u64) -> usize {
        self.advance(now);
        let accepted = self.input.extend(bytes);
        self.arm_input(now);
        accepted
    }

    pub(crate) fn drain_output(&mut self, bytes: &mut [u8]) -> usize {
        self.output.drain(bytes)
    }

    /// Stop the board until the host drains output. Even if advance runs with
    /// this queue full, the completed frame stays in the shifter until space opens.
    pub(crate) fn output_full(&self) -> bool {
        self.output.len == HOST_QUEUE_DEPTH
    }

    fn event_matching(&self, predicate: impl Fn(u64) -> bool) -> Option<u64> {
        [self.rx_at, self.tx_at(), self.timeout_at]
            .into_iter()
            .flatten()
            .filter(|&at| predicate(at))
            .min()
    }

    fn tx_at(&self) -> Option<u64> {
        self.sending
            .as_ref()
            .filter(|frame| frame.loopback || !self.output_full())
            .map(|frame| frame.at)
    }

    fn interrupt_id(&self) -> u8 {
        let trigger = if self.fifo_enabled() {
            [1, 4, 8, 14][usize::from(self.fcr >> 6)]
        } else {
            1
        };
        if self.ier & IER_LINE != 0 && self.overrun {
            IIR_LINE
        } else if self.ier & IER_RX != 0 && self.timed_out {
            IIR_TIMEOUT
        } else if self.ier & IER_RX != 0 && self.rx.len >= trigger {
            IIR_RX
        } else if self.ier & IER_THRE != 0 && self.thre_pending {
            IIR_THRE
        } else if self.ier & IER_MODEM != 0 && self.modem_delta != 0 {
            IIR_MODEM
        } else {
            IIR_IDLE
        }
    }

    fn fifo_enabled(&self) -> bool {
        self.fcr & FCR_ENABLE != 0
    }

    fn fifo_depth(&self) -> usize {
        if self.fifo_enabled() { FIFO_DEPTH } else { 1 }
    }

    fn write_fcr(&mut self, value: u8, now: u64) {
        let toggled = (self.fcr ^ value) & FCR_ENABLE != 0;
        self.fcr = value & 0xc9;
        if toggled || value & FCR_RESET_RX != 0 {
            self.rx.clear();
            self.restart_timeout(now);
            self.arm_input(now);
        }
        if toggled || value & FCR_RESET_TX != 0 {
            // The reset discards the FIFO, not the frame already shifting out.
            self.tx.clear();
            self.thre_pending = true;
        }
    }

    fn write_mcr(&mut self, value: u8, now: u64) {
        let old = self.modem_status();
        self.mcr = value & 0x1f;
        let new = self.modem_status();
        // CTS, DSR and DCD latch either transition; RI only its trailing edge.
        self.modem_delta |= ((old ^ new) >> 4) & 0x0b;
        self.modem_delta |= ((old & !new) >> 4) & 0x04;
        if self.mcr & MCR_LOOP != 0 {
            self.rx_at = None;
        } else {
            self.arm_input(now);
        }
    }

    fn modem_status(&self) -> u8 {
        if self.mcr & MCR_LOOP != 0 {
            ((self.mcr & 0x02) << 3) | ((self.mcr & 0x01) << 5) | ((self.mcr & 0x0c) << 4)
        } else {
            0xb0 // CTS, DSR and DCD asserted on the board's virtual cable.
        }
    }

    fn update_frame_ticks(&mut self) {
        let data = 5 + u64::from(self.lcr & 0x03);
        let parity = u64::from(self.lcr & 0x08 != 0);
        // Stop length is 1, 1.5 or 2 bits; count half-bits to avoid fractions.
        let stop = if self.lcr & 0x04 == 0 {
            2
        } else if data == 5 {
            3
        } else {
            4
        };
        let half_bits = 2 * (1 + data + parity) + stop;
        let divisor = u64::from(self.divisor.max(1));
        self.frame_ticks = (half_bits * 8 * divisor * TICKS_PER_SECOND).div_ceil(UART_CLOCK);
    }

    fn arm_input(&mut self, now: u64) {
        // Pause the virtual sender when the FIFO fills instead of dropping a
        // queued host command. An RBR read or a FIFO reset resumes delivery.
        if self.rx_at.is_none()
            && self.mcr & MCR_LOOP == 0
            && self.rx.len < self.fifo_depth()
            && !self.input.is_empty()
        {
            self.rx_at = Some(now.saturating_add(self.frame_ticks));
        }
    }

    fn receive(&mut self, byte: u8, at: u64) {
        if self.rx.len == self.fifo_depth() {
            self.overrun = true;
            if !self.fifo_enabled() {
                self.rx.clear();
                self.rx.push(byte);
            }
        } else {
            self.rx.push(byte);
        }
        self.restart_timeout(at);
    }

    fn restart_timeout(&mut self, now: u64) {
        self.timed_out = false;
        self.timeout_at = if self.fifo_enabled() && !self.rx.is_empty() {
            Some(now.saturating_add(4 * self.frame_ticks))
        } else {
            None
        };
    }

    fn start_tx(&mut self, now: u64) {
        if self.sending.is_none()
            && let Some(byte) = self.tx.pop()
        {
            self.sending = Some(Frame {
                at: now.saturating_add(self.frame_ticks),
                byte,
                loopback: self.mcr & MCR_LOOP != 0,
            });
            if self.tx.is_empty() {
                self.thre_pending = true;
            }
        }
    }
}

#[derive(Debug)]
struct ByteQueue<const N: usize> {
    bytes: [u8; N],
    head: usize,
    len: usize,
}

impl<const N: usize> ByteQueue<N> {
    fn new() -> Self {
        Self {
            bytes: [0; N],
            head: 0,
            len: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn push(&mut self, byte: u8) {
        debug_assert!(self.len < N);
        self.bytes[(self.head + self.len) % N] = byte;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<u8> {
        if self.is_empty() {
            return None;
        }
        let byte = self.bytes[self.head];
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(byte)
    }

    fn extend(&mut self, bytes: &[u8]) -> usize {
        let count = bytes.len().min(N - self.len);
        let tail = (self.head + self.len) % N;
        let first = count.min(N - tail);
        self.bytes[tail..tail + first].copy_from_slice(&bytes[..first]);
        self.bytes[..count - first].copy_from_slice(&bytes[first..count]);
        self.len += count;
        count
    }

    fn drain(&mut self, bytes: &mut [u8]) -> usize {
        let count = bytes.len().min(self.len);
        let first = count.min(N - self.head);
        bytes[..first].copy_from_slice(&self.bytes[self.head..self.head + first]);
        bytes[first..count].copy_from_slice(&self.bytes[..count - first]);
        self.head = (self.head + count) % N;
        self.len -= count;
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_reset_keeps_the_frame_already_shifting() {
        let mut uart = Uart::new();
        uart.write(2, FCR_ENABLE, 0);
        uart.write(1, IER_THRE, 0);
        for byte in b"abc" {
            uart.write(0, *byte, 0);
        }
        assert_eq!(uart.read(5, 0), 0);
        assert!(!uart.interrupt());

        uart.write(2, FCR_ENABLE | FCR_RESET_TX, 0);
        assert_eq!(uart.read(5, 0), LSR_THRE);
        assert_eq!(uart.read(2, 0), IIR_FIFO | IIR_THRE);
        assert!(!uart.interrupt());

        let done = uart.next_event(0).unwrap();
        uart.advance(done);
        assert_eq!(uart.read(5, done), LSR_THRE | LSR_TEMT);
        let mut output = [0; 3];
        assert_eq!(uart.drain_output(&mut output), 1);
        assert_eq!(output[0], b'a');
    }

    #[test]
    fn loopback_maps_modem_lines_and_delivers_to_rx() {
        let mut uart = Uart::new();
        uart.write(2, FCR_ENABLE, 0);
        uart.write(1, IER_MODEM, 0);
        uart.write(4, MCR_LOOP | 0x0a, 0);
        assert_eq!(uart.read(2, 0), IIR_FIFO | IIR_MODEM);
        assert_eq!(uart.read(6, 0), 0x92);
        assert!(!uart.interrupt());

        uart.write(1, IER_RX, 0);
        uart.write(0, b'A', 0);
        let done = uart.next_event(0).unwrap();
        assert_eq!(uart.read(2, done), IIR_FIFO | IIR_RX);
        assert_eq!(uart.read(0, done), b'A');
        assert_eq!(uart.drain_output(&mut [0]), 0);
    }

    #[test]
    fn host_input_survives_full_fifo_and_queue_wrap() {
        let mut uart = Uart::new();
        uart.write(2, FCR_ENABLE, 0);
        let input: [u8; HOST_QUEUE_DEPTH + 1] = core::array::from_fn(|index| index as u8);
        assert_eq!(uart.queue_input(&input, 0), HOST_QUEUE_DEPTH);

        let mut now = uart.frame_ticks * 32;
        uart.advance(now);
        for &byte in &input[..FIFO_DEPTH] {
            assert_ne!(uart.read(5, now) & LSR_DATA, 0);
            assert_eq!(uart.read(0, now), byte);
        }
        assert_eq!(uart.read(5, now) & (LSR_OVERRUN | LSR_DATA), 0);
        assert_eq!(uart.queue_input(&input[HOST_QUEUE_DEPTH..], now), 1);
        for &byte in &input[FIFO_DEPTH..] {
            now = uart.next_event(now).unwrap();
            assert_ne!(uart.read(5, now) & LSR_DATA, 0);
            assert_eq!(uart.read(0, now), byte);
        }
        assert_eq!(uart.next_event(now), None);
    }

    #[test]
    fn host_output_backpressure_keeps_the_blocked_frame() {
        let mut uart = Uart::new();
        let mut now = 0;
        for _ in 0..HOST_QUEUE_DEPTH - 1 {
            uart.write(0, b'a', now);
            now = uart.next_event(now).unwrap();
            uart.advance(now);
        }
        uart.write(0, b'b', now);
        uart.write(0, b'c', now);
        now = uart.next_event(now).unwrap();
        uart.advance(now);
        assert!(uart.output_full());

        now += uart.frame_ticks;
        uart.advance(now);
        let mut output = [0; HOST_QUEUE_DEPTH];
        assert_eq!(uart.drain_output(&mut output[..1]), 1);
        assert_eq!(output[0], b'a');
        uart.advance(now);
        assert!(uart.output_full());
        assert_eq!(uart.drain_output(&mut output), HOST_QUEUE_DEPTH);
        assert_eq!(&output[HOST_QUEUE_DEPTH - 2..], b"bc");
    }
}
