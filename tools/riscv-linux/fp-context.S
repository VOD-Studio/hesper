/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/* Standalone Linux RV64 U-mode probe: no libc, startup code, or FP globals. */
    .option norelax
    .equ SYS_CLOSE, 57
    .equ SYS_PIPE2, 59
    .equ SYS_READ, 63
    .equ SYS_WRITE, 64
    .equ SYS_EXIT, 93
    .equ SYS_CLONE, 220
    .equ SYS_WAIT4, 260
    .equ SIGCHLD, 17
    .equ AT_HWCAP, 16
    .equ HWCAP_FD, (1 << 5) | (1 << 3) /* F and D bits in Linux AT_HWCAP */

    .section .rodata
pass_line:
    .ascii "FP_CTX_OK:8\n"
    .equ pass_length, . - pass_line
token:
    .byte 0x5a

    .section .text
    .globl _start
    .type _start, @function
_start:
    ld    t0, 0(sp)
    slli  t0, t0, 3
    add   t0, t0, sp
    addi  t0, t0, 16                /* envp after argc, argv and its NULL */
.Lenv:
    ld    t1, 0(t0)
    addi  t0, t0, 8
    bnez  t1, .Lenv
    li    t2, AT_HWCAP
.Lauxv:
    ld    t1, 0(t0)
    beqz  t1, .Lfail
    addi  t0, t0, 16
    bne   t1, t2, .Lauxv
    ld    t1, -8(t0)
    andi  t1, t1, HWCAP_FD
    li    t2, HWCAP_FD
    bne   t1, t2, .Lfail

    la    a0, to_parent
    li    a1, 0
    li    a7, SYS_PIPE2
    ecall
    bnez  a0, .Lfail
    la    a0, to_child
    li    a1, 0
    li    a7, SYS_PIPE2
    ecall
    bnez  a0, .Lfail

    li    a0, SIGCHLD
    li    a1, 0
    li    a2, 0
    li    a3, 0
    li    a4, 0
    li    a7, SYS_CLONE
    ecall
    bltz  a0, .Lfail
    mv    s5, a0                    /* parent holds child PID; child holds zero */
    la    t0, to_parent
    la    t1, to_child
    beqz  s5, .Lchild_setup
    li    s0, 0x3ff0000000001000
    li    s1, (2 << 5) | 5           /* RDN, OF | NX */
    j     .Lsetup
.Lchild_setup:
    mv    t2, t0
    mv    t0, t1
    mv    t1, t2
    li    s0, 0x4000000000002000
    li    s1, (3 << 5) | 10          /* RUP, DZ | UF */
.Lsetup:
    lw    s3, 0(t0)                 /* read from peer */
    lw    s4, 4(t1)                 /* write to peer */
    lw    s6, 0(t1)
    lw    a0, 4(t0)
    li    a7, SYS_CLOSE
    ecall
    bnez  a0, .Lfail
    mv    a0, s6
    li    a7, SYS_CLOSE
    ecall
    bnez  a0, .Lfail

    mv    t0, s0
    .irp n,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    fmv.d.x f\n, t0
    addi  t0, t0, 1
    .endr
    fscsr s1
    li    s2, 8
    bnez  s5, .Lround
    /* Parent cannot check until the child has installed its own FP state. */
    jal   .Lsignal

.Lround:
    mv    a0, s3
    la    a1, received
    li    a2, 1
    li    a7, SYS_READ
    ecall
    li    t0, 1
    bne   a0, t0, .Lfail
    la    t0, received
    lbu   t0, 0(t0)
    li    t1, 0x5a
    bne   t0, t1, .Lfail

    /* Nothing touches the floating-point state before these reads. */
    frcsr t0
    bne   t0, s1, .Lfail
    mv    t0, s0
    .irp n,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    fmv.x.d t1, f\n
    bne   t1, t0, .Lfail
    addi  t0, t0, 1
    .endr

    /* Do exact arithmetic in both precisions using the live frm and flags. */
    li    t0, 0x3f800000            /* 1.0f */
    fmv.w.x f0, t0
    li    t0, 0x40000000            /* 2.0f */
    fmv.w.x f1, t0
    fadd.s f0, f0, f1, dyn
    fmv.x.w t0, f0
    li    t1, 0x40400000            /* 3.0f */
    bne   t0, t1, .Lfail
    li    t0, 0x3ff8000000000000    /* 1.5 */
    fmv.d.x f0, t0
    li    t0, 0x4000000000000000    /* 2.0 */
    fmv.d.x f1, t0
    fmul.d f0, f0, f1, dyn
    fmv.x.d t0, f0
    li    t1, 0x4008000000000000    /* 3.0 */
    bne   t0, t1, .Lfail
    frcsr t0
    bne   t0, s1, .Lfail
    fmv.d.x f0, s0
    addi  t0, s0, 1
    fmv.d.x f1, t0

    addi  s2, s2, -1
    bnez  s2, .Lcontinue
    beqz  s5, .Lsuccess            /* child exits after checking the last token */
    jal   .Lsignal
    mv    a0, s5
    la    a1, wait_status
    li    a2, 0
    li    a3, 0
    li    a7, SYS_WAIT4
    ecall
    bne   a0, s5, .Lfail
    la    t0, wait_status
    lw    t0, 0(t0)
    bnez  t0, .Lfail

    li    a0, 1
    la    a1, pass_line
    li    a2, pass_length
    li    a7, SYS_WRITE
    ecall
    li    t0, pass_length
    bne   a0, t0, .Lfail
.Lsuccess:
    li    a0, 0
    j     .Lexit
.Lcontinue:
    jal   .Lsignal
    j     .Lround
.Lsignal:
    mv    a0, s4
    la    a1, token
    li    a2, 1
    li    a7, SYS_WRITE
    ecall
    li    t0, 1
    bne   a0, t0, .Lfail
    ret
.Lfail:
    li    a0, 1
.Lexit:
    li    a7, SYS_EXIT
    ecall
    unimp
    .size _start, . - _start

    .section .bss
    .balign 4
to_parent:
    .skip 8
to_child:
    .skip 8
wait_status:
    .skip 4
received:
    .skip 1
