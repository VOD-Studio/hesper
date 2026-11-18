//! Native reference for the lightweight JS/Wasm checks; no guest images.
use hesper_riscv_linux::{DRAM_BASE, Machine};

fn with_program<const N: usize>(words: [u32; N]) -> Machine {
    let mut machine = Machine::new();
    machine
        .load(DRAM_BASE, words.map(u32::to_le_bytes).as_flattened())
        .unwrap();
    machine
}

fn main() {
    let integer = [0x0070_0093, 0x0000_0117, 0x0011_2e23, 0x0010_8013];
    let mut machine = with_program(integer);
    assert_eq!(machine.run(4).spent, 4);
    let state = machine.debug_state();
    let signature = u32::from_le_bytes(
        machine
            .inspect(DRAM_BASE + 32, 4)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    println!(
        "{{\"integer\":{{\"program\":{integer:?},\"pc\":\"{}\",\"retired\":\"{}\",\"time\":\"{}\",\"x0\":\"{}\",\"x1\":\"{}\",\"x2\":\"{}\",\"signature\":{signature}}},",
        state.pc, state.retired, state.time, state.x[0], state.x[1], state.x[2]
    );

    let uart = [
        0x1000_00b7, // lui x1,0x10000
        0x0800_0113,
        0x0020_81a3, // set DLAB
        0x0010_0113,
        0x0020_8023, // divisor 1
        0x0030_0113,
        0x0020_81a3, // 8N1
        0x0050_c203,
        0x0012_7213,
        0xfe02_0ce3, // poll data-ready
        0x0000_c283,
        0x0012_8293,
        0x0050_8023, // RBR + 1 -> THR
        0x1050_0073, // WFI until TX completes; no IRQ enabled
    ];
    let mut machine = with_program(uart);
    let before = machine.run(128);
    let input = [0xc3];
    assert_eq!(machine.serial_input(&input), 1);
    let after = machine.run(4096);
    let mut output = [0];
    assert_eq!(machine.serial_output(&mut output), 1);
    let state = machine.debug_state();
    println!(
        "\"uart\":{{\"program\":{uart:?},\"input\":{input:?},\"output\":{output:?},\"at\":\"{}\",\"spent\":{},\"pc\":\"{}\",\"retired\":\"{}\",\"time\":\"{}\"}}}}",
        before.time, after.spent, state.pc, state.retired, state.time
    );
}
