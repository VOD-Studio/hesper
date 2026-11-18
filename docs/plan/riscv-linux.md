# 自研 RISC-V Linux 整机：从本机串口到浏览器

**交付目标**：Hesper 用自己的解释器和设备模型，执行真实 OpenSBI、上游 Linux 内核和 BusyBox 用户空间。先在原生 CLI 通过串口操作 shell，再把**同一机器核心、同一组镜像**放入浏览器 Worker。不是模拟 Linux 命令、翻译 Linux 系统调用、连接远程终端，或把 QEMU/v86 嵌入产品。

**本次边界（L0）**：独立 workspace 库 `hesper-riscv-linux` 和固定 DRAM 地址窗口。库只回答“这段客体物理地址能否完整映射到 RAM 切片”，不分配整机内存、不执行指令。以下 L1–L8 都是实施计划，不是已有功能或运行记录。首个开发任务是 L1，不是做终端 UI。

## 1. 固定一台能说明白的机器

| 项目 | 第一台机器的约定 | 理由与边界 |
| --- | --- | --- |
| CPU | 小端、单 hart（ID 0）、RV64IMAC，另含 Zicsr、Zifencei、Zicntr；M/S/U 特权级 | Linux、固件与静态用户态统一使用匹配的整数 ISA / `lp64` ABI；不使用发行版常见的 `lp64d` 用户空间。无 F/D/V/H，无 SMP。 |
| 执行精度 | 解释执行；体系结构可见的寄存器、异常、页表和设备语义正确 | 不做微架构逐周期模拟、不要求与 6502 执行器共用接口；不把模拟 tick 解释为真实处理器性能。 |
| 虚拟内存 | Bare 和 Sv39，4 KiB 页及两级超页 | 不宣告 Sv48/Sv57、硬件虚拟化或未实现的扩展。SBI 调用陷入真正的 M 态固件；用户 syscall 陷入真正的 S 态 Linux。 |
| 板型 | QEMU `virt` 的明确子集：128 MiB RAM、CLINT、PLIC、一个 NS16550A UART | QEMU/TCG 仅作原生参照。设备树描述 Hesper 实现的机器，而不是复制整份 QEMU DTB。不是任意 `virt` 镜像兼容声明。 |
| 启动链 | OpenSBI `generic/fw_jump.bin` → RV64 Linux `Image` → 未压缩 `newc` initramfs → `/init` → BusyBox `sh` | 不做 U-Boot、UEFI、磁盘、网卡、图形、持久化或通用镜像导入。网页提供静态资源即可，客体无需联网。 |

**宿主分工**：`crates/riscv-linux` 拥有 hart、物理总线、RAM、设备、虚拟时间和装载规则；不访问文件、终端、浏览器或墙钟。原生宿主负责读取镜像、串口字节流和运行预算；Wasm 绑定只转换值；Worker 持有机器实例。CPU 与设备在确有实现时再分内部模块，不预建第二个 CPU crate、通用机器 trait 或空的 Wasm 包。[6502 / Apple I 架构](../architecture.md)与[现有 Web 工作台](../../examples/web/README.md)保持独立。

### 物理地址与 DTB 契约（区间均右开）

| 地址 | 用途与约束 |
| --- | --- |
| `0x0200_0000..0x0201_0000` | CLINT：hart 0 的 `msip`、`mtimecmp`、`mtime`；DTB 的 `timebase-frequency` 与机器同源，首版定为 10 MHz 的虚拟时基。 |
| `0x0c00_0000` 起 | PLIC：一个 hart 的 M/S external 中断上下文。按参考寄存器布局处理优先级、pending、enable、threshold、claim/complete。UART 接 source 10；DTB 端口与上下文顺序须与模型一致。 |
| `0x1000_0000..0x1000_0100` | UART0，寄存器步长 1 字节。`/chosen/stdout-path` 指向此 UART；区分轮询早期输出与由中断驱动的正常收发。 |
| `0x8000_0000..0x8800_0000` | DRAM。`0x8000_0000` 是固件入口；前 2 MiB 留给固件及其运行时数据。Linux `Image` 在 `0x8020_0000`，满足 RV64 的 2 MiB 对齐。 |
| DRAM 内的装载窗口 | 首版固定 initramfs 从 `0x8400_0000` 开始，DTB 放在 `0x8780_0000`；不是为每个窗口分配一块 RAM。生成 `/chosen/linux,initrd-start`、`linux,initrd-end`（尾后地址）和 bootargs；OpenSBI 的 FDT 搬移目标须与布局一致，不能沿用一个可能覆盖镜像的默认偏移。 |

