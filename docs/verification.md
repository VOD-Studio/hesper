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

本轮完成的是**有界的独立参考基线**，不等于 CPU 物理 RESET 对照通过。M2.4、M2／M2.5 最终验收仍未完成；下一步须实现输入同步、内部锁存和提交时机，再对照全部 RESET 场景。未远程运行 CI，未提交、推送或发布；未启动 Apple I，也未选择项目许可证。

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

CPU 已实现引脚的 246 组对照与 RESET 的 419 组参考重现仍是不同范围：前者在 Cargo 回归中执行 CPU，后者在 Node 中执行原模型，Cargo 仅检查 RESET 夹具完整性。默认零延迟 Klaus 中断本轮未重跑，不改变前节失败结论。M2.4／M2 整体验收仍未完成，下一步是实际 RESET 输入同步及中途地址／数据通路；未启动 Apple I。未远程运行 CI，未提交、推送或发布。

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

M2.1～M2.5 的上述本地目标范围已完成，路线图据此勾选；不宣称穷举全部官方指令×引脚相位组合或所有 NMOS 修订。默认 0 延迟 Klaus 中断本轮未重跑，也未将已记录的上游 NMOS 陷阱改判为通过。Apple I／浏览器未启动；未远程运行 CI，未提交、推送或发布，未选择项目许可证。

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

CPU 核心与 `hesper-apple1` 机器模型（`bus.rs`／`pia.rs`／`display.rs`／`keyboard.rs`／`machine.rs`）均未修改，改动仅限 `crates/cli/src/apple1.rs` 的宿主侧终端输入处理。CPU 完整外部体系（SingleStep 151 万、Klaus 三配置、246+419 pins）本轮未重跑，因为 CPU 未修改。未远程运行 CI，未提交、推送或发布。

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
CPU 对照范围本轮未重跑，因为 CPU 未改动。未远程运行 CI，未提交、推送或发布。

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
