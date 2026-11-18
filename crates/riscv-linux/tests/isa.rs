use hesper_riscv_linux::{DRAM_BASE, Machine};

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
