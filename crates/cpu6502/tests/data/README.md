# 自包含测试数据

`opcodes.txt` 是本项目依据 MOS 6500-50A 附录 B 独立手写的规格；不能从 CPU 解码器生成，以免共享实现错误。

`singlestep-decimal.txt` 是 [SingleStepTests/65x02](https://github.com/SingleStepTests/65x02) 提交 `2f6980a2d95757486c7bee24355c360e40e2a224` 中 `6502/v1/69.json` 和 `6502/v1/e9.json` 的 **8 条选定 NMOS 十进制用例**，各 4 条。按文件原始顺序选择 D=1 且 A 或操作数包含无效 BCD 数字的前 4 条，保留输入 A／操作数／C、输出 A／NVZC 以及用于定位记录的初始 PC。测试重新安置指令，I 固定为 1、D 固定为 1；其余存储标志输出由 NVZC 与 I/D 合成。

适用许可证 MIT，原文保存在 [LICENSE-SingleStepTests](LICENSE-SingleStepTests)。没有采用 `nes6502`、`65c02` 数据，也没有复制模拟器实现。

运行：`cargo test -p hesper-cpu6502 --test arithmetic selected_nmos_decimal_vectors`。同时包含在 debug／release workspace 测试中，无需联网。验证所选输入的寄存器结果及立即数指令 2 周期；**不比较原数据完整总线序列，也不表示通过两个 JSON 文件或整个套件**。更大范围的外部一致性验证仍属 M2。
