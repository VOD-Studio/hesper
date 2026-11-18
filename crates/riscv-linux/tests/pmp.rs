use hesper_riscv_linux::{DRAM_BASE, Machine, PrivilegeMode, Trap};

const PMPADDR0: u32 = 0x3b0;
const PMPADDR1: u32 = 0x3b1;
const PMPCFG0: u32 = 0x3a0;
const MSTATUS: u32 = 0x300;
const MEPC: u32 = 0x341;
const MRET: u32 = 0x3020_0073;
const DATA: u64 = DRAM_BASE + 0x300;

const fn addi(rd: u32, rs1: u32, imm: i32) -> u32 {
    (imm as u32 & 0xfff) << 20 | rs1 << 15 | rd << 7 | 0x13
}

const fn csrw(csr: u32, source: u32) -> u32 {
    csr << 20 | source << 15 | 0x1073
}

const fn csrr(rd: u32, csr: u32) -> u32 {
    csr << 20 | rd << 7 | 0x2073
}

fn program(words: &[u32]) -> Machine {
    let mut m = Machine::new();
    for (index, word) in words.iter().enumerate() {
        m.load(DRAM_BASE + index as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    m.load(DATA, &[0x84, 0x33, 0x22, 0x11, 0x55, 0x66, 0x77, 0x88])
        .unwrap();
    m
}

fn retire(m: &mut Machine, count: u32) {
    let progress = m.run(count);
    assert_eq!((progress.retired, progress.spent), (count, count));
}

fn trap(m: &mut Machine, cause: u64, value: u64) {
    let before = m.debug_state();
    assert_eq!(m.run(1).retired, 0);
    let after = m.debug_state();
    assert_eq!(after.x, before.x);
    assert_eq!(
        after.last_trap,
        Some(Trap {
            epc: before.pc,
            cause,
            value
        })
    );
}

#[test]
fn mprv_obeys_priority_and_whole_span_without_restricting_fetch() {
    // NA4 entry 0 denies access. NAPOT entry 1 allows data but not S-mode fetch.
    for (word, cause) in [(0x0000_b283, 5), (0x0050_b023, 7)] {
        let mut m = program(&[
            0x0000_0097,
            addi(1, 1, 0x300),
            0x0020_d113, // srli x2, x1, 2
            csrw(PMPADDR0, 2),
            addi(2, 0, -1),
            csrw(PMPADDR1, 2),
            0x0000_21b7,       // lui x3, 2
            addi(3, 3, -1264), // x3 = 0x1b10
            csrw(PMPCFG0, 3),
            0x0000_a283,       // lw x5, (x1) succeeds in M with unlocked slot 0
            0x0002_11b7,       // lui x3, 0x21
            addi(3, 3, -2048), // MPRV | MPP=S
            csrw(MSTATUS, 3),
            0x0040_a303, // lw x6, 4(x1): outside slot 0, allowed by slot 1
            word,
        ]);
        retire(&mut m, 14);
        assert_eq!(
            m.debug_state().x[5..7],
            [0x1122_3384, 0xffff_ffff_8877_6655]
        );
        let before: [u8; 8] = m.inspect(DATA, 8).unwrap().try_into().unwrap();
        trap(&mut m, cause, DATA);
        assert_eq!(m.inspect(DATA, 8), Some(before.as_slice()));
    }
}

#[test]
fn locked_tor_freezes_both_boundaries_and_applies_in_m_mode() {
    let mut m = program(&[
        0x0000_0097,
        addi(1, 1, 0x300),
        0x0020_d113, // x2 = data address >> 2
        csrw(PMPADDR0, 2),
        addi(3, 2, 2),
        csrw(PMPADDR1, 3),
        0x0000_9237,       // lui x4, 9
        addi(4, 4, -2048), // slot 1 TOR locked, no RWX. slot 0 OFF.
        csrw(PMPCFG0, 4),
        csrw(PMPADDR0, 0),
        csrw(PMPADDR1, 0),
        csrw(PMPCFG0, 0),
        csrr(5, PMPADDR0),
        csrr(6, PMPADDR1),
        csrr(7, PMPCFG0),
        0x0000_b303, // ld x6, (x1)
    ]);
    retire(&mut m, 15);
    assert_eq!(
        m.debug_state().x[5..8],
        [DATA >> 2, (DATA >> 2) + 2, 0x8800]
    );
    trap(&mut m, 5, DATA);
}

#[test]
fn lower_privilege_cannot_execute_without_a_matching_region() {
    for mpp in [0, 1] {
        let mut m = program(&[
            0x0000_0097,
            addi(1, 1, 0x100),
            csrw(MEPC, 1),
            if mpp == 0 { 0x0000_0137 } else { 0x0000_1137 },
            addi(2, 2, -2048 * mpp),
            csrw(MSTATUS, 2),
            MRET,
        ]);
        m.load(DRAM_BASE + 0x100, &addi(5, 0, 7).to_le_bytes())
            .unwrap();
        retire(&mut m, 7);
        assert_ne!(m.debug_state().mode, PrivilegeMode::Machine);
        trap(&mut m, 1, DRAM_BASE + 0x100);
        assert_eq!(m.debug_state().mode, PrivilegeMode::Machine);
    }
}
