# Hesper 项目约定

## 开始工作

- 先阅读 README、相关代码、docs/architecture.md、docs/opcodes.md 和 Git 状态；保留用户未提交的修改。
- 项目名为 Hesper，现有 CLI 包／二进制名为 `hesper`，CPU 库为 `hesper-cpu6502`。不要擅自重命名项目。
- 当前已完成 M1 的指令级实现，下一阶段按 docs/roadmap.md 的 M2 分步完善 CPU；Apple I 延后至独立 M3。路线图是计划，收到实现任务后按对应阶段执行，不因完成规划自动启动后续实现；不提前创建空机器 crate、Web 项目或框架。
- 未经用户明确授权，不 commit、push、发布包或执行破坏性 Git 操作。不擅自选择／更改许可证。
- 用户授权自主提交后，每完成一个可独立验证的功能点就提交一次，包含相应测试；保持每次提交可构建，不混入其他工作。提交授权不代表推送授权。

## 架构与实现

- 优先级：正确性 > 可测试性与可调试性 > 结构清晰 > 性能优化 > 展示效果。
- 默认只实现经典 NMOS 6502；不得混入 65C02 或 NES 2A03 行为。涉及修订差异先核实并记录假设。
- CPU 的所有模拟内存访问必须通过可变借用的 Bus；CPU 不知道机器内存图、ROM、键盘或显示设备。
- 文件、终端、运行速度、步数预算、完成条件和日志属于宿主层。CPU 不执行文件 I/O、打印、休眠或浏览器调用。
- `step` 执行一条指令或一次 IRQ/NMI 入口并报告周期，使用 `StepKind` 区分，不能伪造硬件中断的 opcode。不要新增执行整条指令却叫 `tick` 的接口；不能把周期统计宣传成总线精确。
- 明确区分 `Cpu::new` 的确定性初值与 RESET 的硬件行为；状态中的 B 与位 5 不当作两个持久硬件标志。
- 所有模拟的 8 位／16 位回绕显式使用 wrapping 运算；宿主加载必须检查范围、错误时不得部分写入。
- 所有未支持 opcode 返回地址及字节的结构化错误；M1 的 BRK 已实现为软件中断。不得静默 NOP、假 HALT 或宿主 panic。
- IRQ 电平与 NMI 边沿的宿主采样约定见架构文档；改变采样模型时同时补边界测试，区分指令级近似和逐相位硬件时序。NMOS 的中断入口保留 D，不能混入 CMOS 的清 D 行为。
- trace 只能用执行中捕获的数据或明确无副作用的 RAM 宿主检查接口，禁止为日志额外 `Bus::read`。
- 使用安全、惯用 Rust，禁止 `unsafe`；优先标准库。未经需求证明不引入依赖、异步运行时、GUI、JIT、插件或通用 CPU 框架。
- 不用 `todo!`、`unimplemented!`、假返回值或全局 warning 抑制掩盖缺失实现。不复制其他模拟器实现。

## 测试与文档

- 新 opcode 同时补结果、PC／长度、应变与不应变的标志、周期及边界测试；更新 docs/opcodes.md 的实现与测试列。
- 预期值依据规范独立写出，不调用被测实现生成答案。所有 CPU 程序执行循环必须有明确有限预算。
- 边界重点：分支的有符号偏移及 2/3/4 周期、PC/SP 回绕、小端、JSR/RTS 栈顺序、NMOS 间接 JMP、固定 5 周期的 `STA abs,X`。
- 查疑问优先用 MOS 原始手册，并交叉参考原始测试项目；记录链接、章节、版本与采用的行为。
- 外部测试选择 NMOS `6502` 数据，固定提交版本并记录来源、许可证、运行方法和实际范围。分别报告寄存器／内存结果、周期数量、总线序列。
- 本地测试必须自包含；不得提交 Apple ROM、商业软件或来源不明的二进制文件。不能将部分用例说成整个外部套件通过。
- 不删除有效测试或修改正确预期迎合错误实现；不提交一次性诊断脚本。只有已实现且验证的路线图项可标记完成。

## 必须执行

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p hesper
cargo run -p hesper -- --trace
git diff --check
```

已安装目标时再执行 `cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown`；未执行不得报通过。`rust-toolchain.toml` 使用 stable，本次验证版本在 docs/verification.md；尚未验证 MSRV。

本地检查、远程 CI、提交、推送和发布是不同状态，汇报时分别说明。当前 GitHub Actions 配置不代表远程 CI 已通过。
