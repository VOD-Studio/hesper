# Hesper

用 Rust 编写的复古计算机模拟项目，以独立、可测试、可复用的 **MOS NMOS 6502 CPU 核心**为主体。按 CPU → Apple I → 原版 Apple II → WebAssembly 的顺序推进。

M1 已实现全部 151 个 NMOS 官方 opcode（56 条指令及其寻址方式）、二进制／十进制 ADC/SBC、BRK/RTI 与指令边界上的 IRQ/NMI。另有 64 KiB RAM Bus、复位、寄存器快照、周期统计、结构化错误和自包含 CLI 演示。具体寻址方式、周期、标志和测试映射见 [opcode 清单](docs/opcodes.md)，其余 opcode 均未实现。

## 运行

使用 stable Rust；仓库原有的 `hesper` 包名和二进制名保持不变。

```sh
cargo run -p hesper
cargo run -p hesper -- --trace
cargo run -p hesper -- --max-steps 54
cargo run -p hesper -- --help
```

默认执行上限为 1000 条指令；`--max-steps 0` 可验证超限错误（退出码 1）。参数无效也会明确报错。

演示从 `$FFFC/$FFFD` 的复位向量进入 `$8000`，用 6502 循环将 0～9 写入 `$0200`～`$0209`。宿主在 PC 到达 `$800F` 时停止，**不执行该地址的 NOP**，也不把 BRK 当作退出指令。原始汇编及机器码对照在 [examples/count.asm](examples/count.asm)，CLI 内嵌同一组手工编码字节，不需要外部 ROM 或汇编器。

输出：

```text
$0200..$0209: 0 1 2 3 4 5 6 7 8 9
A=09 X=0A Y=00 SP=FF PC=800F P=27
Completed: 54 instructions, 147 instruction cycles + 7 reset cycles = 154 total cycles
```

`--trace` 每条记录包含取到的指令地址和 opcode、执行前后 A/X/Y/SP/PC/P、单条周期和累计周期；累计值包含 RESET 的 7 周期。trace 使用执行时捕获的数据，不额外读取 Bus。

## 结构

```text
crates/cpu6502/   hesper-cpu6502：Cpu、Registers、Status、Bus、Ram、错误与测试
crates/cli/       hesper：宿主加载、有限步数演示、trace、参数及集成测试
examples/        原创演示汇编与机器码说明
docs/            架构、opcode、资料、路线图与验证记录
```

只有两个 crate，无第三方 Rust 依赖。CPU 不持有整机或 Bus；调用者通过 `reset(&mut bus)`、`step(&mut bus)` 驱动它，用 `registers()` 获取值快照。`step` 返回 `Result<Step, CpuError>`，其中 `StepKind` 区分实际指令与 7 周期的 IRQ/NMI 入口；一次调用不会同时执行中断入口和处理程序指令。中断输入 API 和采样约定见 [架构文档](docs/architecture.md#中断输入与执行事件)。

## 验证

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p hesper
cargo run -p hesper -- --trace
```

环境已安装 Wasm 目标时还可执行：

```sh
cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown
```

本次实际工具链和检查结果见 [验证记录](docs/verification.md)。测试不联网、不使用外部 ROM；覆盖全部 151 个官方 opcode、105 个非官方字节的错误路径、算术穷举、中断及寻址边界，还有演示实际内存与 CLI 成功／失败路径。[GitHub Actions 配置](.github/workflows/ci.yml) 运行上述基础验证与演示；本轮未远程运行 CI。

## 限制与后续

“按指令执行并统计周期”不等于“逐周期总线精确模拟”。当前不重现全部 dummy read、逐周期设备推进或引脚时序；不能据此宣称已验证整条总线访问序列。

未实现非官方 opcode、RDY/SO、机器系统或浏览器前端。非官方字节返回 `UnsupportedOpcode { address, opcode }`；BRK `$00` 是真实软件中断。中断输入在指令边界处理，不模拟指令内部边沿、NMI 抢占中断向量及精确流水线时序。

已通过 8 条固定版本的 SingleStepTests 十进制选定样例，范围及许可证见 [测试数据说明](crates/cpu6502/tests/data/README.md)。未执行完整外部套件。下一步 M2 先加强外部一致性与总线时序验证，再建立 Apple I 文本系统。

构造 CPU 时的零寄存器、全零 RAM 是可重复运行的模拟器约定，**不是硬件上电保证**；NMOS RESET 保留 D 和通用寄存器，程序应自行初始化栈并选择运算模式。具体兼容性假设见 [架构](docs/architecture.md)，后续计划见 [路线图](docs/roadmap.md)，行为依据见 [参考资料](docs/references.md)。

仓库尚未选择许可证，待项目所有者确认；两个包暂设 `publish = false`。未添加 Apple ROM、商业软件或第三方模拟器实现。
