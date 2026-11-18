use hesper_riscv_linux::{DRAM_BASE, HartIsa, Machine, PrivilegeMode, Trap};

const FFLAGS: u32 = 0x001;
const FRM: u32 = 0x002;
const FCSR: u32 = 0x003;
const SSTATUS: u32 = 0x100;
const SATP: u32 = 0x180;
const MSTATUS: u32 = 0x300;
const MISA: u32 = 0x301;
const MTVEC: u32 = 0x305;
const MEPC: u32 = 0x341;
const PMPCFG0: u32 = 0x3a0;
const PMPADDR0: u32 = 0x3b0;
const MRET: u32 = 0x3020_0073;

const INITIAL: u64 = 1 << 13;
const CLEAN: u64 = 2 << 13;
const DIRTY: u64 = INITIAL | CLEAN;
const SD: u64 = 1 << 63;
const FS_SD: u64 = DIRTY | SD;
const MPRV_S: u64 = (1 << 17) | (1 << 11);
const MISA_FD: u64 = (1 << 5) | (1 << 3);

const NX: u64 = 1;
const UF: u64 = 2;
const OF: u64 = 4;
const DZ: u64 = 8;
const NV: u64 = 16;

const VECTOR: u64 = DRAM_BASE + 0x600;
const LOWER_CODE: u64 = DRAM_BASE + 0x800;
const LITERAL: u64 = DRAM_BASE + 0x1000;
const ROOT: u64 = DRAM_BASE + 0x2000;
const DATA: u64 = DRAM_BASE + 0x3000;
const PATTERN: u64 = 0x7ff0_0000_1234_5678;
const C_FLDSP_F0: u16 = 0x2002;
const C_FSDSP_F0_8: u16 = 0xa402;

const fn addi(rd: u32, rs: u32, imm: i32) -> u32 {
    (imm as u32 & 0xfff) << 20 | rs << 15 | rd << 7 | 0x13
}

const fn csr(kind: u32, rd: u32, number: u32, rs: u32) -> u32 {
    number << 20 | rs << 15 | kind << 12 | rd << 7 | 0x73
}

const fn fp(funct7: u32, rd: u32, rs1: u32, rs2: u32, mode: u32) -> u32 {
    funct7 << 25 | rs2 << 20 | rs1 << 15 | mode << 12 | rd << 7 | 0x53
}

const fn fl(width: u32, fd: u32, rs: u32, offset: u32) -> u32 {
    offset << 20 | rs << 15 | width << 12 | fd << 7 | 0x07
}

const fn fs(width: u32, source: u32, rs: u32, offset: u32) -> u32 {
    (offset >> 5) << 25 | source << 20 | rs << 15 | width << 12 | (offset & 31) << 7 | 0x27
}

const fn atomic(op: u32, rd: u32, rs2: u32) -> u32 {
    op << 27 | rs2 << 20 | 1 << 15 | 3 << 12 | rd << 7 | 0x2f
}

#[track_caller]
fn retire(m: &mut Machine, bytes: &[u8]) {
    let pc = m.debug_state().pc;
    m.load(pc, bytes).unwrap();
    let progress = m.run(1);
    assert_eq!(
        (progress.spent, progress.retired, m.debug_state().pc),
        (1, 1, pc + bytes.len() as u64),
        "{:?}",
        m.debug_state().last_trap
    );
}

#[track_caller]
fn run(m: &mut Machine, word: u32) {
    retire(m, &word.to_le_bytes());
}

fn board(isa: HartIsa) -> Machine {
    let mut m = Machine::try_with_isa(isa).unwrap();
    run(&mut m, 0x0000_1f97); // auipc x31, 0x1; literal slot for guest loads
    run(&mut m, 0x0000_0f17); // auipc x30, 0
    run(&mut m, addi(30, 30, 0x5fc));
    run(&mut m, csr(1, 0, MTVEC, 30));
    m
}

fn set_x(m: &mut Machine, rd: u32, value: u64) {
    m.load(LITERAL, &value.to_le_bytes()).unwrap();
    run(m, 31 << 15 | 3 << 12 | rd << 7 | 0x03); // ld rd, 0(x31)
}

