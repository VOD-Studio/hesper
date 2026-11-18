use PrivilegeMode::{Machine as M, Supervisor as S, User as U};
use hesper_riscv_linux::{DRAM_BASE, Machine, PrivilegeMode, Trap};

const SSTATUS: u32 = 0x100;
const SIE: u32 = 0x104;
const STVEC: u32 = 0x105;
const SEPC: u32 = 0x141;
const SCAUSE: u32 = 0x142;
const STVAL: u32 = 0x143;
const SIP: u32 = 0x144;
const MSTATUS: u32 = 0x300;
const MEDELEG: u32 = 0x302;
const MIDELEG: u32 = 0x303;
const MIE: u32 = 0x304;
const MTVEC: u32 = 0x305;
const MEPC: u32 = 0x341;
const MCAUSE: u32 = 0x342;
const MTVAL: u32 = 0x343;
const MIP: u32 = 0x344;
const MHARTID: u32 = 0xf14;

const S_IE: u64 = 1 << 1;
const M_IE: u64 = 1 << 3;
const SPIE: u64 = 1 << 5;
const MPIE: u64 = 1 << 7;
const SPP: u64 = 1 << 8;
const MPP_S: u64 = 1 << 11;
const MPP_M: u64 = 3 << 11;
const S_STACK: u64 = SPP | SPIE | S_IE;
const M_STACK: u64 = MPP_M | MPIE | M_IE;

const INTERRUPT: u64 = 1 << 63;
const SSIP: u64 = 1 << 1;
const MTIP: u64 = 1 << 7;
const SEIP: u64 = 1 << 9;
const ECALL: u32 = 0x0000_0073;
const EBREAK: u32 = 0x0010_0073;
const SRET: u32 = 0x1020_0073;
const MRET: u32 = 0x3020_0073;

const M_VECTOR: i32 = 0x100;
const S_VECTOR: i32 = 0x140;
const S_CODE: i32 = 0x200;
const U_CODE: i32 = 0x240;

const fn auipc(rd: u32) -> u32 {
    rd << 7 | 0x17 // immediate zero
}

const fn lui(rd: u32, imm20: u32) -> u32 {
    imm20 << 12 | rd << 7 | 0x37
}

const fn addi(rd: u32, rs1: u32, imm: i32) -> u32 {
    (imm as u32 & 0xfff) << 20 | rs1 << 15 | rd << 7 | 0x13
}

const fn csrr(rd: u32, csr: u32) -> u32 {
    csr << 20 | 2 << 12 | rd << 7 | 0x73 // CSRRS rd, csr, x0
}

const fn csrrw(rd: u32, csr: u32, rs1: u32) -> u32 {
    csr << 20 | rs1 << 15 | 1 << 12 | rd << 7 | 0x73
}

const fn csrw(csr: u32, rs1: u32) -> u32 {
    csrrw(0, csr, rs1)
}

fn addr(offset: i32) -> u64 {
    DRAM_BASE + 0x1000 + offset as u64
}

fn machine_with_access() -> Machine {
    let mut machine = Machine::new();
    let setup = [
        addi(31, 0, -1),
        csrw(0x3b0, 31), // pmpaddr0: NAPOT covering physical space
        addi(31, 0, 31),
        csrw(0x3a0, 31), // pmpcfg0: NAPOT, RWX
        0x0000_1f97,     // auipc x31, 1
        0xff0f_8067,     // jalr x0, -16(x31): enter test section
    ];
    machine
        .load(DRAM_BASE, setup.map(u32::to_le_bytes).as_flattened())
        .unwrap();
    retire(&mut machine, setup.len() as u32);
    machine
}

fn load<const N: usize>(machine: &mut Machine, offset: i32, words: [u32; N]) {
    machine
        .load(addr(offset), words.map(u32::to_le_bytes).as_flattened())
        .unwrap();
}

#[track_caller]
fn retire(machine: &mut Machine, count: u32) {
    let progress = machine.run(count);
    assert_eq!((progress.spent, progress.retired), (count, count));
}

#[track_caller]
fn at(machine: &Machine, mode: PrivilegeMode, offset: i32) {
    let state = machine.debug_state();
    assert_eq!((state.mode, state.pc), (mode, addr(offset)));
}

#[track_caller]
fn trap(machine: &mut Machine, mode: PrivilegeMode, vector: i32, cause: u64, value: u64) {
    let before = machine.debug_state();
    let progress = machine.run(1);
    assert_eq!(
        (progress.spent, progress.retired, progress.time),
        (1, 0, before.time + 1)
    );
    at(machine, mode, vector);
    let after = machine.debug_state();
    assert_eq!(
        after.last_trap,
        Some(Trap {
            epc: before.pc,
            cause,
            value,
        })
    );
    assert_eq!((after.x, after.retired), (before.x, before.retired));
}

#[test]
fn user_ecall_delegates_to_supervisor_and_nested_supervisor_ecall_returns_from_machine() {
    const SIGNATURE: i32 = 0x300;
    let mut machine = machine_with_access();
    load(
        &mut machine,
        0,
        [
            auipc(1),
            addi(2, 1, M_VECTOR + 1), // vectored: synchronous traps still use BASE
            csrw(MTVEC, 2),
            addi(2, 1, S_VECTOR + 1),
            csrw(STVEC, 2),
            addi(2, 0, 1 << 8), // delegate only U ECALL
            csrw(MEDELEG, 2),
            addi(2, 1, S_CODE),
            csrw(MEPC, 2),
            lui(2, 1),
            addi(2, 2, (MPP_S | MPIE) as i32 - 0x1000),
            csrw(MSTATUS, 2),
            MRET,
        ],
    );
    load(
        &mut machine,
        S_CODE,
        [
            addi(2, 1, U_CODE),
            csrw(SEPC, 2),
            addi(2, 0, SPIE as i32), // SPP=U, enable SIE on return
            csrw(SSTATUS, 2),
            SRET,
        ],
    );
    load(
        &mut machine,
        U_CODE,
        [ECALL, addi(20, 0, 0x5a), 0x3140_a023, EBREAK], // sw x20, 0x300(x1)
    );
    load(
        &mut machine,
        S_VECTOR,
        [
            csrr(10, SCAUSE),
            csrr(11, SEPC),
            csrr(12, STVAL),
            csrr(13, SSTATUS),
            addi(14, 11, 4),
            csrw(SEPC, 14),
            ECALL,
            csrr(15, SSTATUS),
            SRET,
        ],
    );
    load(
        &mut machine,
        M_VECTOR,
        [
            csrr(16, MCAUSE),
            csrr(17, MEPC),
            csrr(18, MTVAL),
            csrr(19, MSTATUS),
            addi(21, 17, 4),
            csrw(MEPC, 21),
            MRET,
        ],
    );

    at(&machine, M, 0);
    retire(&mut machine, 13);
    at(&machine, S, S_CODE);
    retire(&mut machine, 5);
    at(&machine, U, U_CODE);
    trap(&mut machine, S, S_VECTOR, 8, 0);
    retire(&mut machine, 6);
    assert_eq!(machine.debug_state().x[10..13], [8, addr(U_CODE), 0]);
    assert_eq!(machine.debug_state().x[13] & S_STACK, SPIE);
    at(&machine, S, S_VECTOR + 24);
    trap(&mut machine, M, M_VECTOR, 9, 0);
    retire(&mut machine, 4);
    assert_eq!(machine.debug_state().x[16..19], [9, addr(S_VECTOR + 24), 0]);
    assert_eq!(
        machine.debug_state().x[19] & (M_STACK | S_STACK),
        MPP_S | MPIE | SPIE
    );

    retire(&mut machine, 3);
    at(&machine, S, S_VECTOR + 28);
    retire(&mut machine, 1);
    assert_eq!(machine.debug_state().x[15] & S_STACK, SPIE);
    retire(&mut machine, 1);
    at(&machine, U, U_CODE + 4);
    assert_eq!(machine.inspect(addr(SIGNATURE), 4), Some(&[0; 4][..]));
    retire(&mut machine, 2);
    assert_eq!(
        machine.inspect(addr(SIGNATURE), 4),
        Some(&[0x5a, 0, 0, 0][..])
    );

    // Another M trap observes the restored interrupt enables without a U-mode CSR read.
    at(&machine, U, U_CODE + 12);
    trap(&mut machine, M, M_VECTOR, 3, addr(U_CODE + 12));
    retire(&mut machine, 4);
    assert_eq!(
        machine.debug_state().x[19] & (M_STACK | S_STACK),
        MPIE | SPIE | S_IE
    );
}

