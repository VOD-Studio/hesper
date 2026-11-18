use hesper_riscv_linux::{DRAM_BASE, Machine, PrivilegeMode, Trap};

const SATP: u32 = 0x180;
const MSTATUS: u32 = 0x300;
const MTVEC: u32 = 0x305;
const MEPC: u32 = 0x341;
const MCAUSE: u32 = 0x342;
const MTVAL: u32 = 0x343;
const PMPCFG0: u32 = 0x3a0;
const PMPADDR0: u32 = 0x3b0;
const PMPADDR1: u32 = 0x3b1;
const MRET: u32 = 0x3020_0073;
const SFENCE_VMA: u32 = 0x1200_0073;

const MPP_S: u64 = 1 << 11;
const MPRV: u64 = 1 << 17;
const SUM: u64 = 1 << 18;
const MXR: u64 = 1 << 19;
const S_DATA: u64 = MPRV | MPP_S;

const INSN_ACCESS: u64 = 1;
const LOAD_ACCESS: u64 = 5;
const STORE_ACCESS: u64 = 7;
const INSN_PAGE: u64 = 12;
const LOAD_PAGE: u64 = 13;
const STORE_PAGE: u64 = 15;

const V: u64 = 1 << 0;
const R: u64 = 1 << 1;
const W: u64 = 1 << 2;
const X: u64 = 1 << 3;
const U: u64 = 1 << 4;
const A: u64 = 1 << 6;
const D: u64 = 1 << 7;

const NAPOT: u64 = 0x18;
const PMP_R: u64 = 1;
const PMP_RWX: u64 = 7;
const VECTOR: u64 = DRAM_BASE + 0x100;
const LITERALS: u32 = 0x400;
const ROOT: u64 = DRAM_BASE + 0x1000;
const L1: u64 = DRAM_BASE + 0x2000;
const L0: u64 = DRAM_BASE + 0x3000;
const DATA: u64 = DRAM_BASE + 0x7000;
const VA: u64 = 0x0040_0000;
const SIGNATURE: u32 = 0x1122_3344;
const STORE_VALUE: i32 = 0x345;
const SV39: u64 = (8 << 60) | (0x53a7 << 44) | (ROOT >> 12);

const fn auipc(rd: u32) -> u32 {
    rd << 7 | 0x17
}

const fn addi(rd: u32, rs1: u32, imm: i32) -> u32 {
    (imm as u32 & 0xfff) << 20 | rs1 << 15 | rd << 7 | 0x13
}

const fn literal(rd: u32, index: u32) -> u32 {
    (LITERALS + index * 8) << 20 | 31 << 15 | 3 << 12 | rd << 7 | 0x03 // ld
}

const fn csrw(csr: u32, rs: u32) -> u32 {
    csr << 20 | rs << 15 | 0x1073
}

const fn csrr(rd: u32, csr: u32) -> u32 {
    csr << 20 | rd << 7 | 0x2073
}

const LW: u32 = 0x0000_a283; // lw x5, (x1)
const SW: u32 = 0x0020_a023; // sw x2, (x1)

fn write_words(m: &mut Machine, mut address: u64, words: &[u32]) {
    for word in words {
        m.load(address, &word.to_le_bytes()).unwrap();
        address += 4;
    }
}

fn program(words: &[u32], literals: &[u64]) -> Machine {
    let mut m = Machine::new();
    write_words(&mut m, DRAM_BASE, words);
    for (index, value) in literals.iter().enumerate() {
        m.load(
            DRAM_BASE + u64::from(LITERALS) + index as u64 * 8,
            &value.to_le_bytes(),
        )
        .unwrap();
    }
    m
}

