use hesper_riscv_linux::{DRAM_BASE, DRAM_SIZE, Machine, Trap};

const PROGRAM: [u32; 4] = [0x0070_0093, 0x0000_0117, 0x0011_2e23, 0x0010_8013];

#[test]
fn executes_the_probe_identically_under_three_fuel_partitions() {
    let program = PROGRAM.map(u32::to_le_bytes);
    let program = program.as_flattened();
    for partitions in [&[1, 1, 1, 1][..], &[1, 3], &[4]] {
        let mut machine = Machine::new();
        machine.load(DRAM_BASE, program).unwrap();
        let start = machine.debug_state();
        let zero = machine.run(0);
        assert_eq!((zero.spent, zero.retired, zero.time), (0, 0, 0));
        assert_eq!(machine.debug_state(), start);
        for &fuel in partitions {
            let progress = machine.run(fuel);
            assert_eq!((progress.spent, progress.retired), (fuel, fuel));
        }
        let state = machine.debug_state();
        assert_eq!(state.pc, 0x8000_0010);
        assert_eq!(state.x[..3], [0, 7, 0x8000_0004]);
        assert_eq!((state.retired, state.time), (4, 4));
        assert_eq!(machine.inspect(0x8000_0020, 4), Some(&[7, 0, 0, 0][..]));
    }
}

#[test]
fn illegal_instruction_consumes_fuel_without_retiring() {
    let mut machine = Machine::new();
    machine
        .load(DRAM_BASE, &0xffff_ffffu32.to_le_bytes())
        .unwrap();
    let progress = machine.run(1);
    assert_eq!((progress.spent, progress.retired, progress.time), (1, 0, 1));
    let trap = machine.debug_state().last_trap.unwrap();
    assert_eq!(
        (trap.epc, trap.cause, trap.value),
        (DRAM_BASE, 2, 0xffff_ffff)
    );
    assert_eq!(machine.debug_state().pc, 0);
    assert_eq!(machine.run(3).retired, 0);
    assert_eq!(machine.debug_state().time, 4);
}

fn load_probe<const N: usize>(words: [u32; N]) -> Machine {
    let mut machine = Machine::new();
    machine
        .load(DRAM_BASE, words.map(u32::to_le_bytes).as_flattened())
        .unwrap();
    machine
}

fn retire(machine: &mut Machine, count: u32) {
    let progress = machine.run(count);
    assert_eq!((progress.spent, progress.retired), (count, count));
}

fn fault(machine: &mut Machine, cause: u64, value: u64) {
    let before = machine.debug_state();
    let progress = machine.run(1);
    assert_eq!(
        (progress.spent, progress.retired, progress.time),
        (1, 0, before.time + 1)
    );
    let after = machine.debug_state();
    assert_eq!(
        after.last_trap,
        Some(Trap {
            epc: before.pc,
            cause,
            value,
        })
    );
    assert_eq!(after.retired, before.retired);
    assert_eq!(after.x, before.x);
    assert_eq!(after.pc, 0);
}

#[test]
fn loads_extend_by_width_and_stores_write_only_their_width() {
    let mut machine = load_probe([
        0x0000_0097, // auipc x1, 0
        0x1000_8093, // addi  x1, x1, 0x100
        0xff80_8103, // lb    x2, -8(x1)
        0xff80_c183, // lbu   x3, -8(x1)
        0xff80_9203, // lh    x4, -8(x1)
        0xff80_d283, // lhu   x5, -8(x1)
        0xff80_a303, // lw    x6, -8(x1)
        0xff80_e383, // lwu   x7, -8(x1)
        0xff80_b403, // ld    x8, -8(x1)
        0x0080_80a3, // sb    x8, 1(x1)
        0x0080_9523, // sh    x8, 10(x1)
        0x0080_aa23, // sw    x8, 20(x1)
        0x0080_bc23, // sd    x8, 24(x1)
    ]);
    let value = 0xfedc_ba98_8765_c3a1u64;
    machine
        .load(DRAM_BASE + 0xf8, &value.to_le_bytes())
        .unwrap();
    let output = DRAM_BASE + 0x100;
    machine.load(output, &[0x5a; 32]).unwrap();

    retire(&mut machine, 9);
    assert_eq!(
        machine.debug_state().x[2..9],
        [
            0xffff_ffff_ffff_ffa1,
            0xa1,
            0xffff_ffff_ffff_c3a1,
            0xc3a1,
            0xffff_ffff_8765_c3a1,
            0x8765_c3a1,
            value,
        ]
    );
    assert_eq!(machine.debug_state().pc, DRAM_BASE + 36);
    retire(&mut machine, 4);
    let expected = [
        0x5a, 0xa1, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, // sb
        0x5a, 0x5a, 0xa1, 0xc3, 0x5a, 0x5a, 0x5a, 0x5a, // sh
        0x5a, 0x5a, 0x5a, 0x5a, 0xa1, 0xc3, 0x65, 0x87, // sw
        0xa1, 0xc3, 0x65, 0x87, 0x98, 0xba, 0xdc, 0xfe, // sd
    ];
    assert_eq!(machine.inspect(output, expected.len()), Some(&expected[..]));
    assert_eq!(machine.debug_state().pc, DRAM_BASE + 52);
}

