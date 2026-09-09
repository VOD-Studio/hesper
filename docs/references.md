# 行为依据与外部测试边界

本轮查阅日期：2026-09-09。核心和本地测试自行实现；未复制外部模拟器代码，仅引入下述有来源与许可证的 8 条测试样例，未引入 ROM。

## MOS 原始手册

[MOS Technology, MCS6500 Microcomputer Family Programming Manual, 6500-50A，1976 年第二版扫描件](https://www.manuallib.com/download/2023-10-16/MOS%206500%20Microcomputer%20Family%20Programming%20Manual.pdf)。便于核对的同书文字转录：

- [第 5 章](https://lbaeza.neocities.org/mcs6500/6500_ch05)：小端、取指、相对分支的有符号偏移与跨页周期。
- [第 7 章](https://lbaeza.neocities.org/mcs6500/6500_ch07)：索引寄存器操作与 CPX。
- [第 8 章](https://lbaeza.neocities.org/mcs6500/6500_ch08)：栈页、SP 增减、JSR/RTS/PHA/PLA/TXS。例 8.3 中 JSR 先读目标低字节、压入返回地址，再读目标高字节；这影响栈与代码重叠时的结果。
- [第 9 章](https://lbaeza.neocities.org/mcs6500/6500_ch09)：§9.2–9.3 的复位序列和软件初始化；§9.8.1 的间接 JMP；§9.4–9.7 的 IRQ/RTI 与 §9.9–9.11 的 NMI/BRK。RESET 用于取向量并置 I，不替程序初始化通用寄存器、栈或 D。
- [附录 B](https://lbaeza.neocities.org/mcs6500/6500_appb)：本轮 opcode、长度、标志和周期表，特别是 `STA abs,X` 固定 5 周期，以及分支 2／3／4 周期。

旧手册表格和转录不应被理解为涵盖所有芯片修订细节。间接 JMP 页尾行为另用下面的原始测试数据交叉核对；RESET 省略哪些总线访问、调试状态如何表示 B 等具体采用方式见 [architecture.md](architecture.md)。

## 原始测试与晶体管模型

| 项目 | 本轮用途 | 尚未完成 |
| --- | --- | --- |
| [Klaus Dormann 功能测试](https://github.com/Klaus2m5/6502_65C02_functional_tests) | 查阅说明及 `6502_functional_test.a65`：确认 NMOS 官方指令范围及十进制测试限制；源码许可为 GPL-3.0-or-later | 未接入、未运行任何该套件用例；后续另行固定提交和运行配置 |
| [SingleStepTests/65x02](https://github.com/SingleStepTests/65x02) | 核对一个间接 JMP 页尾样例，执行 8 条固定来源十进制样例；[许可证 MIT](https://github.com/SingleStepTests/65x02/blob/2f6980a2d95757486c7bee24355c360e40e2a224/LICENSE) | 未接入完整 JSON runner，未通过整个套件；不使用 `nes6502` 或任何 `65c02` 目录 |
| [Visual6502](https://github.com/trebonian/visual6502) | 查阅项目说明及原始 NMOS 中断研究笔记，作为时序交叉参考 | 未运行模型，未验证引脚／总线时序；后续使用其代码或数据前核对对应许可证 |

SingleStepTests 阅读固定于提交 `2f6980a2d95757486c7bee24355c360e40e2a224`，文件 [`6502/v1/6c.json`](https://github.com/SingleStepTests/65x02/blob/2f6980a2d95757486c7bee24355c360e40e2a224/6502/v1/6c.json)。临时读取后检查名为 `6c ff 70`、初始 PC=`$2887` 的样例：指针 `$70FF` 的低字节为 `$9D`，高字节来自 `$7000` 的 `$98`，结果 PC=`$989D`，数据列出 5 次读取。这是**资料核对，不是 Hesper 执行外部测试的通过记录**；本地边界测试使用独立编写的地址和预期值。

今后引入外部数据必须固定完整提交、变种、数据格式版本及运行配置，保存来源和许可证，写明资源取得方式和执行预算。结果应分开报告寄存器／内存、周期数量、总线访问序列；跑过部分用例不等于通过整个套件。

## Rust 工程资料

- [Cargo workspace 官方文档](https://doc.rust-lang.org/cargo/reference/workspaces.html)：虚拟 workspace、成员、共享包字段和 lint。
- [Clippy 官方用法](https://doc.rust-lang.org/stable/clippy/usage.html)：workspace 全目标检查及 `-D warnings`。

本项目许可证尚待所有者确认，引用资料或未来测试数据的许可证不等于已为 Hesper 选择许可证。

## M1 新增依据与验证范围

十进制行为优先对照 MOS 手册第 2 章，并核对 [Bruce Clark 的原始十进制测试源码](https://github.com/Klaus2m5/6502_65C02_functional_tests/blob/7954e2dbb49c469ea286070bf46cdd71aeb29e4b/6502_decimal_test.a65)，固定提交 `7954e2dbb49c469ea286070bf46cdd71aeb29e4b`。该文件自述 public domain；仓库其他文件的 GPL 不能代替逐文件核对。采用 `cputype=0` 的 NMOS 规则：ADC 的 N/V 与 Z 来源阶段不同，SBC 的四个算术标志取二进制结果。本轮未组装或执行该外部汇编程序；本地穷举及手写边界测试均由本项目实现。

M1 新增 8 条固定来源的 SingleStepTests 十进制输入／结果测试，选择方法、提交、许可证、字段与运行命令见 [测试数据说明](../crates/cpu6502/tests/data/README.md)。这是有界的选定用例结果／周期验证，尚未通过整个外部套件或完整总线序列。

中断先依据 MOS 手册第 9 章及硬件手册附录 A 的 [BRK/IRQ 与 RTI 访问表转录](https://xotmatrix.github.io/6502/6502-single-cycle-execution.html)。旧版 BRK 文字未完整列明 I 的变化，另核对 Visual6502 的 [BRK 与 B 位研究](https://www.nesdev.org/wiki/Visual6502wiki/6502_BRK_and_B_bit) 及 [中断时序研究](https://www.nesdev.org/wiki/Visual6502wiki/6502_Timing_of_Interrupt_Handling)（原 Visual6502 NMOS 研究的镜像，不采用 NES 设备行为）：BRK/PHP 与 IRQ/NMI 的 B 栈映像不同，I 不是在所有指令的同一相位更新，RTI 与 CLI 的轮询时机不同。笔记含待验证观察，本轮没有运行其晶体管模型。

[Avery Lee 的 Altirra Hardware Reference Manual](https://www.virtualdub.org/downloads/Altirra%20Hardware%20Reference%20Manual.pdf)，第 3 章 CPU，Interrupt timing／Setting the I flag with an interrupt pending／Overlapping interrupts，进一步讨论连续改写 I、分支、IRQ/NMI 重叠的时间窗口。特别是它对 PLP 后接设置 I 的指令组合有更细区别；当前统一使用指令前 I 的模型没有复现这些相位差异。将这些作为 M2 微操作与外部时序验证的具体用例，不声称本轮完成逐周期兼容。