fn paged(address: u64, status: u64, pmp_l0: u64, tail: &[u32]) -> Machine {
    const BOOT: &[u32] = &[
        auipc(31),
        literal(1, 0),
        addi(2, 0, STORE_VALUE),
        literal(3, 1),
        csrw(MTVEC, 3),
        literal(3, 2),
        csrw(PMPADDR0, 3),
        addi(3, 0, -1),
        csrw(PMPADDR1, 3),
        literal(3, 3),
        csrw(PMPCFG0, 3),
        literal(3, 4),
        csrw(SATP, 3),
        literal(3, 5),
        csrw(MSTATUS, 3),
    ];
    // Slot 0 optionally restricts the L0 table; slot 1 otherwise permits everything.
    let l0_napot = (L0 >> 2) | ((4096 >> 3) - 1);
    let cfg = (NAPOT | PMP_RWX) << 8 | pmp_l0;
    let mut m = program(BOOT, &[address, VECTOR, l0_napot, cfg, SV39, status]);
    write_words(&mut m, DRAM_BASE + BOOT.len() as u64 * 4, tail);
    retire(&mut m, BOOT.len() as u32);
    m
}

const fn pte(address: u64, flags: u64) -> u64 {
    (address >> 12) << 10 | flags
}

fn set_pte(m: &mut Machine, table: u64, level: u32, va: u64, value: u64) -> u64 {
    let address = table + ((va >> (12 + 9 * level)) & 0x1ff) * 8;
    m.load(address, &value.to_le_bytes()).unwrap();
    address
}

fn link_l0(m: &mut Machine, va: u64) {
    set_pte(m, ROOT, 2, va, pte(L1, V));
    set_pte(m, L1, 1, va, pte(L0, V));
}

fn map_4k(m: &mut Machine, va: u64, pa: u64, flags: u64) -> u64 {
    link_l0(m, va);
    set_pte(m, L0, 0, va, pte(pa, flags))
}

#[track_caller]
fn retire(m: &mut Machine, count: u32) {
    let progress = m.run(count);
    assert_eq!(
        (progress.spent, progress.retired),
        (count, count),
        "{:?}",
        m.debug_state().last_trap
    );
}

#[track_caller]
fn fault(m: &mut Machine, cause: u64, value: u64) {
    let before = m.debug_state();
    let progress = m.run(1);
    assert_eq!((progress.spent, progress.retired), (1, 0));
    let after = m.debug_state();
    assert_eq!(
        after.last_trap,
        Some(Trap {
            epc: before.pc,
            cause,
            value
        })
    );
    assert_eq!((after.mode, after.pc), (PrivilegeMode::Machine, VECTOR));
    assert_eq!(after.x, before.x);
}

#[test]
fn loads_from_4k_2m_and_1g_leaves() {
    const VA_2M: u64 = 0x0060_0000;
    const VA_1G: u64 = 0xffff_ffc0_4000_0000;
    for (table, level, va, pa, offset) in [
        (L0, 0, VA, DATA, 0x298),
        (L1, 1, VA_2M, DRAM_BASE + 0x20_0000, 0x1f_3a28),
        (ROOT, 2, VA_1G, DRAM_BASE, 0x0543_2a60),
    ] {
        let mut m = paged(va + offset, S_DATA, 0, &[LW]);
        match level {
            0 => link_l0(&mut m, va),
            1 => {
                set_pte(&mut m, ROOT, 2, va, pte(L1, V));
            }
            _ => (),
        }
        set_pte(&mut m, table, level, va, pte(pa, V | R | A));
        m.load(pa + offset, &SIGNATURE.to_le_bytes()).unwrap();
        retire(&mut m, 1);
        assert_eq!(m.debug_state().x[5], u64::from(SIGNATURE), "level {level}");
    }
}

#[test]
fn bad_pte_encodings_and_noncanonical_addresses_page_fault() {
    for (access_va, flags, word, cause) in [
        (VA, R | A, LW, LOAD_PAGE), // V=0 despite R=1.
        (VA, V | W | A | D, LW, LOAD_PAGE),
        (VA, V | W | A | D, SW, STORE_PAGE),
        (VA, V | R | A | (1 << 63), LW, LOAD_PAGE), // No Svnapot.
        (VA, V, LW, LOAD_PAGE),                     // Nonleaf at the last level.
        (VA | (1 << 39), V | R | A, LW, LOAD_PAGE), // VPN matches, but not canonical.
    ] {
        let mut m = paged(access_va, S_DATA, 0, &[word]);
        map_4k(&mut m, VA, DATA, flags);
        fault(&mut m, cause, access_va);
    }
}

