# M0 opcode 清单

以下 **25 个 opcode 均已实现，并有本地自动化测试**；“已测试”仅指仓库自包含用例，不表示通过完整外部 CPU 测试套件。测试名均位于 [conformance.rs](../crates/cpu6502/tests/conformance.rs)，实际检查记录见 [verification.md](verification.md)。

长度单位为字节，周期为每条指令。标志列列出可能改变的位，`—` 表示全部保留。所有条目另由 `every_supported_opcode_preserves_unaffected_flags_when_set_or_clear` 验证未受影响标志在置位／清零时的保留。

| opcode | 指令 | 寻址 | 长度 | 周期 | 标志 | 实现 | 已测试：主要用例 |
| --- | --- | --- | ---: | --- | --- | --- | --- |
| A9 | LDA | 立即数 | 2 | 2 | N Z | 是 | `lda_modes_and_ldx_set_nz_and_preserve_other_flags` |
| A5 | LDA | 零页 | 2 | 3 | N Z | 是 | 同上 |
| AD | LDA | 绝对 | 3 | 4 | N Z | 是 | 同上；`operand_and_opcode_fetches_wrap_at_ffff` |
| A2 | LDX | 立即数 | 2 | 2 | N Z | 是 | `lda_modes_and_ldx_set_nz_and_preserve_other_flags` |
| 85 | STA | 零页 | 2 | 3 | — | 是 | `sta_modes_preserve_flags_and_absolute_x_always_costs_five_cycles` |
| 8D | STA | 绝对 | 3 | 4 | — | 是 | 同上；`step_trace_keeps_fetched_opcode_even_if_instruction_overwrites_itself` |
| 9D | STA | 绝对 X | 3 | 5，跨页不再加 | — | 是 | `sta_modes_preserve_flags_and_absolute_x_always_costs_five_cycles`：不跨页、跨页、16 位回绕 |
| AA | TAX | 隐含 | 1 | 2 | N Z | 是 | `tax_txa_set_nz_but_txs_preserves_all_flags` |
| 8A | TXA | 隐含 | 1 | 2 | N Z | 是 | 同上 |
| 9A | TXS | 隐含 | 1 | 2 | — | 是 | 同上 |
| E8 | INX | 隐含 | 1 | 2 | N Z | 是 | `inx_dex_wrap_and_update_only_nz` |
| CA | DEX | 隐含 | 1 | 2 | N Z | 是 | 同上 |
| E0 | CPX | 立即数 | 2 | 2 | N Z C | 是 | `cpx_is_unsigned_compare_with_wrapped_subtraction_nz` |
| D0 | BNE | 相对 | 2 | 2／3／4 | — | 是 | `beq_bne_signed_offsets_pages_and_address_wrap` |
| F0 | BEQ | 相对 | 2 | 2／3／4 | — | 是 | 同上 |
| 4C | JMP | 绝对 | 3 | 3 | — | 是 | `jmp_absolute_reads_little_endian_and_preserves_flags` |
| 6C | JMP | 间接 | 3 | 5 | — | 是 | `jmp_indirect_uses_nmos_same_page_high_byte` |
| 20 | JSR | 绝对 | 3 | 6 | — | 是 | `jsr_rts_return_address_stack_order_and_sp_wrap`；`jsr_late_operand_fetch_handles_code_overlapping_stack` |
| 60 | RTS | 隐含 | 1 | 6 | — | 是 | 上述 JSR/RTS；`jsr_operand_fetch_and_rts_increment_wrap_program_counter` |
| 48 | PHA | 隐含 | 1 | 3 | — | 是 | `pha_pla_are_lifo_with_stack_page_and_sp_wrap` |
| 68 | PLA | 隐含 | 1 | 4 | N Z | 是 | 同上；`pla_sets_and_clears_nz_without_changing_other_flags` |
| 18 | CLC | 隐含 | 1 | 2 | C=0 | 是 | `clc_sec_cld_and_nop_change_only_their_documented_flags` |
| 38 | SEC | 隐含 | 1 | 2 | C=1 | 是 | 同上 |
| D8 | CLD | 隐含 | 1 | 2 | D=0 | 是 | 同上 |
| EA | NOP | 隐含 | 1 | 2 | — | 是 | 同上；`operand_and_opcode_fetches_wrap_at_ffff` |

周期及标志依据 [MOS 6500-50A 附录 B](https://lbaeza.neocities.org/mcs6500/6500_appb)。分支的 2／3／4 分别对应不跳、同页跳、跨页跳；页比较以操作数后的 PC 为基准。

## 未实现

**所有未出现在上表的字节均未实现**，共 231 个。包括官方 BRK `$00`、全部 ADC/SBC、其他官方指令及未列出的寻址方式、非官方 opcode 和 65C02 扩展。它们没有“指令行为测试通过”的状态；`unsupported_opcodes_including_brk_return_context_without_register_changes` 逐一验证其返回错误的契约。

M0 没有额外的 HALT opcode。RESET 是外部操作而非 opcode，单独测试向量、7 周期、状态保留与 SP 递减；宿主演示的完整执行和停止条件由 [demo.rs](../crates/cli/tests/demo.rs) 验证。
