# NMOS opcode 支持清单

**全部 151 个官方 opcode 已实现并通过自包含测试**（56 条指令），同时通过 M2 的 151 万条官方单步用例结果／周期／总线验证。IRQ/NMI 与引脚采样另有测试；非官方指令和物理 RESET 持续输入仍未实现。

下表逐项对应独立手写的 [测试规格](../crates/cpu6502/tests/data/opcodes.txt)，不从 CPU 解码器产生预期值。[official.rs](../crates/cpu6502/tests/official.rs) 中 `every_documented_opcode_has_independent_result_length_flags_and_cycle_expectations` 对每一行断言结果、PC、标志、周期和写入；M0 的 [回归测试](../crates/cpu6502/tests/conformance.rs) 全部保留。

寻址缩写：imp 隐含、acc 累加器、imm 立即数、zp 零页、zpx/zpy 零页索引、abs 绝对、abx/aby 绝对索引、ind 间接 JMP、izx `(zp,X)`、izy `(zp),Y`、rel 相对。`+` 为读取跨页时加 1；分支为不跳／同页跳／跨页跳 2/3/4。写入和 RMW 的索引周期固定。标志未列出的位保持不变。

| opcode | 指令 | 寻址 | 长度 | 周期 | 改变的标志 | 实现 | 测试 |
| --- | --- | --- | ---: | --- | --- | --- | --- |
| 00 | BRK | imp | 1* | 7 | I=1 | 已实现 | 已测试 |
| 01 | ORA | izx | 2 | 6 | NZ | 已实现 | 已测试 |
| 05 | ORA | zp | 2 | 3 | NZ | 已实现 | 已测试 |
| 06 | ASL | zp | 2 | 5 | NZC | 已实现 | 已测试 |
| 08 | PHP | imp | 1 | 3 | — | 已实现 | 已测试 |
| 09 | ORA | imm | 2 | 2 | NZ | 已实现 | 已测试 |
| 0A | ASL | acc | 1 | 2 | NZC | 已实现 | 已测试 |
| 0D | ORA | abs | 3 | 4 | NZ | 已实现 | 已测试 |
| 0E | ASL | abs | 3 | 6 | NZC | 已实现 | 已测试 |
| 10 | BPL | rel | 2 | 2/3/4 | — | 已实现 | 已测试 |
| 11 | ORA | izy | 2 | 5+ | NZ | 已实现 | 已测试 |
| 15 | ORA | zpx | 2 | 4 | NZ | 已实现 | 已测试 |
| 16 | ASL | zpx | 2 | 6 | NZC | 已实现 | 已测试 |
| 18 | CLC | imp | 1 | 2 | C=0 | 已实现 | 已测试 |
| 19 | ORA | aby | 3 | 4+ | NZ | 已实现 | 已测试 |
| 1D | ORA | abx | 3 | 4+ | NZ | 已实现 | 已测试 |
| 1E | ASL | abx | 3 | 7 | NZC | 已实现 | 已测试 |
| 20 | JSR | abs | 3 | 6 | — | 已实现 | 已测试 |
| 21 | AND | izx | 2 | 6 | NZ | 已实现 | 已测试 |
| 24 | BIT | zp | 2 | 3 | NVZ | 已实现 | 已测试 |
| 25 | AND | zp | 2 | 3 | NZ | 已实现 | 已测试 |
| 26 | ROL | zp | 2 | 5 | NZC | 已实现 | 已测试 |
| 28 | PLP | imp | 1 | 4 | NVDIZC | 已实现 | 已测试 |
| 29 | AND | imm | 2 | 2 | NZ | 已实现 | 已测试 |
| 2A | ROL | acc | 1 | 2 | NZC | 已实现 | 已测试 |
| 2C | BIT | abs | 3 | 4 | NVZ | 已实现 | 已测试 |
| 2D | AND | abs | 3 | 4 | NZ | 已实现 | 已测试 |
| 2E | ROL | abs | 3 | 6 | NZC | 已实现 | 已测试 |
| 30 | BMI | rel | 2 | 2/3/4 | — | 已实现 | 已测试 |
| 31 | AND | izy | 2 | 5+ | NZ | 已实现 | 已测试 |
| 35 | AND | zpx | 2 | 4 | NZ | 已实现 | 已测试 |
| 36 | ROL | zpx | 2 | 6 | NZC | 已实现 | 已测试 |
| 38 | SEC | imp | 1 | 2 | C=1 | 已实现 | 已测试 |
| 39 | AND | aby | 3 | 4+ | NZ | 已实现 | 已测试 |
| 3D | AND | abx | 3 | 4+ | NZ | 已实现 | 已测试 |
| 3E | ROL | abx | 3 | 7 | NZC | 已实现 | 已测试 |
| 40 | RTI | imp | 1 | 6 | NVDIZC | 已实现 | 已测试 |
| 41 | EOR | izx | 2 | 6 | NZ | 已实现 | 已测试 |
| 45 | EOR | zp | 2 | 3 | NZ | 已实现 | 已测试 |
| 46 | LSR | zp | 2 | 5 | NZC | 已实现 | 已测试 |
| 48 | PHA | imp | 1 | 3 | — | 已实现 | 已测试 |
| 49 | EOR | imm | 2 | 2 | NZ | 已实现 | 已测试 |
| 4A | LSR | acc | 1 | 2 | NZC | 已实现 | 已测试 |
| 4C | JMP | abs | 3 | 3 | — | 已实现 | 已测试 |
| 4D | EOR | abs | 3 | 4 | NZ | 已实现 | 已测试 |
| 4E | LSR | abs | 3 | 6 | NZC | 已实现 | 已测试 |
| 50 | BVC | rel | 2 | 2/3/4 | — | 已实现 | 已测试 |
| 51 | EOR | izy | 2 | 5+ | NZ | 已实现 | 已测试 |
| 55 | EOR | zpx | 2 | 4 | NZ | 已实现 | 已测试 |
| 56 | LSR | zpx | 2 | 6 | NZC | 已实现 | 已测试 |
| 58 | CLI | imp | 1 | 2 | I=0 | 已实现 | 已测试 |
| 59 | EOR | aby | 3 | 4+ | NZ | 已实现 | 已测试 |
| 5D | EOR | abx | 3 | 4+ | NZ | 已实现 | 已测试 |
| 5E | LSR | abx | 3 | 7 | NZC | 已实现 | 已测试 |
| 60 | RTS | imp | 1 | 6 | — | 已实现 | 已测试 |
| 61 | ADC | izx | 2 | 6 | NVZC | 已实现 | 已测试 |
| 65 | ADC | zp | 2 | 3 | NVZC | 已实现 | 已测试 |
| 66 | ROR | zp | 2 | 5 | NZC | 已实现 | 已测试 |
| 68 | PLA | imp | 1 | 4 | NZ | 已实现 | 已测试 |
| 69 | ADC | imm | 2 | 2 | NVZC | 已实现 | 已测试 |
| 6A | ROR | acc | 1 | 2 | NZC | 已实现 | 已测试 |
| 6C | JMP | ind | 3 | 5 | — | 已实现 | 已测试 |
| 6D | ADC | abs | 3 | 4 | NVZC | 已实现 | 已测试 |
| 6E | ROR | abs | 3 | 6 | NZC | 已实现 | 已测试 |
| 70 | BVS | rel | 2 | 2/3/4 | — | 已实现 | 已测试 |
| 71 | ADC | izy | 2 | 5+ | NVZC | 已实现 | 已测试 |
| 75 | ADC | zpx | 2 | 4 | NVZC | 已实现 | 已测试 |
| 76 | ROR | zpx | 2 | 6 | NZC | 已实现 | 已测试 |
| 78 | SEI | imp | 1 | 2 | I=1 | 已实现 | 已测试 |
| 79 | ADC | aby | 3 | 4+ | NVZC | 已实现 | 已测试 |
| 7D | ADC | abx | 3 | 4+ | NVZC | 已实现 | 已测试 |
| 7E | ROR | abx | 3 | 7 | NZC | 已实现 | 已测试 |
| 81 | STA | izx | 2 | 6 | — | 已实现 | 已测试 |
| 84 | STY | zp | 2 | 3 | — | 已实现 | 已测试 |
| 85 | STA | zp | 2 | 3 | — | 已实现 | 已测试 |
| 86 | STX | zp | 2 | 3 | — | 已实现 | 已测试 |
| 88 | DEY | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| 8A | TXA | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| 8C | STY | abs | 3 | 4 | — | 已实现 | 已测试 |
| 8D | STA | abs | 3 | 4 | — | 已实现 | 已测试 |
| 8E | STX | abs | 3 | 4 | — | 已实现 | 已测试 |
| 90 | BCC | rel | 2 | 2/3/4 | — | 已实现 | 已测试 |
| 91 | STA | izy | 2 | 6 | — | 已实现 | 已测试 |
| 94 | STY | zpx | 2 | 4 | — | 已实现 | 已测试 |
| 95 | STA | zpx | 2 | 4 | — | 已实现 | 已测试 |
| 96 | STX | zpy | 2 | 4 | — | 已实现 | 已测试 |
| 98 | TYA | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| 99 | STA | aby | 3 | 5 | — | 已实现 | 已测试 |
| 9A | TXS | imp | 1 | 2 | — | 已实现 | 已测试 |
| 9D | STA | abx | 3 | 5 | — | 已实现 | 已测试 |
| A0 | LDY | imm | 2 | 2 | NZ | 已实现 | 已测试 |
| A1 | LDA | izx | 2 | 6 | NZ | 已实现 | 已测试 |
| A2 | LDX | imm | 2 | 2 | NZ | 已实现 | 已测试 |
| A4 | LDY | zp | 2 | 3 | NZ | 已实现 | 已测试 |
| A5 | LDA | zp | 2 | 3 | NZ | 已实现 | 已测试 |
| A6 | LDX | zp | 2 | 3 | NZ | 已实现 | 已测试 |
| A8 | TAY | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| A9 | LDA | imm | 2 | 2 | NZ | 已实现 | 已测试 |
| AA | TAX | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| AC | LDY | abs | 3 | 4 | NZ | 已实现 | 已测试 |
| AD | LDA | abs | 3 | 4 | NZ | 已实现 | 已测试 |
| AE | LDX | abs | 3 | 4 | NZ | 已实现 | 已测试 |
| B0 | BCS | rel | 2 | 2/3/4 | — | 已实现 | 已测试 |
| B1 | LDA | izy | 2 | 5+ | NZ | 已实现 | 已测试 |
| B4 | LDY | zpx | 2 | 4 | NZ | 已实现 | 已测试 |
| B5 | LDA | zpx | 2 | 4 | NZ | 已实现 | 已测试 |
| B6 | LDX | zpy | 2 | 4 | NZ | 已实现 | 已测试 |
| B8 | CLV | imp | 1 | 2 | V=0 | 已实现 | 已测试 |
| B9 | LDA | aby | 3 | 4+ | NZ | 已实现 | 已测试 |
| BA | TSX | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| BC | LDY | abx | 3 | 4+ | NZ | 已实现 | 已测试 |
| BD | LDA | abx | 3 | 4+ | NZ | 已实现 | 已测试 |
| BE | LDX | aby | 3 | 4+ | NZ | 已实现 | 已测试 |
| C0 | CPY | imm | 2 | 2 | NZC | 已实现 | 已测试 |
| C1 | CMP | izx | 2 | 6 | NZC | 已实现 | 已测试 |
| C4 | CPY | zp | 2 | 3 | NZC | 已实现 | 已测试 |
| C5 | CMP | zp | 2 | 3 | NZC | 已实现 | 已测试 |
| C6 | DEC | zp | 2 | 5 | NZ | 已实现 | 已测试 |
| C8 | INY | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| C9 | CMP | imm | 2 | 2 | NZC | 已实现 | 已测试 |
| CA | DEX | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| CC | CPY | abs | 3 | 4 | NZC | 已实现 | 已测试 |
| CD | CMP | abs | 3 | 4 | NZC | 已实现 | 已测试 |
| CE | DEC | abs | 3 | 6 | NZ | 已实现 | 已测试 |
| D0 | BNE | rel | 2 | 2/3/4 | — | 已实现 | 已测试 |
| D1 | CMP | izy | 2 | 5+ | NZC | 已实现 | 已测试 |
| D5 | CMP | zpx | 2 | 4 | NZC | 已实现 | 已测试 |
| D6 | DEC | zpx | 2 | 6 | NZ | 已实现 | 已测试 |
| D8 | CLD | imp | 1 | 2 | D=0 | 已实现 | 已测试 |
| D9 | CMP | aby | 3 | 4+ | NZC | 已实现 | 已测试 |
| DD | CMP | abx | 3 | 4+ | NZC | 已实现 | 已测试 |
| DE | DEC | abx | 3 | 7 | NZ | 已实现 | 已测试 |
| E0 | CPX | imm | 2 | 2 | NZC | 已实现 | 已测试 |
| E1 | SBC | izx | 2 | 6 | NVZC | 已实现 | 已测试 |
| E4 | CPX | zp | 2 | 3 | NZC | 已实现 | 已测试 |
| E5 | SBC | zp | 2 | 3 | NVZC | 已实现 | 已测试 |
| E6 | INC | zp | 2 | 5 | NZ | 已实现 | 已测试 |
| E8 | INX | imp | 1 | 2 | NZ | 已实现 | 已测试 |
| E9 | SBC | imm | 2 | 2 | NVZC | 已实现 | 已测试 |
| EA | NOP | imp | 1 | 2 | — | 已实现 | 已测试 |
| EC | CPX | abs | 3 | 4 | NZC | 已实现 | 已测试 |
| ED | SBC | abs | 3 | 4 | NVZC | 已实现 | 已测试 |
| EE | INC | abs | 3 | 6 | NZ | 已实现 | 已测试 |
| F0 | BEQ | rel | 2 | 2/3/4 | — | 已实现 | 已测试 |
| F1 | SBC | izy | 2 | 5+ | NVZC | 已实现 | 已测试 |
| F5 | SBC | zpx | 2 | 4 | NVZC | 已实现 | 已测试 |
| F6 | INC | zpx | 2 | 6 | NZ | 已实现 | 已测试 |
| F8 | SED | imp | 1 | 2 | D=1 | 已实现 | 已测试 |
| F9 | SBC | aby | 3 | 4+ | NVZC | 已实现 | 已测试 |
| FD | SBC | abx | 3 | 4+ | NVZC | 已实现 | 已测试 |
| FE | INC | abx | 3 | 7 | NZ | 已实现 | 已测试 |