fn put_csr(m: &mut Machine, csr_number: u32, value: u64) {
    set_x(m, 29, value);
    run(m, csr(1, 0, csr_number, 29));
}

fn get_csr(m: &mut Machine, csr_number: u32) -> u64 {
    run(m, csr(2, 28, csr_number, 0));
    m.debug_state().x[28]
}

fn put_f(m: &mut Machine, fd: u32, bits: u64) {
    set_x(m, 29, bits);
    run(m, fp(0x79, fd, 29, 0, 0)); // fmv.d.x
}

fn get_f(m: &mut Machine, fs: u32) -> u64 {
    run(m, fp(0x71, 28, fs, 0, 0)); // fmv.x.d observes the whole NaN box
    m.debug_state().x[28]
}

const fn boxed(bits: u32) -> u64 {
    0xffff_ffff_0000_0000 | bits as u64
}

#[track_caller]
fn fault(m: &mut Machine, bytes: &[u8], cause: u64, value: u64) {
    let before = m.debug_state();
    m.load(before.pc, bytes).unwrap();
    let progress = m.run(1);
    let after = m.debug_state();
    assert_eq!((progress.spent, progress.retired, after.pc), (1, 0, VECTOR));
    assert_eq!(after.x, before.x);
    assert_eq!(
        after.last_trap,
        Some(Trap {
            epc: before.pc,
            cause,
            value,
        })
    );
}

fn mret(m: &mut Machine, destination: u64) {
    m.load(m.debug_state().pc, &MRET.to_le_bytes()).unwrap();
    let progress = m.run(1);
    assert_eq!((progress.spent, progress.retired), (1, 1));
    assert_eq!(m.debug_state().pc, destination);
}

fn skip(m: &mut Machine, width: i32) {
    let original = get_csr(m, MEPC);
    run(m, 0x0002_0eb7); // lui x29, 0x20 (MPRV)
    run(m, csr(3, 0, MSTATUS, 29));
    run(m, addi(30, 28, width));
    run(m, csr(1, 0, MEPC, 30));
    mret(m, original + width as u64);
}

#[test]
fn misa_fs_sd_and_csr_aliases_share_one_architectural_state() {
    for (isa, misa_fd, fs_writable) in [(HartIsa::Imac, 0, false), (HartIsa::Imafdc, MISA_FD, true)]
    {
        let mut m = board(isa);
        assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, 0);
        let misa = get_csr(&mut m, MISA);
        assert_eq!(misa & MISA_FD, misa_fd);
        put_csr(&mut m, MISA, misa ^ MISA_FD);
        assert_eq!(get_csr(&mut m, MISA), misa);
        for fs in [0, INITIAL, CLEAN, DIRTY] {
            // SD is read-only, even when written through the other status view.
            put_csr(&mut m, SSTATUS, fs | (if fs == DIRTY { 0 } else { SD }));
            let want = if fs_writable {
                fs | (if fs == DIRTY { SD } else { 0 })
            } else {
                0
            };
            assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, want);
            assert_eq!(get_csr(&mut m, SSTATUS) & FS_SD, want);
        }
    }

    let mut m = board(HartIsa::Imafdc);
    put_csr(&mut m, MSTATUS, INITIAL);
    assert_eq!(get_csr(&mut m, FCSR), 0);
    assert_eq!(get_f(&mut m, 0), 0);
    assert_eq!(get_f(&mut m, 31), 0);
    assert_eq!(get_csr(&mut m, SSTATUS) & FS_SD, INITIAL);
    put_csr(&mut m, FCSR, u64::MAX);
    assert_eq!(get_csr(&mut m, FCSR), 0xff);
    assert_eq!(get_csr(&mut m, FRM), 7);
    assert_eq!(get_csr(&mut m, FFLAGS), 31);
    assert_eq!(get_csr(&mut m, SSTATUS) & FS_SD, DIRTY | SD);
    put_csr(&mut m, FRM, 0xfb);
    assert_eq!(get_csr(&mut m, FCSR), 0x7f);
    put_csr(&mut m, FFLAGS, 0xf5);
    assert_eq!(get_csr(&mut m, FCSR), 0x75);
    put_csr(&mut m, MSTATUS, CLEAN);
    run(&mut m, csr(2, 0, FRM, 0)); // CSRRS x0, frm, x0 is a read, not a write
    assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, CLEAN);
    run(&mut m, csr(7, 27, FFLAGS, 5)); // csrrci x27, fflags, 5
    assert_eq!(m.debug_state().x[27], 0x15);
    assert_eq!(get_csr(&mut m, FCSR), 0x70);
    assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, DIRTY | SD);
    run(&mut m, csr(6, 27, FFLAGS, 2));
    assert_eq!(m.debug_state().x[27], 0x10);
    assert_eq!(get_csr(&mut m, FCSR), 0x72);
}