#[test]
fn misaligned_superpages_page_fault_at_both_levels() {
    for (table, level, pa) in [
        (L1, 1, DRAM_BASE + 0x1000),      // PPN[0] is nonzero.
        (ROOT, 2, DRAM_BASE + 0x20_0000), // PPN[1] is nonzero.
    ] {
        let mut m = paged(VA, S_DATA, 0, &[LW]);
        if level == 1 {
            set_pte(&mut m, ROOT, 2, VA, pte(L1, V));
        }
        set_pte(&mut m, table, level, VA, pte(pa, V | R | A));
        fault(&mut m, LOAD_PAGE, VA);
    }
}

#[test]
fn sum_and_mxr_only_relax_the_intended_data_permissions() {
    for (status, flags, allowed) in [
        (MPRV, R | U, true),
        (MPRV | SUM | MXR, R, false), // U mode cannot access a supervisor page.
        (S_DATA, R, true),
        (S_DATA, R | U, false),
        (S_DATA | SUM, R | U, true),
        (S_DATA, X, false),
        (S_DATA | MXR, X, true),
        (S_DATA | MXR, X | U, false),
        (S_DATA | SUM, X | U, false),
        (S_DATA | SUM | MXR, X | U, true),
    ] {
        let mut m = paged(VA, status, 0, &[LW]);
        map_4k(&mut m, VA, DATA, V | A | flags);
        m.load(DATA, &SIGNATURE.to_le_bytes()).unwrap();
        if allowed {
            retire(&mut m, 1);
            assert_eq!(m.debug_state().x[5], u64::from(SIGNATURE));
        } else {
            fault(&mut m, LOAD_PAGE, VA);
        }
    }

    let mut m = paged(VA, S_DATA | SUM | MXR, 0, &[LW, SW]);
    map_4k(&mut m, VA, DATA, V | R | W | U | A | D);
    retire(&mut m, 2);
    assert_eq!(m.inspect(DATA, 4).unwrap(), STORE_VALUE.to_le_bytes());

    let mut m = paged(VA, S_DATA | SUM | MXR, 0, &[SW]);
    let flags = V | X | U;
    let entry = map_4k(&mut m, VA, DATA, flags);
    fault(&mut m, STORE_PAGE, VA);
    assert_eq!(m.inspect(entry, 8).unwrap(), pte(DATA, flags).to_le_bytes());
}

#[test]
fn u_and_s_instruction_fetch_check_u_even_with_sum() {
    for (status, mode, flags, allowed) in [
        (MPP_S, PrivilegeMode::Supervisor, X, true),
        (MPP_S | SUM, PrivilegeMode::Supervisor, X | U, false),
        (0, PrivilegeMode::User, X, false),
        (0, PrivilegeMode::User, X | U, true),
    ] {
        let mut m = paged(VA, status, 0, &[csrw(MEPC, 1), MRET]);
        map_4k(&mut m, VA, DATA, V | A | flags);
        write_words(&mut m, DATA, &[addi(5, 0, STORE_VALUE)]);
        retire(&mut m, 2);
        assert_eq!((m.debug_state().mode, m.debug_state().pc), (mode, VA));
        if allowed {
            retire(&mut m, 1);
            assert_eq!(m.debug_state().x[5], STORE_VALUE as u64);
        } else {
            fault(&mut m, INSN_PAGE, VA);
        }
    }
}

#[test]
fn page_walk_sets_accessed_on_read_and_dirty_on_write() {
    let mut m = paged(VA, S_DATA, 0, &[LW, SW]);
    let flags = V | R | W | (1 << 5) | (0b10 << 8); // Preserve G and RSW.
    let entry = map_4k(&mut m, VA, DATA, flags);
    m.load(DATA, &SIGNATURE.to_le_bytes()).unwrap();
    retire(&mut m, 1);
    assert_eq!(m.debug_state().x[5], u64::from(SIGNATURE));
    assert_eq!(
        m.inspect(entry, 8).unwrap(),
        pte(DATA, flags | A).to_le_bytes()
    );
    retire(&mut m, 1);
    assert_eq!(m.inspect(DATA, 4).unwrap(), STORE_VALUE.to_le_bytes());
    assert_eq!(
        m.inspect(entry, 8).unwrap(),
        pte(DATA, flags | A | D).to_le_bytes()
    );
}