#[test]
fn faulting_loads_and_stores_do_not_write_back() {
    const MISALIGNED_SETUP: &[u32] = &[
        0x0000_0097, // auipc x1, 0
        0x0820_8093, // addi  x1, x1, 0x82
    ];
    const PAST_END_SETUP: &[u32] = &[
        0x0800_0097, // auipc x1, 0x8000 (DRAM_BASE + 128 MiB)
    ];
    const HIGH_ADDRESS_SETUP: &[u32] = &[
        0x0000_0097, // auipc x1, 0
        0x0010_9193, // slli  x3, x1, 1
        0x0030_80b3, // add   x1, x1, x3
        0x0800_8093, // addi  x1, x1, 0x80
    ];
    let end = DRAM_BASE + DRAM_SIZE as u64;
    let sentinel = [0x5a; 16];
    for (setup, address, guard, load_cause, store_cause) in [
        (MISALIGNED_SETUP, DRAM_BASE + 0x82, DRAM_BASE + 0x80, 4, 6),
        (PAST_END_SETUP, end, end - 16, 5, 7),
        // A 32-bit truncation would alias the guarded region.
        (
            HIGH_ADDRESS_SETUP,
            DRAM_BASE + 0x1_0000_0080,
            DRAM_BASE + 0x80,
            5,
            7,
        ),
    ] {
        for (word, cause) in [
            (0x0000_b103u32, load_cause),  // ld x2, 0(x1)
            (0x0020_b023u32, store_cause), // sd x2, 0(x1)
        ] {
            let mut machine = Machine::new();
            for (index, &word) in setup.iter().enumerate() {
                machine
                    .load(DRAM_BASE + 4 * index as u64, &word.to_le_bytes())
                    .unwrap();
            }
            let tail = DRAM_BASE + 4 * setup.len() as u64;
            // addi x2, x0, -1
            machine.load(tail, &0xfff0_0113u32.to_le_bytes()).unwrap();
            machine.load(tail + 4, &word.to_le_bytes()).unwrap();
            machine.load(guard, &sentinel).unwrap();
            retire(&mut machine, setup.len() as u32 + 1);
            assert_eq!(machine.debug_state().pc, tail + 4);
            assert_eq!(machine.debug_state().x[1..3], [address, u64::MAX]);
            fault(&mut machine, cause, address);
            assert_eq!(machine.inspect(guard, sentinel.len()), Some(&sentinel[..]));
        }
    }
}

#[test]
fn beq_and_jumps_follow_exact_targets_and_link_to_the_next_instruction() {
    let mut machine = load_probe([
        0x0010_0093, //  0: addi x1, x0, 1
        0x0000_8a63, //  4: beq  x1, x0, +20 (not taken)
        0x0010_8463, //  8: beq  x1, x1, +8  (taken)
        0xffff_ffff, // 12: must not execute
        0x0080_02ef, // 16: jal  x5, +8
        0x0070_0393, // 20: addi x7, x0, 7 (return site)
        0x0012_8367, // 24: jalr x6, 1(x5) (odd address, returns to 20)
    ]);
    for offset in [4, 8, 16, 24, 20, 24] {
        retire(&mut machine, 1);
        assert_eq!(machine.debug_state().pc, DRAM_BASE + offset);
    }
    assert_eq!(
        machine.debug_state().x[5..8],
        [DRAM_BASE + 20, DRAM_BASE + 28, 7]
    );
}

#[test]
fn word_division_handles_overflow_and_zero_and_word_results_sign_extend() {
    let mut machine = load_probe([
        0x0000_0097, // auipc x1, 0 (0x8000_0000: positive XLEN, negative W)
        0xfff0_0113, // addi  x2, x0, -1
        0x0220_c1bb, // divw  x3, x1, x2
        0x0200_c23b, // divw  x4, x1, x0
        0x0200_e2bb, // remw  x5, x1, x0
        0x0200_d33b, // divuw x6, x1, x0
        0x0200_f3bb, // remuw x7, x1, x0
        0x0220_e43b, // remw  x8, x1, x2
        0x0000_849b, // addiw x9, x1, 0
        0xfff0_851b, // addiw x10, x1, -1
    ]);
    const MIN_WORD: u64 = 0xffff_ffff_8000_0000;
    retire(&mut machine, 10);
    let state = machine.debug_state();
    assert_eq!(
        state.x[1..11],
        [
            0x8000_0000,
            u64::MAX,
            MIN_WORD,
            u64::MAX,
            MIN_WORD,
            u64::MAX,
            MIN_WORD,
            0,
            MIN_WORD,
            0x7fff_ffff,
        ]
    );
    assert_eq!(state.pc, DRAM_BASE + 40);
}

#[test]
fn fence_i_makes_the_guest_store_visible_to_a_previously_executed_instruction() {
    const REPLACEMENT: u32 = 0x0020_0113; // addi x2, x0, 2
    let mut machine = load_probe([
        0x0000_0097, //  0: auipc   x1, 0
        0x0010_0113, //  4: addi    x2, x0, 1 (overwritten by the guest)
        0x0180_a183, //  8: lw      x3, 24(x1)
        0x0030_a223, // 12: sw      x3, 4(x1)
        0x0000_100f, // 16: fence.i
        0xff1f_f06f, // 20: jal     x0, -16
        REPLACEMENT, // 24: data, never executed here
    ]);
    retire(&mut machine, 2);
    assert_eq!(machine.debug_state().x[2], 1);
    retire(&mut machine, 4);
    assert_eq!(machine.debug_state().pc, DRAM_BASE + 4);
    assert_eq!(
        machine.inspect(DRAM_BASE + 4, 4),
        Some(&REPLACEMENT.to_le_bytes()[..])
    );
    retire(&mut machine, 1);
    let state = machine.debug_state();
    assert_eq!(state.x[2], 2);
    assert_eq!(state.pc, DRAM_BASE + 8);
}

#[test]
fn reserved_fence_function_codes_trap() {
    for funct3 in 2..8 {
        let word = (funct3 << 12) | 0x0f;
        fault(&mut load_probe([word]), 2, u64::from(word));
    }
}
