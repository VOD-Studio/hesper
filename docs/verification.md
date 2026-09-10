# CPU 本地验证记录

当前：M2.1～M2.5 的本地目标范围已验收，包含物理 RESET 输入同步、中途数据通路和全部固定 CPU 对照。范围限定于官方 NMOS 指令及已记录的固定 revD 引脚窗口，不等同于所有芯片修订／电气窗口认证。下面保留各阶段历史结果，最后一节记录物理 RESET 实现与最终代码复验；远程 CI 未运行，Apple I 未启动。

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