#[test]
fn page_table_pmp_denials_report_the_original_access_type() {
    for (word, cause) in [(LW, LOAD_ACCESS), (SW, STORE_ACCESS)] {
        let mut m = paged(VA, S_DATA, NAPOT, &[word]);
        map_4k(&mut m, VA, DATA, V | R | W | A | D);
        fault(&mut m, cause, VA);
    }

    let mut m = paged(VA, MPP_S, NAPOT, &[csrw(MEPC, 1), MRET]);
    map_4k(&mut m, VA, DATA, V | X | A);
    write_words(&mut m, DATA, &[addi(5, 0, STORE_VALUE)]);
    retire(&mut m, 2);
    fault(&mut m, INSN_ACCESS, VA);

    // Reading the leaf is now allowed, but setting A or D would write it.
    for (word, cause, flags) in [(LW, LOAD_ACCESS, V | R), (SW, STORE_ACCESS, V | R | W | A)] {
        let mut m = paged(VA, S_DATA, NAPOT | PMP_R, &[word]);
        let entry = map_4k(&mut m, VA, DATA, flags);
        m.load(DATA, &SIGNATURE.to_le_bytes()).unwrap();
        fault(&mut m, cause, VA);
        assert_eq!(m.inspect(entry, 8).unwrap(), pte(DATA, flags).to_le_bytes());
        assert_eq!(m.inspect(DATA, 4).unwrap(), SIGNATURE.to_le_bytes());
    }
}

#[test]
fn unsupported_satp_mode_preserves_mode_asid_and_ppn() {
    const UNSUPPORTED: u64 = (9 << 60) | (0x3142 << 44) | (L1 >> 12);
    let mut m = program(
        &[
            auipc(31),
            literal(1, 0),
            csrw(SATP, 1),
            csrr(5, SATP),
            literal(1, 1),
            csrw(SATP, 1),
            csrr(6, SATP),
            csrw(SATP, 0),
            csrr(7, SATP),
        ],
        &[SV39, UNSUPPORTED],
    );
    retire(&mut m, 9);
    assert_eq!(m.debug_state().x[5..8], [SV39, SV39, 0]);
}

#[test]
fn second_half_fetch_fault_retries_the_whole_instruction_after_mret() {
    const PC: u64 = VA + 0xffe;
    const SECOND_VA: u64 = VA + 0x1000;
    const SECOND_PA: u64 = DATA + 0x4000;
    let mut m = paged(PC, MPP_S, 0, &[csrw(MEPC, 1), MRET]);
    map_4k(&mut m, VA, DATA, V | X | A);
    let word = addi(5, 5, STORE_VALUE).to_le_bytes();
    m.load(DATA + 0xffe, &word[..2]).unwrap();
    m.load(SECOND_PA, &word[2..]).unwrap();
    write_words(
        &mut m,
        VECTOR,
        &[
            csrr(6, MEPC),
            csrr(7, MCAUSE),
            csrr(8, MTVAL),
            SFENCE_VMA,
            MRET,
        ],
    );
    retire(&mut m, 2);
    assert_eq!(
        (m.debug_state().mode, m.debug_state().pc),
        (PrivilegeMode::Supervisor, PC)
    );
    fault(&mut m, INSN_PAGE, SECOND_VA);

    set_pte(&mut m, L0, 0, SECOND_VA, pte(SECOND_PA, V | X | A));
    retire(&mut m, 5);
    let before = m.debug_state();
    assert_eq!((before.mode, before.pc), (PrivilegeMode::Supervisor, PC));
    assert_eq!(before.x[5], 0);
    assert_eq!(before.x[6..9], [PC, INSN_PAGE, SECOND_VA]);

    retire(&mut m, 1);
    let after = m.debug_state();
    assert_eq!((after.mode, after.pc), (PrivilegeMode::Supervisor, PC + 4));
    assert_eq!(after.x[5], STORE_VALUE as u64);
    assert_eq!(after.retired, before.retired + 1);
}
