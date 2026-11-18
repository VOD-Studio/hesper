/* Loaded at 0x80200000 by the test host in place of the kernel entry. */
.section .text
.global _start
_start:
    li s0, 0x83000000          /* signature in otherwise unused DRAM */
    sd a0, 0(s0)
    sd a1, 8(s0)
    csrr t0, satp
    sd t0, 16(s0)

    li a6, 0                    /* SBI BASE: get specification version */
    li a7, 0x10
    ecall
    bnez a0, fail_base
    beqz a1, fail_base
    sd a1, 24(s0)

    la t0, trap
    csrw stvec, t0
    li t0, 0x220                /* STIE | SEIE */
    csrw sie, t0
    csrsi sstatus, 2
    rdtime a0
    addi a0, a0, 1024
    li a6, 0
    li a7, 0x54494d45          /* SBI TIME: set_timer */
    ecall
    bnez a0, fail_timer
1:
    wfi
    ld t0, 32(s0)
    beqz t0, 1b

    li t0, 0x0c000028          /* UART source priority */
    li t1, 1
    sw t1, 0(t0)
    li t0, 0x0c002080          /* S context enable */
    li t2, 1 << 10
    sw t2, 0(t0)
    li t0, 0x0c201000          /* S context threshold and claim */
    sw zero, 0(t0)
    li t0, 0x10000000
    sb t1, 1(t0)               /* UART IER: received data available */
    sd t1, 40(s0)              /* host sends one byte after this store */
2:
    wfi
    ld t0, 48(s0)
    beqz t0, 2b
    li t0, 2
    sd t0, 40(s0)              /* external trap returned to S mode */
3:
    wfi
    j 3b

.balign 4
trap:
    csrr t3, scause
    bgez t3, fail_trap
    slli t3, t3, 1
    srli t3, t3, 1
    li t4, 5
    beq t3, t4, timer
    li t4, 9
    bne t3, t4, fail_trap
    li t3, 0x0c201004
    lw t4, 0(t3)
    li t5, 10
    bne t4, t5, fail_claim
    li t4, 0x10000000
    lbu t6, 0(t4)
    addi t6, t6, 1
4:
    lbu t2, 5(t4)
    andi t2, t2, 32
    beqz t2, 4b
    sb t6, 0(t4)
    sd t6, 48(s0)
    sw t5, 0(t3)
    sret

timer:
    li a0, -1
    li a6, 0
    li a7, 0x54494d45
    ecall
    bnez a0, fail_timer
    li t3, 1
    sd t3, 32(s0)
    sret

fail_base:
    li t0, 1
    j fail
fail_timer:
    li t0, 2
    j fail
fail_trap:
    li t0, 3
    j fail
fail_claim:
    li t0, 4
fail:
    sd t0, 56(s0)
    j 3b
