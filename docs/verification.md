# CPU 本地验证记录

当前：M2.1～M2.5 的本地目标范围已验收，包含物理 RESET 输入同步、中途数据通路和全部固定 CPU 对照。范围限定于官方 NMOS 指令及已记录的固定 revD 引脚窗口，不等同于所有芯片修订／电气窗口认证。下面保留各阶段历史结果；CPU 相关的最后一节记录物理 RESET 实现与最终代码复验，Apple I 相关的最后一节记录 CLI 交互式回归。远程 CI 未运行。

## M1

日期：2026-09-09。平台：macOS / `aarch64-apple-darwin`。

- 工具链：`stable-aarch64-apple-darwin`。
- `rustc 1.98.1 (48a229cea 2026-09-01)`。
- `cargo 1.98.1 (797e8a9bc 2026-08-05)`。
- 已安装 rustfmt、Clippy、`wasm32-unknown-unknown`。
- `rust-toolchain.toml` 跟随 stable；只记录实际验证版本，尚未确定或验证 MSRV。

本轮功能提交：`05b1df1`（寻址与通用指令）、`9a9d583`（二进制／十进制算术）、`28330fc`（BRK/RTI 与指令边界中断）。下面是这些代码的实际检查结果；随后只更新路线图、验证记录和 CI 工作流名称。

| 命令 | 实际结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check --workspace --all-targets` | 通过 |
| `cargo test --workspace` | 通过：51 个 CPU 测试 + 6 个宿主／CLI 测试，0 失败，0 忽略 |
| `cargo test --workspace --release` | 通过：同样 57 个测试，0 失败，0 忽略 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过，无 warning 抑制 |
| `cargo run -p hesper` | 通过：内存实际输出 `0 1 2 3 4 5 6 7 8 9` |
| `cargo run -p hesper -- --trace` | 通过：54 条 trace，包含前后状态；末条累计 154 周期 |
| `cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown` | 通过：仅 CPU 库目标编译检查，无浏览器运行验证 |
| `git diff --check` / `git diff --cached --check` | 通过 |

演示最后状态为 `A=09 X=0A Y=00 SP=FF PC=800F P=27`，54 条指令耗用 147 周期，另计 RESET 7 周期，共 154 周期。集成测试断言 RAM 的 `$0200..$0209`、复位向量、完成地址和计数；还执行 CLI 验证 trace、帮助、参数错误以及 0／53 步预算失败，54 步预算恰好成功。

测试范围：

- `conformance.rs`：保留 24 个 M0 回归测试；未支持字节的测试随清单更新，逐个确认当前 105 个非官方 opcode 返回明确错误。
- `official.rs`：8 个矩阵／边界测试，独立手写规格覆盖全部 151 个官方 opcode 的结果、PC、标志、写入和周期；另验证分支、索引、零页／16 位回绕、PHP/PLP、BIT 和 RMW。文档清单也已与该独立规格核对为 151 个 opcode／56 条指令。
- `arithmetic.rs`：5 个测试。二进制 ADC/SBC 各穷举 256×256×2 = 131072 组结果与全部算术标志；有效 BCD 各穷举 100×100×2 = 20000 组结果与进借位。另有 17 条手写 NMOS 标志／无效 BCD 边界和 8 条固定来源的外部十进制用例。未声称穷举验证全部十进制输入的 N/V/Z 外部一致性。
- `interrupts.rs`：14 个测试。BRK/RTI 的状态／必要访问／PC 和 SP 回绕、IRQ 电平与掩码轮询、NMI 边沿／优先级／嵌套、RESET 清除待处理请求；全部中断入口和返回周期均断言。
- `crates/cli/tests/demo.rs`：6 个宿主／CLI 测试。

少量 Bus 顺序断言对应必要访问、RMW 旧值回写及栈顺序；没有比较整条逐周期总线序列。中断采用文档定义的指令边界模型，没有验证所有流水线组合、引脚相位或重叠窗口，详见 [架构限制](architecture.md#中断输入与执行事件)。

[GitHub Actions](../.github/workflows/ci.yml) 已配置格式、构建检查、debug／release 测试、Clippy 和两种 CLI 演示。本仓库当前未配置远程地址，本轮没有远程运行 CI，也没有推送或发布。CI 的 Linux 环境执行结果仍未验证。

未执行 Klaus Dormann 汇编测试或 Visual6502 晶体管模型。SingleStepTests 仅执行所选 8 条 NMOS 十进制输入／结果，并检查 2 周期；固定提交和 MIT 许可见 [数据说明](../crates/cpu6502/tests/data/README.md)。间接 JMP 的一条外部记录仍仅用于资料核对，未运行完整 JSON runner 或整个套件。未运行 Apple I、Apple II 或 Web 前端，也未引入任何 Apple ROM。

## M2.1

日期：2026-09-09，工具链与 M1 相同。新增测试工具和固定夹具，未修改 CPU 指令语义。`cargo test --workspace --offline` 与 `cargo test --workspace --release --offline` 各通过 61 个测试（55 个 CPU 包测试 + 6 个 CLI 测试），0 失败、0 忽略。新测试实际执行 672 条原始格式 NMOS 用例，并验证过滤、缺失／损坏数据及差异报告。

`cargo fmt --all -- --check`、全目标 `cargo check`、全目标 Clippy `-D warnings` 和 `git diff --check` 均通过。依赖相关检查使用 `--offline`，开发依赖由本地 Cargo 缓存解析并锁定；普通 CPU 库没有新增运行依赖。全量夹具重放与 `--opcode 69 --case-index 0` 单条重放均实际成功。`python3 tools/prepare_singlestep.py` 已核对 21 份上游文件的哈希以及 672 条夹具的原始顺序选择。

验证范围是所选样例的寄存器／内存与周期数量；未比较完整总线序列，尚未执行全部 151 个官方 opcode 文件或 Klaus 汇编程序。远程 CI 尚未执行。M2.2～M2.5 未完成。

## M2.2

日期、平台及工具链同上。未修改 CPU 指令语义。固定范围实际结果：

| 命令／范围 | 结果 |
| --- | --- |
| `python3 tools/prepare_singlestep.py --full` | 151 个原始文件哈希／数量通过，1510000 条，未包含非官方 opcode |
| `cargo run -p hesper-cpu6502 --example singlestep --release --offline -- --full` | 全部 1510000 条寄存器／内存、周期数量通过；此时未比较总线序列 |
| `python3 tools/prepare_klaus.py` | 固定源码、镜像、listing、许可证哈希通过；缓存内 ca65/ld65 构建及开启全部检查的 decimal 镜像／符号校验通过 |
| `cargo run -p hesper-cpu6502 --example functional --release --offline` | 到达 `$3469`；30646176 条指令，96241364 周期 |
| `cargo run -p hesper-cpu6502 --example functional --release --offline -- --decimal` | 到达 `$024B` 且 ERROR=0；17609915 条指令，53953825 周期；ADC/SBC 各覆盖 256×256×2 种输入，A/N/V/Z/C 全部检查 |

格式检查、全目标 check、debug/release workspace 测试（各 **62 个测试**，0 失败／忽略）、全目标 Clippy `-D warnings`、CPU Wasm 编译检查、CLI 正常及 54 条 trace、`git diff --check` 均实际通过。Cargo 依赖相关命令使用 `--offline`。全量清单不可缩减／换成样例的错误路径进入离线回归。

准备脚本需要 Python 3.12+、make 和 C 编译器，汇编器只在忽略缓存内构建，无系统安装。功能测试使用上游 AS65 镜像，没有声称本地用 AS65 重建；decimal 使用固定 ca65 构建，来源和配置见 [数据说明](../crates/cpu6502/tests/data/README.md)。没有执行 Klaus 中断或 Visual6502 模型；M2.3～M2.5 尚未完成，远程 CI 未运行、未推送。

## M2.3

同日同工具链。引擎已迁移为实际单周期访问，`step` 使用同一套执行阶段及 ALU。全量命令在此版本重新执行：**1510000 条**官方单步用例的寄存器／内存、周期数量、每周期地址／数据／读写均通过；Klaus functional 和全标志 decimal 的指令数／周期数与 M2.2 相同，成功地址和 ERROR 检查均通过。

debug/release workspace 各 **67 个测试**通过，0 失败／忽略。新增读取副作用、RMW 锁存与分次写入、任意 JSR 前缀接续 step、索引写入和七周期 RESET 事件回归。旧 M0/M1 中有意只断言必要访问的 JSR/RTS/RTI/IRQ/RESET 列表，按手册补入真实 dummy read；原有结果、标志、顺序和周期预期保留。

格式、全目标 check、全目标 Clippy `-D warnings`、CPU Wasm 编译和 diff 检查通过。M2.3 仍保留 M1 的中断边界采样约定；没有将总线比较通过等同于引脚相位通过，M2.4～M2.5 待完成。远程 CI 尚未运行。

## M2.4：周期中断采样子阶段

同日同工具链。IRQ/NMI 已改为周期采样、指令阶段轮询，并实现分支轮询差异及 BRK/IRQ 的 NMI 向量抢占。debug/release workspace 各 **73 个测试**通过；96 个固定 revD 引脚场景（2304 周期）逐项比较总线地址／数据／读写／SYNC 通过。`python3 tools/prepare_visual6502.py` 校验模型文件；`node tools/verify_visual6502.cjs` 实际重跑 96 个场景，生成观察与固定 fixture 一致。本地 Node `v26.8.1`。

Klaus 中断适配在初次复核时发现跨行替换误删错误陷阱，那个试跑结果作废，未提交错误镜像。修正后校验保留原始 505 条指令语句及全部陷阱；decimal 适配也增加指令保留校验。`python3 tools/prepare_klaus.py` 重建并校验最终镜像／符号通过。

`cargo run -p hesper-cpu6502 --example interrupt --release --offline -- --feedback-delay 4` 实际到达 `$06F5`，1049 个 step／3013 周期，NMI/IRQ/BRK 顺序计数 `[1,3,2]`。默认 0 延迟实际在 `$075C` 退出失败：与上游注明的真实 NMOS BRK/NMI 重叠 B 位问题一致；1～3 同类失败，5～10 超出该程序另一组响应时序预期。没有关闭陷阱、吞掉错误或把默认配置报告为通过。固定配置、哈希及复現命令见测试数据说明。

格式、全目标 check、全目标 Clippy `-D warnings`、diff 检查通过；完整 151 万官方单步用例已在此版本重新比较结果、周期数量及总线。RDY/SO、半周期和物理 RESET 持续输入尚未实现，M2.4 及 M2 整体仍未验收；远程 CI 未运行。

## M2.4：RDY、SO 与半周期子阶段

同日同工具链。新增 RDY 读等待／写继续、SO 相位采样及 V 写入重叠窗口、`half_cycle`、可恢复的有限周期预算。Step 周期数改为 u64；reset 返回 Result<Step, CpuError> 并包装同一个周期引擎。固定 revD 夹具扩展到 **246 个场景／5904 周期**，全部由原始模型重新生成，旧场景原样保留。新用例发现并修复 SO 与 CLV／算术 V 更新延续到下一取码周期的差异，未改变参考预期。

本地 debug/release workspace 各 **80 个测试**通过，0 失败／忽略；包含长达 1000 周期的等待、读副作用、RMW 连写、SO 保持电平不重触发以及 RESET 超限后恢复。格式、全目标 check、Clippy `-D warnings`、diff 检查通过。151 万官方单步用例在此代码重新执行，寄存器／内存、周期数量和逐周期总线全部通过。

物理 RESET 引脚的持续断言／释放仍未实现；宿主 begin_reset 的七周期契约与物理引脚明确分开。本阶段不据此标记 M2.4 或 M2 整体验收完成。远程 CI 未运行。

## M2.5：有界调试子阶段

新增 debug_state 只读快照、CLI 总线 trace 与 1～4096 条保留上限、外部 runner 共用的末尾 32 周期失败历史。debug/release workspace 各 **83 个测试**通过，Clippy 全目标 `-D warnings` 及格式检查通过。实际运行 `cargo run -p hesper -- --bus-trace --trace-limit 4`，输出最后四个真实周期及原有正确演示结果。测试覆盖 RESET trace、历史截断、超限后诊断、无额外读取，以及不正确的外部预期被明确报告。


## M2 当前代码复验与 CI 分层

2026-09-09，macOS aarch64，Rust/Cargo 1.98.1；Python 3.14.7、Node 26.8.1。已安装 Wasm 目标。以下命令均实际执行，未使用跳过测试或 warning 抑制：

| 命令 | 本地结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check --workspace --all-targets` | 通过 |
| `cargo test --workspace` | 83 个通过，0 失败／忽略；热缓存 1.73 秒 |
| `cargo test --workspace --release` | 83 个通过，0 失败／忽略；热缓存 1.58 秒 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown` | 通过，仅目标编译 |
| `cargo run -p hesper` | 正确输出 0～9；54 条指令／147+7=154 周期 |
| `cargo run -p hesper -- --trace` | 正确输出 54 条指令记录与结果 |
| `cargo run -p hesper -- --bus-trace --trace-limit 4` | 仅保留周期 151～154 的真实记录，结果相同 |
| `git diff --check` | 通过 |

同一代码再次运行全部 **1510000 条官方单步用例**：寄存器／内存、周期数量和总线序列均通过。Klaus functional：30646176 条指令／96241364 周期，成功 `$3469`；Bruce Clark decimal 全检查：17609915 条／53953825 周期，成功 `$024B` 且 ERROR=0。含 Cargo 启动的本机热缓存耗时分别为 3.14／1.42 秒。

Klaus 中断 4 周期反馈延迟：1049 个 step／3013 周期，成功 `$06F5`，顺序计数 `[1,3,2]`。**默认 0 延迟仍在 `$075C` 的上游已知 NMOS 陷阱退出 1**，并实际输出最后 32 个总线周期的诊断，没有将这个配置报告为通过。Visual6502 原模型实际重跑并精确重现 **246 组／5904 周期**观察，CPU 对照全部通过。三个准备脚本重新核对固定数据、汇编产物及来源哈希通过。

CI 分为每次变更的固定小样例回归，以及手动触发的完整外部验证（45 分钟上限）；后者显式准备数据，缺失／损坏／零匹配不跳过。两份 YAML 在本地解析与步骤结构检查通过，Python／Node 工具语法检查通过；Actions v7 标签已核对存在。**未远程运行 CI，未验证 Linux 工作流执行结果**。当前仓库已有 origin 配置；本节不沿用早期阶段“未配置远程”的状态描述。

CPU 普通依赖树仅包含自身，没有第三方运行依赖。当前物理 RESET 断言／保持／释放及其中途窗口仍未实现，因此 M2 整体验收和 M2.5 最终验收继续保留未完成，下一功能点见路线图；Apple I 未启动。

## M2.4：物理 RESET 独立参考基线

2026-09-10，macOS aarch64；Rust/Cargo 1.98.1、Python 3.14.7、**Node v24.11.1**。本节记录实际本地环境；工作流配置的 Node 26 本轮未远程验证。

按用户明确选择，本轮只落地参考基线及重放入口，**没有修改 CPU 执行器，没有新增物理 RESET API**。固定 revD 新增 **419 组／26816 周期**观察，原有 246 组／5904 周期夹具的字节及 SHA-256 原样保留。新夹具、覆盖表、寄存器见证写入和已发现的同步／PC／SP／RDY／NMI 差异见 [数据说明](../crates/cpu6502/tests/data/README.md#物理-reset-对照)。

| 命令／范围 | 实际本地结果 |
| --- | --- |
| `python3 tools/prepare_singlestep.py --full` | 固定 151 个官方文件／1510000 条，哈希和数量通过 |
| `python3 tools/prepare_klaus.py` | 固定源码、镜像、汇编产物及哈希通过 |
| `python3 tools/prepare_visual6502.py` | 固定 revD 的 7 个源文件哈希通过 |
| `node tools/verify_visual6502.cjs` | 原模型实际重跑：246 组 pins、419 组 reset 分别精确重现；本机本次 214.66 秒 |
| `node tools/verify_visual6502.cjs --suite reset --case reset-registers-stack-wrap` | 精确单场景重放通过；非零 A/X/Y、SP 回绕和 D/C/I 见证符合固定观察 |
| `node tools/verify_visual6502.cjs --suite pins --case nop-irq-0` | 原有套件单场景重放通过 |
| `cargo test -p hesper-cpu6502 --test pins` | 7 个通过；其中 RESET 项仅做离线数据完整性检查，不执行 CPU RESET 引脚 |
| `cargo fmt --all -- --check`、`cargo check --workspace --all-targets` | 通过 |
| `cargo test --workspace`、`cargo test --workspace --release` | 各 84 个通过，0 失败／忽略 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown` | 通过，仅 CPU 库目标编译 |
| `cargo run -p hesper`、`-- --trace`、`-- --bus-trace --trace-limit 4` | 三种实际运行均输出正确结果；54 条指令、147+7=154 周期；分别输出 54 条指令记录和最后 4 个总线周期 |
| `node --check tools/verify_visual6502.cjs` | 通过 |

全量 CPU 已实现范围重新运行：官方 SingleStep **1510000 条**寄存器／内存、周期数量、总线序列全部通过；Klaus functional 到 `$3469`，30646176 条／96241364 周期；Bruce Clark 全标志 decimal 到 `$024B` 且 ERROR=0，17609915 条／53953825 周期；Klaus 中断明确采用 `--feedback-delay 4`，到 `$06F5`，1049 step／3013 周期、顺序计数 `[1,3,2]`。默认 0 延迟本轮未重跑，不改变前节记录的上游 NMOS 陷阱失败结论。

重放工具错误路径实际执行：未知套件、零匹配／跨套件场景、重复参数、未指定单套件的记录、局部记录均退出 1。隔离临时目录内移除／损坏夹具，分别验证缺文件和哈希错误退出 1；故意改动首个总线地址并同步临时哈希后，准确报告 `reset-registers-stack-wrap cycle 0`、预期／实际元组及重放命令。没有改动固定预期来让检查通过；临时错误检查目录已自动清除。

本轮完成的是**有界的独立参考基线**，不等于 CPU 物理 RESET 对照通过。M2.4、M2／M2.5 最终验收仍未完成；下一步须实现输入同步、内部锁存和提交时机，再对照全部 RESET 场景。未远程运行 CI，本轮按功能点本地提交，未 push；未启动 Apple I，也未选择项目许可证。

## M2.4：栈地址锁存与 SP 提交

2026-09-10，macOS aarch64；rustc 1.98.1（48a229cea）、Cargo 1.98.1、Python 3.14.7、Node v24.11.1。本轮实现物理 RESET 所需的一个执行器前置改造：所有栈访问使用独立地址游标，SP 按已观察的提交阶段更新；没有新增物理 RESET API。

### 独立观察与实际 CPU 对照

沿用固定 revD 驱动及源文件哈希，只在宿主实验中额外读取每周期下降沿后的 SP；不修改原模型，不以 Hesper 输出生成预期。初始 SP=`$FD` 时，代表性的逐周期 SP 如下（包含 opcode fetch）：

| 程序 | 固定 revD 观察到的 SP 序列 |
| --- | --- |
| `JSR $1234` | FD、34、34、34、34、FB |
| `BRK` | FD、FD、FD、FD、FA、FA、FA |
| `PHA` | FD、FD、FC |
| `PLA` | FD、FD、FE、FE |
| `RTS` | FD、FD、FD、FF、FF、FF |
| `RTI` | FD、FD、FD、FD、00、00 |

RDY 相位扫描确认：JSR 的目标低字节等待不会提前写入 SP；最终高字节等待则会提交最终 SP。PLA/PLP 的栈 dummy read、RTS/RTI 的返回低字节读取、入口的第三次栈读取，均可在地址仍等待时更新 SP。因此不能用可见 SP 重新生成等待地址，也不能在每次等待时再次增减它。

实际运行临时 CPU runner，经公共 `half_cycle` 驱动并逐周期比较地址／数据／方向／SYNC 和 SP，**34 组／268 周期全部一致**：28 组普通指令／IRQ/NMI／RDY 场景，另 6 组是从已同步 RESET 观察截取的宿主入口片段。后者明确使用 `begin_reset`，只验证七周期入口及其 RDY 延长，**不验证物理 RESET 的断言／同步／保持／释放**。JSR 运行输出同时显示可见 SP=`$34` 而内部栈游标依次为 `$01FD/$01FC/$01FB`。一次性 runner 源码已移除；SP 提交规则进入常规执行器和永久回归。

新增回归先在旧实现运行，JSR 第 2 个周期实际 SP=`$FD`、预期 `$34`，测试失败；修正后通过。新增 3 个测试保护分周期 SP、两次七周期预算耗尽后的 RDY 重读与恢复，以及中途宿主 RESET 使用当前可见 SP、丢弃旧地址游标但保留已完成写入；已有 RESET 测试补充 SP 回绕及提交周期断言。

### 该阶段代码完整复验

| 命令／范围 | 实际本地结果 |
| --- | --- |
| `cargo fmt --all -- --check`、`cargo check --workspace --all-targets` | 通过 |
| `cargo test --workspace`、`cargo test --workspace --release` | 各 87 个通过，0 失败／忽略 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown` | 通过，仅 CPU 库目标编译 |
| `cargo run -p hesper`、`-- --trace`、`-- --bus-trace --trace-limit 4` | 三种实际运行通过；54 条指令、147+7=154 周期，结果及有界 trace 保持 |
| `python3 tools/prepare_singlestep.py --full`、`python3 tools/prepare_klaus.py` | 固定源文件、数量及哈希通过 |
| `cargo run -p hesper-cpu6502 --example singlestep --release -- --full` | 151 个官方 opcode／1510000 条；寄存器／内存、周期数量、总线序列全部通过 |
| `cargo run -p hesper-cpu6502 --example functional --release` | `$3469`，30646176 条指令／96241364 周期 |
| `cargo run -p hesper-cpu6502 --example functional --release -- --decimal` | 全标志及非法 BCD，通过于 `$024B`；17609915 条指令／53953825 周期 |
| `cargo run -p hesper-cpu6502 --example interrupt --release -- --feedback-delay 4` | `$06F5`，1049 step／3013 周期；顺序计数 `[1,3,2]` |
| `python3 tools/prepare_visual6502.py`、`node tools/verify_visual6502.cjs` | 固定 7 个源文件哈希通过；原模型精确重现 246 组 pins 和 419 组 reset；本次合计 215.11 秒 |

CPU 已实现引脚的 246 组对照与 RESET 的 419 组参考重现仍是不同范围：前者在 Cargo 回归中执行 CPU，后者在 Node 中执行原模型，Cargo 仅检查 RESET 夹具完整性。默认零延迟 Klaus 中断本轮未重跑，不改变前节失败结论。M2.4／M2 整体验收仍未完成，下一步是实际 RESET 输入同步及中途地址／数据通路；未启动 Apple I。未远程运行 CI，本轮按功能点本地提交，未 push。

## 物理 RESET 实现与 M2 本地整体验收

2026-09-10，macOS aarch64；`rustc 1.98.1 (48a229cea 2026-09-01)`、`cargo 1.98.1 (797e8a9bc 2026-08-05)`、Python 3.14.7、Node v24.11.1。下列结果来自本地实际运行；尚未验证 MSRV，未远程运行工作流配置的 Node 26。

### 执行器与实际 CPU 对照

- 新增 `set_reset_line`，分别保留原始输入、下降相位采样、时序链停止和进行中锁存。断言不会在 setter 中直接调用宿主复位；同步前已经开始的周期与传送仍可发生。
- 普通执行留下真实数据／ALU 输入及反馈状态，RESET 保持阶段继续表达选中的旧指令传送和 RMW 尾部。内部 PC、外部地址锁存、栈地址及 SP 提交分开，不按默认 `$EA` 数据或固定地址拼接轨迹。
- 同步释放、短脉冲、再次断言、RDY 等待及 NMI 保留窗口由同一周期引擎处理。向量高字节读完成时只报告一次 `StepKind::Reset`，周期包含接管／保持／等待；已有宿主七周期入口与有限预算恢复契约保留。
- 两个夹具的字节和哈希均未改变。`pins.rs` 对全部 **246 组／5904 周期**及 **419 组／26816 周期**实际驱动 CPU，核对地址、数据、读写、SYNC 和处理程序寄存器见证写入。RESET 不再只是完整性检查。
- CPU 比较先运行同一真实引导程序，再恢复引导后的完整 RAM 快照。只注入 `Registers` 会丢失数据通路残留；只执行引导却不恢复 RAM，则与模型引导后施加 RAM 覆盖的顺序不同。两者均不能作为相同起始状态。

RESET 执行器接入前，固定 `reset-registers-stack-wrap` 在 c2 的 SYNC 比较即失败；实现后全部固定观察通过。另在未修改且哈希核对的 revD 上独立改变 PC、A、RAM 和向量，得到 **37 组／2368 周期**，与实际 CPU 全部一致。这是额外交叉验证，不冒充新增固定夹具；一次性 CPU 运行器源文件已移除。

保留的边界回归包括：

- 物理 RESET 保持超过默认七周期预算后，原状态接续，并按固定 NOP 释放延迟完成入口。
- 仅向量高字节读报告完成，下一周期才取处理程序 opcode；未跨采样点的脉冲不触发。
- 改变返回 PCL／栈数据后的短脉冲：PCL=`$47`、DL=`$5C`，被抑制压栈后 SYNC 读取 `$44FC`，下一 dummy read 读取 `$47FC`；栈数据没有被写坏。
- RESET 强制内部 IR 时，外部总线仍保留实际 `$EA`，诊断事件类型为 `Reset`，不能伪造 `Instruction { opcode: 0 }`。此回归先在旧诊断路径实际失败，修正后随最终 debug/release 测试通过。

### 最终代码完整复验

| 命令／范围 | 实际本地结果 |
| --- | --- |
| `cargo fmt --all -- --check`、`cargo check --workspace --all-targets` | 通过 |
| `cargo test --workspace` | 91 个测试通过，含两个完整固定 CPU 引脚套件 |
| `cargo test --workspace --release` | 同样 91 个测试通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown` | 已安装目标，CPU 库目标编译通过；不是浏览器验证 |
| `cargo run -p hesper`、`cargo run -p hesper -- --trace`、`cargo run -p hesper -- --bus-trace --trace-limit 4` | 三种实际 CLI 运行通过；54 条指令，147 指令周期＋7 宿主 RESET 周期＝154 总周期；bus trace 仅保留最后 4 周期，并显示 RESET 输入／锁存 |
| `python3 tools/prepare_singlestep.py --full`、`python3 tools/prepare_klaus.py` | 本轮已核对固定来源、文件数量及哈希 |
| `cargo run -p hesper-cpu6502 --example singlestep --release -- --full` | 151 个官方 opcode／1510000 条；寄存器／内存、周期数量、总线序列全部通过 |
| `cargo run -p hesper-cpu6502 --example functional --release` | `$3469`，30646176 条指令／96241364 周期 |
| `cargo run -p hesper-cpu6502 --example functional --release -- --decimal` | 全 A/N/V/Z/C 及非法 BCD，通过于 `$024B`；17609915 条指令／53953825 周期 |
| `cargo run -p hesper-cpu6502 --example interrupt --release -- --feedback-delay 4` | `$06F5`，1049 step／3013 周期；NMI/IRQ/BRK 顺序计数 `[1,3,2]` |
| `python3 tools/prepare_visual6502.py`、`node tools/verify_visual6502.cjs` | 固定 7 个源文件哈希通过；原模型分别精确重现 246 组 pins 与 419 组 reset；本次合计 223.29 秒 |

最后一行只证明原模型重放，实际 CPU 通过由 Cargo 引脚套件证明；两项都已执行。最终诊断事件修正后重新运行了格式／编译、debug/release、Clippy、Wasm 目标、三种 CLI 和上述全部 CPU 外部程序。

M2.1～M2.5 的上述本地目标范围已完成，路线图据此勾选；不宣称穷举全部官方指令×引脚相位组合或所有 NMOS 修订。默认 0 延迟 Klaus 中断本轮未重跑，也未将已记录的上游 NMOS 陷阱改判为通过。Apple I／浏览器未启动；未远程运行 CI，本轮按功能点本地提交，未 push，未选择项目许可证。

## M3.1：Apple I 机器总线与 PIA

2026-09-10，macOS aarch64；rustc 1.98.1、Cargo 1.98.1。

新增 `crates/apple1`：MC6821 PIA 寄存器模型与 Apple I 地址译码 Bus。CPU 未修改。

| 命令／范围 | 实际本地结果 |
| --- | --- |
| `cargo fmt --all -- --check`、`cargo check --workspace --all-targets` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test --workspace` | **104** 个通过（91 CPU/CLI + 13 apple1），0 失败／忽略 |
| `cargo test --workspace --release` | **104** 个通过 |
| `cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown` | 通过 |

新增测试覆盖：PIA 复位状态、DDR/OR 选择、数据读取混合输出与输入引脚、CA1 边沿触发 IRQA1 标志、读数据清除中断标志、控制寄存器只读位保护。Bus 测试覆盖 RAM 读写、ROM 只读、PIA 寄存器访问、开路总线返回最后读取值、RAM 加载超范围错误。

`Apple1Bus` 地址映射：`$0000‑$0FFF` 4 KiB RAM、`$D010‑$D013` PIA、`$FF00‑$FFFF` 256 B ROM、其余开路总线。ROM 由宿主通过 `Apple1Bus::new(&[u8; 256])` 加载，未包含在 crate 中。Woz Monitor hex dump 已从公开仓库核对但未提交。

CPU 已实现功能的完整外部体系（SingleStep 151 万、Klaus 三配置、246+419 pins）本轮未重跑，因为 CPU 未修改。Apple I 未启动。

## M3.4：Woz Monitor 联调与 M3 整体验收

2026-09-10，macOS aarch64；rustc 1.98.1、Cargo 1.98.1。

Woz Monitor 256 字节 ROM 嵌入 `crates/apple1/tests/wozmon.rs` 测试夹具。修复键盘输入队列为 FIFO（原为 LIFO），Display 输出屏蔽 bit 7 以匹配 Apple I 7 位 ASCII 终端行为。CPU 核心未修改。

| 命令／范围 | 实际本地结果 |
| --- | --- |
| `cargo test --workspace` | **112** 个通过（91 CPU/CLI + 21 apple1），0 失败／忽略 |
| `cargo test --workspace --release` | **112** 个通过 |
| `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo run -p hesper` | 演示正常；54 条指令／154 周期 |
| Woz Monitor 启动 | 提示符输出正确 |
| 内存检查 | FF00.FF0F 转储含 D8 58 等 ROM 字节 |
| 内存写入+运行 | $0300 短程序写入后运行，输出 `*` |
| 写入+回读 | $0300 写入 AB CD EF 后回读一致 |

Woz Monitor 的四种基本操作（内存检查、范围转储、写入、运行）均通过实际 CPU 执行验证。没有修改 ROM、拦截 I/O 或伪造设备响应。Apple I 未启动意味着尚未通过 CLI 实时联调，但离线回归已覆盖所有交互路径。

M3 Apple I 文本系统本地目标范围已验收。未远程运行 CI，未提交 ROM，未推送。

## Apple I CLI 交互式回归：Enter 键 CR/LF 处理

2026-09-10，macOS aarch64；rustc 1.98.1、Cargo 1.98.1。

M3.4 记录"Apple I 未启动意味着尚未通过 CLI 实时联调"——本轮首次用真实 PTY 会话跑 `cargo run -p hesper -- apple1 --rom <ROM>` 二进制，发现该缺口不是空白，而是一个实际 bug：`crates/cli/src/apple1.rs` 的交互循环把 `read_line` 得到的整行（含行尾）逐字符原样 `type_char` 进模拟键盘。真实终端处于 canonical 模式时，内核 tty 层的 `ICRNL` 会把用户按下的物理 Enter（CR，`0x0D`）在送达 `read_line` 之前转换成 LF（`0x0A`）；Woz Monitor ROM 只认 `0x0D` 结束一行，从不认 `0x0A`。结果：真实终端里敲 Enter 后 monitor 永远收不到结束信号，命令不会被执行——用非 PTY 管道直接灌入含真实 `\r` 字节的输入可以绕过，这正是此前所有验证（含 `crates/apple1/tests/wozmon.rs`）走的路径，因此从未暴露。

修复：`run_apple1` 在逐字符输入前先剥掉 `read_line` 返回内容的行尾（`\n`、`\r\n` 或 `\r`），逐字符类型化后统一补发一次 `\r`，与既有输出侧的 CR→换行转换（`print_output`）对称。新增 `crates/cli/tests/apple1.rs`：三个测试直接 spawn 构建好的二进制、通过真实管道喂入裸 LF 结尾的命令行（等价于真实终端 ICRNL 转换后到达进程的字节），断言 stdout 里出现预期响应。修复前用 `git stash` 临时还原代码复验：3 个测试里 2 个必然失败（`apple1_cli_boots_to_prompt` 不受影响，因为启动阶段不依赖任何键盘输入）；还原修复后 3 个全部通过。

另外用真实 `hub` PTY 会话手动复验：启动 `cargo run -p hesper -- apple1 --rom wozmon.bin`，敲 `FF00.FF0F` + Enter 正确输出 `FF00: D8 58 A0 7F 8C 12 D0 A9`；敲 `300: AB CD EF` + Enter 再敲 `300.302` + Enter 正确回读出 `0300: AB CD EF`。这是本项目第一次通过真实交互式终端会话（而非库 API 或非 TTY 管道）驱动 Apple I 通过。

| 命令／范围 | 实际本地结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check --workspace --all-targets` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test --workspace` | **115** 个通过（新增 3 个 `crates/cli/tests/apple1.rs`），0 失败／忽略 |
| `cargo test --workspace --release` | **115** 个通过 |
| 真实 PTY 会话：内存检查、写入+回读 | 通过（见上） |

CPU 核心与 `hesper-apple1` 机器模型（`bus.rs`／`pia.rs`／`display.rs`／`keyboard.rs`／`machine.rs`）均未修改，改动仅限 `crates/cli/src/apple1.rs` 的宿主侧终端输入处理。CPU 完整外部体系（SingleStep 151 万、Klaus 三配置、246+419 pins）本轮未重跑，因为 CPU 未修改。未远程运行 CI，本轮按功能点本地提交，未 push。

## M3 继续实现：ROM 资源合规、屏幕模型、物理 RESET 与 CLI raw-mode 生命周期

2026-09-10，macOS aarch64；rustc 1.98.1、Cargo 1.98.1。

按 [roadmap.md](roadmap.md#m3apple-i-文本系统部分实现尚未整体验收) 的接续顺序（M3.1 → M3.2 → M3.3 → M3.4）依次关闭如下项：

**M3.1**：移除 `crates/apple1/tests/wozmon.rs`／`crates/cli/tests/apple1.rs` 内嵌的 256 字节 Woz Monitor ROM；改为从调用者提供的 `HESPER_APPLE1_ROM` 路径加载并核对 SHA-256，测试标记 `#[ignore]`。退役 `tools/extract_wozmon.py`／`make wozmon`，新增 `tools/verify_wozmon_hash.py`／`make wozmon-verify`／`make wozmon-tests`。补齐 `crates/apple1/src/lib.rs` 的固定配置文档与 `crates/apple1/tests/data/README.md` 的资源／向量／许可说明。

**M3.2**：`Display` 新增 40×24 屏幕网格与光标（`screen()`/`cursor()`），CR／写满 40 列触发滚动；新增 6 个 `display.rs` 单元测试。`crates/apple1/tests/machine.rs` 新增 5 个键盘握手测试、1 个 CPU 驱动的显示忙时覆写测试、1 个批次一致性测试（`run_cycles(300)` 与 300×`run_cycles(1)` 结果逐字节相同）。过程中发现并修正 `Pia6821::set_ca1`/`set_cb1` 的真实 bug（状态标志错误绑定在中断使能位上，回归测试见 `pia.rs`）。

**M3.3**：`Apple1::reset` 改为通过 `set_reset_line` 断言／保持／释放并复用 `run_cycles` 的每周期设备推进（`tick_one_cycle`），而不是直接调用宿主 `Cpu::begin_reset()`；签名改为 `Result<(), CpuError>`，所有调用点已更新。`Keyboard::reset` 改名为 `resync`，不再清空排队输入（真实键盘编码器不接到系统复位线）。`crates/cli/src/apple1.rs` 按 `std::io::IsTerminal` 分流：真实终端进入 `crossterm` raw mode 逐键事件循环（Ctrl-C／Ctrl-D 退出、Ctrl-R 物理 RESET、Ctrl-L 仅清宿主终端、Ctrl-P 暂停／继续、Ctrl-N 重建机器）；非终端沿用原逐行读取路径。新增 `signal-hook` 处理 SIGTERM/SIGINT/SIGHUP/SIGQUIT，确保外部信号也能触发 `RawMode` 的终端恢复。

**真实 PTY 验证**（Python `pty.fork`，非 `hub` 管道，也非本文档此前的 `hub` PTY 会话）：

