; Original Hesper M0 demo. Documentation only; no assembler is needed to run.
; Exact bytes are embedded in crates/cli/src/lib.rs (DEMO_PROGRAM).
; Address  Bytes       Assembly
        .org $8000
start:  CLD             ; 8000 D8          explicitly select binary mode
        LDX #$FF        ; 8001 A2 FF       explicitly initialize the stack
        TXS             ; 8003 9A
        LDX #$00        ; 8004 A2 00
loop:   TXA             ; 8006 8A
        STA $0200,X     ; 8007 9D 00 02
        INX             ; 800A E8
        CPX #$0A        ; 800B E0 0A
        BNE loop        ; 800D D0 F7       $800F - 9 = $8006
done:   NOP             ; 800F EA          host stops BEFORE this instruction

        .org $FFFC
        .word start     ; FFFC 00 80       little-endian RESET vector

; Completion is a host PC check, not a CPU halt instruction.
; 4 setup instructions + 10 * 5 loop instructions = 54 instructions.
; 8 setup cycles + 9 * 14 + 13 loop cycles = 147 instruction cycles.
; RESET contributes 7 more: 154 total cycles. NOP at done is not executed.
