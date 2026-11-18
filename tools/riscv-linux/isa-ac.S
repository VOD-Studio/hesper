/* riscv-tests 3df703cd52d5f0d8224e3c50bf03c6cb70efff98:
 * rv64ua-p-amoadd_w #2/#3 and rv64uc-p-rvc #6/#7 (independent data copies).
 * Replayed without the upstream privileged startup or tohost path.
 */
.option norvc
.option norelax
.text
.globl _start, probe_end, signature
_start:
    lla x10, signature
    lla a3, atomic_data
    li a1, -2048
    amoadd.w.aqrl a4, a1, (a3)
    lw a5, 0(a3)
    sd a4, 0(x10)
    sd a5, 8(x10)

    mv x5, x10
    lla a1, word_data
    .option push
    .option rvc
    c.lw a0, 4(a1)
    .option pop
    addi a0, a0, 1
    .option push
    .option rvc
    c.sw a0, 4(a1)
    c.lw a2, 4(a1)
    .option pop
    sd a2, 16(x5)

    lla a1, double_data
    .option push
    .option rvc
    c.ld a0, 0(a1)
    .option pop
    addi a0, a0, 1
    .option push
    .option rvc
    c.sd a0, 0(a1)
    c.ld a2, 0(a1)
    .option pop
    sd a2, 24(x5)
probe_end:
    j probe_end
.balign 8
atomic_data:
    .word 0x80000000
word_data:
    .word 0, 0xfedcba98
.balign 8
double_data:
    .dword 0xfedcba9976543210
signature:
    .zero 32
signature_end:
