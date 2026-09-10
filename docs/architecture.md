# 架构与兼容性

## 边界

依赖方向为 CLI／未来前端 → 宿主或机器层 → `hesper-cpu6502`。M0～M2 只建立 CPU 库和 CLI 两个 crate。

- **CPU 库**：寄存器、状态、指令解释、寻址、栈、复位、周期数量及错误。`instruction.rs` 为显式 opcode／寻址／周期表，`cpu.rs` 保存状态及 ALU 语义，`cpu/cycle.rs` 推进总线阶段；不使用解码宏或多层指令对象。
- **Bus**：`read(&mut self, addr: u16) -> u8` / `write(&mut self, addr: u16, value: u8)`。可变读取为未来设备副作用预留空间。CPU 不持有 Bus，`cycle`、`step` 和 `reset` 接收外部 `&mut dyn Bus`，便于测试、替换内存和组合设备。
- **Ram**：独立的全零 64 KiB 数组 Bus；模拟访问以 `u16` 地址进行，宿主 `load` 检查整段范围，失败不写入。`as_slice` 仅用于这个 RAM 的无副作用宿主检查，不是通用设备读取接口。
- **CLI**：`src/lib.rs` 包含实际演示字节和有限步数 runner，供入口与集成测试共用；`main.rs` 负责参数、输出和退出码。runner 从复位向量启动，CPU 逐条写出结果，完成地址和预算都由宿主决定。

未来 Apple I 与 Apple II 各自实现机器 Bus、内存映射和设备状态，复用 CPU。M2 先完善 CPU，机器层按路线图延后至 M3/M4；前端负责加载用户有权使用的资源、输入输出和调度。机器／CPU 不依赖浏览器；WebAssembly 的绑定和渲染到 M5 再建立。

## 状态与 RESET

`Cpu::new()` / `Default` 约定 A/X/Y/SP/PC 为 0、六个存储标志为 false（调试 P 为 `$20`）。RAM 默认清零。它们是为可复现测试选择的初始值，**不声称真实硬件上电时拥有这些值**。

