/* Straight-line RV64F/D observations. Each case emits six little-endian u64s:
 * result bits, fflags, operands (a/b/c), and rm (0x70+frm for dynamic, 0xff if absent).
 * Single-precision FP results are observed with FMV.X.D to include the NaN box.
 */
.option norvc
.option norelax
.text
.globl _start, probe_end, signature
.set fd_cases, 0

.macro begin op, mode=0xff, dynamic=none
fd_case_\@_\op:
    csrwi fflags, 0
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, \mode
    .ifnc \dynamic,none
        csrwi frm, \dynamic
    .endif
.endm

.macro emit
    .set fd_cases, fd_cases + 1
    csrr t1, fflags
    sd t2, 0(s1)
    sd t1, 8(s1)
    sd a1, 16(s1)
    sd a2, 24(s1)
    sd a3, 32(s1)
    sd a4, 40(s1)
    addi s1, s1, 48
.endm

.macro arg x, f, precision, data
    lla t0, \data
    ld \x, 0(t0)
    .ifc \precision,s
        flw \f, 0(t0)
    .else
        fld \f, 0(t0)
    .endif
.endm

.macro binary op, precision, a, b, rounding=none, mode=0xff, dynamic=none
    begin \op, \mode, \dynamic
    arg a1, f0, \precision, \a
    arg a2, f1, \precision, \b
    .ifc \rounding,none
        \op f2, f0, f1
    .else
        \op f2, f0, f1, \rounding
    .endif
    fmv.x.d t2, f2
    emit
.endm

.macro unary op, precision, a, rounding, mode
    begin \op, \mode
    arg a1, f0, \precision, \a
    .ifc \rounding,none
        \op f2, f0
    .else
        \op f2, f0, \rounding
    .endif
    fmv.x.d t2, f2
    emit
.endm

.macro fused op, precision, a, b, c, rounding, mode
    begin \op, \mode
    arg a1, f0, \precision, \a
    arg a2, f1, \precision, \b
    arg a3, f2, \precision, \c
    \op f3, f0, f1, f2, \rounding
    fmv.x.d t2, f3
    emit
.endm

.macro compare op, precision, a, b
    begin \op
    arg a1, f0, \precision, \a
    arg a2, f1, \precision, \b
    \op t2, f0, f1
    emit
.endm

.macro classify precision, data
    begin fclass.\precision
    arg a1, f0, \precision, \data\()_\precision
    fclass.\precision t2, f0
    emit
.endm

.macro to_int op, precision, a, rounding, mode, dynamic=none
    begin \op, \mode, \dynamic
    arg a1, f0, \precision, \a
    \op t2, f0, \rounding
    emit
.endm

.macro from_int op, a, rounding, mode
    begin \op, \mode
    lla t0, \a
    ld a1, 0(t0)
    .ifc \rounding,none
        \op f2, a1
    .else
        \op f2, a1, \rounding
    .endif
    fmv.x.d t2, f2
    emit
.endm

.macro to_int_cases p, kind
    to_int fcvt.\kind\().\p, \p, fraction_\p, rmm, 4
    to_int fcvt.\kind\().\p, \p, qnan_\p, rne, 0
    to_int fcvt.\kind\().\p, \p, neg_inf_\p, rne, 0
.endm