装载器先验证**所有**区间再写入：除了输入字节长度，还要检查 Linux Image header 的有效镜像大小、固件 ELF/map 中的驻留段、BSS 与 scratch 需求、DTB 总长度和固件修改 FDT 所需空间。拒绝地址回绕、跨界、相互重叠及不支持的镜像格式；固件驻留区在交给内核的树中正确保留（`reserved-memory` / `no-map`）。RAM 之外由物理总线单独译码，未知地址报 access fault，不读回零蒙混。宿主空切片允许放在 RAM 尾后；不能据此允许 CPU 读写尾后地址。

## 2. 分阶段实施与退出条件

每阶段先写一项可观察的失败判据，再实现，最后记录命令、输入版本和预算。表中的产物必须真实出现才能进入下一阶段；早期探针不能代替 Linux 启动证明。

### L1 · 建立可重复的客体与参考启动（不依赖 Hesper CPU）

1. 以 QEMU 9.2.0 `virt`/TCG、OpenSBI v1.6 `generic`、Linux 6.12 系列和静态 BusyBox 为起点。把**实际能配套启动的精确版本/提交**、交叉编译器与 libc、构建命令、完整 config、DTS、来源和许可证写入清单。不要使用浮动的“最新版”。检查用户 ELF 的 ABI 和反汇编；预编译 `lp64d` 的 libc 不能混入软浮点工具链。
2. OpenSBI 设 XLEN=64、与目标一致的 ISA/ABI、固件基址 `0x80000000`、`FW_JUMP_ADDR=0x80200000`、`FW_JUMP_FDT_ADDR=0x87800000`。内核选 64 位 S 态、MMU、SBI、PLIC、8250 串口/控制台、initrd、procfs、devtmpfs、ELF 和脚本加载器；关闭 SMP、FPU、向量及首版不宣告的可选指令要求。区分可探测的可选优化与编译产物的硬需求。
3. initramfs 收入静态 BusyBox、`/bin/sh`、可执行 `/init`、`/dev/console`、`/proc` 等。`/init` 挂载 procfs/devtmpfs，把标准流接到串口控制台，启动交互 shell。固定文件顺序、属主、权限和时间戳；固定内核构建标识。保存 `fw_jump.bin`、`Image`、`initramfs.cpio`、`board.dtb` 的字节数、SHA-256 和重建方法。大文件放 `.cache/`，不让普通 Cargo 测试联网。
4. 用 `dtc` 编译目标树；用 QEMU `dumpdtb` + `dtc` 对照 CPU、实际 RAM、CLINT/PLIC 中断连线、UART 及 chosen 节点。以 `qemu-system-riscv64 -machine virt,aclint=off,aia=none -accel tcg -smp 1 -m 128M -display none -serial stdio -monitor none` 为基线，指定匹配的 CPU 扩展、`-bios`、`-kernel`、`-initrd`、`-dtb` 和 `-append 'console=ttyS0 rdinit=/init'`。检查最终传给内核的树：QEMU 可修订 chosen 和装载地址，不能直接把它的地址当作 Hesper 的实际值。

**退出**：参考机能冷启动到 shell；向串口输入 `uname -a`、`cat /proc/cpuinfo` 并捕获客体返回的内核版本、`processor : 0` 与 RISC-V ISA 信息。保存逐字节输入和串口输出，检查结果行而非命令回显。没有参考启动，不用 Hesper 的输出反推“正确预期”。

### L2 · 裸机整数执行

- 一个 hart 持有 32 个整数寄存器和 PC；`x0` 恒零。物理地址/客体寄存器用 `u64`；完成地址检查后才转宿主切片下标，不能把 RV64 地址截为 wasm32 `usize`。连续 RAM 一次分配，执行循环不按指令分配对象。
- 顺序实现取指、RV64I（含 W 形态的 32 位结果符号扩展）、M（除零、最小负数除以 −1）、A（LR/SC 预约、失效和 AMO）、C（16/32 位混合及半字边界），再加 fence 和 CSR 访问入口。未实现/非法编码走非法指令异常，不能当 NOP。非对齐数据访问选定合法的 trap 策略并固定；跨页取指留给 L4 联测。
- 批执行按有限步数返回，记录退休指令数和**另计**的推进预算，避免持续 trap 而永不归还宿主。调试读不能消费外设数据。失败报告只保留有限窗口：PC、指令字节、相关寄存器和最近访存。

**退出**：无文件 I/O 的小程序从 RAM 执行、写出预期签名并停在指定 PC；分批与一次推进结果相同。固定一组上游 ISA 测例与独立参考模拟器的架构状态/签名交叉比较，列出覆盖的扩展和例外，不声称完整 ISA 认证。

