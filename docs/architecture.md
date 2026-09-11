# 架构与兼容性

## 边界

依赖方向为 CLI／未来前端 → 宿主或机器层 → `hesper-cpu6502`。M0～M2 只建立 CPU 库和 CLI 两个 crate。

- **CPU 库**：寄存器、状态、指令解释、寻址、栈、复位、周期数量及错误。`instruction.rs` 为显式 opcode／寻址／周期表，`cpu.rs` 保存状态及 ALU 语义，`cpu/cycle.rs` 推进总线阶段；不使用解码宏或多层指令对象。
- **Bus**：`read(&mut self, addr: u16) -> u8` / `write(&mut self, addr: u16, value: u8)`。可变读取为未来设备副作用预留空间。CPU 不持有 Bus，`cycle`、`step` 和 `reset` 接收外部 `&mut dyn Bus`，便于测试、替换内存和组合设备。
- **Ram**：独立的全零 64 KiB 数组 Bus；模拟访问以 `u16` 地址进行，宿主 `load` 检查整段范围，失败不写入。`as_slice` 仅用于这个 RAM 的无副作用宿主检查，不是通用设备读取接口。
- **CLI**：`src/lib.rs` 包含实际演示字节和有限步数 runner，供入口与集成测试共用；`main.rs` 负责参数、输出和退出码。runner 从复位向量启动，CPU 逐条写出结果，完成地址和预算都由宿主决定。

未来 Apple I 与 Apple II 各自实现机器 Bus、内存映射和设备状态，复用 CPU。M2 先完善 CPU，机器层按路线图延后至 M3/M4；前端负责加载用户有权使用的资源、输入输出和调度。机器／CPU 不依赖浏览器；WebAssembly 的绑定和渲染到 M5 再建立。

## Apple I 机器层（M3）

`hesper-apple1` 把 CPU、内存映射 Bus、PIA、键盘与视频终端放在一个板级时钟模型下，`crates/cli` 只驱动这一个入口。机器层的公开面是窄的：

```rust
pub struct Tick { pub cpu: Option<Cycle>, pub refresh: bool, pub frame_completed: bool }
Apple1::new(rom) / tick() / run_ticks(ticks) / reset()
Apple1::master_ticks() / cpu_cycles() / video_frames() / io_pending()
Apple1::set_reset_line(asserted) / reset_line_asserted() / clear_screen()
Apple1::type_char() / type_str() / drain_output() / cpu() / bus() / bus_mut() / display() / keyboard()
```

- **一个 master tick 是一个 14.31818 MHz 晶振周期**（`crates/apple1/src/timing.rs`）。D11 的 ÷14 产生字符时钟；D6/D7 级联给出 65 槽水平序列，`H6 && H10` 在计数 129／139／149／159（槽 34／44／54／64）选出四个刷新槽。一个水平周期 910 master tick，其中 61 次真实 CPU 总线访问。
- **刷新抑制 Φ2，不用 RDY**。刷新槽上 CPU 停在 Φ2、PIA 无 E、没有总线访问；板时钟、视频计数与 B3 单稳态照常推进。`tick().cpu` 只在真实 Φ2 完成时给出 `Cycle`，所以 `cpu_cycles()` 小于 `master_ticks()/14`。刷新不经 `Bus::read/write`，因此不改变 open bus 的上次读取值，也不产生伪造的 `stalled` 记录。
- **`set_reset_line` 与 `begin_reset` 不混用**：前者是物理 RESET 输入（CPU、PIA 与键盘重同步都经它），后者是宿主同步入口。`Apple1::reset()` 是物理线路的同步封装，按**真实 CPU 周期**计保持与完成预算。
- **PIA 的 CB2 是真实输出握手**（`pia.rs`）：CRB 位 5/4/3 选择模式，写 ORB 只在下一个 E 沿拉低 CB2，CB1 的有效沿（由 CRB 位 1 选择，方向按数据表定义的低电平有效）释放它。PB7 由 `DA = !CB2` 驱动；`data_lines()` 把未驱动线解析为 TTL 高。
- **视频终端是循环存储模型**（`display.rs`）：1024 个六位字符槽（960 可见 + 64 消隐）、40 字符 2519 行缓冲、C7 请求锁存。字符只在光标槽被扫到时被接受（一圈 ≈ 一帧），CR 逐槽清到行尾，滚动由垂直重载前移显示原点完成。屏幕／光标／输出流是宿主文本投影，保存在不参与控制逻辑的并行数组里。
- **机器重建**（Ctrl-N）归零两个时钟与显示；物理 RESET 不归零它们、不清屏、不丢弃已排队输入。
- **不建模**：DRAM 单元电荷保持本身、TTL 传播延迟与单稳态容差、2513 字模与像素／视频合成、D8/D9 preset 的十进制值与其帧长影响。逐项边界见 `docs/apple1/hardware-evidence.md`。