#[test]
fn csr_readonly_and_privilege_faults_do_not_write_the_destination() {
    const WRITE_ZERO: u32 = csrrw(5, MHARTID, 0);
    const WRITE_NONZERO: u32 = csrrw(5, MHARTID, 5);
    const READ: u32 = csrr(5, MHARTID);
    let mut machine = machine_with_access();
    load(
        &mut machine,
        0,
        [
            auipc(1),
            addi(2, 1, M_VECTOR),
            csrw(MTVEC, 2),
            addi(5, 0, 0x5a),
            READ,
            addi(5, 0, 0x5a),
            WRITE_ZERO, // unlike CSRRS, CSRRW still writes with rs1=x0
            WRITE_NONZERO,
            csrr(6, MHARTID),
            addi(2, 1, S_CODE),
            csrw(MEPC, 2),
            lui(2, 1),
            addi(2, 2, MPP_S as i32 - 0x1000),
            csrw(MSTATUS, 2),
            MRET,
        ],
    );
    load(&mut machine, S_CODE, [READ]);
    load(
        &mut machine,
        M_VECTOR,
        [
            csrr(10, MCAUSE),
            csrr(11, MEPC),
            csrr(12, MTVAL),
            csrr(13, MSTATUS),
            addi(14, 11, 4),
            csrw(MEPC, 14),
            MRET,
        ],
    );

    retire(&mut machine, 5);
    assert_eq!(machine.debug_state().x[5], 0);
    retire(&mut machine, 1);
    for (offset, illegal) in [(24, WRITE_ZERO), (28, WRITE_NONZERO)] {
        at(&machine, M, offset);
        trap(&mut machine, M, M_VECTOR, 2, u64::from(illegal));
        retire(&mut machine, 4);
        assert_eq!(
            machine.debug_state().x[10..13],
            [2, addr(offset), u64::from(illegal)]
        );
        assert_eq!(machine.debug_state().x[13] & M_STACK, MPP_M);
        retire(&mut machine, 3);
        at(&machine, M, offset + 4);
        assert_eq!(machine.debug_state().x[5], 0x5a);
    }
    retire(&mut machine, 7);
    at(&machine, S, S_CODE);
    assert_eq!(machine.debug_state().x[6], 0);
    trap(&mut machine, M, M_VECTOR, 2, u64::from(READ));
    retire(&mut machine, 4);
    assert_eq!(
        machine.debug_state().x[10..13],
        [2, addr(S_CODE), u64::from(READ)]
    );
    assert_eq!(machine.debug_state().x[13] & M_STACK, MPP_S);
    retire(&mut machine, 3);
    at(&machine, S, S_CODE + 4);
    assert_eq!(machine.debug_state().x[5], 0x5a);
}