### L3 · 特权、异常与固件所需的保护

- 实现 M/S/U 模式、CSR 特权和只读检查、WARL 字段、`misa`、hart ID、trap 向量、cause/epc/tval、异常委派、中断使能与 pending；实现 `MRET`、`SRET`、`ECALL`、`EBREAK`、`WFI`。保留精确 fault PC，异常指令不退休。`cycle/time/instret` 区分语义并执行访问权限检查。
- 按固定固件验证 PMP 探测、地址匹配、优先级和 M/S/U 访问限制，测试固件驻留区被拒绝与合法 RAM 放行；同步处理 `mstatus.MPRV` 的有效特权级。不存在的可选 CSR 按规范响应，不为通过固件探测而随意读零。

**退出**：原创 M→S→U 探针分别执行 U/S ECALL、非法 CSR、越权访存、trap 返回；断言模式、委派目标、PC、cause/tval 及受保护 RAM 未变。不能把处理 SBI ECALL 的工作偷移到宿主。

### L4 · Sv39 地址翻译

- 先做不带 TLB 的三级页表遍历；实现 canonical VA、叶/非叶 PTE、无效编码、R/W/X/U、SUM/MXR、A/D 更新策略（首版选硬件置位）、超页对齐和 Bare/Sv39 切换。页表访问本身走受 PMP 约束的物理访问路径；区分 page fault 与 access fault。
- 分开取指、读、写/AMO 的权限与异常；32 位指令若横跨两页要分别翻译。`SFENCE.VMA` 在无缓存实现中无需失效缓存，但必须执行指令合法性检查。内核探测其他 `satp.MODE` 时按 WARL 规则拒绝，不把任意模式解释成 Sv39。

**退出**：用手工构造页表验证 4 KiB/2 MiB/1 GiB 叶映射、错误对齐、U/S 权限、跨页第二半条取指故障、改写 PTE 后重试；核对 fault 地址、epc、原因与 A/D 位。先证明受限探针能在分页下跑，再排查 Linux 早期映射。

### L5 · 设备、中断与真实 OpenSBI

- CLINT 由确定性虚拟时间推进，不读宿主系统时钟。明确每推进单位如何增加 `mtime`，以及 WFI 时如何前进到下一个 compare/外部输入；已到期 compare 不能重复触发虚假边沿。没有待处理事件时返回等待，等待期间不忙转，也不能令定时器永不唤醒。
- UART 先通轮询 TX，再实现 RX 队列、DLAB、线路状态、FIFO/中断使能与识别；读寄存器的副作用必须正确。中断路径为 UART → PLIC → hart external interrupt；定时器路径为 CLINT → M 态 OpenSBI → S 态 Linux。测试 claim/complete、屏蔽/解除屏蔽、读取后撤销电平及同时到达的事件。
- 加入事务式装载。复位入口直接设在 M 态固件首指令（不宣称模拟 QEMU 的 mask ROM），`a0=0`、`a1=DTB`；选 `FW_JUMP` 不依赖 `fw_dynamic_info`。检查固件日志之外的交接状态：首条内核指令前应为 S 态、`satp=0`、hartid 与 FDT 指针正确。需要的 ISA/CSR/设备若有差异，先在独立小探针中复现。

**退出**：真实 OpenSBI 能交接给装在内核地址的 S 态探针；探针通过 SBI BASE 查询版本、经 TIME 定时后从 WFI 恢复、经 UART 中断读入一个由宿主发送的字节并输出变换后的结果。仅看到固件 banner、宿主回显或 earlycon 输出不算通过。

### L6 · 原生 Linux 首次端到端验收

- 此时才给现有 CLI 增加独立 `linux` 子命令，与 `apple1` 平级。宿主从清单读取 L1 镜像并验哈希，批量推进、收发串口字节，支持非 TTY 脚本和交互会话。错误区分装载失败、客体 trap 循环、预算耗尽与用户退出；不更改无参数的既有行为。
- 用固定 bootargs 冷启动。按里程碑定位：OpenSBI 交接 → 内核启用分页 → 正常 `ttyS0` 接管 → `/init` 成为 PID 1 → 可交互 shell。若卡死，冻结最后一次推进，输出 PC/特权级、trap CSR、`satp`、中断与 UART 状态、串口尾部；用同一镜像在参考机定位首次可见分歧。

**退出**：真实 CLI/PTY 等待完整 shell 提示后，发送 L1 两条命令及带唯一标记的 `printf`，匹配返回的独立结果行、版本和 CPU 字段，而不是输入回显；非 TTY 跑同一脚本有界退出。另测缺文件、错误哈希及过大镜像能明确失败。在 `docs/verification.md` 记下具体散列、命令、指令/时间预算、峰值内存和串口记录。到这里才称“原生 Hesper 能启动这套 Linux”。

### L7 · 同一核心编译为 Wasm，接入 Worker

- 在 L6 通过后增加薄绑定，沿用现有 `wasm-bindgen` 打包方式。宿主接口只需装载、有限预算运行、发送/取出串口字节、结束实例与读取有限诊断；不把每条指令、每个字符或整块 RAM 封成 JS 对象。机器 RAM 一次分配；加载前检查镜像长度，避免在主线程、Worker 和 Wasm 同时保留多份大镜像。
- Linux 是独立工作区，不能复用 Apple I 的 40×24 字符屏或其键码过滤规则。Worker 内分批运行，回到事件循环后再发起下一批；不要在等待 shell 的单次同步 Wasm 调用中锁住 Worker。串口按字节成批传输，主线程负责终端显示、输入和取消/销毁。新会话不接收上一代 Worker 的迟到输出；暂停与恢复不能重装镜像或重置客体时钟。
- 浏览器只取可信清单指定的静态资源并校验；约定来源不可用或内存不足时的显式错误。首版不依赖 KVM、线程、SharedArrayBuffer 或跨源隔离响应头。镜像能否公开分发与本机能否构建是两个门禁；未满足再分发条件时使用本地选择的同哈希镜像验收，不擅自打包上线。

**退出**：Node/Bun 对实际 Wasm 产物重放脚本，校验与 L6 相同的客体结果；不同批次预算不改变按同一虚拟时刻注入输入的观察。记录内存峰值与冷启动耗时；这一步仍不叫“浏览器验收”。

### L8 · 真实浏览器验收与范围冻结

- 在 HTTP 上打开生产构建，用真实浏览器冷启动到 shell，键盘输入 `uname -a`、`cat /proc/cpuinfo`，复验版本、hart 和 ISA；输出必须来自 Worker 内的客体，页面不能自行生成。验证启动中取消、输入、暂停/继续、销毁后从空白 RAM 重启和页面仍可操作。至少记下浏览器版本、宿主配置、镜像下载字节数、到提示符耗时与最长不可响应时间，不拿原生耗时推算浏览器性能。
- `make wasm-browser-test` 扩入 Linux 绑定对照；Web 工作台另跑 `bun run check`、`bun run build` 和实际页面检查。普通离线回归只用小探针；真实镜像走显式集成门禁，数据未准备必须失败，不能 skip 后标绿。

**退出**：固定镜像在 QEMU、原生 Hesper、浏览器 Hesper 三处均留有独立交互证据。浏览器无法通过时保留 L6 的原生结论，不把构建成功或 Node 的通过写成网页已跑通。部署、Release 与新增设备是后续独立决策。

## 3. 执行纪律与依据

- 每个可独立观察的增量一个提交。L0 的聚焦命令是 `cargo test --locked -p hesper-riscv-linux`；`make verify` 保持现有 workspace 门禁。自 L2 起把首个分歧缩为可离线重放的探针；既不修改 6502 指令/总线契约，也不拿现有 Apple I 测试当作 RISC-V 的验收。
- 性能先测后改：记录解释器每秒退休指令数、到 shell 的退休数/虚拟时间和宿主耗时，区分镜像下载、复制和解释执行。只有 profile 证实页表遍历等为瓶颈才考虑 TLB 等优化；必须保留相同的异常、失效语义和跨宿主结果。
- 随参考镜像提供 Linux/BusyBox 对应源码、config、构建脚本及适用许可材料，核对 OpenSBI、工具链运行库和其他客体文件的分发要求。缺少权利或来源记录时不进 CDN/Pages，不把用作参照的 QEMU 计为运行时依赖。
- 原始依据：[RISC-V 非特权/特权 ISA 手册](https://docs.riscv.org/reference/isa/)、[SBI 规范](https://docs.riscv.org/reference/sbi/)、[Linux RISC-V 启动 ABI](https://docs.kernel.org/arch/riscv/boot.html)、[OpenSBI FW_JUMP](https://github.com/riscv-software-src/opensbi/blob/v1.6/docs/firmware/fw_jump.md) 与 [generic 默认偏移](https://github.com/riscv-software-src/opensbi/blob/v1.6/platform/generic/objects.mk)、[QEMU virt 设备说明](https://www.qemu.org/docs/master/system/riscv/virt.html) 与 [v9.2.0 地址表](https://github.com/qemu/qemu/blob/v9.2.0/hw/riscv/virt.c)。实施阶段以固定修订和可重放记录为准；上述链接不是 Hesper 的兼容性证明。