`reset` 采用 NMOS 的七周期入口结果：I 置位，当前 SP 以 8 位回绕减 3，PC 从 `$FFFC` 低字节、`$FFFD` 高字节取得，A/X/Y 和 N/V/D/Z/C 保留，栈不写入。返回 `Result<Step, CpuError>`（无等待时 `cycles=7`），不包含保持 RESET 引脚的时间或第一条指令。先对当前 PC 读两次，再读取当前 SP、SP−1、SP−2 对应的栈地址，最后读取向量。`begin_reset()` 启动同一序列，由宿主调用 `cycle` 分次推进；它可放弃未完成指令，但并非物理 RESET 引脚的保持／释放时序。[MOS 手册第 9 章](https://lbaeza.neocities.org/mcs6500/6500_ch09)

因此新 CPU 首次 reset 得到 SP=`$FD`，重复 reset 继续递减；这不是“RESET 将 SP 固定设成 FD”。演示显式初始化 X、SP 和 D，让计数程序不依赖它们的上电初值；结果摘要中未使用的 Y=`$00` 和 V=0 仍来自模拟器构造约定，并非硬件保证。

`Registers` 为可复制值快照；`from_registers` 是明确的调试／测试状态注入入口，不执行复位。`Status` 保存 C/Z/I/D/V/N 六个 bool；`bits()` 固定位 5 为 1、B 位为 0，`from_bits()` 忽略位 4/5。PHP/BRK 压栈时单独合成 B=1，IRQ/NMI 为 B=0；位 5 均为 1。PLP/RTI 不恢复 B 或位 5 为持久状态。`from_registers` 将外部中断线设为未断言、清空待处理中断；它不是包含引脚和锁存状态的完整存档。

## NMOS 假设

- 使用常见 NMOS 指令结果与周期规则；指令结果保持经典 NMOS，精细中断窗口以固定 Visual6502 revD 作交叉参考；不支持变种切换。
- 保留 NMOS `JMP ($xxFF)` 从 `$xx00` 获取目标高字节的行为，包括指针 `$FFFF`；不采用 65C02 的修正。
- 分支从操作数后 PC 加有符号偏移，再比较起点／终点页：不跳 2 周期，同页跳 3，跨页跳 4。跨 `$FFFF` 同样回绕。
- `STA abs,X` 地址回绕且固定 5 周期。JSR 压入指令最后一字节的地址，高字节先入栈；RTS 先取低再取高并加 1。JSR 的高操作数在栈写入后读取，保留代码与栈重叠时的结果。
- D 是真实存储标志且 RESET 不清除；CLD/SED 与 ADC/SBC 的二进制及 NMOS 十进制运算均已实现。没有把 D 忽略的 NES 2A03 运算混入默认实现。
- 采用常见后期 NMOS 的正确 ROR 行为，不仿真最早芯片的 ROR 缺陷。BRK/RTI 与周期采样 IRQ/NMI 已实现；RDY/SO 已按下述数字采样约定建模；非官方指令尚未实现。
- 内存 RMW 指令先回写旧值再写新值；NMOS 的 BRK/IRQ/NMI 均不清 D。

行为来源与原始测试项目交叉核对范围见 [references.md](references.md)，逐项支持状态见 [opcodes.md](opcodes.md)。

## 执行精度与调试

**“按指令执行并统计周期”不等于“逐周期总线精确模拟”。** `step` 返回 `Step { address, kind, before, after, cycles }`，周期统计包含已支持指令适用的附加周期。CPU 不积累宿主时间；CLI 单独累计指令周期和复位周期。

M2.3 已将整条指令的控制流程拆为私有阶段和地址／数据锁存状态。`cycle(&mut bus) -> Result<Cycle, CpuError>` 每次只执行一次真实总线访问，返回地址、数据、读写方向、SYNC 和可选的完成事件。`at_instruction_boundary()` 判断是否处于下一次取指前；`step` 循环调用同一个 `cycle` 引擎。先用 `cycle` 执行一部分再用 `step` 完成时，返回的 `before` 和周期数仍覆盖整条指令。ALU 结果只有一份实现，不维护第二套整指令解释器。

已补齐官方指令的 dummy read、索引中间地址、RMW 旧值回写、栈和跳转顺序。151 万条固定 NMOS 官方用例的每周期地址／数据／读写均已比较；RESET 另有七次读取的独立测试。读取副作用设备、分次推进和 trace 不增加读取也有回归。寄存器快照只反映该调用结束时的软件状态，不保证每个内部晶体管锁存器的相位更新时刻。

这次升级实际修改了执行模型。当前已引入数字采样／同步状态及半周期接口；进一步提高物理引脚兼容性仍可能需要改动阶段和锁存状态；Bus 外围的宿主负责设备时钟及事件注入。当前逐周期总线通过不等于 IRQ/NMI/RDY/SO 的逐相位兼容认证。

不支持的 opcode 返回地址和字节，寄存器（含 PC）不变；opcode 读取已发生，Bus 的读取副作用不回滚，也不虚构完成周期。宿主应报告并停下。trace 只消费成功 `Step` 的已捕获信息，即使该指令改写了自己的 opcode，也显示实际执行的字节。

十进制 ADC 的 Z 取二进制和，N/V 取低位十进制修正后的中间和，A/C 取高位修正结果；SBC 的 N/V/Z/C 均取二进制差，A 再做十进制修正。无效 BCD 数字也按常见 NMOS 半字节修正规则计算，不拒绝或偷偷当作二进制。有效 BCD 的结果与进借位已穷举，Bruce Clark 的有效／无效 BCD 全输入 A/N/V/Z/C 检查已通过（固定配置见测试数据说明）。

## 中断输入与执行事件

M2.4 当前已用周期采样替代 M1 的指令边界近似。`set_irq_line(true)` / `set_nmi_line(true)` 表示低有效引脚断言；宿主可在 `cycle` 或 `half_cycle` 调用之间改变电平。每个 Phi2 结束采样 IRQ 电平及 I，NMI 比较本周期与上次采样电平来锁存断言边沿；完全落在两个采样点之间的短脉冲不被接受。

普通指令完成时使用上一周期已采样的请求；因此最后周期才出现的请求通常延迟一条指令。CLI/SEI/PLP 更新 I 与轮询的顺序由执行阶段自然产生，RTI 较早恢复 I；不再用指令前后 I 的整条特判。已测试 CLI／PLP 后接 SEI／PLP／RTI。taken 同页分支保留操作数阶段的轮询结果，跨页分支另有一次轮询机会。

NMI 边沿忽略 I，持续断言不重复产生边沿，未服务边沿会合并；在轮询处被接受后优先于 IRQ。IRQ 在进入处理程序后需要重新采样，不会携带旧的排队请求穿过 NMI。RESET 请求清空中断锁存，并保留外部电平。

`StepKind::Instruction { opcode }` 是实际取到的指令；`Irq` / `Nmi` / `Reset` 是入口启动原因，没有伪造 opcode。BRK 压 PC+2、P 的 B=1；IRQ/NMI 压当前 PC、B=0；I 在旧 P 压栈后置位，D 保留。到压 P 的周期开始时选择向量：足够早的 NMI 可将 BRK 或 IRQ 的向量替换为 `$FFFA/B`，但保持已经选择的返回地址和 B 位；更晚的边沿保留到处理程序轮询。入口的 `StepKind` 保留启动原因，实际选中的向量可从总线 trace 判断。RTI 恢复 P 和 PC，不加 1。

固定 Visual6502 **NMOS revD** 提交 `d8ecc129b34e0eaf320e0400fcf33329475bdb1e` 的 246 个原始程序／引脚事件场景用于交叉核对，共 5904 个总线周期；包含分支、连续改写 I、短脉冲、BRK／IRQ 向量抢占、RDY 等待和 SO 与全部六种改写 V 指令的重叠。数据、重现命令及来源许可见 [测试说明](../crates/cpu6502/tests/data/README.md)。不能将这一修订的观察推广到所有 NMOS 芯片。

物理 RESET 引脚仍未接入 CPU。`begin_reset` 是有明确七周期行为的宿主请求，不冒充完整物理 RESET 引脚模型。独立的 `visual6502/reset.json` 保存断言／保持／释放、中途写入、栈、RDY 和中断交叉窗口的 revD 观察；普通测试只核对这份数据的完整性，不将其作为 CPU 已通过的对照。参考驱动可按套件或场景重放，详见 [测试说明](../crates/cpu6502/tests/data/README.md#reset-参考基线尚未接入-cpu)。

这些观察表明，不能仅增加一个保持位并在释放时调用 `begin_reset`：断言同步期间仍有已发生的写入，时序链停下前还可能改变中间 PC/SP；栈地址使用的中间值也不总是软件快照中的 SP。下一步必须据此区分内部锁存和架构提交，并保留每次真实 Bus 访问。具体差异清单见测试说明；不能用少数程序的固定地址特判代替执行模型。寄存器快照仍不是包含全部中间状态的存档。

## RDY、SO 与半周期

`half_cycle` 在 Phi1／Phi2 间交替；Phi1 返回 `None`，Phi2 完成唯一一次 Bus 访问并返回 `Some(Cycle)`。`next_clock_phase` 表示下一次调用的相位；`cycle` 完成当前周期（若尚未执行 Phi1 则先执行它）。这是数字相位调度；没有模拟电压、建立／保持时间或晶体管传播延迟。寄存器仍是软件执行状态，不能据此读取所有物理内部锁存器的相位值。

`set_ready(false)` 令下一次读停在原地址，`Cycle.stalled=true`；每个等待周期仍真实读取设备。恢复时才使用该次读值。写周期继续，包括 RMW 连续两次写入；等待期间继续采样 IRQ/NMI/SO。`Step.cycles` 使用 `u64` 并包含等待。`step`／`reset` 每次最多推进 7 周期；超限返回可恢复的 `CycleBudgetExceeded`，不是死循环。需要更多等待时用 `step_with_cycle_budget`，或宿主带预算逐周期驱动；恢复 RESET 用 step/cycle，不重复调用 reset。`reset` 与 begin_reset/cycle 共用引擎，不绕开引脚采样。

`set_so_line(true)` 表示 SO 低有效断言；在 Phi1 采样下降边沿，再于下一 Phi1 更新 V。保持低电平不重复触发。根据固定 revD 相位扫描，BIT／PLP／RTI 的 V 写入延续至下一 Phi1，ADC／SBC 再晚一个 Phi1，CLV 的清除窗口覆盖两者；这些写入优先于重叠的 SO 更新。内部保留相应 V 写入窗口，避免在整条指令结束时简单置位／清除所导致的错误分支。该规则是本项目对固定模型观察的数字归纳，不宣称适用于所有 NMOS 修订或任意窄脉冲。

## 宿主诊断

`debug_state()` 无需 Bus，提供下一时钟相位、当前操作／下一总线阶段、地址和数据中间值、等待计数、逻辑输入、IRQ/NMI/SO 采样及待处理状态、延迟 V 写入。`ExecutionPhase` 是执行器阶段名，不能等同于 MOS 内部 T 状态或晶体管节点。此快照没有恢复接口，也不包含所有 CPU 状态，不能作为存档格式。

CLI 的 `run_demo_with_trace` 提供真实 Cycle 和指令完成事件，既有 `run_demo` 仍只回调指令。`--trace`／`--bus-trace` 共用有界历史，默认 64 条，`--trace-limit` 限定为 1～4096；异常退出也保留最后记录。CPU 本身不保存日志或打印。

外部 SingleStep/Klaus runner 共用宿主诊断 helper，最多保留最后 32 个实际总线周期及其指令完成事件；失败先给首个差异和当前 debug_state，再给有限历史。SingleStep 同时给固定版本、opcode、用例索引与重放命令。数据解析失败不假造执行 trace；未支持 opcode 的错误保留实际读取的地址和字节。
