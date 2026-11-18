/* Instruction bodies from riscv-tests 3df703cd52d5f0d8224e3c50bf03c6cb70efff98:
 * rv64ui-p-add case 7, rv64ui-p-lw case 7, rv64um-p-divw cases 7 and 8.
 * Replayed in M-mode without the upstream privileged startup or tohost path.
 * Provenance and license: docs/verification.md, RISC-V Linux L2b.
 */
.option norvc
.option norelax
.text
.globl _start, probe_end, signature
_start:
    lla x10, signature
    li x1, -1 << 31
    li x2, -32768
    add x3, x1, x2
    sd x3, 0(x10)

    lla x4, tdat4
    lw x5, -8(x4)
    sd x5, 8(x10)

    li x2, -1
    divw x6, x1, x2
    sd x6, 16(x10)
    divw x6, x1, x0
    sd x6, 24(x10)
probe_end:
    j probe_end
.balign 8
tdat:
    .word 0x00ff00ff, 0xff00ff00, 0x0ff00ff0
tdat4:
    .word 0xf00ff00f
signature:
    .zero 32