#[test]
fn machine_timer_requires_pending_local_and_global_enables_and_stays_pending_until_deasserted() {
    const VECTOR: i32 = M_VECTOR + 4 * 7;
    const RESUME: i32 = 14 * 4;
    let mut machine = machine_with_access();
    load(
        &mut machine,
        0,
        [
            auipc(1),
            addi(2, 1, M_VECTOR + 1),
            csrw(MTVEC, 2),
            addi(3, 0, MTIP as i32),
            addi(4, 0, M_IE as i32),
            csrw(MIE, 3),
            csrw(MSTATUS, 4),
            csrr(5, MIP), // both enables set, not pending
            csrw(MSTATUS, 0),
            csrr(6, MIP), // pending and locally enabled, globally disabled
            csrw(MIE, 0),
            csrw(MSTATUS, 4),
            csrr(7, MIP), // pending and globally enabled, locally disabled
            csrw(MIE, 3),
            csrr(15, MSTATUS), // interrupted before this instruction
        ],
    );
    load(
        &mut machine,
        VECTOR,
        [
            csrr(10, MCAUSE),
            csrr(11, MEPC),
            csrr(12, MTVAL),
            csrr(13, MSTATUS),
            csrr(14, MIP),
            MRET, // retry the interrupted instruction, not EPC + 4
        ],
    );

    retire(&mut machine, 9);
    assert_eq!(machine.debug_state().x[5], 0);
    machine.set_interrupt_lines(MTIP);
    retire(&mut machine, 5);
    assert_eq!(machine.debug_state().x[6..8], [MTIP, MTIP]);
    at(&machine, M, RESUME);
    trap(&mut machine, M, VECTOR, INTERRUPT | 7, 0);
    retire(&mut machine, 6);
    assert_eq!(
        machine.debug_state().x[10..13],
        [INTERRUPT | 7, addr(RESUME), 0]
    );
    assert_eq!(machine.debug_state().x[13] & M_STACK, MPP_M | MPIE);
    assert_eq!(machine.debug_state().x[14], MTIP);
    at(&machine, M, RESUME);
    trap(&mut machine, M, VECTOR, INTERRUPT | 7, 0);

    machine.set_interrupt_lines(0);
    retire(&mut machine, 6);
    assert_eq!(machine.debug_state().x[14], 0);
    at(&machine, M, RESUME);
    retire(&mut machine, 1);
    assert_eq!(machine.debug_state().x[15] & M_STACK, MPIE | M_IE);
}