## 状态与 RESET

`Cpu::new()` / `Default` 约定 A/X/Y/SP/PC 为 0、六个存储标志为 false（调试 P 为 `$20`）。RAM 默认清零。它们是为可复现测试选择的初始值，**不声称真实硬件上电时拥有这些值**。

`reset` 采用 NMOS 的七周期入口结果：I 置位，调用时可见的 SP 以 8 位回绕减 3，PC 从 `$FFFC` 低字节、`$FFFD` 高字节取得，A/X/Y 和 N/V/D/Z/C 保留，栈不写入。返回 `Result<Step, CpuError>`（无等待及外部引脚干扰时 `cycles=7`），不包含保持 RESET 引脚的时间或第一条指令。先对当前 PC 读两次，再读取当前 SP、SP−1、SP−2 对应的栈地址，最后读取向量。`begin_reset()` 启动同一序列，由宿主调用 `cycle` 分次推进；它丢弃未完成指令和旧复位同步状态，以调用时的寄存器新建入口，但不会撤销已经发生的 Bus 写入或改变外部引脚电平。物理 RESET 另由 `set_reset_line` 建模，不能混用两种入口。[MOS 手册第 9 章](https://lbaeza.neocities.org/mcs6500/6500_ch09)

因此新 CPU 首次 reset 得到 SP=`$FD`，重复 reset 继续递减；这不是“RESET 将 SP 固定设成 FD”。演示显式初始化 X、SP 和 D，让计数程序不依赖它们的上电初值；结果摘要中未使用的 Y=`$00` 和 V=0 仍来自模拟器构造约定，并非硬件保证。

`Registers` 为可复制值快照；`from_registers` 是明确的调试／测试状态注入入口，不执行复位。`Status` 保存 C/Z/I/D/V/N 六个 bool；`bits()` 固定位 5 为 1、B 位为 0，`from_bits()` 忽略位 4/5。PHP/BRK 压栈时单独合成 B=1，IRQ/NMI 为 B=0；位 5 均为 1。PLP/RTI 不恢复 B 或位 5 为持久状态。`from_registers` 将外部中断线设为未断言、清空待处理中断；它不是包含引脚和锁存状态的完整存档。

### 栈地址与 SP 提交

执行器为每个操作保存独立的 8 位栈地址游标，以操作启动时的 SP 初始化。所有栈访问和 RDY 重读都使用这个游标，而不是随时读取 `Registers.sp`；游标只有在相应总线阶段推进时才回绕增减。SP 的可见值按固定 revD 普通栈序列的提交点更新：

- JSR 读取目标低字节后暂将它放入 SP，原栈地址仍保留；两次压栈期间不把地址游标回写 SP，最终读取目标高字节的阶段才恢复 SP−2。
- BRK／IRQ／NMI／宿主 RESET 在第三次栈访问阶段一次提交 SP−3，而不是每次栈访问都改写 SP。
- RTS 在返回地址低字节读取阶段提交 SP+2；RTI 在返回地址低字节读取阶段提交 SP+3。
- PHA/PHP 在压栈时提交 SP−1；PLA/PLP 在栈 dummy read 阶段提交 SP+1。

上述不依赖新读值的 SP 提交不被 RDY 阻止，但总线地址游标仍等待恢复；重复等待不能重复增减 SP。JSR 目标低字节则只在读周期获准推进时才写入 SP。这些规则已比较固定 revD 的总线与下降相位后 SP 观察，不宣称其他寄存器的全部中间相位也已吻合。

因此中途调用 `registers()` 可以看到 JSR 借用的 SP；`step` 的原始 `before`、指令结束的 `after` 和总周期契约保持不变。宿主中途 `begin_reset` 使用这个**当时可见的 SP**，不恢复上一操作的栈游标，也不冒充物理 RESET 的同步／暂态影响。`debug_state().execution.stack_address` 单独报告栈地址游标；它不表示下一次访问一定在栈页。

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

已补齐官方指令的 dummy read、索引中间地址、RMW 旧值回写、栈和跳转顺序。151 万条固定 NMOS 官方用例的每周期地址／数据／读写均已比较；宿主 RESET 另有七次读取的独立测试，物理 RESET 有下述 419 组总线与寄存器见证对照。读取副作用设备、分次推进和 trace 不增加读取也有回归。寄存器快照只反映该调用结束时的软件状态，不保证每个内部晶体管锁存器的相位更新时刻。

这次升级实际修改了执行模型。当前已引入数字采样／同步状态及半周期接口；进一步提高物理引脚兼容性仍可能需要改动阶段和锁存状态；Bus 外围的宿主负责设备时钟及事件注入。当前逐周期总线通过不等于 IRQ/NMI/RDY/SO 的逐相位兼容认证。

不支持的 opcode 返回地址和字节，寄存器（含 PC）不变；opcode 读取已发生，Bus 的读取副作用不回滚，也不虚构完成周期。宿主应报告并停下。trace 只消费成功 `Step` 的已捕获信息，即使该指令改写了自己的 opcode，也显示实际执行的字节。

十进制 ADC 的 Z 取二进制和，N/V 取低位十进制修正后的中间和，A/C 取高位修正结果；SBC 的 N/V/Z/C 均取二进制差，A 再做十进制修正。无效 BCD 数字也按常见 NMOS 半字节修正规则计算，不拒绝或偷偷当作二进制。有效 BCD 的结果与进借位已穷举，Bruce Clark 的有效／无效 BCD 全输入 A/N/V/Z/C 检查已通过（固定配置见测试数据说明）。

## 中断输入与执行事件

M2.4 当前已用周期采样替代 M1 的指令边界近似。`set_irq_line(true)` / `set_nmi_line(true)` 表示低有效引脚断言；宿主可在 `cycle` 或 `half_cycle` 调用之间改变电平。每个 Phi2 结束采样 IRQ 电平及 I，NMI 比较本周期与上次采样电平来锁存断言边沿；完全落在两个采样点之间的短脉冲不被接受。

普通指令完成时使用上一周期已采样的请求；因此最后周期才出现的请求通常延迟一条指令。CLI/SEI/PLP 更新 I 与轮询的顺序由执行阶段自然产生，RTI 较早恢复 I；不再用指令前后 I 的整条特判。已测试 CLI／PLP 后接 SEI／PLP／RTI。taken 同页分支保留操作数阶段的轮询结果，跨页分支另有一次轮询机会。

NMI 边沿忽略 I，持续断言不重复产生边沿，未服务边沿会合并；在轮询处被接受后优先于 IRQ。IRQ 在进入处理程序后需要重新采样，不会携带旧的排队请求穿过 NMI。宿主 `begin_reset` 清空中断锁存并保留外部电平；物理 RESET 的清除窗口见下节。

`StepKind::Instruction { opcode }` 是实际取到的指令；`Irq` / `Nmi` / `Reset` 是入口启动原因，没有伪造 opcode。BRK 压 PC+2、P 的 B=1；IRQ/NMI 压当前 PC、B=0；I 在旧 P 压栈后置位，D 保留。到压 P 的周期开始时选择向量：足够早的 NMI 可将 BRK 或 IRQ 的向量替换为 `$FFFA/B`，但保持已经选择的返回地址和 B 位；更晚的边沿保留到处理程序轮询。入口的 `StepKind` 保留启动原因，实际选中的向量可从总线 trace 判断。RTI 恢复 P 和 PC，不加 1。

固定 Visual6502 **NMOS revD** 提交 `d8ecc129b34e0eaf320e0400fcf33329475bdb1e` 的 246 个原始程序／引脚事件场景用于交叉核对，共 5904 个总线周期；包含分支、连续改写 I、短脉冲、BRK／IRQ 向量抢占、RDY 等待和 SO 与全部六种改写 V 指令的重叠。数据、重现命令及来源许可见 [测试说明](../crates/cpu6502/tests/data/README.md)。不能将这一修订的观察推广到所有 NMOS 芯片。

## 物理 RESET

`set_reset_line(true)` 表示低有效断言；setter 只改变输入，Phi2 结束的下降相位才采样。未跨过采样点的脉冲不触发。采样与时序链停下／写抑制之间还有一周期传播：已经启动的总线访问及内部传送仍可发生，不能撤销此前的写入，也不能在 setter 中立即调用 `begin_reset`。

执行器将外部输入、采样级、时序链停止和复位进行中锁存分开。`ResetHold` 每周期仍执行真实 Bus 读取；停止的是主时序推进，而不是冻结全部寄存器。旧指令选中的数据／ALU／栈传送及 RMW 尾部阶段仍可作用。内部地址锁存与存储的 PC 分开，例如 PCL 与读入数据在地址高字节通路重叠时，外部高地址可为 `DL & PCL`，存储的 PCH 却为 PCL；短脉冲释放时 SYNC 与随后 dummy read 因而可能不同。中间值来自实际数据和传送，不含夹具地址特判。

释放同样需要同步，随后通过同一执行器执行强制取指、dummy read、三次栈读及 `$FFFC/D` 向量读取。RDY 不阻止 RESET 采样，但会保持读阶段；从保持状态恢复也保留 RDY 的相位延迟。再次断言可以打断先前的复位入口，保留其已经发生的访问及寄存器影响。

物理复位在向量高字节读取完成时报告一次 `StepKind::Reset`，不伪造被打断指令的完成事件。`before`／`address` 是首次同步接管周期开始时的寄存器／PC；`cycles` 从该周期累计至完成，包含保持及 RDY 等待。同一未完成复位中的再次断言重启内部时序，但保留原始事件快照和累计周期。`at_instruction_boundary()` 在物理复位期间为 false。宿主七周期 `begin_reset`／`reset` 契约不变。

RESET 固定使用 `$FFFC/D`，不会被 NMI 抢占。边沿检测保持运行，复位向量低字节阶段之前的待处理 NMI 被清除，该阶段及之后新采样的边沿留给处理程序后的轮询；因此固定观察中 h20 的 NMI 消失、h22 的 NMI 保留。外部电平不被强行释放。I 在复位栈阶段设置；复位入口本身不清 D，但被打断指令的残留传送可能改变寄存器和标志。

`visual6502/reset.json` 的 **419 组／26816 周期**全部通过实际 CPU 的地址／数据／读写／SYNC 对照，包括处理程序对 A/X/Y/SP/P 的见证写入。比较前执行与参考模型相同的真实引导，再恢复引导后 RAM 快照；不能仅注入 `Registers` 来假定内部锁存器也已恢复。另用改变 PC、寄存器、RAM 和向量的 37 组独立观察交叉验证数据依赖行为。覆盖表和命令见 [测试说明](../crates/cpu6502/tests/data/README.md#物理-reset-对照)。范围限定为固定 revD 及记录的场景，不宣称所有 opcode×引脚相位组合、其他 NMOS 修订或模拟电气传播均已验证。

## RDY、SO 与半周期

`half_cycle` 在 Phi1／Phi2 间交替；Phi1 返回 `None`，Phi2 完成唯一一次 Bus 访问并返回 `Some(Cycle)`。`next_clock_phase` 表示下一次调用的相位；`cycle` 完成当前周期（若尚未执行 Phi1 则先执行它）。这是数字相位调度；没有模拟电压、建立／保持时间或晶体管传播延迟。寄存器仍是软件执行状态，不能据此读取所有物理内部锁存器的相位值。

`set_ready(false)` 令下一次读停在原地址，`Cycle.stalled=true`；每个等待周期仍真实读取设备，恢复时才使用该次读值推进指令。`stalled` 表示 RDY 阻止推进，不表示所有寄存器被冻结：上述已经安排的 SP 提交仍可发生。单独由 RESET 保持的周期不因此标为 RDY stalled。写周期继续，包括 RMW 连续两次写入，除非同步后的 RESET 写抑制生效；等待期间继续采样 IRQ/NMI/SO/RESET。`Step.cycles` 使用 `u64` 并包含等待。`step`／`reset` 每次最多推进 7 周期；RDY 或 RESET 保持超限返回可恢复的 `CycleBudgetExceeded`，不是死循环。需要更多等待时用 `step_with_cycle_budget`，或宿主带预算逐周期驱动；恢复复位用 step/cycle，不重复调用 reset。两种 RESET 输入与普通执行共用引擎，不绕开引脚采样。

`set_so_line(true)` 表示 SO 低有效断言；在 Phi1 采样下降边沿，再于下一 Phi1 更新 V。保持低电平不重复触发。根据固定 revD 相位扫描，BIT／PLP／RTI 的 V 写入延续至下一 Phi1，ADC／SBC 再晚一个 Phi1，CLV 的清除窗口覆盖两者；这些写入优先于重叠的 SO 更新。内部保留相应 V 写入窗口，避免在整条指令结束时简单置位／清除所导致的错误分支。该规则是本项目对固定模型观察的数字归纳，不宣称适用于所有 NMOS 修订或任意窄脉冲。

## 宿主诊断

`debug_state()` 无需 Bus，提供下一时钟相位、当前操作／下一总线阶段、地址和数据中间值、独立栈地址游标、等待计数、逻辑输入、IRQ/NMI/SO 采样及待处理状态、延迟 V 写入。RESET 另显示输入 `pins.reset` 与 `latches.reset_sample/reset_stop/reset_active`，分别观察采样、停止和进行中状态。`ExecutionPhase`（含 `ResetHold`）是执行器阶段名，不能等同于 MOS 内部 T 状态或晶体管节点。此快照没有恢复接口，也不包含所有 CPU 状态，不能作为存档格式。

CLI 的 `run_demo_with_trace` 提供真实 Cycle 和指令完成事件，既有 `run_demo` 仍只回调指令。`--trace`／`--bus-trace` 共用有界历史，默认 64 条，`--trace-limit` 限定为 1～4096；异常退出也保留最后记录。CPU 本身不保存日志或打印。

外部 SingleStep/Klaus runner 共用宿主诊断 helper，最多保留最后 32 个实际总线周期及其指令完成事件；失败先给首个差异和当前 debug_state，再给有限历史。SingleStep 同时给固定版本、opcode、用例索引与重放命令。数据解析失败不假造执行 trace；未支持 opcode 的错误保留实际读取的地址和字节。