#[test]
fn imac_and_fs_off_trap_without_retiring_or_accessing_memory() {
    for isa in [HartIsa::Imac, HartIsa::Imafdc] {
        let mut m = board(isa);
        // Unmapped address: illegal instruction takes precedence over its memory fault.
        for word in [
            fl(3, 0, 0, 0),
            fs(3, 0, 0, 0),
            fp(0x00, 1, 0, 0, 0),
            fp(0x71, 7, 0, 0, 0),
            0x0000_0043, // fmadd.s
            0x0000_0047, // fmsub.s
            0x0000_004b, // fnmsub.s
            0x0000_004f, // fnmadd.s
        ] {
            fault(&mut m, &word.to_le_bytes(), 2, u64::from(word));
            skip(&mut m, 4);
        }
        for csr_number in [FCSR, FFLAGS, FRM] {
            for word in [
                csr(2, 27, csr_number, 0),
                csr(1, 0, csr_number, 0),
                csr(5, 0, csr_number, 1),
            ] {
                fault(&mut m, &word.to_le_bytes(), 2, u64::from(word));
                skip(&mut m, 4);
            }
        }
        for half in [0x2000, 0xa000, C_FLDSP_F0, C_FSDSP_F0_8] {
            fault(&mut m, &half.to_le_bytes(), 2, u64::from(half));
            skip(&mut m, 2);
        }
        assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, 0);
    }
}

#[test]
fn supervisor_and_user_access_the_same_fp_registers_only_when_fs_is_enabled() {
    for (mode, mpp) in [
        (PrivilegeMode::User, 0),
        (PrivilegeMode::Supervisor, 1 << 11),
    ] {
        let mut m = board(HartIsa::Imafdc);
        put_csr(&mut m, PMPADDR0, u64::MAX);
        put_csr(&mut m, PMPCFG0, 0x0f); // TOR RWX
        put_csr(&mut m, MSTATUS, INITIAL);
        put_f(&mut m, 0, PATTERN);
        put_csr(&mut m, FCSR, 0x63);
        put_csr(&mut m, MEPC, LOWER_CODE);
        put_csr(&mut m, MSTATUS, CLEAN | mpp);
        mret(&mut m, LOWER_CODE);
        assert_eq!(m.debug_state().mode, mode);
        assert_eq!(get_f(&mut m, 0), PATTERN);
        assert_eq!(get_csr(&mut m, FCSR), 0x63);
        run(&mut m, csr(5, 0, FRM, 4));
        if mode == PrivilegeMode::Supervisor {
            assert_eq!(get_csr(&mut m, SSTATUS) & FS_SD, DIRTY | SD);
        }
        fault(&mut m, &0x0000_0073u32.to_le_bytes(), 8 + mode as u64, 0); // ecall
        assert_eq!(get_csr(&mut m, FCSR), 0x83);
        assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, DIRTY | SD);
        put_csr(&mut m, MEPC, LOWER_CODE);
        put_csr(&mut m, MSTATUS, mpp);
        mret(&mut m, LOWER_CODE);
        let word = fp(0x71, 28, 0, 0, 0); // fmv.x.d
        fault(&mut m, &word.to_le_bytes(), 2, u64::from(word));
        assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, 0);
    }
}