| 场景 | 结果 |
| --- | --- |
| 启动后 `termios.tcgetattr` 读取的 pty 主端属性 | `ICANON=False, ECHO=False`（raw mode 生效） |
| Ctrl-R | 输出 `[RESET]`，重新显示 `\` 提示符 |
| Ctrl-P 两次 | 依次输出 `[PAUSED]`、`[RESUMED]` |
| Ctrl-L | 输出真实 ANSI 清屏序列 `\x1b[2J\x1b[1;1H` |
| 输入小写 `ff00.ff0f` + Enter（不经管道，逐键发送） | 被转大写后由 Woz Monitor 执行，正确输出 `FF00: D8 58 A0 7F 8C 12 D0 A9` |
| Ctrl-N | 输出 `[NEW MACHINE]`，重新显示 `\` 提示符 |
| Ctrl-C | 输出 `[stopped]`，进程退出码 0，pty termios 恢复为 `ICANON=True, ECHO=True` |
| 外部 `os.kill(pid, SIGTERM)`（另一独立脚本） | 输出 `[stopped by signal]`，退出码 0，termios 同样恢复为 `ICANON=True, ECHO=True`——修复前用同一脚本复现：SIGTERM 会直接杀死进程，termios 停留在 raw mode |
| `--max-cycles 60000`（交互模式） | 输出 `[max cycles reached: 60013]`，退出码 0 |

**ROM 复验**：本地缓存一份合法获取的 Woz Monitor ROM 于已忽略的 `.cache/apple1/wozmon.bin`（不提交），设置 `HESPER_APPLE1_ROM` 后：

| 命令 | 结果 |
| --- | --- |
| `python3 tools/verify_wozmon_hash.py .cache/apple1/wozmon.bin` | `OK` |
| `cargo test -p hesper-apple1 --test wozmon -- --ignored` | 4 个通过 |
| `cargo test -p hesper --test apple1 -- --ignored` | 3 个通过 |
| 未设置 `HESPER_APPLE1_ROM` 时运行上两条 | 明确 panic 失败（4／3 个），不是静默跳过 |

| 命令／范围 | 实际本地结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check --workspace --all-targets` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test --workspace` | **121** 个通过，**7** 个 `#[ignore]`（4 个 apple1 wozmon + 3 个 cli apple1，均需 `HESPER_APPLE1_ROM`），0 失败 |
| `cargo test --workspace --release` | 同上，**121** 个通过 |
| `cargo run -p hesper` | 演示正常；54 条指令／154 周期 |

CPU 核心（`hesper-cpu6502`）本轮未修改，因此未重跑 SingleStep 151 万／Klaus 三配置／246+419 pins 完整外部一致性范围。仍未关闭的项（详见 [roadmap.md M3.4](roadmap.md#m3apple-i-文本系统部分实现尚未整体验收)）：Apple I 配置依据来自二级技术资料而非逐页手册核对；RDY 在当前 Apple I 基础配置没有实际驱动方；`--trace`／`--bus-trace` 对 apple1 CLI 仍未实现；显示滚动只在单元测试层面证明，未额外用真实 PTY 会话录制填满 24 行的转录。**M3 整体验收因此仍未勾选**。未远程运行 CI，未推送或发布。

## Woz Monitor 下载与校验工具迁移至 Bun

2026-09-10：以 `tools/prepare_wozmon.ts` 替换并删除 `tools/verify_wozmon_hash.py`。
本节之前的 Python 命令是历史执行记录，不再是当前入口。用户显式运行
`bun tools/prepare_wozmon.ts`／`make wozmon` 时下载公开 HEX 转录，严格解析
256 字节并核对既有 SHA-256 后才写入 `.cache/apple1/wozmon.bin`；
`--verify`／`make wozmon-verify` 只读本地文件。普通构建与测试不下载 ROM。
Make 入口将 ROM 转为绝对路径，避免 Cargo 从 crate 目录运行测试时找不到相对路径。

本地证据（Bun 1.4.0）：

| 命令／范围 | 结果 |
| --- | --- |
| `bun tools/prepare_wozmon.ts`、`make wozmon` | 真实下载成功；256 字节，SHA-256 为 `e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25`；另以 Python hashlib 独立核对 |
| `make wozmon-verify`、`make wozmon-tests` | 校验通过，4 个机器 ROM 测试与 3 个真实 CLI 测试通过 |
| `bun test tools/prepare_wozmon.test.ts` | 1 个离线回归通过：合法 HEX、正确长度但错误哈希的下载不能覆盖已有文件；无真实 ROM 夹具 |
| 临时 CLI smoke（已清理） | 禁止网络时 help／本地校验仍成功；缺文件、错误长度／哈希、非法参数均失败；HTTP／网络／非法 HEX／哈希失败不覆盖已有 ROM，拒绝的下载不新建文件 |
| 临时路径 smoke（已清理） | 真实下载可创建含空格的嵌套目录；Make 校验与 7 个 ROM 测试接受含空格的绝对路径 |
| `make verify` | 格式、全目标检查、debug/release workspace 测试、Clippy、CLI 演示及空白检查全部通过 |

未修改 CPU 语义、固定外部数据或期望 ROM 指纹，未运行完整 CPU 外部一致性层或远程 CI。

## Visual6502 参考驱动重写为 TypeScript，改用 Bun 运行

2026-09-10：`tools/verify_visual6502.cjs`（Node CommonJS）改写为 `tools/verify_visual6502.ts`，
改用 `bun tools/verify_visual6502.ts` 运行；旧文件已删除。逻辑与原脚本逐行对应：同样的
246 组 pins／419 组 reset 场景构造、同样的 `vm` 沙箱加载固定哈希校验过的 6 个上游源文件、
同样的 CLI 参数（`--suite`／`--case`／`--record`）与差异报告格式。未修改任何固定夹具、
哈希或场景数据；仅重写驱动脚本本身。

改写过程中发现并绕开一个 Bun `node:vm` 兼容性限制：Node 的 `vm.createContext` 会把脚本内
顶层 `var` 声明（例如上游 `macros.js` 的 `var memory = Array();`）真正“contextify”为沙箱对象
的同一份绑定，宿主对该属性的**整体重新赋值**（`ctx.memory = newArray`）和脚本内部读取共享
同一存储位置。Bun 1.4.0 的 `node:vm` 沙箱不满足这一点：宿主整体重新赋值只更新宿主侧可见的
属性，与脚本内部闭包实际引用的绑定完全脱钩（用最小复现验证：脚本内声明 `var memory=[]` 后，
宿主 `ctx.memory = [...]` 再调用脚本内定义的 `mRead`/`mWrite` 读到的是重新赋值前的旧绑定，
`vm.runInContext("memory", ctx) === ctx.memory` 为 `false`）。反之，脚本内部执行的重新赋值
（`vm.runInContext("memory = new Array(65536).fill(0xea);", ctx)`）之后，宿主对已存在数组的
**按索引写入**（`ctx.memory[addr] = value`，不整体替换引用）会正确双向同步。修复只改了一处：
把原来的 `c.memory = Array(65536).fill(0xea)`（宿主整体重新赋值）换成等价的
`vm.runInContext("memory = new Array(65536).fill(0xea);", c)`；其余所有内存写入本来就是按
索引写入，未改动。若不加这一处修复，直接把旧 `.cjs` 用 `bun` 运行也会在引导阶段卡死并报
`bootstrap cycle budget exceeded`（已用未修改的旧文件复现，确认问题在 Bun 的 `vm` 而非改写）。

同时把新脚本改为使用本仓库已有的 Bun 原生约定（对齐 `tools/prepare_wozmon.ts`）：
`Bun.file()`/`Bun.write()` 取代 `node:fs` 的同步读写，`Bun.CryptoHasher` 取代 `node:crypto`，
`Bun.argv` 取代 `process.argv`；`node:path` 与 `node:vm` 保留，因为 Bun 没有原生替代。

本地证据（Bun 1.4.0，macOS aarch64）：

| 命令／范围 | 结果 |
| --- | --- |
| `bun tools/verify_visual6502.ts` | 246 组 pins、419 组 reset 全部精确重现，与固定 fixture 序列化逐字节一致；本机本次 34.7s（此前同等 Node 运行约 215s） |
| `bun tools/verify_visual6502.ts --suite pins --case nop-irq-0` | 单场景重放通过 |
| `bun tools/verify_visual6502.ts --suite reset --case reset-registers-stack-wrap` | 单场景重放通过 |
| `bun tools/verify_visual6502.ts --suite reset --record /tmp/hesper-reset-record.json` | 写出 419 条记录到临时路径，行数与内容格式符合预期，未覆盖仓库固定 fixture |
| 错误路径：未知 `--suite`、缺值参数、重复 flag、`--case`+`--record` 同时使用 | 均按原语义报错并以退出码 1 结束 |

同步更新 `Makefile`（`visual6502` 目标）、`README.md`、`AGENTS.md`、
`crates/cpu6502/tests/data/README.md`、`.github/workflows/full-cpu.yml`
（`setup-node@v7` 替换为 `oven-sh/setup-bun@v2`，版本固定 `1.4.0`）中引用该工具的命令。
`docs/references.md` 与本文件更早的历史小节按当时实际使用 Node／`.cjs` 的记录原样保留，
不回溯改写。

未修改 CPU 语义、固定夹具或期望数据；`cargo test -p hesper-cpu6502 --test pins` 相关的
CPU 对照范围本轮未重跑，因为 CPU 未改动。未远程运行 CI，本轮按功能点本地提交，未 push。

## 三个数据准备脚本的 Bun 回归测试与 `make tools-test` 入口

2026-09-11：`tools/prepare_singlestep.test.ts`、`tools/prepare_klaus.test.ts`、
`tools/prepare_visual6502.test.ts` 已随上一节的 Bun 迁移提交（`3771fbd`）加入，本轮不新增
测试文件，只实际运行确认并补上此前缺失的文档入口（`crates/cpu6502/tests/data/README.md`
此前只记录了 `verify_visual6502.test.ts` 与 `prepare_wozmon.test.ts`）。

本地证据（Bun 1.4.0，macOS aarch64）：

| 命令／范围 | 结果 |
| --- | --- |
| `bun test tools/prepare_visual6502.test.ts tools/prepare_singlestep.test.ts tools/prepare_klaus.test.ts`（暖缓存） | 8 个测试、38 个断言全部通过，2.55s |
| 同上两个下载脚本的冷缓存路径 | 临时移走 `.cache/cpu6502/visual6502/d8ecc129…` 与 Klaus `bin_files/6502_functional_test.bin` 后 4 个测试通过（真实下载 4.94s，其中 visual6502 单测 4.14s，暖缓存时仅数十毫秒），随后恢复原缓存目录与镜像 |
| 覆盖范围 | `--help`／非法参数退出码；冷缓存真实下载与暖缓存复用输出逐字节相同；visual6502 按 manifest 核对 7 个模型文件 SHA-256；Klaus `--binary-only` 核对 65536 字节镜像哈希；singlestep 672 条夹具选择与 151 万条全量清单两种校验模式 |
| `cargo test --workspace`（= `make test`，用于确认覆盖边界） | 121 通过、0 失败、7 ignored（共 128 个测试函数：`crates/apple1/tests/wozmon.rs` 4 个与 `crates/cli/tests/apple1.rs` 3 个需 Woz ROM）；不包含任何 `tools/*.test.ts` |
| 新增 `make tools-test`（= `bun test tools/`） | 5 个文件、14 个测试、57 个断言全部通过，12.32s（含 `verify_visual6502.test.ts` 的 `--record` 全量 246 组重放） |

未修改任何脚本、固定夹具、清单或哈希；CPU 未改动，因此未重跑 SingleStep 151 万／Klaus
三配置／246+419 pins 的完整外部一致性范围。Klaus 完整 decimal／interrupt 构建（`make`
与 C 编译器）仍按手动命令执行，不在该回归内。未远程运行 CI，未推送或发布。

同步新增 `Makefile` 的 `tools-test` 目标（不加入 `verify`，因为冷缓存需要联网），并在
`README.md`、`AGENTS.md`、`crates/cpu6502/tests/data/README.md`、
`crates/apple1/tests/data/README.md` 记录该入口与 `cargo test --workspace` 的覆盖边界。

## Apple I 功能级修复：PIA 语义、RESET 丢键、CLEAR SCREEN、严格预算、ROM 身份、trace 与终端网格

2026-09-11：本轮只改 `crates/apple1` 与 `crates/cli`，未触碰 CPU 执行语义。

### 修复前实际复现的缺陷

全部在修复前的代码上实测复现，不是推测：

| 缺陷 | 修复前实测 |
| --- | --- |
| PIA 数据地址读取忽略 CR 第 2 位 | 写 `DDRB=$7F` 后从 `$D012` 读回 `$00` |
| 键盘按非数据访问被"消费" | 未读的 `X` 经两次 RESET 后从 `$D010` 读到 `$00` |
| `--max-cycles` 只在批次之间比较 | `--max-cycles 1` 实际执行到 50013 周期 |
| 每条输入都能买到新批次 | 预算 51000 配 100 行 `F` 实际执行到 262013 周期 |
| 非法参数与越界加载 panic | `--cycles-per-char 0` 与 `load_ram(0x1001, &[1])` 均 panic |
| ROM 无身份校验 | 256 字节但内容错误的文件被 CLI 正常接受并启动 |
| Apple I trace | `--trace`／`--bus-trace` 只打印 "not yet implemented" |
| 终端呈现 | 只写字符流，由宿主终端按自身宽度换行滚动；机器 40×24 屏幕从未被呈现；Ctrl-L 只清宿主画面 |

另有一处文档级事实错误：此前结论"Apple I 没有独立清屏硬件输入"与
*Apple-1 Operation Manual* Section I / KEYBOARD 明确列出的 **RESET** 与
**CLEAR SCREEN** 两个按钮相矛盾，已在 `docs/references.md` 与
`crates/apple1/src/lib.rs` 更正。

### 本地证据

| 命令／范围 | 结果 |
| --- | --- |
| `cargo test --locked --workspace` | 157 通过、0 失败、14 ignored |
| `cargo test --locked --workspace --release` | 157 通过、0 失败、14 ignored |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | 无告警 |
| `make verify`（fmt/check/test/test-release/clippy/demo/diff） | 全部通过 |
| `bun tools/prepare_wozmon.ts --verify .cache/apple1/wozmon.bin` | 256 字节、SHA-256 `e5af0d1c…5c25` |
| `cargo test -p hesper-apple1 --test wozmon -- --ignored`（debug 与 release） | 各 4 通过 |
| `cargo test -p hesper --test apple1 -- --ignored`（debug 与 release） | 各 10 通过 |
| `cargo run -p hesper`／`-- --trace`／`-- --bus-trace --trace-limit 4` | 输出与迁移前逐字一致（54 指令、147+7=154 周期） |
| 真实 PTY 脚本（临时 Python `pty.fork`，非管道） | 14 个场景全部通过 |

新增／改写的设备与宿主回归（离线、不需要 ROM）：

- `crates/apple1/src/pia.rs`：DDR 经数据地址回读、只有外设数据读清对应端口标志（DDR 读／
  控制读／OR 写都不清、也不清另一端口）、`take_port_a_read` 一次性读事件、RESET 保持期间
  清一次寄存器并丢弃写入且任何 CA1/CB1 沿都不置标志、释放后重新可配置、`display_data`
  要求 OR 选择且 DDRB 低七位全为输出。
- `crates/apple1/src/display.rs`：`cycles_per_char` 改为 `NonZeroU64`（零延时在类型上不可
  构造）、CLEAR SCREEN 清屏并归位光标、清屏与在途字符互不影响。
- `crates/apple1/src/bus.rs`：`RamLoadError` 覆盖末字节可加载、跨末地址失败且 RAM 不变、
  `$1000` 空加载成功、`$1001`/`$FFFF` 非法起址返回结构化错误而非 panic，错误文本给出
  Apple I 的 4 KiB 容量。
- `crates/apple1/tests/machine.rs`（17 项）：连续两次 RESET 后 `XY` 各回显一次、已被 CPU 读过
  的键不因 RESET 重放、RESET 线保持跨多个批次期间 PIA 保持复位且未读键不丢、DDRB 非全输出
  时写 ORB 不产生字符、系统 RESET 保留屏幕且在途字符仍完成、CLEAR SCREEN 只动屏幕
  （周期数／寄存器／debug_state／RAM／键盘队列不变），以及脚本化时间线（物理 RESET 断言
  与释放落在指定周期、一个未读键、结束时仍有在途字符）在 1/7/31/64/400 周期分批下逐 `Cycle`、
  `debug_state`、RAM、输出、屏幕、光标、Port B 忙位与总周期数全等。
- `crates/apple1/tests/wozmon.rs`：`run_until_idle` 换成 `run_until(predicate)`，四个场景各等
  具体完成结果（完整提示符 `[5C, 0D]`、两行 16 字节完整转储、`ram_slice()` 确认写入后的完整
  `0300: AB CD EF` 回读行、命令回显之后的 `*`），每次等待预算 1000000 周期，CPU 错误或耗尽
  预算时带最近 64 周期轨迹、寄存器与屏幕失败。
- `crates/cli/src/apple1.rs`（12 个单元测试）：零预算不跑任何周期、预算 1/2/5 精确停在 RESET
  中途、批次边界耗尽后不再执行、机器重建后会话计数与天花板保留、两种 trace 共用一个上限、
  渲染器在 80×30 不重排 40 列、24 行窗口不画状态行、20×10 裁剪且 1×1 隐藏光标、0 宽或 0 高
  不发坐标命令、机器屏幕上的 ESC/BEL 画成空格。
- `crates/cli/tests/apple1.rs`：7 个离线失败用例（零／非数值延时、trace-limit 0/4097/非数值、
  非数值 max-cycles、缺 ROM、255/257 字节 ROM、256 字节错哈希）均非零退出、stderr 有具体
  诊断、stdout 为空；10 个 ROM-gated 用例含 `--max-cycles 0`/`1` 精确报告、预算 51000 配
  100 行输入精确报告 51000、20 周期 bus-trace 给出 20 条含 `$FFFC`/`$FFFD` 的记录、
  `--trace-limit 4` 只留四条、开关 trace 不改变 stdout、`300: 02` + `300R` 触发
  `unsupported opcode $02 at $0300` 且仍报告出错前的记录。

### 真实 PTY 场景（14/14 通过）

临时 Python 脚本用 `pty.fork` 驱动 `target/debug/hesper`，解析渲染器**实际发出**的
alternate-screen／line-wrap／清屏／光标定位／bracketed-paste 序列并还原 40×24 网格，遇到
无法识别的序列即失败；每个会话有墙钟上限。脚本只在执行期生成，不进仓库。

1. 80×30 与 40×24 两种窗口启动 Woz Monitor，按键无需 Enter 即被回显，命令仍以 Enter 执行，
   `300: AB CD EF` 写入后 `300.302` 回读一致。
2. `300: A2 29 A9 41 20 EF FF CA D0 F8 4C 0A 03` + `300R` 输出的 41 个 `A` 在机器第 40 列换行
   （32+9 跨行），宿主第 41 列起始终为空——不按 80 列重排；Ctrl-L 清空机器画面，Ctrl-R 才
   回到 monitor 提示符。
3. `300: A2 1A A9 41 20 EF FF A9 0D 20 EF FF CA D0 F3 4C 0F 03` + `300R` 打印 26 次 `A`+CR 后
   自旋，最终网格第 0..22 行为 `A` 加 39 个空格、第 23 行全空、光标 `(23,0)`——与手工推算的
   滚动结果逐格一致。
4. 暂停期间排入 `300.302` 不执行，恢复后只执行一次；连续按键与 Resize 期间 CPU 仍推进；
   暂停状态下 Ctrl-R 保留屏幕并保持暂停、Ctrl-L 零周期清屏、Ctrl-N 重建后仍暂停。
5. 缩到 20×10 只绘制 10×20 且不越界、机器网格不重排，恢复 80×30 后完整重绘原内容。
6. bracketed paste 的整段文本被送进模拟键盘并执行。
7. stdout 重定向而 stdin 是终端时，输出文件里没有任何 `ESC[` 序列，机器输出仍完整
   （本轮据此发现并修正了 `EnableBracketedPaste` 写进重定向 stdout 的泄漏）。
8. Ctrl-C、预算停止、外部 `SIGTERM` 均退出码 0，unsupported opcode 退出码非 0；四种退出后
   termios 的 ICANON/ECHO 均恢复，主屏幕、光标可见与自动换行均恢复，trace 与错误信息出现在
   离开 alternate screen 之后。

### 明确保留的近似与不宣称的范围

- 视频板 DRAM 刷新时钟（Operation Manual Section III：每 65 个时钟 4 个刷新周期并抑制 Φ2）
  **不建模**；`cycles_per_char` 是固定近似；显示忙时写入按功能级协议覆写在途字符。
- CLEAR SCREEN 是一次性功能动作，不建模按钮脉冲宽度；终端呈现只投影可打印 ASCII，不是原版
  字符 ROM 仿真。
- CLI 固定只接受上述唯一 SHA-256 的 Woz Monitor 镜像；机器库仍接受任意合法 256 字节原创 ROM。
- 本节是**功能级修复通过**，不是 Apple I 整机逐周期准确，也不是完整 MC6821 芯片认证；
  M3 整体验收仍未勾选（缺口见 `docs/roadmap.md`）。
- CPU 执行语义未改动，因此未重跑 SingleStep 151 万／Klaus 三配置／246+419 pins 的完整外部
  一致性范围。未远程运行 CI，未推送或发布。

## Apple I 输入输出固定大写

键盘入队采用 `(c & 0x7F).to_ascii_uppercase()`；显示仅在计时完成的提交点大写化，输出队列与屏幕使用同一个字节，锁存器及 Port B 引脚读回保持原字符语义。CLI 删除按键与粘贴的重复转换，保留既有 bracketed-paste 生命周期。

本地验证：

- `cargo test --locked -p hesper-apple1 --test machine`：18 项通过，覆盖小写重复读取、混合大小写 FIFO、高位字母及标点/控制字符、CPU 独立写入小写后的输出与屏幕。
- `cargo test --locked -p hesper-apple1 --lib`：31 项通过，包括忙/就绪 Port B 分别读回 `0xE1` / `0x61` 而完成显示为 `A`、计时、折行及标点/CR。
- `cargo test --locked -p hesper --lib`：12 项通过；`make verify` 通过（debug/release workspace、fmt/check/Clippy、demo）。
- `make wozmon-verify ROM=.cache/apple1/wozmon.bin`：256 字节，SHA-256 `e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25`；未下载。`make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试、10 项真实 CLI 测试通过，包括 `300: aB cD eF` 的大写回显与 RAM 回读。首次单独用相对 `HESPER_APPLE1_ROM` 路径运行时因测试工作目录不同找不到文件；Makefile 的绝对路径解决了该运行前置问题。
- 真实管道：`--cycles-per-char 1 --max-cycles 2000000`，输入 `300: a9 61 20 ef ff 4c 05 03` 与 `300r` 后 EOF，十秒限时内退出码 0；stdout 包含 `300R`、`0300: A9A` 且无 ASCII 小写。
- 真实 PTY：临时 Python 驱动在 80×25 终端解析实际 ANSI 帧恢复 40×24 网格。逐键输入上述命令，随后 Ctrl-R 回 monitor、Ctrl-L 清屏，再分别发送 `ESC[200~`/`ESC[201~` 包裹的无换行命令并单独按 Enter；两条命令间等待回显。两种输入均显示大写命令与 `0300: A9A`，不是只回显而未执行。
- PTY 确认启动 `ESC[?2004h`、raw 模式及 alternate screen；Ctrl-C 退出码 0，确认 `ESC[?2004l`、离开 alternate screen、恢复自动换行，完整 termios 恢复。初版驱动在会话首领退出后查询 slave 触发 macOS `ENOTTY`；改由仍存活的 PTY 会话包装进程在 CLI 退出后检查 termios，最终完整场景通过。临时驱动已移除。

仅验证本次字符规范化契约；未运行远程 CI，不扩大 Apple I 硬件兼容性声明。

## 2026-09-11 主板刷新与显示时序（完整替换两处近似）

本轮把 `crates/apple1` 的两处用户选定近似——"刷新不建模"与"固定字符延时"——一次性替换为按原图数字边沿驱动的板级模型；唯一运行模式即原板时序，没有保留兼容参数、别名或快进开关。**范围**：数字边沿模型，不声称模拟 TTL 传播延迟、单稳态容差或真实上电随机态。

**依据**：Operation Manual 印刷页 8 REFRESH（每 65 周期 4 个刷新周期、Φ2 被抑制、CPU 保持 Φ1；与 RDY 无关）；原图 terminal sheet 1（drawing 00101）的 D6/D7 计数、H6/H10 选通、2504 循环存储、2519 行缓冲、C7 写入门控与 CR 清行电路；processor sheet 2（drawing 00100）的 CB2→DA、DA→PB7、RDA→B3→CB1；MC6820 印刷页 47 / scan leaf 48 Table 5 与 MC6821 Figure 18 的 CB2 输出模式。原件走线勘误另见 <https://www.willegal.net/appleii/apple1-hardware.htm>（D6/D7 若干输入浮空、VINH 两处标法同网）。

**实现**

- 新增 `crates/apple1/src/timing.rs`：14.31818 MHz 主时钟（一个 master tick 一个晶振周期）、D11 ÷14 字符时钟、D6/D7 的 65 槽水平序列（计数 95–159）、`H6 && H10` 在槽 34/44/54/64 选刷新、262 行垂直帧与 192 行可见区，行/场位置由单一 `vline` 派生以免两个计数器漂移。
- `machine.rs`：公开面改为 `tick()/run_ticks()/master_ticks()/cpu_cycles()/video_frames()/io_pending()` 与 `Tick{cpu,refresh,frame_completed}`；刷新槽不调用 Φ2 相位推进，CPU 停在 Φ2、PIA 无 E、无总线访问；B3 单稳态 51 master tick（`ceil(3.5µs × 14.31818MHz)`），可重触发、在刷新期间照常计时；`reset()` 的保持与完成预算按**真实 CPU 周期**计。删除 `Apple1::cycle/run_cycles/total_cycles`。
- `pia.rs`：CB2 输出握手按 CRB 位 5/4/3 建模（100 写脉冲由 CB1 有效沿释放、101 由下一个 E 释放、110/111 手动、0xx 输入不产生 strobe）；`e_rising_edge()` 由机器在真实 Φ2 调用；`data_lines()` 把未驱动线解析为 TTL 高；CA1/CB1 的有效沿按数据表语义修正为"位 1 选择离开低有效电平的跳变"（`asserted == true` 即引脚为低）。
- `display.rs`：1024 槽循环存储（960 可见 + 64 消隐）、40 字符 2519 行缓冲、C7 请求锁存（DA 上升沿捕获、被接受时清除）、CR 逐槽清到行尾、消隐期清备用槽、滚动＝垂直重载前移显示原点。宿主文本投影另存并行数组，不参与控制逻辑。
- CLI：删除 `--cycles-per-char` 与其测试；`Session` 分别累计会话 CPU 周期与主板时钟，总线 trace 增 `M=<会话主板时钟>` 前缀并保留 `C` 序号；批处理静默条件改为"无输出且 `io_pending()==false` 连续三个**完整视频帧**"，不再用固定周期数。

**时序场景证据**（均为可重复断言，不是一次性观察）

- 一个水平周期 910 master tick、65 个字符时钟、61 次真实 CPU 总线访问、4 个刷新窗；刷新窗内无任何 CPU 总线访问。
- 字符接受时刻由光标槽决定：同一行内连续两个被接受字符的间隔恰为「一帧 + 一个字符时钟」（238,434 tick）；写入后需等终端扫到光标槽，实测常在 2000 tick 内仍未接受、而在下一圈接受。
- 列 34 是刷新槽，其上的字符仍在 Φ2 被抑制时被接受，且 B3 脉冲长度仍为 51 master tick，不被刷新拉长。
- CR 清行期间送入的字符顺延：`AB\rC` 得到 row0 `AB`、row1 `C`（列 0）、光标 (1,1)、输出 `AB\rC`。
- 消隐期清槽：滚动后新底行全空，循环存储中对应槽被清零。
- 真实 ROM 的 `$FFEF` 处 `2C 12 D0 / 30 FB`（`BIT`/`BMI`）确认 PB7=1 为忙。

**命令与实际结果**

- `cargo test -p hesper-apple1 --lib`：51 项通过（含 14 项显示、7 项时序、PIA 握手与有效沿）。
- `cargo test -p hesper-apple1 --test machine`：24 项通过。
- `cargo test -p hesper-apple1 --test timing`：7 项通过（跨设备边界：刷新停钟、刷新槽上的接受、一圈一字符、写入时刻与接受时刻分离、CLEAR SCREEN 与握手重叠、CR 清行中施加 RESET／CLEAR、错误保留已完成输出）。
- `cargo test -p hesper --lib`：12 项通过；`cargo test -p hesper --test apple1`：5 项通过、10 项忽略；`cargo test -p hesper --test demo`：7 项通过。
- `make verify`：**本地检查全部通过**（fmt、check --all-targets、workspace debug 189 项、workspace release 189 项、Clippy `-D warnings`、demo）。
- `make wozmon-verify ROM=.cache/apple1/wozmon.bin`：256 字节，SHA-256 `e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25`（未下载，用缓存）。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试 + 10 项真实 CLI 测试全部通过。
- 真实管道（无速度参数）：`300: aB cD eF` 后 `300.302` → stdout 含完整 `0300: AB CD EF`、命令回显与 `[stopped]`，退出码 0。
- 真实管道（原创程序经 Woz Monitor 的 ECHO 输出）：把 `A9 41 20 EF FF A9 42 20 EF FF 4C 00 03` 存入 `$0300` 后 `300R` → stdout 出现持续 `ABAB…`。
- 真实 PTY（临时 Python `pty.fork` 驱动，解析真实 ANSI 还原网格，GRID_ROWS=30 以容纳状态行）：18 项全部通过——40 列网格、启动提示、26 行带标记文本并滚动、暂停／恢复、CLEAR SCREEN、RESET、重建机器、bracketed paste 输入、Ctrl-C 退出码 0、SIGTERM、`--max-cycles` 停止，以及重定向 stdout 无 ANSI 且含完整转储。会话首领退出时父进程持续读取 master fd 并 `waitpid(WNOHANG)` 回收（macOS 下不读取会卡住退出路径）。临时驱动未入库。

**保留边界与不声称的内容**

- 本轮是数字边沿模型：不模拟 TTL 传播延迟、74123 的元件容差与温度特性、DRAM 单元电荷保持本身、真实上电随机态。
- 未实现 2513 字符字模、D1 像素移位寄存器与复合视频合成：屏幕是字符格投影。
- D8/D9 垂直计数器 preset 的十进制值（图上 191）与滚动时的帧长变化未逐位复现（H18）；滚动只保证发生在同一垂直边界且可见内容／光标结果一致。
- PIA 地址别名（H04）与 Port A/B 读回差异（H12）仍未实现。
- CPU 执行语义未改动，因此未重跑 SingleStep 151 万／Klaus 三配置／246+419 pins 的完整外部一致性范围；`hesper-cpu6502` 自身的 workspace 测试（conformance/cycles/interrupts/pins/official/arithmetic/external）在本轮 `make verify` 中全绿。
- 局部数字模型通过不等于实板示波器对照、模拟电气认证或远程 CI 通过；本轮未运行远程 CI，未提交或推送。

## 2026-09-11 TUI 首版实现（本地 review 交付）

基线为 `c5e14dc`，开始时工作区干净；本节的改动限于 CLI 宿主、自动化入口、测试和说明文档，没有修改 CPU、PIA 或 Apple-1 机器时序。新增 Ratatui 0.30.2（复用既有 Crossterm 0.29）实现中文启动中心、Apple-1 40×24 投影运行页、会话/顶栏菜单、配置页、显示设置、帮助、确认与错误弹窗。配置只保存经校验的绝对 ROM 路径和显示偏好；无参数双 TTY 进入中心，`demo` 固定保留脚本化演示，管道/重定向和 `TERM=dumb` 保留文本宿主。

**实际命令与结果**

- `cargo test --locked -p hesper --lib`：17 项通过。新增覆盖配置 roundtrip（含中文/空格路径）、极小 TestBackend、终端显示宽度截断、粘贴 CR/LF 归一化以及临时菜单和主动暂停的独立性。
- `cargo test --locked -p hesper --test demo`：9 项通过。确认非 TTY 的无参数输出仍与 `demo` 完全一致，显式 `tui` 在管道下失败且不输出控制序列。
- `cargo test --locked -p hesper --test apple1`：5 项通过，10 项真实 ROM 用例按原约定忽略。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：Apple-1 机器 4 项和 CLI 管道 10 项真实 ROM 测试通过；使用既有本地 ROM，未下载。SHA-256 为 `e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25`。
- `make verify`：通过（fmt、workspace debug/release 测试、Clippy、显式 `demo` 和 diff 检查）。另执行 `cargo build --locked -p hesper --release`，通过。

**真实终端记录**

- macOS PTY，`120×40`：`target/debug/hesper` 实际进入备用屏并渲染 `HESPER` 中文启动中心；外部 `SIGTERM` 后观测到 `[stopped by signal]`，且 bracketed-paste、自动换行和备用屏退出序列完整出现。ANSI 转录保存在忽略目录 `.cache/tui-qa/center-120x40.ansi`，未提交。
- P01 的“启动中心出现”和 P14 的“外部信号清理”已实测；P02–P13、P15–P16 的完整人工键盘/ROM/resize 矩阵尚未逐项在桌面终端实测。自动化覆盖了其中的入口、配置、粘贴、暂停、极小尺寸、预算和文本流回归，但不替代人工视觉验收。

未执行 commit、push、MR、远程 CI 或发布。未验证 Linux/Windows，也没有把上述未测 PTY 流程标为通过。

## 2026-09-14 TUI review 九项修复

基线为 `05bdbb6`，开始时工作区干净。按用户要求逐功能点本地提交，改动限于 CLI 宿主及其测试：

- 配置页替换机器前使用已经校验的资源打开确认框，默认取消；取消不修改当前机器、启动资源或保存配置。确认替换沿用同一 `Session`，保留累计预算和 trace；启动错误保留可恢复的故障机器。
- F2/Ctrl+N 等按键引起的状态变化立即请求绘制。重新上电默认取消；确认框为提示和按钮保留固定空间，避免窄窗口或长路径挤掉操作。
- 路径粘贴按配置表单焦点接收，保留中文、空格和字面 shell 符号；多行或控制字符整体拒绝并提示。
- 文件浏览器使用 Ratatui `ListState` 跟随选中项滚动，覆盖第 16 项之后及 512 项边界。
- 旧 Apple-1 交互入口在 `TERM=dumb` 下使用文本流，不再启用屏幕控制。
- 显式真彩色、256 色、单色分别使用对应调色板；auto 保守探测能力并尊重 `NO_COLOR`，显式偏好可覆盖 auto。退出时恢复 Crossterm 原有色彩策略。ASCII 边框使用 `+/-/|`，默认边框使用圆角字符。
- 持续状态和瞬时通知分区显示；弹窗限制在内容区，运行、暂停、故障状态保持可见。
- 帮助、设置、信息页保留有界返回历史，重复打开和重访已有层级不产生导航循环。
- `TraceOptions` 构造统一校验 `1..=4096`；公共 TUI 入口在终端初始化前拒绝非法参数，`Apple1Launch::default()` 使用 64 条的默认上限。

本轮实际验证：

- `cargo test --locked -p hesper --lib --test demo --test apple1`：34 项 library 测试、9 项 demo 测试、5 项非 ROM Apple-1 测试通过；10 项 ROM 测试在此命令中按约定忽略。新增 17 项回归测试，使用原创最小 ROM/程序和 `TestBackend`，常规测试不依赖外部 ROM。
- `make verify`：格式、全目标检查、workspace debug/release 测试、Clippy、demo 和 diff 检查全部通过。转录位于忽略目录 `.cache/tui-review/verify-fixed.log`。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试及 10 项 CLI 真实 ROM 测试全部通过，使用既有本地 ROM，未下载。转录位于 `.cache/tui-review/wozmon-fixed.log`。
- `cargo build --locked -p hesper --release`：通过。
- macOS PTY 分组回归，release 二进制、`120×40`：启动通知过期后 F2/Ctrl+N 立即可见、重新上电默认取消、暂停时 RESET 状态与通知同时可见且通知自动过期、重复帮助返回、配置替换确认与取消、中文/空格路径粘贴、浏览并选择第 18 个文件，均通过实际事件和输出断言。
- PTY 色彩回归：`NO_COLOR=1` 时显式 Truecolor 输出 RGB 序列；Ansi256 输出 indexed 序列且无 RGB；切到 Mono 清除颜色；Ascii 不再输出线框字符。三组 PTY 在 SIGTERM 后均观测到备用屏和 bracketed-paste 清理序列；外层 PTY 宿主在 Hesper 退出后、终端会话关闭前比较 termios，确认恢复原值。相关转录为 `.cache/tui-review/{interaction,paths,colors}-fixed.ansi`。
- `TERM=dumb` PTY 运行在 60,000 周期正常退出，输出没有 ESC 字节；转录为 `.cache/tui-review/term-dumb-fixed.ansi`。

`44×30` 最小布局、缩小窗口、确认按钮可见性和浏览器边界另由 `TestBackend` 验证。上述 PTY 和 buffer 证据不等于完整桌面终端字体/视觉验收；未实测 Linux/Windows，未运行远程 CI。此次按要求执行本地提交，未 push、创建 MR 或发布。


## 2026-09-14 — TUI 视觉布局与上下文提示

本轮基于 `3ae3779`，按截图反馈调整宿主 TUI，未修改 CPU、主板时序或会话预算：

- 根布局随窗口高度伸展，状态与快捷键位于最后两行。启动中心限宽 104 列，宽屏列表固定 26 列；不足 80 列时改为上下排列，主操作始终保留独立行。
- 模拟器名称、介绍、CPU、显示、RAM、ROM 校验状态与实际资源路径分行呈现。Enter 随资源和会话状态执行配置、启动、返回保留会话或运行演示；无效资源进入配置页并显示错误。已有会话仍使用已加载的资源，不重新依赖原文件。
- 文字样式继承所在面板的背景，消除黑色文字底块；列表、菜单和设置的选中项突出整行，主操作使用独立强调样式。保留 Truecolor、Ansi256、Mono 和 ASCII 边框选择。
- 顶部菜单只在获得菜单焦点时高亮。底栏跟随页面、弹窗及暂停状态切换提示；窄屏按优先级省略完整提示，不裁掉半个快捷键。就绪、运行、暂停、故障使用不同强调程度；短暂通知与持续状态保持独立。
- Apple-1 运行区限宽 92 列；隐藏侧栏时为 44 列。屏幕与状态面板对齐，均为 26 行外框，内部字符屏保持 40×24；缩放不改变机器状态。

实际验证结果：

- `cargo test --locked -p hesper --lib tui::tests`：23 项通过。本轮增加 3 项交互/布局回归，覆盖最小尺寸、宽窄布局分界、长中文路径与警告、Enter 的实际去向、菜单焦点及上下文快捷键；缩放同时断言完整字符屏、侧栏对齐、屏幕内容和累计周期不变。
- `make verify`：fmt、全目标 check、workspace debug/release 测试、Clippy `-D warnings`、demo 与 diff 检查全部通过。最终转录：`.cache/tui-visual-qa/verify-final.log`。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试与 10 项 CLI 真实 ROM 测试通过。使用既有本地 ROM，未下载。转录：`.cache/tui-visual-qa/wozmon.log`。
- `cargo build --release --locked -p hesper`：通过，release 二进制已更新。
- release 二进制的 macOS PTY 检查：`44×30`、`80×30`、`120×40`、`180×50` 均通过启动中心、列表切换、真实 demo 结果、菜单、缩小至 `40×24` 后恢复的断言；另验证真实 ROM 启动、暂停与 RESET、返回保留会话、无效 ROM 的可见配置错误。转录：`.cache/tui-visual-qa/visual-results.log`。
- 复跑前轮 PTY 交互回归：F2/Ctrl+N、默认取消、通知过期、帮助返回、资源替换确认、中文路径粘贴、文件浏览滚动、色彩模式与 ASCII 均通过；SIGTERM 后 termios、备用屏、bracketed paste 恢复。转录：`.cache/tui-visual-qa/regression-results.log`。

视觉证据：将实际 PTY 输出的字符、前景色、背景色和强调样式重绘为 PNG，检查宽屏/窄屏启动中心、菜单与运行页。代表图位于 `.cache/tui-visual-qa/launcher-120x40-apple1.png`、`launcher-44x30-apple1.png` 和 `apple1-reset.png`。这些是终端输出重绘图；电脑控制工具因安全限制拒绝访问 Ghostty，未完成 Ghostty 原生窗口和字体的视觉验收。未实测 Linux/Windows 或运行远程 CI。本轮按功能点本地提交，未 push。

## 2026-09-14 — 顶栏下拉菜单与鼠标操作

菜单按标题的终端字符坐标向下展开，宽度随菜单内容适配；绘制与鼠标命中共用几何信息。左键打开、执行或收起，展开后移动鼠标切换分类和高亮项目；F10/F2、方向键、Enter/Esc 保留。鼠标输入服从确认框、错误框、文件浏览器的独占焦点及最小窗口限制。显示设置接入既有 `ui.mouse` 字段：新配置默认开启，已保存的 `false` 保留。`TerminalGuard` 按设置启停鼠标捕获，并负责退出清理；采用现有 [Crossterm 0.29 鼠标接口](https://docs.rs/crossterm/0.29.0/crossterm/event/index.html)，未增加依赖或改动 CPU/机器行为。

PTY 检查另发现：窄屏启动中心的中文字符可能横跨下拉框边缘，仅调用 `Clear` 会留下不合法的宽字符组合，导致实际终端 diff 漏画边框。现于共享弹窗绘制入口清除跨边界字符的两格；回归同时断言原始 frame 和 TestBackend 接收的实际 diff，修复前能复现边框错误。

实际验证结果：

- `cargo test -p hesper --lib --locked`：41 项通过，新增 3 项鼠标/下拉交互回归及 1 项配置默认值/显式关闭回归；涵盖 `44×30`、`80×30`、`120×40`、`180×50` 和两种边框。
- `make verify`：最终代码的 fmt、全目标 check、workspace debug/release 测试、Clippy、demo 与 diff 检查全部通过。转录：`.cache/tui-menu-qa/verify.log`。debug/release 二进制均已更新。
- release 二进制的 macOS PTY：`44×30` 与 `120×40` 通过 SGR 鼠标点击、悬停切换、菜单执行、显示设置开关鼠标、点击外部收起；正常退出后鼠标捕获、备用屏、bracketed paste 和 termios 恢复。
- 既有本地 Woz Monitor ROM 的 PTY：运行时打开菜单停止推进、关闭后恢复、鼠标暂停和重新上电确认隔离通过；菜单打开时 SIGTERM 退出后同样完成终端恢复。未下载 ROM。转录：`.cache/tui-menu-qa/pty-results.log` 与该目录中的 `.ansi` 文件。
- 视觉核对：检查实际终端输出重绘的 `mouse-44x30-session.png` 与 `apple1-mouse-session.png`，下拉框贴合对应标题、边框完整，底部提示仍可见。

上述图像是 PTY 输出重绘；系统 Terminal 的界面控制被工具安全限制拒绝，未完成原生桌面终端字体/鼠标的视觉验收。未实测 Linux/Windows 或运行远程 CI。本轮按功能点本地提交，未 push。

## 2026-09-14 — 固定配置路径

默认配置改为用户主目录下的 `~/.config/hesper/config.toml`，通过现有 `directories::BaseDirs::home_dir()` 定位主目录；不再使用系统特定的配置目录。当前用户的原配置已复制到新位置，ROM 路径与鼠标等偏好保持一致，旧文件保留。

`cargo test -p hesper --lib --locked`（41 项）和 `make verify` 全部通过，debug/release 二进制已更新。release 的 macOS PTY 检查确认：启动读入既有鼠标与 ROM 设置、配置页显示新路径、执行校验保存时实际替换新路径文件且配置内容不变、旧文件未被修改、退出恢复终端。证据：`.cache/tui-config-qa/verify.log`、`pty-results.log`、`config-path-saved.txt` 与 `config-path-fixed.ansi`。本轮按功能点本地提交，未 push。


## 2026-09-14 — BASIC 加载地址与第二组 RAM

Apple I 固定配置增加 `$E000–$EFFF` 的独立可写 4 KiB RAM，与 `$0000–$0FFF` 共计 8 KiB。`--program-address` 支持十进制、`0x`/`0X` 前缀和引号包裹的 `$` 前缀，默认仍为 `$0000`；整段加载必须位于同一块 RAM，跨边界、开路区、PIA、ROM 与 16 位溢出均报错，失败不产生部分加载。文本模式、TUI 资源校验、替换和重新上电共用地址；物理 RESET 保留两块 RAM。

实测发现用户提供的 `basic-c.bin` 在 `$E3D5` 执行 `BIT $D0F2`，此前仅映射 `$D010–$D013` 会把它当作开路总线，从而停在显示忙轮询。按已核对的 [H04 片选条件](apple1/hardware-evidence.md#h04pia-完整选择条件与寄存器镜像)，PIA 现采用 `(addr & 0xF010) == 0xD010`，别名共享寄存器与读取副作用。新增别名回归在修复前失败（经 `$D0F2` 写 DDRB 后，`$D012` 错读为 0），修复后通过。未改 CPU、PIA 握手时序或 BASIC 镜像。

实际验证：

- `make verify`：格式、全目标 check、workspace debug/release 测试、Clippy、demo 和 diff 检查通过；debug/release 各 226 项通过、16 项真实 ROM 测试按约定 ignored。转录：`.cache/basic-qa/verify.log`。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试与 12 项 CLI 测试通过；新增原创程序通过 BASIC 使用的 `$D0F2` 别名输出 `*`，覆盖默认地址、十进制及三种十六进制前缀，另验证禁止跨 RAM 边界和加载到 I/O/ROM。
- TUI 单元回归覆盖高地址程序启动、RESET 保留两块 RAM、重新上电恢复原始字节及地址，并清除运行期间的 RAM 修改。
- `cargo build --locked -p hesper --release`：已更新 release 二进制。实际 debug/release 管道加载用户的 4096 字节 `basic-c.bin` 后，`E000R` 进入 BASIC，`PRINT 1+2` 输出 `3`；保存 `FOR I=1 TO 3` / `PRINT I*I` / `NEXT I` / `END` 四行后，`RUN` 输出 `1`、`4`、`9` 并返回提示符，未耗尽 8000000 周期预算。转录：`.cache/basic-qa/basic-debug.log`、`basic-release.log`、`basic-results.log`。
- release 的 macOS PTY（120×40）验证了同一 CLI 加载地址进入 TUI、BASIC 计算、Ctrl-R 后再次运行、Ctrl-N 确认重新上电后按 `$E000` 重载；启动中心显示 8 KiB RAM 与 `4096 B @ $E000`，正常退出后 termios、备用屏与 bracketed paste 恢复。转录：`.cache/basic-qa/pty-results.log`、`basic-tui-fixed.ansi` 及相关 `.txt` 屏幕快照。

BASIC 文件来自用户本地 `~/Downloads/basic-c.bin`，SHA-256 为 `e423c5c1acff4bea521a72dd4ce4b1435a442cfaad2604b1bfd6edd0fb6d0fc9`；Woz Monitor 使用既有缓存。两份镜像均未改写、下载或提交。这是该镜像的算术与行号程序实测，不是全部 BASIC 语义或其他 BASIC 版本的兼容性认证。未运行完整外部 CPU corpus、远程 CI、Linux/Windows 或原生桌面终端视觉验收。本轮按功能点本地提交，未 push。


## 2026-09-14 — TUI 可编辑程序加载地址

启动配置页新增“程序加载地址”文本字段，CLI 地址仅作为初始值，未指定时显示 `0x0000`。Tab/Enter 可从程序路径进入地址字段，再进入校验或启动按钮；支持普通输入、Backspace、单行粘贴和 Ctrl-U 清空，F4 只用于路径字段。CLI 和 TUI 共用 `parse_program_address`，支持十进制与 `0x`/`0X`/`$` 前缀；格式错误在读取资源前显示，完整文件的 RAM 范围仍由共享加载器校验。启动及替换使用编辑后的地址，重新上电继续使用已确认的资源地址。程序路径及地址保留在当前 TUI 进程内，未增加持久化配置字段。

验证结果：

- `cargo test --locked -p hesper --lib`：44 项通过，新增字段导航/输入/粘贴隔离和非法地址不能替换现有会话的回归；既有重建地址测试继续通过。
- `make verify`：全目标检查、workspace debug/release 测试（各 228 项通过，16 项 ROM 测试按约定 ignored）、Clippy、demo、fmt 与 diff 检查通过。转录：`.cache/tui-address-qa/verify.log`。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试与 12 项 CLI 测试通过，覆盖移动到共享模块后的 CLI 地址解析。转录：`.cache/tui-address-qa/wozmon.log`。
- `cargo build --locked -p hesper --release`：release 二进制已更新。macOS PTY 在 44×30 和 120×40 下仅运行 `hesper tui`，通过配置页输入用户本地 `basic-c.bin` 路径及地址：`0x10000` 显示格式错误，`0xE001` 显示整段越界，改为 `0xE000` 后成功启动；`E000R` / `PRINT 1+2` 实际输出 `3`。宽屏另将地址改为十进制 `57344` 后确认替换会话，BASIC 输出 `5`；重新上电后再次运行输出 `7`。退出恢复 termios、备用屏和 bracketed paste，既有配置文件内容不变。转录和屏幕文本：`.cache/tui-address-qa/pty-results.log`、`editable-address-44-*.txt`、`editable-address-120-*.txt` 及 `.ansi`。

未修改 CPU/Apple I 机器行为或 BASIC 镜像；未运行完整外部 CPU corpus、远程 CI、Linux/Windows 或原生桌面终端视觉验收。本轮按功能点本地提交，未 push。


## 2026-09-14 — 配置表单布局与显式编辑

基于 `8c2e115` 的可编辑加载地址继续调整，配置页改成居中、最大 96 列的卡片；字段、说明、操作按钮与配置文件路径分区。方向键及 Tab/Shift+Tab 负责选择，Enter 进入草稿编辑，Enter 确认后保留焦点，Esc 放弃草稿；未进入编辑时不接收文字或粘贴。输入支持 UTF-8 边界上的光标移动、Home/End、删除和单行粘贴；长路径按显示宽度跟随光标，菜单或浏览器打开时隐藏表单光标。F4 在编辑时选中的文件仍属于草稿，可以撤销。

验证结果：

- `cargo test --locked -p hesper --lib tui::tests`：33 项通过。新增回归覆盖未确认前防误输入、Esc 撤销、中文光标与删除、浏览器返回草稿，以及 44×30、80×30、120×40、180×50 下按钮、路径尾部与光标可见；包含彩色和单色模式、菜单覆盖与恢复。误输入用例在修复前确实失败。
- `make verify`：格式、全目标检查、workspace debug/release 测试、Clippy、demo 和 diff 检查通过。转录：`.cache/tui-qa/config-form-verify.log`。
- macOS cmux 原生终端运行本次 debug 二进制，实际检查卡片布局、蓝色选择态、黄色编辑态与可见光标；方向键进入程序字段，Enter 后输入中文路径，Esc 后恢复未选择程序；加载地址清空并输入 `0xE000`，Enter 确认后停留当前字段。测试退出，无资源启动或配置保存操作。

这次原生终端验证仅覆盖配置编辑流程，不代表完整 P01–P16、Linux/Windows 或远程 CI 验收。未改 CPU 或 Apple I 机器行为，未添加依赖。本轮按功能点本地提交，未 push。

## 2026-09-14：内置 BASIC 预置与程序选择

按用户请求内置其提供的 `basic-huston.bin`，4096 字节，SHA-256 为 `311c85f22996e655ae3a0881e0841a547c52f5ec20cd810035ec91ce13a27cbe`。镜像逐字节保持原样，由 CLI 宿主通过 `include_bytes!` 打包；预置加载到 `$E000–$EFFF`，启动后在 Woz Monitor 输入 `E000R`。TUI 配置页 F3 可选择预置、本地文件或不加载程序；CLI 增加 `--preset basic-huston` 与 `--list-presets`。程序选择不写入个人配置，替换会话沿用既有确认和累计周期预算。

本地验证：

- `make verify`：格式、全目标 check、workspace debug/release 测试各 236 项通过，17 项 ROM 测试按约定 ignored；Clippy、demo 与 diff 检查通过。日志：`.cache/presets-qa/verify.log`。
- `make wozmon-tests`：既有缓存 ROM 下 4 项机器测试与 13 项 CLI 测试通过。新增真实二进制回归运行 `PRINT 1+2` 得到 `3`，行号循环得到 `1、4、9`，正常 EOF 退出。日志：`.cache/presets-qa/wozmon.log`。
- macOS PTY 在 44×30、120×40 下实际从 F3 列表选择 BASIC、取消后重选、确认替换已有会话，随后运行 BASIC 得到 `3`。宽屏另外验证物理 RESET 后 `E2B3R` 暖启动保留行号程序、`RUN` 得到 `42`，重新上电再运行 BASIC 得到 `7`；退出恢复 termios、备用屏与 bracketed paste，既有个人配置内容不变。日志与屏幕文本：`.cache/presets-qa/pty-results.log`、`preset-*.txt`。
- 离线回归检查镜像哈希、完整 4 KB 加载、预置与本地文件切换、固定地址、RESET 保留／重建恢复原始字节，以及窄屏标题和选择列表可见。`cargo build --locked -p hesper --release` 已更新本地可执行文件。

本轮只改宿主加载与选择界面，未添加依赖。未运行完整外部 CPU corpus、远程 CI 或 Linux/Windows 终端验证。本轮按功能点本地提交，未 push。

## 2026-09-14：内置 apple1software.com 全部程序

按用户请求把 [The Apple-1 Software Library](https://apple1software.com/) 发布的全部程序内置为预置：
4 个分类（Games 游戏 / Fun 娱乐 / Programming 编程 / Utilities 工具）、42 个程序页、56 个 RAM 数据块，
共 74 983 字节，2026-09-14 一次性下载后逐字节保存，未改写、未重定位、未重新汇编。

- 存储：`crates/cli/assets/programs/<分类>/<id>-<地址>.bin`，一个 RAM 块一个文件。来源是站点自己的
  Wozmon 传输接口 `/{category}/{slug}/wozmon?basic=false&autostart=true`（JSON 内 base64），即站点
  通过 Web Serial 发给真机的同一份数据。宿主只用 `include_bytes!` 打包，CLI 与 TUI 没有任何网络代码；
  Woz Monitor ROM 仍由 `--rom` 外部提供，未内置。
- 元数据与启动命令：`crates/cli/src/presets.rs` 的 `ProgramPreset` 逐条记录分类、作者、年份、站点标注
  的许可证、来源页、载入地址、启动命令与 `needs_expansion`；`--list-presets` 与 TUI 程序列表都直接读它。
- 启动命令取站点清单最后一行（autostart 的 `xxxxR`）。八个 BASIC 语言程序（Blackjack、Dobble、
  Hamurabi、Mini-Startrek、Lunar Lander ASCII Graphics、Twinkle、Resistor Calculator、Stopwatch）在
  站点上总是与 BASIC 一起传输，因此预置按同样方式把站点 Huston BASIC 写到 `$E000`（4096 B，
  SHA-256 `311c85f2…`，与用户 2026-09-14 提供的 `basic-huston.bin` 逐字节相同，故只保留一份），并保留
  站点的 `$004A` 磁带头块；启动命令是站点给出的 BASIC 热入口 `E2B3R`，随后 `RUN`。
- 宿主加载从「单地址单镜像」改为多块：`ProgramImage` 先校验每个块、再一次性写入，任一越界时在任何字节
  写入前失败。
- `little-tower` 的站点列表是 `$0300–$14CD`，需要 `$1000–$1FFF` 扩展内存卡；本机只建模
  `$0000–$0FFF` 与 `$E000–$EFFF` 两块 4 KiB RAM。该预置保留并标记 `needs_expansion`，选择列表写明原因，
  加载以 RAM 分组错误被拒绝——不裁剪镜像、不伪造可运行。
- TUI：程序列表按四个分类分组，分类表头不可选（↑↓ 跳过表头、←→ 在分类间跳转并在两端停住）；下方详情栏
  显示作者/年份、载入范围、**启动命令**、来源页与许可证；配置页「程序」字段下方显示
  `预置 N B · 启动后输入 xxxxR`；启动成功后状态提示同样带上启动命令。

本地验证：

- `make verify`：格式、全目标 check、workspace debug/release 测试、Clippy、demo 与 diff 检查通过。
  转录：`.cache/presets-library-qa/verify.log`。
- `cargo test --workspace`：debug 与 release 各 239 项通过，20 项真实 ROM 测试按约定 ignored。新增
  预置回归覆盖 42 条唯一 id、每块落在单一 RAM 分组、启动地址落在该预置自己写入的块内、
  `needs_expansion` 与分组校验一致，以及 `basic-huston` 镜像哈希；TUI 回归覆盖分类表头不可选中、
  上下键不落在表头上、左右键按站点分类顺序移动，以及绘制行数与分页索引一致。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试与 16 项 CLI 测试通过。新增三项真实
  二进制回归：`--preset resistor-calculator` 在 `E2B3R` 后 `RUN` 输出 `THE RESISTOR CALCULATOR` /
  `CREATED BY PAOLO DI LEO`（BASIC 与程序块确实一起载入并可执行）；`--preset 15-puzzle` 在 `0300R` 后
  输出 `15 PUZZLE - BY JEFF JETTON` 与 `INSTRUCTIONS (Y/N)?`；`--preset little-tower` 以
  「must fit within one Apple I RAM bank」被拒绝。转录：`.cache/presets-library-qa/wozmon.log`。
- 56 个资产文件的 SHA-256 与本轮下载的站点 listing 逐块比对一致（42 个程序、74 983 字节）；逐文件哈希与
  来源页记录在 `crates/cli/assets/README.md`，可用其中的 curl 命令复核。
- 实际二进制冒烟（debug）：`--preset hamurabi` 在 `E2B3R` 后 `LIST` 输出带行号的完整程序、`RUN` 输出
  `TRY YOUR HAND AT GOVERNING ANCIENT SUMERIA…`；`--preset blackjack` 在 `E2B3R` 后 `LIST` 输出
  `10 DIM A$(13)…`（覆盖 `$0800` 程序块）。
- macOS 真实 PTY（120×40，F10 → Apple-1 配置 → F3）：29 项断言全部通过——分类表头与四个分类的
  左右键跳转、详情栏的载入范围与启动命令、源码页与许可证、选中 BASIC (Huston) 后配置页显示
  `启动后输入 E000R`、替换运行中会话先确认、状态提示携带启动命令、`E000R` 后 `PRINT 6*7` 实际输出
  `42`、退出码 0，以及 termios、备用屏与 bracketed paste 恢复且无非预期转义序列。临时脚本与日志：
  `.cache/presets-library-qa/pty_picker.py`、`pty.log`。
- `cargo build --locked -p hesper --release`：本地 release 可执行文件已更新。

本轮只改宿主资源与界面，未改 CPU 或 Apple I 机器行为，未添加依赖。许可证状态：站点只在 8 个页面声明
许可证（6 个 MIT、2 个 Custom License），其余 34 个页面——包括全部历史磁带程序与 Apple BASIC——未声明；
仓库按站点发布原样保留、逐文件记录来源与哈希，不主张新的许可证、公有领域状态或额外兼容性。未运行完整
外部 CPU corpus、远程 CI、Linux/Windows 或原生桌面终端视觉验收。本轮按功能点本地提交，未 push。


## 2026-09-14：Apple I 现状复核与文档同步

本次文档更新前，在同一会话核对了当前源码并实际运行以下命令；这些结果属于当前固定配置，不扩展为完整硬件或全部预置程序的兼容性认证：

- `cargo test --workspace --locked --offline`：239 项通过、20 项真实 ROM 测试按约定 ignored。
- `cargo test --workspace --release --locked --offline`：同样 239 项通过、20 项 ignored。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：使用既有本地 ROM，4 项机器测试与 16 项 CLI 测试全部通过；未下载或修改 ROM。
- `cargo build --locked --offline -p hesper` 后运行真实 debug 二进制：Woz Monitor 写入并回读 `$0300` 的 `AB CD EF`；`--bus-trace --trace-limit 4` 只在 stderr 保留 4 条记录；Huston BASIC 的 `PRINT 6*7` 输出 `42`，行号循环输出 `1、4、9`；15 Puzzle 输出标题及 `INSTRUCTIONS (Y/N)?`。这些场景在 8000000 周期预算内正常结束；`little-tower` 因超出单一 RAM 分组以退出码 1 被拒绝。
- macOS 真实 PTY（120×40）：从配置页选择预置、左右切换四个分类、确认替换已有会话后启动 Huston BASIC；机器字符屏出现 `>PRINT 6*7` 和独立结果行 `42`。正常退出码为 0，ICANON/ECHO 恢复，输出包含离开备用屏和关闭 bracketed paste 的序列。

PTY 复核发现此前缓存脚本 `.cache/presets-library-qa/pty_picker.py` 的 BASIC 判据过宽：在整个终端网格搜索 `42` 会命中侧栏周期数，机器只回显 `E0` 时就可能报告成功。因此上节的“29 项断言通过”不能单独证明 BASIC 计算完成。本次临时副本将判据限定为机器字符屏单元去空白后严格等于 `42`，重新执行后观察到完整命令和结果。缓存脚本未改，临时副本已删除；这不是 BASIC 执行器故障，也不是原生桌面终端的视觉验收。

随后仅更新文档：README 补齐 Apple I 能力、依赖方向、ROM 联调命令与预置限制；路线图移除已失效的刷新／固定字符延时／CB2 缺口描述；架构摘要同步三个 crate 的职责；硬件依据明确 H12 差异在 Port A 输出模式，而非 Port B 输出锁存读回。H12、H18 和视频／电气边界仍保留，未勾选 M3 整体验收。历史测试结果和提交状态不因本次文档同步而改写。

文档检查：4 份说明的 44 个本地链接及相应锚点、代码围栏和末尾换行通过检查；Bun Markdown 渲染通过，表格列数一致。`cargo run --locked --offline -p hesper -- apple1 --help` 与 `--list-presets` 实际执行成功，确认 README 使用的选项、预置 id 和 BASIC 启动命令。

未修改源码、测试或程序资产，未新增依赖；文档更新后未重跑 Rust 全套检查或完整外部 CPU corpus，未运行远程 CI、Linux/Windows 或原生桌面终端视觉验收。本轮按功能点本地提交，未 push。

## 2026-09-14：PIA 读回路径按数据表建模（H12）

对本轮提出的两项数字硬件差异逐条复核。**H12 的一手原文**取自 P21（MC6821 DS9435R5，`Motorola_MC6821_NMOS_Peripheral_Interface_Adapter_1985_Motorola_djvu.txt` 第 3168–3190 行）的 PORT A-B HARDWARE CHARACTERISTICS：**"Notice the differences between a Port A and Port B read operation when in the output mode. When reading Port A, the actual pin is read, whereas the B side read comes from an output latch, ahead of the actual pin."** 同节另述 A 侧按 CMOS 30%–70% 电平驱动并带内部上拉（输入模式仍连接），B 侧为三态 NMOS 缓冲、无上拉、输入模式浮空。

代码改动（`crates/apple1/src/pia.rs`）：Port A 的读路径抽成 `pin_a_levels()`——返回**实际引脚**电平，输出位由 PIA 输出缓冲驱动（因此引脚电平即 ORA），输入位为外设电平 `pins_a`；Port B 的读路径抽成 `port_b_read_levels()`——输出位读 ORB 输出锁存、输入位读引脚。`read_port_a_data` / `read_port_b_data`、模块文档与 `pins_a` 字段文档同步更新；`lib.rs` 的模块摘要同步。

测试：`port_a_read_returns_the_pin_driven_by_ora_and_by_the_keyboard`（两条驱动源合成一次读）、`port_b_read_returns_the_output_latch_while_bit_7_reads_the_pin`（锁存位不受输出线上外加电平影响，PB7 仍读引脚）。

**结论与更正**：这两种读法的表达式形状相同，只有在 PIA 之外另有器件争用同一输出线时才会出现可观察差别；本模型不表示外部争用，也不表示 A 侧上拉与 B 侧浮空。因此「Port A 输出模式读 ORA 而非实际引脚」应更正为：Port A 的输出位读的就是引脚，而在无争用的数字模型里该引脚电平由 PIA 输出缓冲决定，即 ORA。H12 作为**读回路径**不再是未实现项；剩余的电气边界（争用／上拉／浮空）已明确记为不建模。

**H18 复核**（M76 Sheet 1/3 TERMINAL SECTION，scan page 13）：D8、D9 为两只 74161；预置输入接线为 D8 的 P0–P3 共接一条网络、D9 的 P2 接地图符、D9 的 P0/P1/P3 与 D8 同网，据此预置字为 1011 1111 = `$BF` = 191（65 个计数正好走满这对 74161），与既有「图上 191」一致；D8 的 Q0–Q3 与 D9 的 Q0/Q1 标为 V0–V5，D9 的 Q2/Q3 进 D10（7400）两输入，TC 标 `LAST`，PE（低有效）由 D8/D9 左下方的门电路驱动，MR 走另一条独立网络。该共享预置网络在本次裁剪范围外的终点与重载译码未追到，**滚动是否改变预置、以及滚动帧长变化均未确认**，因此 `timing.rs` 的固定 262 行帧本轮未改动。这是保持不变的边界，不是本轮修复项；把它写成实现需要先补上原图证据。

| 命令 | 实际结果 |
| --- | --- |
| `cargo test -p hesper-apple1 --lib` | 55 项通过，含上述两条新读回路径测试 |
| `cargo test -p hesper-apple1` | 全部通过；4 项真实 ROM 测试按约定 ignored |
| `cargo test --workspace --locked --offline` | 240 项通过、20 项 ignored |
| `cargo test --workspace --release --locked --offline` | 240 项通过、20 项 ignored |
| `make verify` | 格式检查、全目标 check、debug／release 测试、Clippy `-D warnings`、demo 与 diff 检查全部通过（`verify: 本地检查全部通过`） |

CPU 核心（`hesper-cpu6502`）本轮未修改，因此未重跑 SingleStep 151 万／Klaus 三配置／246＋419 pins 的完整外部一致性范围。未运行远程 CI，未做原生桌面终端视觉验收。资料核对使用 M76 与 P21 的公开扫描件；除上述结论外未从扫描件推断其它电气参数。本轮按功能点本地提交，未 push。

## 2026-09-14：升级 TOML 配置依赖

仅将 `crates/cli/Cargo.toml` 的 `toml` 约束由 `0.9` 升至 `1.1.6`；`cargo update --offline -p toml --precise 1.1.6` 将锁文件中的 `toml` 更新为 `1.1.6+spec-1.1.0`、`toml_datetime` 更新为 `1.1.1+spec-1.1.0`，移除 `winnow 0.7.15`。`sha2 0.10.9`、`signal-hook 0.3.18` 保持不变；未改 Rust 源码、配置 schema 或程序资产。

本地验证：

- `cargo test --locked --offline -p hesper config::tests`：2 项配置测试通过。
- 临时 Rust 冒烟程序直接引入当前 `config.rs`，实际调用 `config::load` / `save`：3 组旧版 TOML 生成的配置可由新版读取，修改后保存的文件仍可由旧版读取；覆盖 Unicode／空格路径和鼠标偏好，修改前的新旧序列化文本一致。未知字段、错误字段类型、不支持的 schema 版本、损坏语法共 4 组输入均被拒绝。仅使用临时配置文件，未访问个人配置；临时项目已删除。
- `make verify`：格式、全目标检查、debug／release 各 240 项测试、Clippy `-D warnings`、demo／trace／bus-trace 和 diff 检查全部通过。每种构建的 20 项真实 ROM 测试按约定 ignored。

本轮未改 CPU／机器时序，未运行完整外部 CPU corpus、ROM-gated 测试、远程 CI 或跨平台／真实终端验收。本轮本地提交，未 push。

## 2026-09-14：配置页默认聚焦程序字段并按 Enter 打开选择框

仅改宿主 TUI（`crates/cli/src/tui.rs`）：`ConfigForm` 初始焦点由 `Rom` 改为 `Program`；`config_key` 中 Enter 对 `ConfigFocus::Program` 的处理去掉 `preset.is_some()` 守卫，无论是否已选预置都直接 `open_programs()` 打开「选择程序」弹层（即用户截图中的选择框）。相应文案同步：02 字段右侧徽标恒为 `Enter 选择`、字段提示改为 `Enter 选择预置 · F4 浏览本地文件`、默认状态行改为 `Enter 选择程序；其他字段 Enter 编辑；F4 浏览路径。`、页脚 Enter 提示在程序行恒为「选择」、帮助页补充「配置页默认聚焦程序字段，直接按 Enter 即打开预置程序选择框」。其余字段行为不变：ROM／加载地址 Enter 仍进入编辑，F3 仍为全局快捷键，弹层内 Esc 返回表单。

受影响测试按新契约更新而非删除：`config_fields_require_enter_and_escape_cancels_only_the_edit` 断言默认焦点为程序行、裸 Enter 打开弹层、Esc 回到表单，本地路径编辑经弹层「本地二进制文件…」行进入；`path_paste_targets_only_the_focused_field_and_preserves_literal_text` 同样改走弹层；ROM 编辑相关用例显式设置 `ConfigFocus::Rom`。

| 命令 | 实际结果 |
| --- | --- |
| `cargo test -p hesper --lib tui` | 36 项通过（18 项非 tui 过滤） |
| `cargo test --workspace` / `--release` | 各 240 项通过、20 项真实 ROM 测试按约定 ignored |
| `make verify` | 格式、全目标 check、debug／release 测试、Clippy `-D warnings`、demo 与 diff 检查全部通过。转录：`.cache/tui-config-enter-qa/verify.log` |
| `make wozmon-tests ROM=.cache/apple1/wozmon.bin` | 4 项机器测试与 16 项 CLI 测试通过 |
| macOS 真实 PTY（120×40，`cargo run … apple1 --rom …` → F10 → Apple-1 配置） | 6 项断言全部通过：表单首帧 02 行带 `Enter 选择` 徽标且无编辑器打开；裸 Enter 立即出现「选择程序」弹层（含分类与 15 Puzzle）；Esc 返回表单；Shift+Tab 到 ROM 行后 Enter 进入 `编辑中` 而非弹层。脚本与转录：`.cache/tui-config-enter-qa/pty_enter.py`、`pty_enter.log`。断言限定在弹层／表单可见文本，未做全屏子串匹配 |

本轮未改 CPU、Apple I 机器行为、配置 schema 或程序资产，未添加依赖；PTY 验收覆盖配置页 Enter 路径，不代表完整 P01–P16、Linux/Windows 或原生桌面终端视觉验收。本轮本地提交，未 push。

## 2026-09-14：条件垂直重载与滚动帧长修复（H18）

补核 M76 Sheet 1/3 的 C10、C9、D8/D9、D15 与 C7 写控制路径，并以 TI 74161／74174 功能说明核对同步边沿规则。共享预置源不是恒高：`P = NOR(D6.Q3, /VBL)`，P=1 加载 `$BF`、P=0 加载 `$00`；`/LOAD = /WC1 OR /VBL`。这更正了上节只追到预置位、未追到网络终点的阶段性结论。原图裁剪位置、器件数据表和原板走线勘误链接见 [硬件依据 H18](apple1/hardware-evidence.md#h18-补充器件依据与独立旁证)；独立 VHDL 重建仅作为旁证，不替代原图或实板验证。

实现变更：

- `timing.rs` 用 D8/D9 八位计数与 D15 使能代替固定 262 行取模；同步 LOAD 优先于计数使能，加载零不伪造帧完成。MEMΦ 独立于 CPU 刷新，改为扫描线 7 的 40 拍可见串与每条指定消隐扫描线的 8 拍，而非首扫描线／每消隐行一拍。
- `display.rs` 的物理读头仅在 MEMΦ 前进，CR／写控制反馈进入实际垂直计数器；屏幕原点从读头与光栅的相对位置推导。删除 `scroll_pending`、独立加 40 的滚动和未被读取的整行缓存，不把字符投影冒充 2519／2513／D1 像素链。`Timing::tick` 调用与直接构造旧事件的测试均已迁移。
- DA 撤销会取消未接收请求，修复新 MEMΦ 相位下 RESET 后遗留请求把浮空数据线写入屏幕的问题；既有 `reset_clears_a_character_offered_but_not_yet_taken` 回归通过。CLEAR 仍是原子宿主动作，取消 CR／写控制并按当前光栅重对齐空白循环存储，不改变 CPU、PIA 或板时钟。
- 光标在重载前可短暂进入备用槽；TUI 与既有文本终端一样，只在可见字符区域内显示光标。保留工作树已有的配置页 Enter 改动及其验证记录，未暂存或覆盖它们。

先加入机器回归 `bottom_row_cr_reloads_the_scan_and_advances_the_carousel_one_row`：旧实现实际返回 238420 tick，期望 239330 tick，测试失败；修复后通过。计数器层逐 tick 验证如下，额外 40 拍确实重复光栅槽 920–959，而不是只修改帧计数：

| 计数器场景 | 扫描线 | master tick | MEMΦ |
| --- | --- | --- | --- |
| 正常帧 | 262 | 238420 | 1024 |
| V192/H95 注入一次 BF 同步重载 | 263 | 239330 | 1064 |

机器层另以真实 PIA 握手验证隔离的底行 CR、底行末列可打印折行及后续字符、三个连续底行 CR 的完整屏幕与输出顺序，以及 CLEAR 在重载前／刚重载后的两侧介入：后续三帧恢复正常长度，屏幕无幽灵滚动，随后字符稳定落在左上角。**263 只对应所列隔离条件，连续滚动没有硬编码此帧长。**

| 命令或场景 | 实际结果 |
| --- | --- |
| `cargo test --locked --offline -p hesper-apple1 --test timing` | 11 项通过，含上述边界回归 |
| `cargo test --locked --offline -p hesper-apple1` | 92 项通过；4 项真实 ROM 测试按约定 ignored |
| `make verify`（此前执行 `cargo fmt --all`） | 格式、全目标 check、debug／release 各 246 项测试、Clippy `-D warnings`、三种 demo 与 diff 检查全部通过；每种构建的 20 项真实 ROM 测试按约定 ignored |
| `make wozmon-tests ROM=.cache/apple1/wozmon.bin` | 4 项机器测试与 16 项 CLI 测试通过；使用既有 ROM，未下载或修改镜像 |
| macOS 真实 PTY，120×40，实际 debug 二进制 | Woz Monitor 用 `300R` 进入临时 19 字节回显程序；40 个 A 后的 B 落在下一行首列；清屏后输入 A–Z 各一行再输入 DONE，整个 40×24 机器面板严格等于 D–Z 加底行 DONE，并在继续运行时保持；再次 CLEAR 后 HOME 稳定出现在左上角 |
| 同一 PTY 正常退出 | Ctrl-C 后确认退出，退出码 0，ICANON／ECHO 恢复，离开备用屏并关闭 bracketed paste，ANSI 网格解析无未知序列 |

PTY 判据只比较机器面板，不从侧栏周期数推断字符输出。临时回显程序复用 Woz Monitor 已配置的 PIA，并先等待先前显示握手结束；清屏、折行、滚动均由实际运行的机器完成，没有直接设置屏幕。首次临时程序误把已选择 ORB 的 `$D012` 当作 DDRB，修正了探针的初始化协议；退出探针也补齐既有确认弹层，未为这些探针问题修改产品行为。

README、架构、路线图、硬件依据与 Apple I 概览已同步。CPU 核心与其总线／指令语义未改，未重跑 SingleStep／Klaus／Visual6502 完整外部层；未运行远程 CI、跨平台或原生桌面终端视觉验收。C7／TTL 亚字符传播、物理 CLEAR 按钮脉宽、2519 行重放、2513／D1 像素链、DRAM 电荷保持及上电随机态仍未认证，M3 整体验收保持未勾选。临时探针与转录清理后不纳入仓库；没有新增依赖或程序资产。

本轮按功能点本地提交，未 push。

## 2026-09-15：PA7 板级固定高电平修复（H09）

M76 印刷页 7 的 KBD/DSP Interface 图明确将 PA7 接到 +5V；它不是随首次键盘输入才出现的高位，也不依赖通用 PIA 的内部上拉模型。修复前原创 6502 程序选择 Port A 数据寄存器、执行 `LDA $D010` 并写入 RAM，首次按键前得到 `$00`，输入 A 后才得到 `$C1`。

`Apple1Bus::new` 现在通过既有输入接口初始化 PA7；通用 `Pia6821::new`、DDR／端口读回逻辑、键盘选通和 B3 的 51 master tick 均未修改。键盘呈现字符时保留 PA7，物理 RESET 保留外部引脚电平，机器重建重新建立板级接线。同步键盘与引脚观察接口注释、架构、路线图和 H09 依据；区分固定数字接线、尚未实现的驱动／高阻解析与模拟电气边界，不据此勾选 M3 整体验收。

验证：

- 新增 `pa7_is_high_before_the_first_key_across_reset_and_recreation`，通过真实 CPU 总线读取覆盖首次按键前、未按键时再次 RESET、输入 A、按键后 RESET 和重建。先在旧实现运行：首个断言实际为 0、期望 128，失败；修复后 `cargo test --locked --offline -p hesper-apple1 --test machine` 的 25 项全部通过。
- 临时 Rust 程序重跑原始 CPU→PIA→RAM 复现：首次按键前 `$80`、未按键 RESET 后 `$80`、输入 A 后 `$C1`、再次 RESET 后 `$C1`、重建后 `$80`；独立 PIA 默认输入仍为 `$00`。临时源码与可执行文件已删除。
- `cargo fmt --all` 后执行 `make verify`（`CARGO_NET_OFFLINE=true`）：格式、全目标 check、debug／release 各 247 项测试、Clippy `-D warnings`、demo／trace／bus-trace 与 diff 检查全部通过；每种构建的 20 项真实 ROM 测试按约定 ignored。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试与 16 项 CLI 测试通过，使用既有缓存，未下载或修改 ROM。

本轮未改 CPU 执行语义、B3 时序或终端界面；未运行完整外部 CPU corpus、远程 CI、真实 PTY 或实板电气对照。没有新增依赖。

本轮按功能点本地提交，未 push。

<a id="preset-compatibility-2026-09-15"></a>

## 2026-09-15：预置兼容性分级与逐项矩阵

补齐[全部 42 项的兼容性矩阵](../crates/cli/assets/README.md#compatibility-matrix)：
加载范围、运行条件／已知限制、可引用的局部证据、尚未验收的功能各自独立记录。
未运行的程序明确写未验证；历史 LIST、标题与开场证据不升级为游戏或工具功能通过。
旧节中的 `needs_expansion` 是当时实现，当前以 `ProgramPreset::limitation` 取代：
`UnmappedLoad` 表示镜像越界，`UnmappedTestRam` 表示能加载但诊断目标 RAM 缺失，
无已知限制也不表示完整验收。CLI 列表与 TUI 详情使用同一说明，不以说明字段替代加载器的真实范围校验。

定向运行使用当前 debug 二进制和既有 `.cache/apple1/wozmon.bin`，执行前核对 256 字节
WozMon 的 SHA-256 为 `e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25`；
未下载、修改或内置 ROM。每项上限 8000000 CPU 周期：

| 预置 | 实际输入 | 实际输出与停止原因 |
|---|---|---|
| `memory-test-1000-1fff` | `0280R\n` | `00 1000 00 10` 后返回 Monitor；stderr `[stopped]` |
| `memory-test-0009-027f` | `0280R\n\n` | `PASS 01` 至 `PASS 06`；stderr `[max cycles reached: 8000000]` |
| `memory-test-03a2-0fff` | `0280R\n\n` | `PASS 01`；同上 |
| `memory-test-e000-efff` | `0280R\n\n` | `PASS 01`；同上 |

`00 1000 00 10` 按[程序说明](https://apple1software.com/utilities/memory-test/1000-1fff/)解读为：
全零测试在 `$1000` 期望 `$00`，实际读到 `$10`。这是对缺失目标 RAM 的预期诊断，不是
“预置能加载，所以 RAM 可用”。四项进程退出码都是 0，因此验收断言检查具体输出与停止原因。
最初对已映射范围只输入启动行时，批处理宿主在三个静默帧后读到 EOF，没有等到完整测试；
追加一个诊断不消费的换行，保持 I/O 待处理，使其运行至显式周期上限。未为此修改批处理策略。
矩阵附有可重放命令。

界面与回归验证：

- `--list-presets` 实际输出覆盖全部 42 个 id：1 项不可加载、1 项可加载但预期诊断报错、
  40 项提示功能未完整验收；矩阵 42 行无遗漏／重复，顺序与资源清单一致。
- 真实 macOS PTY，120×40 与 44×30：从运行中会话进入配置页和程序选择器，依次选择
  15 Puzzle、Little Tower、Memory Test (1000-1FFF)。三个兼容性提示与启动命令在两种宽度下
  均完整可见；确认内存诊断预置后能替换原会话并实际加载。此处只证明 TUI 选择及加载，
  上述诊断执行结果来自真实 CLI 字符流，不冒充 TUI 中执行完成。
- 两种尺寸均正常退出码 0，ICANON／ECHO 恢复、离开备用屏并关闭 bracketed paste；
  ANSI 网格解析无未知 CSI。没有用侧栏数值或退出码替代业务判据。
- 定向测试：`cargo test --locked --offline -p hesper --lib presets` 的 3 项、
  `presets_can_be_listed_without_rom_and_conflicting_options_are_rejected` 的 1 项通过。
  移除清单测试对显示文案的固定断言，保留全部 id 可发现、顺序一致与参数冲突的行为断言。
- `cargo fmt --all` 后执行 `make verify`（`CARGO_NET_OFFLINE=true`）：
  格式、全目标 check、debug／release 各 247 项测试、Clippy `-D warnings`、
  demo／trace／bus-trace 与 diff 检查全部通过；每种构建的 20 项真实 ROM 测试按约定 ignored。
- `make wozmon-tests ROM=.cache/apple1/wozmon.bin`：4 项机器测试与 16 项 CLI 测试通过，
  包括 `little-tower` 继续被拒绝，以及 Huston BASIC 算术／循环、15 Puzzle 标题和
  Resistor Calculator 热启动的既有局部场景。

源码、测试和界面验证后同步 README、路线图及示例边界说明；未改 CPU、RAM 映射、
加载策略、PIA 或预置镜像，没有新增依赖或永久测试文件。未运行其余预置的完整功能场景、
完整外部 CPU corpus、远程 CI、跨平台或实板对照；M3 整体验收状态不变。
文档的 76 个本地链接目标、两个新增锚点、矩阵列数、代码围栏与末尾换行检查通过。
临时 CLI 转录、PTY 脚本与网格快照已清理，不纳入仓库。
本轮按功能点本地提交，未 push。

## 2026-09-15：TUI 启动选择中心与配置页鼠标交互及菜单栏悬停支持

仅改宿主 TUI（`crates/cli/src/tui.rs`）：
1. 启动选择中心（`Page::Center`）：提取 `center_layout` 几何共享给绘制与命中判定；支持鼠标点击选择模拟器、触发主动作或跳转配置／信息；支持鼠标悬停高亮模拟器条目（`theme.hover()`）、主动作按钮（`theme.primary_hover()`）及底部快捷项徽章。
2. 启动配置页（`Page::Config`）：提取 `config_layout` 几何；支持鼠标点击字段聚焦进入编辑模式、点击程序字段打开选择器、点击底部按钮（校验并保存／启动／取消）；取消时放弃在途编辑，其余操作自动提交在途编辑。
3. 顶部菜单栏：支持鼠标悬停标签（`theme.hover()`）反馈，展开时保持 `theme.selected()`，受 `mouse` 配置与窗口尺寸约束。

验证：
- 新增单元测试 `center_mouse_input_supports_machine_selection_and_actions`、`config_mouse_input_supports_field_selection_and_buttons`、`center_mouse_hover_highlights_machines_actions_and_shortcuts`、`menu_bar_mouse_hover_highlights_tabs`。
- `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- `make verify`：格式、全目标 check、debug／release 各 251 项测试及冒烟全部通过。

本轮按功能点本地提交，未 push。

## 2026-09-15：TUI 启动配置页鼠标悬停高亮支持

仅改宿主 TUI（`crates/cli/src/tui.rs`）：
1. 启动配置页（`Page::Config`）：复用既有 `ConfigFocus` 与 `config_layout`，在 `App` 中跟踪 `config_hover: Option<ConfigFocus>`；鼠标移动实时更新悬停元素。
2. 字段悬停：未激活字段悬停时光标边框与标题提亮为强调色（`theme.title()`），输入内容呈现暗色面板背景微高亮（`theme.hover()`），下方提示文字提亮为普通文本色（`theme.text()`）。
3. 按钮悬停：底部三大按钮（校验并保存／启动／取消）悬停时光标边框提亮为 `theme.title()`，按钮背景亮起为高对比反白（`theme.primary_hover()`）。
4. 状态隔离：离开字段或按钮、切换页面或弹层展开时自动清理悬停状态；受 `mouse` 开关与窗口尺寸约束。

验证：
- 新增单元测试 `config_mouse_hover_highlights_fields_and_buttons`，覆盖 ROM 字段、加载地址字段及三大按钮的悬停高亮属性。
- `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- `make verify`：格式、全目标 check、debug／release 各 252 项测试及冒烟全部通过。

本轮按功能点本地提交，未 push。

## 2026-09-15：修正历史小节中不实的提交状态行

六个小节末行仍写「未提交、推送或发布」，但对应改动均已进入本地 `master`：M2.4 物理 RESET 独立参考基线（`228431c`）、M2.4 栈地址锁存与 SP 提交、物理 RESET 实现与 M2 本地整体验收（`7ba7bc4`、`d6236e9`）、Apple I CLI Enter 键 CR/LF 处理（`92acd28`）、Visual6502 参考驱动改用 Bun（`3771fbd`）、H18 条件垂直重载与滚动帧长修复（`ef6b031`）。这些行写入时反映的是该轮提交前的真实状态，随后被同一次提交原样带入，未按约定改成提交后的实际状态。

本次只把这六行文字改为「本轮按功能点本地提交，未 push。」：技术结论、验证范围、证据表格与未验收清单均未改动，未改源码、测试、工具、程序资产或依赖，也未重跑 Rust 检查、外部 CPU corpus、ROM-gated 测试或远程 CI。

本轮按功能点本地提交，未 push。

## 2026-09-15：接通 Apple I 数字视频链

范围：原始六轨编码、2519 逐字符装入／循环、固定 P-Lab 替换字模、74166 串行像素、视频光标及 C13 复合同步；每个 `Apple1::tick()` 返回 `VideoSample`。沿用现有主时钟、存储读头、写入握手与 H18 重载，不修改 CPU 执行语义。新增宿主 `video` example，用原创 6502 程序经真实 PIA 写字，导出有界 PGM、CSV 和 SVG。依据、相位、字模许可与电气边界见 [数字视频链](apple1/video.md)。

先运行 `cargo test --locked -p hesper-apple1 space_and_erasure_use_the_same_raw_tracks`，复现失败：输入空格的轨道值为 32，擦除轨道值为 0。修复后存储原始 B6，反相及光标门控在 C10／2519 输入端执行，输入空格与擦除使用同一编码。首次 CPU 写入 F 在 `7*910+25*14+3` tick 接收；第一个像素到 `9*910+25*14+11` 才出现，证明生产路径经过行重放与 D1 装载而非字符屏即时绘制。

本地验证：

- `cargo test --locked -p hesper-apple1 --test video`：5 项通过，覆盖真实 CPU 写入可见时刻、非对称字形与字符间距、六组硬件别名、空白／清屏、RESET 保留像素与板时钟闪烁、连续滚动的水平同步及滚动后像素位置。
- `make verify`：格式、全目标 check、debug／release 各 **263 项**非忽略测试、Clippy（`-D warnings`）、CLI demo 与 trace、空白检查全部通过。新增共 11 项回归（6 个单元测试、5 个生产路径集成测试），无新 Cargo 依赖。
- `make wozmon-tests`：使用既有 `.cache/apple1/wozmon.bin`，Apple I 4 项、CLI 16 项 ROM-gated 测试通过；未下载或嵌入该 ROM。
- `cargo run --locked -p hesper-apple1 --release --example video -- .cache/apple1-video-qa/normal`：采到 **238420 master tick／262 行／16.652 ms**。逐点采图并用 `sips` 转为 PNG，已查看实际图像和同段采样生成的同步波形。
- 滚动采样使用 TEXT=`A\rB\r…Y\r`（25 个字母各跟真实 CR），FRAME=`24`：**239330 master tick／263 行／16.715 ms**，产物位于 `.cache/apple1-video-qa/scroll`。已查看图像；多出的一行来自真实计数器重扫，采集端没有固定帧高。

产物为忽略缓存，不作为预期夹具反向导入测试。`frame.png`／`frame.pgm` 来自 `Tick.video`，`video.csv` 保留每个主时钟采样，`sync.svg` 绘制实际 H/V/复合同步。TUI 保留字符观察用途。

固定字模来源与 SHA-256 已记录，行 0、Q5..Q1 及代表字形按原厂图核对；全部 64 个字形与目标原板掩膜的独立逐点认证仍未完成。模拟视频电压、负载、TTL 传播／串扰、RC 容差和真实上电行为不在本次数字模型验收内。M3 整体验收保持未勾选；CPU 核心未改，未重跑完整 SingleStep／Klaus／Visual6502 外部层，未运行远程 CI。

本轮按功能点本地提交，未 push。
