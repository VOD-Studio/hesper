# M1 本地验证记录

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