#[test]
fn rounding_and_fused_multiply_add_match_known_boundary_bits() {
    let mut m = board(HartIsa::Imafdc);
    put_csr(&mut m, MSTATUS, INITIAL);
    put_f(&mut m, 0, boxed(0x3f80_0000)); // 1.0
    put_f(&mut m, 1, boxed(0x3380_0000)); // 2^-24 (half of one ulp)
    for (mode, bits) in [
        0x3f80_0000,
        0x3f80_0000,
        0x3f80_0000,
        0x3f80_0001,
        0x3f80_0001,
    ]
    .into_iter()
    .enumerate()
    {
        put_csr(&mut m, FFLAGS, 0);
        run(&mut m, fp(0, 2, 0, 1, mode as u32));
        assert_eq!(get_f(&mut m, 2), boxed(bits));
        assert_eq!(get_csr(&mut m, FFLAGS), NX);
        put_csr(&mut m, FCSR, (mode as u64) << 5);
        run(&mut m, fp(0, 2, 0, 1, 7));
        assert_eq!(get_f(&mut m, 2), boxed(bits));
        assert_eq!(get_csr(&mut m, FFLAGS), NX);
    }

    put_f(&mut m, 0, boxed(0x3f80_0001)); // 1 + 2^-23
    put_f(&mut m, 1, boxed(0x3f7f_fffe)); // 1 - 2^-23
    put_f(&mut m, 2, boxed(0xbf80_0000));
    put_csr(&mut m, FFLAGS, 0);
    run(&mut m, 2 << 27 | 1 << 20 | 3 << 7 | 0x43); // fmadd.s f3, f0, f1, f2, rne
    assert_eq!(get_f(&mut m, 3), boxed(0xa880_0000)); // -2^-46, not a rounded zero
    assert_eq!(get_csr(&mut m, FFLAGS), 0);
}

#[test]
fn nan_boxing_nan_payloads_and_integer_saturation_follow_riscv_rules() {
    let mut m = board(HartIsa::Imafdc);
    put_csr(&mut m, MSTATUS, INITIAL);
    put_f(&mut m, 0, 0xffff_fffe_ff80_0456); // not a NaN-boxed narrow value
    put_f(&mut m, 1, boxed(0x3f80_0000));
    run(&mut m, fp(0, 2, 0, 1, 0)); // fadd.s sees a canonical qNaN
    assert_eq!(get_f(&mut m, 2), boxed(0x7fc0_0000));
    run(&mut m, fp(0x70, 7, 0, 0, 1)); // fclass.s
    assert_eq!(m.debug_state().x[7], 1 << 9);
    run(&mut m, fp(0x70, 7, 0, 0, 0)); // fmv.x.w reads the unboxed low bits
    assert_eq!(m.debug_state().x[7], 0xffff_ffff_ff80_0456);
    assert_eq!(get_csr(&mut m, FFLAGS), 0);

    put_f(&mut m, 0, boxed(0x7f80_0456)); // sNaN
    put_f(&mut m, 1, boxed(0x8000_0000));
    run(&mut m, fp(0x10, 2, 0, 1, 0)); // fsgnj.s preserves payload
    assert_eq!(get_f(&mut m, 2), boxed(0xff80_0456));
    run(&mut m, fp(0x70, 7, 2, 0, 1));
    assert_eq!(m.debug_state().x[7], 1 << 8);
    assert_eq!(get_csr(&mut m, FFLAGS), 0);

    put_f(&mut m, 2, boxed(0));
    run(&mut m, fp(0x14, 3, 1, 2, 0)); // fmin.s -0, +0
    assert_eq!(get_f(&mut m, 3), boxed(0x8000_0000));
    run(&mut m, fp(0x14, 3, 1, 2, 1)); // fmax.s -0, +0
    assert_eq!(get_f(&mut m, 3), boxed(0));
    assert_eq!(get_csr(&mut m, FFLAGS), 0);

    put_f(&mut m, 2, boxed(0x7fc0_0456));
    put_csr(&mut m, MSTATUS, CLEAN);
    run(&mut m, fp(0x50, 7, 2, 1, 2)); // feq.s qNaN, -0
    assert_eq!(m.debug_state().x[7], 0);
    assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, CLEAN);
    run(&mut m, fp(0x50, 7, 2, 1, 1)); // flt.s qNaN, -0 raises NV
    assert_eq!(m.debug_state().x[7], 0);
    assert_eq!(get_csr(&mut m, FFLAGS), NV);
    assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, DIRTY | SD);
    put_csr(&mut m, FFLAGS, 0);
    run(&mut m, fp(0x50, 7, 0, 2, 2)); // feq.s sNaN, qNaN raises NV
    assert_eq!(get_csr(&mut m, FFLAGS), NV);
    put_csr(&mut m, FFLAGS, 0);
    run(&mut m, fp(0x14, 3, 0, 2, 0)); // fmin.s sNaN, qNaN
    assert_eq!(get_f(&mut m, 3), boxed(0x7fc0_0000));
    assert_eq!(get_csr(&mut m, FFLAGS), NV);

    for (bits, result, flags) in [
        (0x41ef_ffff_ffe0_0000, u64::MAX, 0), // u32::MAX fits, XLEN sign-extends
        (0x41f0_0000_0000_0000, u64::MAX, NV), // 2^32 saturates
        (0xbff0_0000_0000_0000, 0, NV),       // -1 saturates to zero
        (0x7ff8_0000_0000_0123, u64::MAX, NV), // NaN saturates to maximum
        (0x3ff8_0000_0000_0000, 2, NX),       // 1.5 rounds to 2
    ] {
        put_f(&mut m, 0, bits);
        put_csr(&mut m, FFLAGS, 0);
        run(&mut m, fp(0x61, 7, 0, 1, 0)); // fcvt.wu.d
        assert_eq!(m.debug_state().x[7], result, "source {bits:#018x}");
        assert_eq!(get_csr(&mut m, FFLAGS), flags);
    }
}

#[test]
fn fflags_accrue_independently_and_a_reserved_rm_cannot_change_state() {
    let mut m = board(HartIsa::Imafdc);
    put_csr(&mut m, MSTATUS, INITIAL);
    put_f(&mut m, 0, boxed(0x3f80_0000));
    put_f(&mut m, 1, boxed(0x3380_0000));
    run(&mut m, fp(0, 2, 0, 1, 0));
    assert_eq!(get_csr(&mut m, FFLAGS), NX);
    put_f(&mut m, 1, boxed(0));
    run(&mut m, fp(0x0c, 2, 0, 1, 0));
    assert_eq!(get_csr(&mut m, FFLAGS), NX | DZ);
    put_f(&mut m, 0, boxed(0x7f7f_ffff));
    put_f(&mut m, 1, boxed(0x4000_0000));
    run(&mut m, fp(0x08, 2, 0, 1, 0));
    assert_eq!(get_csr(&mut m, FFLAGS), NX | DZ | OF);
    put_f(&mut m, 0, boxed(1));
    run(&mut m, fp(0x0c, 2, 0, 1, 0));
    assert_eq!(get_csr(&mut m, FFLAGS), NX | DZ | OF | UF);
    put_f(&mut m, 0, boxed(0));
    run(&mut m, fp(0x0c, 2, 0, 0, 0));
    assert_eq!(get_csr(&mut m, FFLAGS), NX | DZ | OF | UF | NV);
    put_csr(&mut m, FFLAGS, 0);
    assert_eq!(get_csr(&mut m, FFLAGS), 0);

    put_f(&mut m, 2, PATTERN);
    for (frm, rm) in [(0, 5), (0, 6), (5, 7), (6, 7), (7, 7)] {
        put_csr(&mut m, FCSR, frm << 5 | DZ);
        put_csr(&mut m, MSTATUS, CLEAN);
        let word = fp(0x0c, 2, 0, 0, rm);
        fault(&mut m, &word.to_le_bytes(), 2, u64::from(word));
        skip(&mut m, 4);
        assert_eq!(get_f(&mut m, 2), PATTERN);
        assert_eq!(get_csr(&mut m, FCSR), frm << 5 | DZ);
        assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, CLEAN);
    }
    // A reserved frm is irrelevant for instructions without a dynamic rm.
    run(&mut m, fp(0x10, 2, 0, 1, 0)); // fsgnj.s
    assert_eq!(get_f(&mut m, 2), boxed(0));
    run(&mut m, fp(0, 2, 1, 1, 0)); // fadd.s with static RNE
    assert_eq!(get_f(&mut m, 2), boxed(0x4080_0000));
}

