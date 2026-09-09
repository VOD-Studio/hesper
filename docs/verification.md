# CPU 本地验证记录

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