周期与标志来源：[MOS 6500-50A 附录 B](https://lbaeza.neocities.org/mcs6500/6500_appb)。额外测试覆盖所有分支、索引读取跨页与 16 位回绕、零页指针回绕、NMOS RMW 的旧值／新值两次写入，以及原有栈和间接 JMP 边界。

表外 105 个字节为未实现的非官方 opcode（含别名、NOP、JAM），全部返回 `UnsupportedOpcode`；错误契约逐个遍历。RESET、IRQ/NMI 输入不是 opcode。

`*` BRK 编码长度 1 字节，但会读取并越过一个填充字节，保存 PC+2。BRK/PHP 入栈 B=1，IRQ/NMI 入栈 B=0；位 5 入栈为 1，B 不持久存储。RTI 恢复六个标志与原 PC，不加 1。

ADC/SBC 的所有模式已有上述矩阵测试；[arithmetic.rs](../crates/cpu6502/tests/arithmetic.rs) 另穷举二进制 A／操作数／C（每条 131072 组）与有效 BCD 结果／进借位（每条 20000 组），覆盖 NMOS 特有标志阶段、无效 BCD 和固定来源的 8 条外部十进制样例。十进制模式不增加周期。

[interrupts.rs](../crates/cpu6502/tests/interrupts.rs) 验证 BRK/RTI 的完整状态与必要栈访问、IRQ/NMI 的 7 周期、掩码／边沿／优先级／嵌套／RESET，以及周期模型下 CLI/SEI/PLP 的 I 采样延迟与 RTI 的恢复后采样。模型精度限制见 [架构文档](architecture.md#中断输入与执行事件)。


M2.3：上表全部 151 个官方 opcode 还通过固定 SingleStepTests NMOS `6502/v1` 每文件 10000 条、共 1510000 条用例的寄存器／内存、周期数量和完整总线序列比较。版本与逐文件哈希见 [数据说明](../crates/cpu6502/tests/data/README.md)。这不包含非官方 opcode 或外部引脚相位；RESET 的七次访问另由 `cycles.rs` 与 `conformance.rs` 验证。


M2.4：`pins.rs` 的 246 组固定 revD 场景核对 IRQ/NMI 的采样、分支轮询、向量抢占、RDY 及 SO；SO 扫描覆盖六种写 V 指令。`cycles.rs` 另验证等待、读副作用、栈／地址阶段、中途复位请求及有界恢复。这些是明确场景的验证，物理 RESET 保持／释放和所有引脚交叉窗口尚未完成。
