# Hesper

用 Rust 编写的复古计算机模拟项目，以独立、可测试、可复用的 **MOS NMOS 6502 CPU 核心**为主体。按 CPU → Apple I → 原版 Apple II → WebAssembly 的顺序推进。

当前已实现全部 151 个 NMOS 官方 opcode（56 条指令及其寻址方式）、二进制／十进制 ADC/SBC、BRK/RTI 与周期采样 IRQ/NMI。另有 64 KiB RAM Bus、复位、寄存器快照、周期统计、结构化错误和自包含 CLI 演示。具体寻址方式、周期、标志和测试映射见 [opcode 清单](docs/opcodes.md)，其余 opcode 均未实现。

## 运行

使用 stable Rust；仓库原有的 `hesper` 包名和二进制名保持不变。

```sh
cargo run -p hesper
cargo run -p hesper -- --trace
cargo run -p hesper -- --bus-trace --trace-limit 200
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

`--trace` 每条记录包含取到的指令地址和 opcode、执行前后 A/X/Y/SP/PC/P、单条周期和累计周期；累计值包含 RESET 的 7 周期。`--bus-trace` 显示每周期地址、数据、读写、SYNC、等待、下一阶段、引脚和锁存。默认只保留末尾 64 条记录，`--trace-limit 1..4096` 调整上限，两种 trace 可同时启用；结束或超限失败时输出保留记录。trace 使用执行时捕获的数据，不额外读取 Bus。

## 结构

```text
crates/cpu6502/   hesper-cpu6502：Cpu、Registers、Status、Bus、Ram、错误与测试
crates/cli/       hesper：宿主加载、有限步数演示、trace、参数及集成测试
examples/        原创演示汇编与机器码说明
docs/            架构、opcode、资料、路线图与验证记录
```

只有两个 crate，CPU 库无第三方运行依赖；外部测试工具使用开发依赖解析 JSON 和校验哈希。CPU 不持有整机或 Bus；调用者通过 `reset(&mut bus)`、`step(&mut bus)` 、单周期 `cycle(&mut bus)` 或 `half_cycle(&mut bus)` 驱动它，用 `registers()` 获取寄存器、`debug_state()` 获取只读阶段／锁存快照。`step` 返回 `Result<Step, CpuError>`，其中 `StepKind` 区分实际指令与 7 周期的 IRQ/NMI/RESET 入口；一次调用不会同时执行中断入口和处理程序指令。中断输入 API 和采样约定见 [架构文档](docs/architecture.md#中断输入与执行事件)。

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

本次实际工具链和检查结果见 [验证记录](docs/verification.md)。测试不联网、不使用外部 ROM；覆盖全部 151 个官方 opcode、105 个非官方字节的错误路径、算术穷举、中断及寻址边界，还有演示实际内存与 CLI 成功／失败路径。[快速 CI](.github/workflows/ci.yml) 运行上述基础验证、Wasm 编译与演示；[全量 CI](.github/workflows/full-cpu.yml) 手动触发数据准备和完整官方用例、Klaus、revD 对照。本轮未远程运行 CI。

## 限制与后续

“按指令执行并统计周期”不等于“逐周期总线精确模拟”。M2.3 已提供真正逐周期的总线接口，`step` 包装同一个引擎；全部 151 万条官方单步用例通过地址、数据和读写序列比较。宿主可按周期或半周期推进设备；物理 RESET 持续输入仍待 M2.4 完善。

未实现非官方 opcode、物理 RESET 保持／释放时序、机器系统或浏览器前端。非官方字节返回 `UnsupportedOpcode { address, opcode }`；BRK `$00` 是真实软件中断。M2.4 已接入周期中断采样、分支轮询和 NMI 抢占 BRK/IRQ 向量，246 条固定 revD 引脚 trace 交叉验证通过；RDY 读等待、SO 边沿与半周期输入已实现，物理 RESET 保持时序仍待完善。

M2.2 已通过固定版本的 **151 个官方 opcode／151 万条 SingleStepTests 用例**（寄存器、内存和周期数量），以及 Klaus 功能测试、Bruce Clark 全标志十进制穷举；M2.3 同时通过总线序列比较。672 条原始格式样例保留为离线快速回归。数据准备、重放命令和许可证见 [测试数据说明](crates/cpu6502/tests/data/README.md)。M2 接下来完成物理 RESET 并做整体验收；Apple I 延后到 CPU 验收后的独立 M3，见 [路线图](docs/roadmap.md)。

构造 CPU 时的零寄存器、全零 RAM 是可重复运行的模拟器约定，**不是硬件上电保证**；NMOS RESET 保留 D 和通用寄存器，程序应自行初始化栈并选择运算模式。具体兼容性假设见 [架构](docs/architecture.md)，后续计划见 [路线图](docs/roadmap.md)，行为依据见 [参考资料](docs/references.md)。

仓库尚未选择许可证，待项目所有者确认；两个包暂设 `publish = false`。未添加 Apple ROM、商业软件或第三方模拟器实现。