.macro precision_cases p
    binary fadd.\p, \p, one_\p, half_ulp_\p, rne, 0
    binary fadd.\p, \p, one_\p, half_ulp_\p, rtz, 1
    binary fadd.\p, \p, one_\p, half_ulp_\p, rdn, 2
    binary fadd.\p, \p, one_\p, half_ulp_\p, rup, 3
    binary fadd.\p, \p, one_\p, half_ulp_\p, rmm, 4
    .irp r,0,1,2,3,4
        binary fadd.\p, \p, neg_one_\p, neg_half_ulp_\p, dyn, (0x70+\r), \r
        to_int fcvt.w.\p, \p, neg_fraction_\p, dyn, (0x70+\r), \r
    .endr
    binary fadd.\p, \p, inf_\p, neg_inf_\p, rne, 0
    binary fsub.\p, \p, one_\p, next_\p, rne, 0
    binary fsub.\p, \p, one_\p, one_\p, rdn, 2
    binary fmul.\p, \p, next_\p, prev2_\p, rne, 0
    binary fmul.\p, \p, max_\p, fraction_\p, rtz, 1
    binary fmul.\p, \p, min_sub_\p, half_\p, rmm, 4
    binary fdiv.\p, \p, one_\p, fraction_\p, rup, 3
    binary fdiv.\p, \p, one_\p, zero_\p, rne, 0
    binary fdiv.\p, \p, zero_\p, zero_\p, rne, 0
    unary fsqrt.\p, \p, fraction_\p, rdn, 2
    unary fsqrt.\p, \p, neg_one_\p, rne, 0
    unary fsqrt.\p, \p, neg_zero_\p, rne, 0
    fused fmadd.\p, \p, next_\p, prev2_\p, neg_one_\p, rne, 0
    fused fmsub.\p, \p, next_\p, prev2_\p, one_\p, rne, 0
    fused fnmsub.\p, \p, one_\p, one_\p, neg_half_ulp_\p, rup, 3
    fused fnmadd.\p, \p, one_\p, one_\p, half_ulp_\p, rdn, 2
    fused fmadd.\p, \p, inf_\p, zero_\p, qnan_\p, rne, 0
    binary fsgnj.\p, \p, snan_\p, neg_zero_\p
    binary fsgnjn.\p, \p, snan_\p, neg_zero_\p
    binary fsgnjx.\p, \p, snan_\p, neg_zero_\p
    binary fmin.\p, \p, zero_\p, neg_zero_\p
    binary fmax.\p, \p, neg_zero_\p, zero_\p
    binary fmin.\p, \p, snan_\p, one_\p
    binary fmax.\p, \p, one_\p, qnan_\p
    binary fmin.\p, \p, qnan_\p, snan_\p
    binary fmax.\p, \p, snan_\p, qnan_\p
    compare feq.\p, \p, zero_\p, neg_zero_\p
    compare feq.\p, \p, qnan_\p, one_\p
    compare feq.\p, \p, one_\p, snan_\p
    compare flt.\p, \p, neg_one_\p, min_sub_\p
    compare flt.\p, \p, qnan_\p, one_\p
    compare fle.\p, \p, one_\p, one_\p
    compare fle.\p, \p, one_\p, snan_\p
    .irp part,neg_inf,neg_one,neg_min_sub,neg_zero,zero,min_sub,one,inf,snan,qnan
        classify \p, \part
    .endr
    .irp kind,w,wu,l,lu
        to_int_cases \p, \kind
    .endr
.endm

