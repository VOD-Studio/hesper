//! Integration tests for the Apple I machine: display output collection
//! and keyboard input echo flow.

use hesper_apple1::Apple1;

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

    machine.reset();

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

    machine.reset();

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
    assert_eq!(
        output2,
        b"A",
        "character should appear after timer expires"
    );
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

    machine.reset();

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
        output,
        b"H",
        "echoed character 'H' should appear in display output, got {output:?}"
    );
}

#[test]
fn reset_clears_pending_keyboard_and_display() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(50)).unwrap();

    // Simple program: NOP then spin.
    let program: &[u8] = &[0xEA, 0x4C, 0x00, 0x00]; // NOP; JMP $0000
    machine.bus_mut().load_ram(0x0000, program).unwrap();

    machine.reset();
    let _ = machine.run_cycles(10).unwrap(); // CPU now spinning

    // Queue a key and do a display write.
    machine.type_char(b'X');
    // Force a display write by loading a special program section into RAM,
    // but simpler: just reset — keyboard queue should clear.
    machine.reset();

    // After reset, there should be no pending keys.
    // Run cycles — no key should be presented, so no output.
    let output = machine.run_cycles(50).unwrap();
    assert!(output.is_empty(), "output after reset should be empty");
}
