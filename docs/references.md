# 行为依据与外部测试边界

查阅日期：2026-09-09。核心和本地测试自行实现；未复制外部模拟器实现，未引入 Apple ROM。当前执行范围见下表；文末的 M1／M2.1 小节保留当时的验证边界，最新结果以 [verification.md](verification.md) 为准。

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
| [Klaus／Bruce Clark](https://github.com/Klaus2m5/6502_65C02_functional_tests/tree/7954e2dbb49c469ea286070bf46cdd71aeb29e4b) | 功能程序、全标志 decimal 全输入和显式 4 周期反馈延迟中断配置已运行；原文件许可分别为 GPL-3.0-or-later／public domain | 默认 0 延迟中断程序会触发上游注明的 NMOS BRK/NMI 陷阱；保留失败，不能声称全部配置通过 |
| [SingleStepTests/65x02](https://github.com/SingleStepTests/65x02/tree/2f6980a2d95757486c7bee24355c360e40e2a224/6502) | 全部 151 个官方 NMOS opcode／151 万用例已比较结果、周期数量和总线；MIT，哈希见测试数据清单 | 非官方 opcode 未实现，不使用 `nes6502` 或 `65c02` 数据，不宣称整个项目数据集通过 |
| [Visual6502](https://github.com/trebonian/visual6502/tree/d8ecc129b34e0eaf320e0400fcf33329475bdb1e) | revD 模型生成的 246 组 IRQ/NMI/RDY/SO 与 419 组物理 RESET 观察均已实际对照 CPU。模型按文件保留 MIT／CC BY-NC-SA 3.0 等声明，仅在缓存内使用 | 原模型重放与 CPU 对照分别执行；不推广为所有 NMOS 修订、全部指令×引脚组合或电气相位窗口认证 |

SingleStepTests 阅读固定于提交 `2f6980a2d95757486c7bee24355c360e40e2a224`，文件 [`6502/v1/6c.json`](https://github.com/SingleStepTests/65x02/blob/2f6980a2d95757486c7bee24355c360e40e2a224/6502/v1/6c.json)。临时读取后检查名为 `6c ff 70`、初始 PC=`$2887` 的样例：指针 `$70FF` 的低字节为 `$9D`，高字节来自 `$7000` 的 `$98`，结果 PC=`$989D`，数据列出 5 次读取。这是**资料核对，不是 Hesper 执行外部测试的通过记录**；本地边界测试使用独立编写的地址和预期值。

今后引入外部数据必须固定完整提交、变种、数据格式版本及运行配置，保存来源和许可证，写明资源取得方式和执行预算。结果应分开报告寄存器／内存、周期数量、总线访问序列；跑过部分用例不等于通过整个套件。

## Apple I

- Apple-1 Operation Manual (1976). Original schematics, memory map, Woz Monitor listing.
  <https://archive.org/details/Apple-1_Operation_Manual_1976_Apple_a>
- Apple-1 Operation Manual (1976), Section I "GETTING THE SYSTEM RUNNING" → "TEST PROGRAM"（原厂整机测试程序，字节 `A9 00 AA 20 EF FF E8 8A 4C 02 03`，见 [`apple1/05-manual-test-program.md`](apple1/05-manual-test-program.md)）。手册 OCR 全文有大量十六进制数字识别错误，逐字节核对时交叉比对了两份独立转录：
  <https://www.applefritter.com/content/programming-woz-monitor>（Mike Willegal 指出该程序位于手册第 2 页）、
  <https://obsolescence.wixsite.com/obsolescence/kim-uno-apple-1>（KIM Uno 项目文档给出同一字节序列）。
  已在 Hesper 上实测运行，输出的连续字节流与手册原文描述一致。
- MC6821 Peripheral Interface Adapter datasheet. PIA register model.
- Woz Monitor hex dump: 256 bytes at `$FF00‑$FFFF`, RESET vector at `$FFFC/D` points to `$FF00`.
  Reproducible assembly source at <https://github.com/jefftranter/6502/tree/master/asm/wozmon>
  Hex dump verified against <https://github.com/alangarf/apple-one/blob/master/roms/wozmon.hex>
- 内存映射（4 KiB RAM 于 `$0000`、PIA 于 `$D010‑D013`、256 B monitor ROM 于 `$FF00‑$FFFF`、跳线可重新分配 4 KiB 分区）、未映射地址浮空、PIA RESET 引脚与 6502 RES 共用同一系统复位信号、RESET 不清屏（复位线不接视频板移位寄存器存储，也不接键盘编码器）、14.31818 MHz 晶振四分频 NTSC 色副载频再分频出 1.023 MHz CPU 时钟：均来自 2026‑09‑10 网络检索的二级/技术爱好者资料（applefritter.com、apple2history.org、righto.com「Inside Apple‑1's shift register memory」、68kmla.org 等），未逐页核对手册或原理图扫描件原文。40×24 字符、CR 或写满行触发的硬件滚动、影子显示光标闪烁同样来自这批二级资料。这些结论已用于 `crates/apple1/src/lib.rs` 与 `Display` 的固定配置，但**不构成逐页原始手册/原理图核对**；后续如有条件应直接核对 archive.org 扫描件对应页面并在此处补充精确页码引用。
- Apple-1 Operation Manual (1976) OCR 全文（<https://archive.org/download/Apple-1_Operation_Manual_1976_Apple_a/Apple-1_Operation_Manual_1976_Apple_a_djvu.txt>）：
  - Section I / "KEYBOARD" 明确列出键盘上的两个按钮 **RESET** 与 **CLEAR SCREEN**，二者是独立硬件输入。据此纠正了此前"Apple I 没有独立清屏硬件输入"的错误结论：RESET 仍然不清屏，但清屏本身是真实的视频板输入，`Apple1::clear_screen` 就是它的功能级模型（一次性动作，不建模按钮脉冲宽度）。
  - Section III / "REFRESH" 说明每 65 个时钟中有 4 个用于 DRAM 刷新并在此期间抑制 Φ2。本项目**明确不建模**该刷新时钟：`Apple1` 的每个 cycle 都是一次真实 CPU 周期，没有被刷新占用的周期。这是本轮用户选定保留的近似，不是"RDY 未接线"这种接线缺口，也不能被当作整机逐周期准确。
- MC6821 寄存器参考（Leiden University，<https://www.eld.leidenuniv.nl/~moene/software/rdl/html/group__rdl__pia.html>）：控制寄存器第 2 位对**读写双方**选择 DDR 还是外设数据寄存器；只有读取外设数据寄存器（或硬件 RESET）清除该端口的中断标志。据此修正了 `Pia6821::read` 此前无论第 2 位为何都返回数据寄存器并清标志的 bug（写 `DDRB=$7F` 后读回 `$00`、写输出寄存器也清标志、键盘被非数据访问"消费"）。Motorola 原始数据表 PDF 镜像已定位但本轮提取失败，因此**没有**逐页核对原厂数据表全文。
- Woz Monitor ROM 的版权状态存在公开争议（多个爱好者站点将其视为事实上可自由转载，但 Apple 从未正式以开放许可证发布）；本项目不内嵌、不提交该镜像。用户可显式运行 Bun 工具下载公开转录并校验 SHA-256（`e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25`，对应上面 alangarf/apple-one 转录），默认保存到忽略缓存；普通构建和测试不下载。公开可下载不代表使用或再分发授权。资源获取与校验方式见 [`crates/apple1/tests/data/README.md`](../crates/apple1/tests/data/README.md)。
- MC6821 状态标志与中断使能位独立：CA1/CB1 满足有效沿时 CRA/CRB 第 7 位无条件置位，第 0 位（中断使能）只决定是否同时拉低外部 IRQ 输出脚，不影响标志本身——软件可在中断禁用状态下轮询标志，Woz Monitor 自身的 `BIT $D011`/`BPL` 键盘轮询正是如此。2026‑09‑10 网络检索确认该行为（数据表转录性技术资料），修复了 `Pia6821::set_ca1`/`set_cb1` 此前把标志置位错误地绑定在使能位上的 bug（细节与回归测试见 `crates/apple1/src/pia.rs`）。

## Rust 工程资料

- [Cargo workspace 官方文档](https://doc.rust-lang.org/cargo/reference/workspaces.html)：虚拟 workspace、成员、共享包字段和 lint。
- [Clippy 官方用法](https://doc.rust-lang.org/stable/clippy/usage.html)：workspace 全目标检查及 `-D warnings`。

本项目许可证尚待所有者确认，引用资料或未来测试数据的许可证不等于已为 Hesper 选择许可证。

## M1 新增依据与验证范围

十进制行为优先对照 MOS 手册第 2 章，并核对 [Bruce Clark 的原始十进制测试源码](https://github.com/Klaus2m5/6502_65C02_functional_tests/blob/7954e2dbb49c469ea286070bf46cdd71aeb29e4b/6502_decimal_test.a65)，固定提交 `7954e2dbb49c469ea286070bf46cdd71aeb29e4b`。该文件自述 public domain；仓库其他文件的 GPL 不能代替逐文件核对。采用 `cputype=0` 的 NMOS 规则：ADC 的 N/V 与 Z 来源阶段不同，SBC 的四个算术标志取二进制结果。本轮未组装或执行该外部汇编程序；本地穷举及手写边界测试均由本项目实现。

M1 新增 8 条固定来源的 SingleStepTests 十进制输入／结果测试，选择方法、提交、许可证、字段与运行命令见 [测试数据说明](../crates/cpu6502/tests/data/README.md)。这是有界的选定用例结果／周期验证，尚未通过整个外部套件或完整总线序列。

中断先依据 MOS 手册第 9 章及硬件手册附录 A 的 [BRK/IRQ 与 RTI 访问表转录](https://xotmatrix.github.io/6502/6502-single-cycle-execution.html)。旧版 BRK 文字未完整列明 I 的变化，另核对 Visual6502 的 [BRK 与 B 位研究](https://www.nesdev.org/wiki/Visual6502wiki/6502_BRK_and_B_bit) 及 [中断时序研究](https://www.nesdev.org/wiki/Visual6502wiki/6502_Timing_of_Interrupt_Handling)（原 Visual6502 NMOS 研究的镜像，不采用 NES 设备行为）：BRK/PHP 与 IRQ/NMI 的 B 栈映像不同，I 不是在所有指令的同一相位更新，RTI 与 CLI 的轮询时机不同。笔记含待验证观察，本轮没有运行其晶体管模型。

[Avery Lee 的 Altirra Hardware Reference Manual](https://www.virtualdub.org/downloads/Altirra%20Hardware%20Reference%20Manual.pdf)，第 3 章 CPU，Interrupt timing／Setting the I flag with an interrupt pending／Overlapping interrupts，进一步讨论连续改写 I、分支、IRQ/NMI 重叠的时间窗口。特别是它对 PLP 后接设置 I 的指令组合有更细区别；当前统一使用指令前 I 的模型没有复现这些相位差异。将这些作为 M2 微操作与外部时序验证的具体用例，不声称本轮完成逐周期兼容。

## M2.1 原始格式验证

沿用 SingleStepTests 固定提交 `2f6980a2d95757486c7bee24355c360e40e2a224`，读取 [6502 数据格式与测试流程](https://github.com/SingleStepTests/65x02/tree/2f6980a2d95757486c7bee24355c360e40e2a224/6502)。首批 21 个 opcode 各取前 32 条，按原始状态运行并比较寄存器／内存与周期数量；上游和夹具各自的 SHA-256 见 [清单](../crates/cpu6502/tests/data/singlestep/manifest.json)。MIT 许可沿用数据目录内原文。当前不比较完整总线访问序列，具体命令与选择规则见 [数据说明](../crates/cpu6502/tests/data/README.md)。


M2.3 总线阶段依据 MOS 6500-10A（1976 年第二版）附录 A 的 [单周期表转录](https://xotmatrix.com/6502/6502-single-cycle-execution.html)，与固定 SingleStepTests NMOS 原始事件逐项交叉比较。分支实际为先读操作数后 PC，再在跨页时读未修正高字节的地址；不把旧手册表中的简略地址描述当作完整相位模型。RESET 使用只读的栈入口，物理引脚时序仍属 M2.4。


M2.4 已实际运行 [Visual6502 revD 固定模型](https://github.com/trebonian/visual6502/tree/d8ecc129b34e0eaf320e0400fcf33329475bdb1e)，依据原始模型生成的引脚时间表交叉比较，而不复制模拟器指令实现。[Klaus 中断源码](https://github.com/Klaus2m5/6502_65C02_functional_tests/blob/7954e2dbb49c469ea286070bf46cdd71aeb29e4b/6502_interrupt_test.a65) 的 `nmi_trap` 明确提示并发 BRK/NMI 的 B 位断言可能在真实 NMOS 失败；记录零延迟失败与显式 4 周期反馈延迟通过的范围，见测试数据说明。


RDY 的读等待／写继续和 SO 的低有效边沿以 MOS 原始硬件资料为起点，细化窗口由固定 revD 模型的原创程序相位扫描交叉核对。V 写入延续规则属于对这些原始观察的数字归纳；[编程手册 §3.6](https://lbaeza.neocities.org/mcs6500/6500_ch03) 给出 SO 与 V 以及 ADC/BIT/CLV/PLP/RTI/SBC 的关系，不能单凭指令表推导完整相位优先级。

CI 的手动触发与步骤语法依据 [GitHub Actions 官方说明](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)，工具安装参数依据 [setup-python](https://github.com/actions/setup-python) 与 [setup-node](https://github.com/actions/setup-node) 的官方 README。配置存在不等于远程任务通过。

2026-09-10：物理 RESET 参考基线同时核对上述编程手册 §9.1～9.3，以及 [MOS 6500-10A Hardware Manual（1976），§1.4.1.2.11 RES](https://archive.org/details/mcs-6500-family-hardware-manual-1976-01/page/n47/mode/2up)。手册给出复位期间禁止写入、释放后向量启动、I 置位和软件初始化要求；启动表将向量前的部分地址列为 `?`／don't-care，没有规定所有断言相位的中间 PC/SP。数据中的同步延迟、过渡地址、暂态寄存器与 RDY／NMI 交叉结果是固定 revD 的数字观察，不提升为手册保证。内部时序／数据通路的具体机制仍需执行器建模验证，不能仅凭观察给硬件节点强加未经核对的解释。

2026-09-10：普通栈序列的 SP 提交时机继续用同一固定 revD 原模型核对。实验沿用 `tools/verify_visual6502.cjs` 的哈希校验、真实引导和总线采集，只在每周期第二次 `halfStep`（下降沿）后额外调用 `readSP()`，不修改模型或以 Hesper 生成预期。观察覆盖 JSR、BRK、PHA/PHP、PLA/PLP、RTS/RTI、IRQ/NMI、已同步 RESET 的七周期入口，以及各个相关读取阶段的 RDY 等待。具体 SP 序列、实际比较范围和与物理 RESET 的区别见 [验证记录](verification.md)。JSR 借用 SP 和等待期间仍可提交 SP 属于这些 revD 观察，不由指令最终结果表推导，也不推广到所有寄存器／芯片修订。

物理 RESET 实现仍以 MOS 原始手册 §9.1～9.3 为外部行为起点，细化同步及暂态来自上述固定模型的实际半周期观察。临时探针只读取未修改模型的 Reset0/C1x5Reset、时序、DL、ALU 及数据通路控制，不复制模拟器／网表进 CPU。独立改变 PC、A、栈数据、操作数和向量后，再将 37 组／2368 周期观察与实际 CPU 比较；预期不由 Hesper 生成。

关键数据通路观察：被抑制的压返回高字节阶段，PCL/DB 与 DL/ADH 重叠，外部高地址为 `DL & PCL`，存储的 PCH 则为 PCL；短脉冲释放可分别暴露两者。例如 PCL=`$47`、DL=`$5C` 时，SYNC 地址为 `$44FC`，随后 dummy read 为 `$47FC`。普通阶段留下的 ALU 输入、进位和反馈决定其他暂态，不能用默认 RAM 的 `$EA` 或指令族乘法公式代替这些传送。资料中的 don't-care 地址没有从固定预期中删除。