#[test]
fn delegated_external_interrupt_obeys_target_privilege_and_preserves_guest_pending_bits() {
    const VECTOR: i32 = S_VECTOR + 4 * 9;
    const RESUME: i32 = S_CODE + 12;
    let mut machine = machine_with_access();
    load(
        &mut machine,
        0,
        [
            auipc(1),
            addi(2, 1, S_VECTOR + 1),
            csrw(STVEC, 2),
            addi(2, 0, (SEIP | SSIP) as i32),
            csrw(MIDELEG, 2),
            addi(2, 0, SEIP as i32),
            csrw(SIE, 2),
            addi(2, 0, SSIP as i32),
            csrw(SIP, 2),
            addi(2, 1, S_CODE),
            csrw(MEPC, 2),
            lui(2, 1),
            addi(2, 2, (MPP_S | MPIE | M_IE) as i32 - 0x1000),
            csrw(MSTATUS, 2),
            csrr(5, SIP), // delegated interrupt must not be taken in M
            csrr(6, MIP),
            csrr(7, MIE),
            MRET,
        ],
    );
    load(
        &mut machine,
        S_CODE,
        [
            csrr(8, SIP), // pending but SIE is off in S-mode
            addi(2, 0, S_IE as i32),
            csrw(SSTATUS, 2),
            csrr(15, SSTATUS), // first return resumes here
            addi(2, 1, U_CODE),
            csrw(SEPC, 2),
            csrw(SSTATUS, 0),
            SRET,
        ],
    );
    load(&mut machine, U_CODE, [addi(20, 0, 0x5a)]);
    load(
        &mut machine,
        VECTOR,
        [
            csrr(10, SCAUSE),
            csrr(11, SEPC),
            csrr(12, STVAL),
            csrr(13, SSTATUS),
            csrr(14, SIP),
            SRET,
        ],
    );

    retire(&mut machine, 14);
    machine.set_interrupt_lines(SEIP);
    retire(&mut machine, 4);
    assert_eq!(
        machine.debug_state().x[5..8],
        [SEIP | SSIP, SEIP | SSIP, SEIP]
    );
    at(&machine, S, S_CODE);
    retire(&mut machine, 3);
    assert_eq!(machine.debug_state().x[8], SEIP | SSIP);
    at(&machine, S, RESUME);
    trap(&mut machine, S, VECTOR, INTERRUPT | 9, 0);
    retire(&mut machine, 5);
    assert_eq!(
        machine.debug_state().x[10..13],
        [INTERRUPT | 9, addr(RESUME), 0]
    );
    assert_eq!(machine.debug_state().x[13] & S_STACK, SPP | SPIE);
    assert_eq!(machine.debug_state().x[14], SEIP | SSIP);
    machine.set_interrupt_lines(0);
    retire(&mut machine, 1);
    at(&machine, S, RESUME);
    retire(&mut machine, 4);
    assert_eq!(machine.debug_state().x[15] & S_STACK, SPIE | S_IE);

    // The same request is gated in S with SIE=0, but not after SRET drops to U.
    machine.set_interrupt_lines(SEIP);
    retire(&mut machine, 1);
    at(&machine, U, U_CODE);
    trap(&mut machine, S, VECTOR, INTERRUPT | 9, 0);
    machine.set_interrupt_lines(0);
    retire(&mut machine, 5);
    assert_eq!(
        machine.debug_state().x[10..13],
        [INTERRUPT | 9, addr(U_CODE), 0]
    );
    assert_eq!(machine.debug_state().x[13] & S_STACK, 0);
    assert_eq!(machine.debug_state().x[14], SSIP);
    retire(&mut machine, 1);
    at(&machine, U, U_CODE);
    retire(&mut machine, 1);
    assert_eq!(machine.debug_state().x[20], 0x5a);
}

#[test]
fn wfi_waits_without_spending_fuel_and_wakes_with_global_interrupts_off() {
    let mut machine = machine_with_access();
    let started = machine.debug_state().time;
    load(
        &mut machine,
        0,
        [
            addi(1, 0, 128),
            csrw(MIE, 1),
            0x1050_0073,    // WFI
            csrr(2, 0xc00), // cycle
            csrr(3, 0xc01), // time
            csrr(4, 0xc02), // instret
        ],
    );
    let wait = machine.run(50);
    assert_eq!((wait.spent, wait.retired, wait.time), (3, 3, started + 3));
    let before = machine.debug_state();
    machine.set_interrupt_lines(1 << 3); // MSIP is pending but individually disabled.
    assert_eq!(machine.run(50).spent, 0);
    assert_eq!(machine.debug_state(), before);
    machine.set_interrupt_lines(MTIP);
    retire(&mut machine, 3);
    assert_eq!(
        machine.debug_state().x[2..5],
        [started + 3, started + 4, started + 5]
    );
    assert_eq!(machine.debug_state().last_trap, None);
}

#[test]
fn faulting_reset_vector_and_unmapped_trap_vector_are_bounded() {
    let mut machine = Machine::new();
    machine.load(DRAM_BASE, &EBREAK.to_le_bytes()).unwrap();
    let progress = machine.run(32);
    assert_eq!((progress.spent, progress.retired), (32, 0));
    let state = machine.diagnostics();
    assert_eq!(state.stalled_traps, 32);
    assert_eq!(
        state.machine_trap,
        Trap {
            epc: 0,
            cause: 1, // instruction access fault at the unmapped mtvec
            value: 0,
        }
    );
}