#[test]
fn compressed_fldsp_accepts_f0_and_float_stores_invalidate_overlapping_lr() {
    let mut m = board(HartIsa::Imafdc);
    m.load(DATA, &PATTERN.to_le_bytes()).unwrap();
    set_x(&mut m, 2, DATA);
    put_csr(&mut m, MSTATUS, INITIAL);
    retire(&mut m, &C_FLDSP_F0.to_le_bytes());
    assert_eq!(get_f(&mut m, 0), PATTERN);
    assert_eq!(get_csr(&mut m, SSTATUS) & FS_SD, DIRTY | SD);
    put_csr(&mut m, SSTATUS, CLEAN);
    retire(&mut m, &C_FSDSP_F0_8.to_le_bytes());
    assert_eq!(m.inspect(DATA + 8, 8).unwrap(), PATTERN.to_le_bytes());
    assert_eq!(get_csr(&mut m, SSTATUS) & FS_SD, CLEAN);

    set_x(&mut m, 1, DATA);
    run(&mut m, atomic(2, 4, 0)); // lr.d x4, (x1) covers [DATA, DATA+8)
    run(&mut m, fs(2, 0, 1, 4)); // fsw consumes the raw low 32 bits, even unboxed
    run(&mut m, atomic(3, 5, 0)); // sc.d x5, x0, (x1) must fail
    assert_eq!(m.debug_state().x[4], PATTERN);
    assert_eq!(m.debug_state().x[5], 1);
    assert_eq!(
        m.inspect(DATA, 8).unwrap(),
        &[0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12]
    );
    assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, CLEAN);
}

#[test]
fn failed_fp_access_preserves_register_memory_flags_and_fs() {
    let mut m = board(HartIsa::Imafdc);
    let original = 0x1234_5678_abcd_ef01u64;
    m.load(DATA, &original.to_le_bytes()).unwrap();
    m.load(DATA + 8, &original.to_le_bytes()).unwrap();
    put_csr(&mut m, MSTATUS, INITIAL);
    put_csr(&mut m, FCSR, 0x63);
    put_f(&mut m, 0, PATTERN);
    for (address, status, load_cause, store_cause) in [
        (DATA + 1, 0, 4, 6),
        (DATA, MPRV_S, 5, 7),
        (DATA, MPRV_S, 13, 15),
    ] {
        if load_cause == 13 {
            // Unmapped Sv39 VA, with page-table reads allowed by PMP.
            put_csr(&mut m, PMPADDR0, u64::MAX);
            put_csr(&mut m, PMPCFG0, 0x0f); // TOR RWX from zero to PMPADDR0 << 2
            put_csr(&mut m, SATP, 8 << 60 | ROOT >> 12);
        }
        set_x(&mut m, 1, address);
        for (word, cause) in [(fl(3, 0, 1, 0), load_cause), (fs(3, 0, 1, 0), store_cause)] {
            put_csr(&mut m, MSTATUS, CLEAN | status);
            fault(&mut m, &word.to_le_bytes(), cause, address);
            skip(&mut m, 4);
            assert_eq!(get_f(&mut m, 0), PATTERN);
            assert_eq!(get_csr(&mut m, FCSR), 0x63);
            assert_eq!(get_csr(&mut m, MSTATUS) & FS_SD, CLEAN);
            assert_eq!(m.inspect(DATA, 8).unwrap(), original.to_le_bytes());
            assert_eq!(m.inspect(DATA + 8, 8).unwrap(), original.to_le_bytes());
        }
    }
}
