use hesper_riscv_linux::{DRAM_BASE, DRAM_SIZE, Machine, Trap};

fn machine(words: &[u32]) -> Machine {
    let mut machine = Machine::new();
    for (index, word) in words.iter().enumerate() {
        machine
            .load(DRAM_BASE + index as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    machine
}

const fn atomic(op: u32, width: u32, rd: u32, rs2: u32) -> u32 {
    op << 27 | rs2 << 20 | 1 << 15 | width << 12 | rd << 7 | 0x2f
}

#[test]
fn one_reservation_per_success_or_failure_and_overlapping_writes_invalidate() {
    let mut m = machine(&[
        0x0000_0097,        // auipc x1, 0
        0x1000_8093,        // addi x1, x1, 0x100
        0x00b0_0113,        // addi x2, x0, 11
        atomic(2, 2, 3, 0), // lr.w x3, (x1)
        atomic(3, 2, 4, 2), // sc.w x4, x2, (x1)
        atomic(3, 2, 5, 0), // sc.w x5, x0, (x1) without another LR
        atomic(2, 2, 6, 0), // lr.w x6, (x1)
        0x0000_80a3,        // sb x0, 1(x1) (same value, still a store)
        atomic(3, 2, 7, 0), // sc.w x7, x0, (x1)
        atomic(2, 3, 8, 0), // lr.d x8, (x1)
        atomic(3, 3, 9, 0), // sc.d x9, x0, (x1), host invalidated
    ]);
    let data = DRAM_BASE + 0x100;
    m.load(data, &[0, 0, 0, 0x80, 0xde, 0xad, 0xbe, 0xef])
        .unwrap();
    for fuel in [4, 1, 1, 3, 1] {
        assert_eq!(m.run(fuel).retired, fuel);
    }
    m.load(data + 7, &[0xff]).unwrap();
    assert_eq!(m.run(1).retired, 1);
    assert_eq!(
        m.debug_state().x[3..10],
        [0xffff_ffff_8000_0000, 0, 1, 11, 1, 0xefbe_adde_0000_000b, 1]
    );
    assert_eq!(
        m.inspect(data, 8),
        Some(&[11, 0, 0, 0, 0xde, 0xad, 0xbe, 0xff][..])
    );
}

#[test]
fn amo_returns_the_original_value_and_uses_the_store_fault_class() {
    let mut m = machine(&[
        0x0000_0097,
        0x1000_8093,
        0x0010_0113,
        atomic(0, 2, 3, 2),  // amoadd.w x3, x2, (x1)
        atomic(16, 2, 4, 2), // amomin.w x4, x2, (x1)
        atomic(28, 3, 5, 2), // amomaxu.d x5, x2, (x1)
        0x0020_8093,
        atomic(0, 2, 6, 2), // misaligned amoadd.w
    ]);
    let data = DRAM_BASE + 0x100;
    let high = 0x1234_5678_0000_0000u64;
    m.load(data, &(high | 0x7fff_ffff).to_le_bytes()).unwrap();
    assert_eq!(m.run(7).retired, 7);
    assert_eq!(
        m.debug_state().x[3..6],
        [0x7fff_ffff, 0xffff_ffff_8000_0000, high | 0x8000_0000]
    );
    let before = m.debug_state();
    assert_eq!(m.run(1).retired, 0);
    assert_eq!(m.debug_state().x, before.x);
    assert_eq!(
        m.debug_state().last_trap,
        Some(Trap {
            epc: before.pc,
            cause: 6,
            value: data + 2
        })
    );
    assert_eq!(
        m.inspect(data, 8),
        Some(&(high | 0x8000_0000).to_le_bytes()[..])
    );
}

#[test]
fn compressed_jalr_links_across_two_bytes_and_32_bit_fetch_can_start_at_halfword() {
    let mut m = Machine::new();
    m.load(
        DRAM_BASE,
        &[
            0x01, 0x00, // 0: c.nop
            0x97, 0x02, 0x00, 0x00, // 2: auipc x5, 0
            0x93, 0x82, 0xb2, 0x00, // 6: addi x5, x5, 11 (odd address 13)
            0x82, 0x92, // 10: c.jalr x5 (target 12, link 12)
            0x05, 0x04, // 12: c.addi x8, 1
            0x00, 0x00, // 14: illegal c.addi4spn with zero immediate
        ],
    )
    .unwrap();
    for (retired, pc) in [(1, 2), (1, 6), (1, 10), (1, 12), (1, 14)] {
        assert_eq!(m.run(1).retired, retired);
        assert_eq!(m.debug_state().pc, DRAM_BASE + pc);
    }
    assert_eq!(m.debug_state().x[1], DRAM_BASE + 12);
    assert_eq!(m.debug_state().x[8], 1);
    assert_eq!(m.run(1).retired, 0);
    assert_eq!(
        m.debug_state().last_trap,
        Some(Trap {
            epc: DRAM_BASE + 14,
            cause: 2,
            value: 0
        })
    );
}

#[test]
fn last_halfword_is_fetchable_without_accessing_past_dram() {
    let tail = DRAM_BASE + DRAM_SIZE as u64 - 2;
    for (halfword, retired, epc) in [(0x0001u16, 1, tail + 2), (0x0093u16, 0, tail)] {
        let mut m = machine(&[
            0x0800_0297, // auipc x5, 0x8000
            0xffe2_8067, // jalr x0, -2(x5)
        ]);
        m.load(tail, &halfword.to_le_bytes()).unwrap();
        assert_eq!(m.run(2).retired, 2);
        assert_eq!(m.run(1).retired, retired);
        if retired != 0 {
            assert_eq!(m.run(1).retired, 0);
        }
        assert_eq!(
            m.debug_state().last_trap,
            Some(Trap {
                epc,
                cause: 1,
                value: tail + 2
            })
        );
    }
}