_start:
    li t0, 0x2000
    csrs mstatus, t0
    lla s1, signature

    precision_cases s
    precision_cases d
    from_int fcvt.s.w, int_pattern, rup, 3
    from_int fcvt.s.wu, int_pattern, rup, 3
    from_int fcvt.s.l, int_pattern, rup, 3
    from_int fcvt.s.lu, int_pattern, rup, 3
    from_int fcvt.d.w, int_pattern, none, 0
    from_int fcvt.d.wu, int_pattern, none, 0
    from_int fcvt.d.l, int_pattern, rup, 3
    from_int fcvt.d.lu, int_pattern, rup, 3
    unary fcvt.s.d, d, one_plus_s_half_ulp_d, rne, 0
    unary fcvt.s.d, d, one_plus_s_half_ulp_d, rmm, 4
    unary fcvt.s.d, d, snan_d, rne, 0
    unary fcvt.d.s, s, min_sub_s, none, 0
    unary fcvt.d.s, s, snan_s, none, 0
    to_int fcvt.wu.d, d, u32_max_d, rne, 0
    to_int fcvt.wu.d, d, neg_fraction_d, rtz, 1

    begin fmv.w.x
    lla t0, int_pattern
    ld a1, 0(t0)
    fmv.w.x f0, a1
    fmv.x.d t2, f0
    emit
    begin fmv.x.w
    arg a1, f0, s, snan_neg_s
    fmv.x.w t2, f0
    emit
    begin fmv.d.x
    lla t0, snan_d
    ld a1, 0(t0)
    fmv.d.x f0, a1
    fmv.x.d t2, f0
    emit
    begin fmv.x.d
    arg a1, f0, d, snan_d
    fmv.x.d t2, f0
    emit

    begin fadd.s_unboxed, 0
    arg a1, f0, d, unboxed_s
    arg a2, f1, s, one_s
    fadd.s f2, f0, f1, rne
    fmv.x.d t2, f2
    emit
    begin fclass.s_unboxed
    arg a1, f0, d, unboxed_s
    fclass.s t2, f0
    emit
    begin fmv.x.w_unboxed
    arg a1, f0, d, unboxed_s
    fmv.x.w t2, f0
    emit

    begin flw
    arg a1, f0, s, snan_neg_s
    fmv.x.d t2, f0
    emit
    begin fld
    arg a1, f0, d, snan_d
    fmv.x.d t2, f0
    emit
    begin fsw
    arg a1, f0, d, unboxed_s
    lla s0, scratch
    sd zero, 8(s0)
    fsw f0, 12(s0)
    ld t2, 8(s0)
    emit
    begin fsd
    arg a1, f0, d, snan_d
    lla s0, scratch
    sd zero, 8(s0)
    fsd f0, 8(s0)
    ld t2, 8(s0)
    emit

    lla s0, scratch
    mv sp, s0
    begin c.fld
    arg a1, f0, d, min_sub_d
    sd a1, 0(s0)
    .option push
    .option rvc
    c.fld f8, 0(s0)
    .option pop
    fmv.x.d t2, f8
    emit
    begin c.fsd
    arg a1, f8, d, neg_one_d
    sd zero, 8(s0)
    .option push
    .option rvc
    c.fsd f8, 8(s0)
    .option pop
    ld t2, 8(s0)
    emit
    begin c.fldsp
    arg a1, f1, d, snan_d
    sd a1, 0(sp)
    .option push
    .option rvc
    c.fldsp f0, 0(sp)
    .option pop
    fmv.x.d t2, f0
    emit
    begin c.fsdsp
    arg a1, f0, d, neg_inf_d
    sd zero, 8(sp)
    .option push
    .option rvc
    c.fsdsp f0, 8(sp)
    .option pop
    ld t2, 8(sp)
    emit
probe_end:
    j probe_end

.balign 8
zero_s:          .dword 0x00000000
neg_zero_s:      .dword 0x80000000
one_s:           .dword 0x3f800000
neg_one_s:       .dword 0xbf800000
half_s:          .dword 0x3f000000
half_ulp_s:      .dword 0x33800000
neg_half_ulp_s:  .dword 0xb3800000
next_s:          .dword 0x3f800001
prev2_s:         .dword 0x3f7ffffe
fraction_s:      .dword 0x40200000
neg_fraction_s:  .dword 0xc0200000
min_sub_s:       .dword 0x00000001
neg_min_sub_s:   .dword 0x80000001
max_s:           .dword 0x7f7fffff
inf_s:           .dword 0x7f800000
neg_inf_s:       .dword 0xff800000
snan_s:          .dword 0x7f800123
snan_neg_s:      .dword 0xff800456
qnan_s:          .dword 0x7fc00123
zero_d:          .dword 0x0000000000000000
neg_zero_d:      .dword 0x8000000000000000
one_d:           .dword 0x3ff0000000000000
neg_one_d:       .dword 0xbff0000000000000
half_d:          .dword 0x3fe0000000000000
half_ulp_d:      .dword 0x3ca0000000000000
neg_half_ulp_d:  .dword 0xbca0000000000000
next_d:          .dword 0x3ff0000000000001
prev2_d:         .dword 0x3feffffffffffffe
fraction_d:      .dword 0x4004000000000000
neg_fraction_d:  .dword 0xc004000000000000
min_sub_d:       .dword 0x0000000000000001
neg_min_sub_d:   .dword 0x8000000000000001
max_d:           .dword 0x7fefffffffffffff
inf_d:           .dword 0x7ff0000000000000
neg_inf_d:       .dword 0xfff0000000000000
snan_d:          .dword 0x7ff0000000000123
qnan_d:          .dword 0x7ff8000000000123
u32_max_d:       .dword 0x41efffffffe00000
one_plus_s_half_ulp_d: .dword 0x3ff0000010000000
int_pattern:     .dword 0x80000000ffffffff
unboxed_s:       .dword 0xfffffffeff800456
scratch:         .zero 16
signature:
    /* Reserve one record per emitted observation; the counter is bound at assembly time. */
    .zero fd_cases * 48
signature_end:
