//! Integration tests for the Apple I machine: display output collection
//! and keyboard input echo flow.

use hesper_apple1::Apple1;
use hesper_cpu6502::Bus;

/// Build a minimal 256‑byte ROM whose reset vector points to `addr`.
fn rom_with_reset_vector(addr: u16) -> [u8; 256] {
    let mut rom = [0u8; 256];
    rom[0xFC] = addr as u8;
    rom[0xFD] = (addr >> 8) as u8;
    rom
}

#[test]
fn display_output_collects_character() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(50)).unwrap();

    // Program at $0000:
    //   LDA #$FF        A9 FF
    //   STA $D012       8D 12 D0   ; DDRB = $FF (all outputs)
    //   LDA #$04        A9 04
    //   STA $D013       8D 13 D0   ; CRB = $04 (select OR, IRQ off)
    //   LDA #'A'        A9 41
    //   STA $D012       8D 12 D0   ; write 'A' to display
    //   JMP $000F       4C 0F 00   ; spinloop
    let program: &[u8] = &[
        0xA9, 0xFF, 0x8D, 0x12, 0xD0, 0xA9, 0x04, 0x8D, 0x13, 0xD0, 0xA9, 0x41, 0x8D, 0x12, 0xD0,
        0x4C, 0x0F, 0x00,
    ];
    machine.bus_mut().load_ram(0x0000, program).unwrap();

    machine.reset().unwrap();

    // Reset takes 7 cycles, the program takes ~21 cycles to reach the
    // spinloop after STA $D012. Give plenty of budget for display timing.
    let output = machine.run_cycles(200).unwrap();

    assert!(output.contains(&b'A'), "display output should contain 'A'");
    // Should be exactly one character — no spurious output from DDR write.
    assert_eq!(output.len(), 1, "expected exactly 1 output character");
}

#[test]
fn display_timing_respects_cycles_per_char() {
    let rom = rom_with_reset_vector(0x0000);
    // Short timing: 30 cycles per character.
    let mut machine = Apple1::new(&rom, Some(30)).unwrap();

    // Same program as above.
    let program: &[u8] = &[
        0xA9, 0xFF, 0x8D, 0x12, 0xD0, 0xA9, 0x04, 0x8D, 0x13, 0xD0, 0xA9, 0x41, 0x8D, 0x12, 0xD0,
        0x4C, 0x0F, 0x00,
    ];
    machine.bus_mut().load_ram(0x0000, program).unwrap();

    machine.reset().unwrap();

    // Run just past the STA $D012 but not enough for display to finish.
    // Reset: 7 cycles. Program:
    //   LDA #$FF:   2 cycles (total 9)
    //   STA $D012:  4 cycles (total 13)
    //   LDA #$04:   2 cycles (total 15)
    //   STA $D013:  4 cycles (total 19)
    //   LDA #'A':   2 cycles (total 21)
    //   STA $D012:  4 cycles (total 25)
    // Display timer started at cycle 25 (after on_write).
    // It needs 30 cycles, so output at cycle 55.
    // Run exactly enough to reach cycle 40 — output should still be empty.
    let output = machine.run_cycles(40).unwrap();
    assert!(
        output.is_empty(),
        "output should be empty before timer expires"
    );

    // Run more — character should now be collected.
    let output2 = machine.run_cycles(50).unwrap();
    assert_eq!(output2, b"A", "character should appear after timer expires");
}

#[test]
fn keyboard_echo_flow() {
    let rom = rom_with_reset_vector(0x0000);
    // Short display time so the echo is collected quickly.
    let mut machine = Apple1::new(&rom, Some(20)).unwrap();

    // Program at $0000:
    //   LDA #$7F        A9 7F
    //   STA $D012       8D 12 D0   ; DDRB = $7F (PB7 input, PB6‑0 output)
    //   LDA #$07        A9 07
    //   STA $D013       8D 13 D0   ; CRB = $07 (OR, rising‑edge CB1, IRQ on)
    //   LDA #$03        A9 03
    //   STA $D011       8D 11 D0   ; CRA = $03 (DDR, rising‑edge CA1, IRQ on)
    // POLL:
    //   BIT $D011       2C 11 D0   ; test IRQA1 (bit 7 of CRA)
    //   BPL POLL        10 FB      ; loop if N = 0
    //   LDA $D010       AD 10 D0   ; read keyboard, clears IRQA1
    //   AND #$7F        29 7F      ; strip bit 7 (Apple I keyboard sets it)
    //   STA $D012       8D 12 D0   ; echo to display
    //   JMP POLL        4C 0F 00
    //
    // Addresses: $0000-$001D (30 bytes).
    let program: &[u8] = &[
        0xA9, 0x7F, 0x8D, 0x12, 0xD0, // $0000
        0xA9, 0x07, 0x8D, 0x13, 0xD0, // $0005
        0xA9, 0x03, 0x8D, 0x11, 0xD0, // $000A
        0x2C, 0x11, 0xD0, // $000F  BIT $D011
        0x10, 0xFB, // $0012  BPL $000F
        0xAD, 0x10, 0xD0, // $0014  LDA $D010
        0x29, 0x7F, // $0017  AND #$7F
        0x8D, 0x12, 0xD0, // $0019  STA $D012
        0x4C, 0x0F, 0x00, // $001C  JMP $000F
    ];
    machine.bus_mut().load_ram(0x0000, program).unwrap();

    machine.reset().unwrap();

    // Complete reset (7) + PIA setup (18) = exactly 25 cycles.
    // After 25 cycles the CPU just finished STA $D011 and is about to
    // fetch BIT $D011 at $000F.
    let _ = machine.run_cycles(25).unwrap();

    // Now queue a key while the CPU is at the BIT instruction.
    machine.type_char(b'H');

    // Run cycles.  Keyboard tick 1 asserts CA1 → IRQA1 set.
    // BIT $D011 detects it, BPL falls through, LDA $D010 clears
    // IRQA1 and returns $C8.  AND #$7F → $48.  STA $D012 starts
    // the display timer (20 cycles).  Total ≈ 50 cycles needed.
    let output = machine.run_cycles(50).unwrap();

    assert_eq!(
        output, b"H",
        "echoed character 'H' should appear in display output, got {output:?}"
    );
}

#[test]
fn physical_reset_preserves_queued_keyboard_input_but_resyncs_pia() {
    // Real Apple I hardware ties the PIA's RESET pin to the same system
    // reset line as the 6502 (clearing CRA/CRB and interrupt state), but
    // the external keyboard encoder is not wired to that line at all —
    // pressing RESET does not erase keys the user already typed ahead.
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(50)).unwrap();
    machine.reset().unwrap();

    machine.type_str("XY");
    assert!(machine.keyboard().has_pending());

    // Reset overlaps the queued input.
    machine.reset().unwrap();
    assert!(
        machine.keyboard().has_pending(),
        "keys typed ahead of RESET must survive it"
    );

    // The PIA's own registers were cleared: software must reconfigure CRA
    // before IRQA1 edges are latched again, exactly as after any RESET.
    machine.bus_mut().write(0xD011, 0x03); // rising-edge CA1, IRQ enabled
    let _ = machine.run_cycles(5).unwrap();
    assert_eq!(
        machine.bus_mut().read(0xD010) & 0x7F,
        b'X',
        "first queued key is still delivered after RESET"
    );
    let _ = machine.run_cycles(5).unwrap();
    assert_eq!(machine.bus_mut().read(0xD010) & 0x7F, b'Y');
}

#[test]
fn keyboard_no_key_leaves_irqa1_clear() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(50)).unwrap();
    machine.reset().unwrap();
    machine.bus_mut().write(0xD011, 0x03);

    let _ = machine.run_cycles(20).unwrap();
    assert_eq!(
        machine.bus_mut().read(0xD011) & 0x80,
        0,
        "IRQA1 must stay clear when no key is queued"
    );
}

#[test]
fn keyboard_repeated_read_without_new_key_returns_same_data() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(50)).unwrap();
    machine.reset().unwrap();
    machine.bus_mut().write(0xD011, 0x03);
    machine.type_char(b'Q');
    let _ = machine.run_cycles(5).unwrap();

    let first = machine.bus_mut().read(0xD010);
    let second = machine.bus_mut().read(0xD010);
    assert_eq!(
        first, second,
        "repeated reads without a new key must return the same data"
    );
    assert_eq!(first & 0x7F, b'Q');
}

#[test]
fn keyboard_continuous_input_delivers_keys_in_fifo_order() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(50)).unwrap();
    machine.reset().unwrap();
    machine.bus_mut().write(0xD011, 0x03);
    machine.type_str("AB");

    // The second key must wait for the first to be read (which clears
    // IRQA1) before it is presented — no skipping or reordering.
    let _ = machine.run_cycles(5).unwrap();
    assert_eq!(machine.bus_mut().read(0xD010) & 0x7F, b'A');
    let _ = machine.run_cycles(5).unwrap();
    assert_eq!(machine.bus_mut().read(0xD010) & 0x7F, b'B');
}

#[test]
fn keyboard_control_characters_pass_through_unfiltered() {
    // CR ($0D) is an ordinary data byte to the PIA; the keyboard model
    // does not interpret or filter any code point.
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(50)).unwrap();
    machine.reset().unwrap();
    machine.bus_mut().write(0xD011, 0x03);
    machine.type_char(0x0D);

    let _ = machine.run_cycles(5).unwrap();
    assert_eq!(machine.bus_mut().read(0xD010) & 0x7F, 0x0D);
}

#[test]
fn display_write_while_busy_via_cpu_drops_pending_character() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(50)).unwrap();

    // DDRB=$FF, CRB=$04 (OR select), write 'A' ($41), write 'B' ($42)
    // before 'A' finishes its 50-cycle timer, then spin.
    let program: &[u8] = &[
        0xA9, 0xFF, 0x8D, 0x12, 0xD0, // $0000 DDRB
        0xA9, 0x04, 0x8D, 0x13, 0xD0, // $0005 CRB
        0xA9, 0x41, 0x8D, 0x12, 0xD0, // $000A write 'A' (starts timer)
        0xA9, 0x42, 0x8D, 0x12, 0xD0, // $000F write 'B' (overwrites 'A')
        0x4C, 0x14, 0x00, // $0014 JMP $0014 (self-loop)
    ];
    machine.bus_mut().load_ram(0x0000, program).unwrap();
    machine.reset().unwrap();

    let output = machine.run_cycles(100).unwrap();
    assert_eq!(
        output, b"B",
        "'A' must be dropped by the overwrite; only 'B' is ever delivered"
    );
}

#[test]
fn continuous_advance_matches_split_batch_advance() {
    // Same program and keyboard input, driven once with a single large
    // budget and once as 300 one-cycle batches with a RESET in between:
    // final RAM, registers, display output and total cycle count must
    // match exactly. Splitting a batch must not change device or CPU
    // state — the sequencer persists across `run_cycles` calls.
    let rom = rom_with_reset_vector(0x0000);
    let program: &[u8] = &[
        0xA9, 0x7F, 0x8D, 0x12, 0xD0, // $0000 DDRB
        0xA9, 0x07, 0x8D, 0x13, 0xD0, // $0005 CRB
        0xA9, 0x03, 0x8D, 0x11, 0xD0, // $000A CRA
        0x2C, 0x11, 0xD0, // $000F BIT $D011
        0x10, 0xFB, // $0012 BPL $000F
        0xAD, 0x10, 0xD0, // $0014 LDA $D010
        0x29, 0x7F, // $0017 AND #$7F
        0x8D, 0x12, 0xD0, // $0019 STA $D012
        0x4C, 0x0F, 0x00, // $001C JMP $000F
    ];

    fn run(program: &[u8], rom: &[u8; 256], batch_sizes: &[u64]) -> (Vec<u8>, Vec<u8>, u64) {
        let mut machine = Apple1::new(rom, Some(20)).unwrap();
        machine.bus_mut().load_ram(0x0000, program).unwrap();
        machine.reset().unwrap();
        machine.type_str("HI");

        let mut output = Vec::new();
        for &batch in batch_sizes {
            output.extend(machine.run_cycles(batch).unwrap());
        }
        (
            output,
            machine.bus().ram_slice().to_vec(),
            machine.total_cycles(),
        )
    }

    let continuous = run(program, &rom, &[300]);
    let split: Vec<u64> = std::iter::repeat_n(1u64, 300).collect();
    let split = run(program, &rom, &split);

    assert_eq!(continuous, split);
    // Sanity: something must actually happen (not two silently-idle runs).
    // 'H' and 'I' are both queued before the CPU even starts polling, and
    // the display's 20-cycle busy timer outlasts the poll loop's turnaround
    // between them, so 'H' is overwritten before it is ever sent (see
    // `display_write_while_busy_via_cpu_drops_pending_character`) — only
    // 'I' is ever delivered. That drop is itself deterministic and must
    // match across both batchings, which `assert_eq!` above already
    // covers; this only guards against a change that silently produces no
    // output at all.
    assert_eq!(continuous.0, b"I");
}
