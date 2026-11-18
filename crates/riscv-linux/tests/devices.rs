use hesper_riscv_linux::{DRAM_BASE, Machine};

const WFI: u32 = 0x1050_0073;
const MIE: u32 = 0x304;
const MIP: u32 = 0x344;

const fn lui(rd: u32, imm: u32) -> u32 {
    imm << 12 | rd << 7 | 0x37
}

const fn op_imm(op: u32, funct: u32, rd: u32, rs: u32, imm: u32) -> u32 {
    (imm & 0xfff) << 20 | rs << 15 | funct << 12 | rd << 7 | op
}

const fn addi(rd: u32, rs: u32, imm: u32) -> u32 {
    op_imm(0x13, 0, rd, rs, imm)
}

const fn load(width: u32, rd: u32, address: u32, offset: u32) -> u32 {
    op_imm(0x03, width, rd, address, offset)
}

const fn store(width: u32, value: u32, address: u32, offset: u32) -> u32 {
    (offset & 0xfe0) << 20 | value << 20 | address << 15 | width << 12 | (offset & 31) << 7 | 0x23
}

const fn csrw(csr: u32, rs: u32) -> u32 {
    op_imm(0x73, 1, 0, rs, csr)
}

const fn csrr(rd: u32, csr: u32) -> u32 {
    op_imm(0x73, 2, rd, 0, csr)
}

fn machine<const N: usize>(words: [u32; N]) -> Machine {
    let mut m = Machine::new();
    m.load(DRAM_BASE, words.map(u32::to_le_bytes).as_flattened())
        .unwrap();
    m
}

#[test]
fn wfi_jumps_to_timer_and_reprogramming_clears_the_interrupt_level() {
    let image = [
        lui(1, 0x2004), // mtimecmp
        lui(2, 0x200c), // mtime + 8
        addi(3, 0, 128),
        csrw(MIE, 3),
        addi(3, 0, 1000),
        store(3, 3, 1, 0),
        csrr(4, MIP),
        WFI,
        csrr(5, 0xc01), // time equals compare on the first resumed instruction
        load(3, 6, 2, 0xff8),
        csrr(7, MIP),
        addi(3, 3, 1000),
        store(3, 3, 1, 0),
        csrr(8, MIP),
    ];
    let mut a = machine(image);
    let mut b = machine(image);
    assert_eq!(a.run(15).retired, 14); // one fuel to advance the waiting event
    for _ in 0..15 {
        assert_eq!(b.run(1).spent, 1);
    }
    let state = a.debug_state();
    assert_eq!(state, b.debug_state());
    assert_eq!(state.x[4..9], [0, 1000, 1001, 128, 0]);
    assert_eq!(state.time, 1006);
    assert_eq!(state.last_trap, None);
}

#[test]
fn uart_timeout_notifies_only_s_context_and_claim_requires_completion() {
    let mut m = machine([
        lui(1, 0x10000), // UART0
        lui(2, 0x0c002), // PLIC enables
        lui(3, 0x0c201), // PLIC S context
        lui(4, 0x0c000), // PLIC priorities
        addi(5, 0, 0x80),
        store(0, 5, 1, 3), // DLAB
        addi(5, 0, 1),
        store(0, 5, 1, 0), // DLL (not THR)
        addi(6, 0, 3),
        store(0, 6, 1, 3), // normal 8N1
        addi(6, 0, 0xc1),
        store(0, 6, 1, 2),    // RX FIFO, threshold 14
        store(0, 5, 1, 1),    // IER RX
        store(2, 5, 4, 0x28), // priority 1
        addi(6, 0, 1024),
        store(2, 6, 2, 0x80), // enable source 10 in S context only
        addi(6, 0, 512),
        csrw(MIE, 6),
        WFI,
        store(2, 5, 3, 0), // threshold 1 masks priority 1
        csrr(18, MIP),
        store(2, 0, 3, 0),
        csrr(19, MIP),
        load(4, 20, 1, 2),  // IIR: FIFO enabled and character timeout
        load(2, 21, 3, 4),  // claim UART
        load(2, 22, 3, 4),  // second claim is empty
        store(2, 21, 3, 4), // completion with RX line still asserted
        load(2, 23, 3, 4),  // claim UART again
        load(4, 24, 1, 0),  // consume the byte and clear RX line
        load(4, 25, 1, 5),  // DR bit now clear
        store(2, 23, 3, 4),
        load(2, 26, 3, 4),  // no re-pending
        store(0, 24, 1, 0), // emit the received byte through THR
        WFI,
    ]);
    assert_eq!(m.run(100).retired, 19);
    let mut out = [0; 2];
    assert_eq!(m.serial_output(&mut out), 0);
    assert_eq!(m.serial_input(b"K"), 1);
    assert_eq!(m.run(100).retired, 15);
    let state = m.debug_state();
    assert_eq!(state.x[18..25], [0, 512, 0xcc, 10, 0, 10, u64::from(b'K')]);
    assert_eq!(state.x[25] & 1, 0);
    assert_eq!(state.x[26], 0);
    assert_eq!(state.last_trap, None);
    assert_eq!(m.serial_output(&mut out), 1);
    assert_eq!(out[0], b'K');
    assert_eq!(m.run(1).spent, 0);
}
