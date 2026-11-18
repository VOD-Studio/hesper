# 自研 RISC-V Linux 整机：从本机串口到浏览器

**交付目标**：Hesper 用自己的解释器和设备模型，执行真实 OpenSBI、上游 Linux 内核和 BusyBox 用户空间。先在原生 CLI 通过串口操作 shell，再把**同一机器核心、同一组镜像**放入浏览器 Worker。不是模拟 Linux 命令、翻译 Linux 系统调用、连接远程终端，或把 QEMU/v86 嵌入产品。

**起点（L0）**：独立 workspace 库 `hesper-riscv-linux` 和固定 DRAM 地址窗口。L0 只校验 RAM 切片范围，不分配整机内存、不执行指令。各卡实际进度以[验证记录](../verification.md)为准；QEMU 上的启动不是 Hesper 的兼容性证据。

**实施结果**：同一组锁定镜像已通过 QEMU 参考、原生 Hesper（管道和 PTY）、
Bun / Node.js Wasm 与生产网页 Chromium 验收。见 [L6 本机记录](../verification.md#risc-v-linux-l6本机-cli-冷启动与串口)
及 [L8 浏览器记录](../verification.md#risc-v-linux-l8生产网页与范围冻结)。
浏览器使用本地文件；镜像未随站点分发。兼容范围仍为下文的单 hart 固定板。

## 0. 给实施者的入口（6.2 sol 从这里开始）

起点是分支 `feat/riscv-linux` 的 `d3f25f7`。如果分支上已有后续提交，以实际产物和验证记录判断进度，不回退到起点。先看[现有库](../../crates/riscv-linux/src/lib.rs)、[DRAM 边界测试](../../crates/riscv-linux/tests/dram.rs)和[仓库门禁](../../Makefile)。下表按卡列出工作与退出证据；每次只为当张卡创建用得到的文件。不要另起分支树、重写 README 或先搭一套通用虚拟机框架。

| 卡号（依次解锁） | 改动落点与第一个产物 | 交给下一张卡的证据 |
| --- | --- | --- |
| L1a 固定输入 → L1b 制作客体 → L1c 参考启动 | 主机工具 `tools/riscv-linux/`；版本清单、构建配置、`board.dts`、串口驱动脚本。只动工具与测试资料，不动 Rust CPU、CLI 或 Web。 | 从干净输出目录得到四个具名镜像；QEMU 上脚本通过，并记录有效 CPU/DTB 和镜像哈希。 |
| L2a 取指/写回 → L2b 整数 → L2c 原子/压缩 | `crates/riscv-linux/src/`，先写 `tests/isa.rs` 的四指令探针，再拆私有 hart、decode、物理访问。 | 每组有独立签名和首个分歧；同一程序按 1 步和多步推进一致。 |
| L3a CSR/陷入 → L3b 返回/委派 → L3c PMP | 库内特权状态；`tests/privilege.rs`。先做 U→S、S→M 两条陷入路径，再加入保护区。 | 同时核对故障指令地址、目标向量、模式和没有发生的写入。 |
| L4a 遍历 → L4b 权限/故障 → L4c 跨页 | 私有 MMU；`tests/sv39.rs` 以手工页表访问已知物理签名。 | 翻译正确、故障分类正确、失败后能从同一 EPC 重试。 |
| L5a 时钟/设备 → L5b 装载/交接 → L5c 固件探针 | 板级总线与设备、装载器；`tests/devices.rs`、`tests/boot.rs`。设备先寄存器测试，后进真实 OpenSBI。 | S 态探针从定时和 UART 中断返回。仅见串口文字不解锁 L6。 |
| L6 原生 shell | `crates/cli/src/main.rs` 分派至独立 Linux 会话；`tools/riscv-linux/native.py` 驱动真实二进制，复用参考串口脚本。 | L1c 的同一交互脚本通过管道和 PTY；记录预算与串口尾部。 |
| L7 绑定/调度 → L8 页面 | 新绑定在 L7 才入 workspace；绑定对照在 `tests/wasm/`，页面在 `examples/web/`。 | 真实 Wasm 与原生探针对照；随后真实浏览器操作同哈希客体，两份报告分开。 |

**续做规则**：认领最早一张没有退出证据的卡。第一条命令先检查它的输入，不凭上一轮聊天判断“已完成”。在 `docs/verification.md` 追加通过的卡号、提交对应的配置、命令、输入指纹、预期/实际、预算与记录路径。失败交接写成 `卡号 / 可复现命令 / 首个预期≠实际 / 最小输入 / 下一项检查`。大日志进 `.cache/`，关键差异和重放方法留在版本库；只写“已测”不能交接。如果缺工具，指出具体可执行文件或不支持的 ABI，不把跳过写成通过。每张可独立验证的行为增量通过对应门禁后再提交，不推送、不发布。

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

**L1a：先落输入和宿主工具，不写模拟器。** 首次候选冻结为 QEMU `v9.2.0`（TCG）、OpenSBI `v1.6`、Linux `v6.12`、Buildroot `2024.02.12` 自带的 BusyBox `1.36.1` 与 musl。它们是根据上游源码选的构建输入，**不是已启动的组合**。第一份提交应能获取并校验这些固定源码；记录下载 URL、提交或发行包版本、独立来源的 SHA-256、补丁和许可证。拒绝未钉版本的系统 QEMU、空校验列表、哈希不符和静默回退。宿主先检查 `make`、C/C++ 编译器、Python 3 与 Buildroot 所需构建工具；在用到时检查 `dtc/fdtget`、支持 TCG 的 `qemu-system-riscv64`。不要要求用户预装客体 libc 或把宿主 `/usr/bin/gcc` 当成交叉编译器。

**交付目录**：把可评审的小输入放 `tools/riscv-linux/`（版本锁、Buildroot defconfig、kernel fragment、rootfs overlay、DTS、准备和参考启动脚本）；仓库根下的 `.cache/riscv-linux/` 分 `src/`、`build/`、`images/`、`logs/`。清单区分**源码锁**与**产物锁**：产物每项记相对路径、字节数、SHA-256、装载地址/有效区间；另记 CPU 参数、bootargs、`kernelrelease`、编译器、config 指纹和源码锁 ID。四个产物名固定为 `fw_jump.bin`、`Image`、`initramfs.cpio`、`board.dtb`。未实测前不填虚构哈希；第一次参考验收后提交锁，后续重建只比对，更新必须单独审查。ELF、map、完整构建/串口日志留在缓存。普通 Cargo 测试不联网。

**L1b：按这个顺序制作客体，混用 ABI 是第一排查项。**

构建入口是 `make riscv-linux-data`，构建器需要原生 Linux 和 Python 3.11+。
若宿主缺少 `dtc` 等工具，可使用固定 Debian 12 基础镜像：

```sh
docker build -t hesper-riscv-build tools/riscv-linux
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/work" -w /work \
  -e RISCV_LINUX_JOBS=8 hesper-riscv-build make riscv-linux-data
```

设置 `RISCV_LINUX_CACHE` 到另一空目录可独立复建；只共享已验的源码归档和
Buildroot 下载缓存，不共享 `build/` 或 `images/`。构建记录和验收状态见
[验证记录](../verification.md#risc-v-linux-l1a固定源码输入)。

1. 用 Buildroot 的内部工具链，不用发行版 `riscv64-linux-gnu` 的默认 `lp64d` sysroot。起始 defconfig 逐项设为 `y`：`BR2_riscv`、`BR2_riscv_custom`、`BR2_RISCV_64`、`BR2_RISCV_USE_MMU`、`BR2_RISCV_ISA_RVM`、`BR2_RISCV_ISA_RVA`、`BR2_RISCV_ISA_RVC`、`BR2_RISCV_ABI_LP64`、`BR2_TOOLCHAIN_BUILDROOT_MUSL`、`BR2_STATIC_LIBS`、`BR2_INIT_BUSYBOX`、`BR2_TARGET_ROOTFS_CPIO`、`BR2_TARGET_ROOTFS_CPIO_NONE`、`BR2_REPRODUCIBLE`。I 由 custom 选择，不选 G/F/D/V。[Buildroot 架构选项](https://github.com/buildroot/buildroot/blob/2024.02.12/arch/Config.in.riscv)与[该版 BusyBox](https://github.com/buildroot/buildroot/blob/2024.02.12/package/busybox/busybox.mk)是依据。用绝对路径执行 `make -C "$BR" O="$BR_OUT" BR2_DEFCONFIG="$DEFCONFIG" defconfig` 再 `make -C "$BR" O="$BR_OUT"`；`BR` 是已校验源码，`BR_OUT` 是缓存构建目录。核对实际 `.config` 和 `host/bin/` 下生成的编译器，以其带尾部 `-` 的前缀设置 `CROSS_COMPILE`。不要在 Buildroot 里另建一份内核。
2. 在 defconfig 指定 `BR2_ROOTFS_OVERLAY`，放入 0755 的 `/init`；在构建 cpio **前**创建 overlay。`/init` 用 `/bin/sh` 挂载 `devtmpfs` 到 `/dev`、`proc` 到 `/proc`、`sysfs` 到 `/sys`，`export PS1='hesper# '`，`exec /bin/sh -i </dev/console >/dev/console 2>&1`。收录 `/bin/sh` 对 BusyBox 的链接和 mount、uname、cat、printf applet；核对 `/dev/console` 字符设备主次号 5:1。产物取 `"$BR_OUT/images/rootfs.cpio"`，不是直接打包 `target/`；见 [newc 与 console 节点生成规则](https://github.com/buildroot/buildroot/blob/2024.02.12/fs/cpio/cpio.mk)。`cpio -itv` 检查条目、权限与解释器；对 BusyBox 跑交叉 `readelf -h -A -l`：ELF64/RISC-V、soft-float ABI，不能有 `PT_INTERP`，不能要求 F/D/V。RVC 标志本身不表示浮点。
3. 用同一交叉工具链运行 OpenSBI 的 `make`，指定 `PLATFORM=generic PLATFORM_RISCV_XLEN=64 PLATFORM_RISCV_ISA=rv64imac_zicsr_zifencei PLATFORM_RISCV_ABI=lp64 FW_TEXT_START=0x80000000 FW_JUMP=y FW_JUMP_ADDR=0x80200000 FW_JUMP_FDT_ADDR=0x87800000`。Buildroot 默认的 GNU Binutils 2.40 不识别 `zicntr` 属性，但能以这些参数编码 `rdtime`、`rdcycle` 和 `rdinstret`；只从编译参数去掉该属性，客体板仍提供 Zicntr。保留 `platform/generic/firmware/fw_jump.{elf,bin}`；检查 ELF 程序头、符号与最终编译参数。绝对地址优先于 generic 默认 OFFSET；FDT 参数是搬移目标，不会替你生成 DTB，见 [v1.6 交接汇编](https://github.com/riscv-software-src/opensbi/blob/v1.6/firmware/fw_jump.S)。
4. 独立构建 Linux：先导出 `ARCH=riscv` 和上一步的 `CROSS_COMPILE`，运行 `make -C "$LINUX" O="$KOUT" defconfig`；从内核源码目录运行 `scripts/kconfig/merge_config.sh -m -O "$KOUT" "$KOUT/.config" "$FRAGMENT"`，再执行 `make -C "$LINUX" O="$KOUT" olddefconfig` 和 `make -C "$LINUX" O="$KOUT" Image`。检查**最终** `.config`：RV64、MMU、SBI、`RISCV_ISA_C`、`SIFIVE_PLIC`、`SERIAL_8250`/`SERIAL_8250_CONSOLE` 和 OF 串口驱动、`BLK_DEV_INITRD`、`BINFMT_ELF`/`BINFMT_SCRIPT`、`PROC_FS`、`SYSFS`、`DEVTMPFS` 均为内建；`SMP/FPU/RISCV_ISA_V` 关闭。不要只改 fragment 就宣称选项生效；[v6.12 架构 Makefile](https://github.com/torvalds/linux/blob/v6.12/arch/riscv/Makefile)决定实际 ISA/ABI。取 `"$KOUT/arch/riscv/boot/Image"` 并保留 `vmlinux`。固定 `SOURCE_DATE_EPOCH`、`KBUILD_BUILD_TIMESTAMP/USER/HOST/VERSION` 和本地版本字符串；在另一干净构建目录复验哈希，不把第一次生成的哈希当作可重复构建证明。
5. 按**实际** cpio 长度生成最终 `board.dts`/`board.dtb`，预留固件修改空间。根节点和 `soc` 用两 cell 地址/大小，`cpus` 用一 cell hart ID、零 size cell。CPU0 标 `mmu-type="riscv,sv39"`，列出真实 ISA，`cpu-intc` 为 hart 本地中断父节点；CLINT 的 `interrupts-extended` 接本地 3/7，PLIC 的 M/S 接 11/9；UART 为 `ns16550a`、`clock-frequency=3686400`、PLIC 源 10。`/memory` 为 128 MiB、`/chosen` 设置 stdout-path、`console=ttyS0 rdinit=/init` 和两项 initrd 地址。`dtc` 编译后用 `fdtget` 逐项读回，确认地址 cell 的高低顺序，检查 `initrd-end - initrd-start` 等于**字节数**。保留 `reserved-memory`、phandle 和设备连线；不复制 QEMU 的 PCI、virtio、RTC。对照 [Linux 的 CPU 节点约束](https://github.com/torvalds/linux/blob/v6.12/Documentation/devicetree/bindings/riscv/cpus.yaml)。

**L1c：一台参考机、同一物理布局。** 在同一构建宿主内运行 `make riscv-linux-qemu` 从锁定源码构建 QEMU/TCG，再运行 `make riscv-linux-reference`。参考门禁不下载、不构建，只消费已准备的 QEMU 和 [`images.lock.json`](../../tools/riscv-linux/images.lock.json) 所列镜像。CPU 参数固定在 `qemu-cpu.txt`；通过 QMP 读回 IMAC、Zicsr/Zifencei/Zicntr、S/U、Sv39、PMP 及禁用的 F/D/V/H、Sstc、Sv48/Sv57。另存**不传自定义树时**的 `dumpdtb`；QEMU 的不可配置隐式特性单独记录，不加入 Hesper 的板描述。参考命令的启动部分为：

```sh
qemu-system-riscv64 -machine virt,aclint=off,aia=none -accel tcg -smp 1 -m 128M \
  -cpu "$(cat "$PROFILE/qemu-cpu.txt")" -display none -monitor none -serial stdio \
  -bios "$IMAGES/fw_jump.bin" -kernel "$IMAGES/Image" -dtb "$IMAGES/board.dtb" \
  -device "loader,file=$IMAGES/initramfs.cpio,addr=0x84000000,force-raw=on"
```

`PROFILE` 指向已冻结配置目录，`IMAGES` 指向已校验的 `images/`。这里**不用 `-initrd`**：在 QEMU v9.2.0 的[装载实现](https://github.com/qemu/qemu/blob/v9.2.0/hw/riscv/boot.c)中，128 MiB、内核入口 `0x80200000` 会让 `-initrd` 放到 `0x84200000` 并改写 chosen，不是本板的 `0x84000000`。raw loader 不设置 PC，initrd 起止和 bootargs 已在传入树中。用实际固件交接值/客体 `/proc/device-tree/chosen` 复核，不把源 DTS 当运行时证据。先保留原始串口日志，再按 L6 的同一脚本判定成功。若默认 QEMU CPU 能启动而收紧后失败，首查 ELF attributes、最终内核 config 和可选扩展探测；不放宽 Hesper 板型来“通过”参考机。

**退出**：参考机能冷启动到 shell；向串口输入 `uname -a`、`cat /proc/cpuinfo` 并捕获客体返回的内核版本、`processor : 0` 与 RISC-V ISA 信息。保存逐字节输入和串口输出，检查结果行而非命令回显。没有参考启动，不用 Hesper 的输出反推“正确预期”。

### L2 · 裸机整数执行

- 一个 hart 持有 32 个整数寄存器和 PC；`x0` 恒零。物理地址/客体寄存器用 `u64`；完成地址检查后才转宿主切片下标，不能把 RV64 地址截为 wasm32 `usize`。连续 RAM 一次分配，执行循环不按指令分配对象。
- 顺序实现取指、RV64I（含 W 形态的 32 位结果符号扩展）、M（除零、最小负数除以 −1）、A（LR/SC 预约、失效和 AMO）、C（16/32 位混合及半字边界），再加 fence 和 CSR 访问入口。未实现/非法编码走非法指令异常，不能当 NOP。非对齐数据访问选定合法的 trap 策略并固定；跨页取指留给 L4 联测。
- 批执行按有限步数返回，记录退休指令数和**另计**的推进预算，避免持续 trap 而永不归还宿主。调试读不能消费外设数据。失败报告只保留有限窗口：PC、指令字节、相关寄存器和最近访存。

**L2a 第一条可执行规格**：从 `DRAM_BASE` 起装入下列 4 个小端指令字，初始寄存器清零，M 态、Bare，不读取镜像文件。只实现这四条所需的执行/访存闭环，再扩展指令族；不要在生产代码中识别此程序。这里的停止条件是宿主在执行第 5 条前用尽 4 步预算，不把 `EBREAK` 当作关机。

```asm
.option norvc
addi  x1, x0, 7       # 0x00700093
auipc x2, 0           # 0x00000117；x2 = DRAM_BASE + 4，不是下一条 PC
sw    x1, 28(x2)      # 0x00112e23；签名位于 DRAM_BASE + 0x20
addi  x0, x1, 1       # 0x00108013；写 x0 被丢弃
```

预期 `pc=0x80000010`，`x0=0, x1=7, x2=0x80000004`，签名字节为 `07 00 00 00`，退休 4 条。逐步与 `1+3`、`4` 三种切片比较。随后按 **load/store 宽度与符号 → 分支/JAL/JALR 链接地址 → W 与乘除边界 → LR/SC/AMO.W/D → 压缩指令** 扩展。每族至少包含一个会区分错误实现的反例，例如 `DIVW(INT32_MIN,-1)` 符号扩展、连续两次 `SC.W`（不重新 LR）第二次必失败、合法 32 位指令起于半字边界。`FENCE.I` 要能在客体改写代码后取到新指令；无指令缓存不等于把所有 fence 编码都当 NOP。

**推进接口的行为（名字可随现有代码，语义不可省）**：原生和 Wasm 共用一个 `run(fuel)` 和同一个单步内核，`fuel=0` 不产生副作用。一轮只处理一次可中断的状态转移：接收中断、尝试执行一条指令或推进等待事件；分别记 `spent`、`retired` 和虚拟时间。同步异常/中断不增加 `retired`，仍消耗预算。耗尽预算是可恢复返回，返回后下一批不能重复已提交的访存；客体非法指令交给客体 trap，宿主装载错误才是宿主错误。第一版不做 public 通用 `Bus`；区分取指、读、写/原子、无副作用调试观察，设备寄存器绝不可用一次“预读”来生成 trace。

外部交叉验证从 [riscv-tests](https://github.com/riscv-software-src/riscv-tests) 的 `rv64ui-p`、`rv64um-p`、`rv64ua-p`、`rv64uc-p` 中各挑明确用例，锁定提交与环境子模块。保存源、最终反汇编、入口、签名地址和独立参考结果。若其启动代码依赖尚未完成的特权 CSR，L2 先重放相同指令主体与操作数，明确标为“指令探针”；L3 后才跑完整测试 ELF。`tohost` 仅作外部测试宿主的停止观察，不做成产品板上 Linux 可访问的设备。失败先缩成一个用例及首个不同的 PC/寄存器/内存值，不能改期望来匹配解释器。

**退出**：无文件 I/O 的小程序从 RAM 执行、写出预期签名并停在指定 PC；分批与一次推进结果相同。固定一组上游 ISA 测例与独立参考模拟器的架构状态/签名交叉比较，列出覆盖的扩展和例外，不声称完整 ISA 认证。

### L3 · 特权、异常与固件所需的保护

- 实现 M/S/U 模式、CSR 特权和只读检查、WARL 字段、`misa`、hart ID、trap 向量、cause/epc/tval、异常委派、中断使能与 pending；实现 `MRET`、`SRET`、`ECALL`、`EBREAK`、`WFI`。保留精确 fault PC，异常指令不退休。`cycle/time/instret` 区分语义并执行访问权限检查。
- 按固定固件验证 PMP 探测、地址匹配、优先级和 M/S/U 访问限制，测试固件驻留区被拒绝与合法 RAM 放行；同步处理 `mstatus.MPRV` 的有效特权级。不存在的可选 CSR 按规范响应，不为通过固件探测而随意读零。

**L3 验收矩阵**（先用自己的异常处理程序，不等 OpenSBI 给反馈）：

| 探针 | 必须检查的状态，不只比较终态寄存器 |
| --- | --- |
| U ECALL 委派给 S；S ECALL 未委派给 M | `scause=8` / `mcause=9`；各自 EPC 指向 ECALL 而非下一条。处理程序显式递增 EPC 后 `xRET`；核对 xPP、xPIE、xIE 的入栈和恢复。 |
| 读、写只读 CSR 的差别 | 用 `CSRRS` 且 `rs1=x0` 合法读取 `mhartid`；`CSRRW` 写同一只读 CSR 触发 illegal instruction（cause 2）。较低特权读取 M CSR 也触发，目的寄存器不改。 |
| 中断尚未使能、变为可取、委派后处在更高特权 | 分开维护外设电平、pending、enable、全局使能与目标特权；vectored 模式只有异步中断走 `BASE + 4×cause`，同步异常仍去 BASE。不能把“未取中断”误写成“清除设备请求”。 |
| PMP 区域边界和锁定位 | 固定一套 OpenSBI 能探测出的槽数、粒度、支持的 TOR/NA4/NAPOT 模式并记录；匹配按优先级且须覆盖整个访问。测试 S/U 未命中拒绝、M 对未锁普通条目的规则及已锁条目、MPRV 对数据而非取指的作用。不要靠放行所有访问启动固件。 |

CSR 按用途纳入：先 `mstatus/misa/mhartid` 与 trap CSR，再 `mie/mip/medeleg/mideleg` 以及受掩码约束的 S 态视图，最后 `mcounteren/scounteren`、PMP、`satp` 和固件实际探测的 CSR。每项记录复位值、可写掩码、WARL 值和别名关系；没有实现的可选项按该 CSR 的架构定义处理，不批量返回零。

**退出**：原创 M→S→U 探针分别执行 U/S ECALL、非法 CSR、越权访存、trap 返回；断言模式、委派目标、PC、cause/tval 及受保护 RAM 未变。不能把处理 SBI ECALL 的工作偷移到宿主。

### L4 · Sv39 地址翻译

- 先做不带 TLB 的三级页表遍历；实现 canonical VA、叶/非叶 PTE、无效编码、R/W/X/U、SUM/MXR、A/D 更新策略（首版选硬件置位）、超页对齐和 Bare/Sv39 切换。页表访问本身走受 PMP 约束的物理访问路径；区分 page fault 与 access fault。
- 分开取指、读、写/AMO 的权限与异常；32 位指令若横跨两页要分别翻译。`SFENCE.VMA` 在无缓存实现中无需失效缓存，但必须执行指令合法性检查。内核探测其他 `satp.MODE` 时按 WARL 规则拒绝，不把任意模式解释成 Sv39。

**L4 故障判据**：用固定 VA 造三级小端 64 位 PTE，先用 4 KiB 叶读出 `0x11223344`，再分别改一个条件。`W=1,R=0`、非法超页 PPN、用户取指非 U 页 → 对应取指/读/写 page fault（12/13/15）；PMP 阻止读取页表 → 按原访问类型报 access fault（1/5/7），不是“页不存在”。SUM 只放宽 S 态的数据访问，不允许 S 执行 U 页；MXR 允许读取 execute-only 页，不授予写权限。跨页探针让 32 位指令的首半字位于页尾 `…ffe`、第二页无映射：EPC 指向首半字、`tval` 指向下一页起点；补上映射并返回后只退休一次。记录修改的叶 PTE：读使 A 置位，成功写使 D 置位。对无效 `satp.MODE` 的写不应顺便改掉原 PPN。

**退出**：用手工构造页表验证 4 KiB/2 MiB/1 GiB 叶映射、错误对齐、U/S 权限、跨页第二半条取指故障、改写 PTE 后重试；核对 fault 地址、epc、原因与 A/D 位。先证明受限探针能在分页下跑，再排查 Linux 早期映射。

### L5 · 设备、中断与真实 OpenSBI

- CLINT 由确定性虚拟时间推进，不读宿主系统时钟。明确每推进单位如何增加 `mtime`，以及 WFI 时如何前进到下一个 compare/外部输入；已到期 compare 不能重复触发虚假边沿。没有待处理事件时返回等待，等待期间不忙转，也不能令定时器永不唤醒。
- UART 先通轮询 TX，再实现 RX 队列、DLAB、线路状态、FIFO/中断使能与识别；读寄存器的副作用必须正确。中断路径为 UART → PLIC → hart external interrupt；定时器路径为 CLINT → M 态 OpenSBI → S 态 Linux。测试 claim/complete、屏蔽/解除屏蔽、读取后撤销电平及同时到达的事件。
- 加入事务式装载。复位入口直接设在 M 态固件首指令（不宣称模拟 QEMU 的 mask ROM），`a0=0`、`a1=DTB`；选 `FW_JUMP` 不依赖 `fw_dynamic_info`。检查固件日志之外的交接状态：首条内核指令前应为 S 态、`satp=0`、hartid 与 FDT 指针正确。需要的 ISA/CSR/设备若有差异，先在独立小探针中复现。

**L5a 固定接线与台架**（先用总线读写，不靠串口日志猜寄存器）：

| 部件 | 实现起点 | 可区分错误实现的验收 |
| --- | --- | --- |
| CLINT | hart 0 的 `msip=base+0`、`mtimecmp=base+0x4000`、`mtime=base+0xbff8`。处理参考固件用到的 32/64 位访问；`time` CSR 取同一时基。 | `mtime < cmp` 时无 MTIP，等于时置位，把 cmp 移到未来后撤销；WFI 不在无事件时空耗宿主。用固定输入 tick，按不同批大小重放。 |
| PLIC | 32 位 MMIO。priority `+4×id`、pending `+0x1000`、enable `+0x2000+0x80×ctx`；threshold `+0x200000+0x1000×ctx`，claim/complete 再加 4。ctx 0/1 分别接 hart 0 的 MEIP/SEIP，ID 0 表示无源。 | 给 UART 源 10 设非零优先级并只在 S context 使能；检查阈值对通知的屏蔽、claim 读的副作用、complete 后源仍拉高时重报，撤销源后不重报。不能把 claim 实现为无副作用调试读取。 |
| UART | 字节偏移 0/1 受 DLAB 控制（RBR/THR、IER 或 DLL/DLM）；2 是读 IIR / 写 FCR；3–7 为 LCR/MCR/LSR/MSR/SCR。确定 FIFO 深度、触发水位及超时策略，以固定版本 8250 驱动的探测与 16550A 语义为准。 | 配置波特率时写 DLL 不能输出字符；RBR 只消费一个字节；RX 有数据与 IER 屏蔽分开；低于 FIFO 水位的单字节也要能按接收超时送达；空队列调试观察不得清除中断。 |

首版定时模型是功能模型：正常推进一次 CPU 尝试走一个虚拟 tick，`timebase-frequency=10000000`，不要求墙钟 10 MHz。已停在 WFI 时先判断本地使能中断的唤醒条件（与实际取 trap 的全局条件分开）；否则最多推进至最早的 compare、UART 超时或脚本输入事件，重新计算电平再决定是否取 trap。没有未来事件就返回等待；暂停时不推进时间。串口脚本以虚拟 tick 或“收到完整提示符”注入，不能用宿主 `sleep` 代替硬件时序。

**L5b 装载顺序**：先从已验哈希的文件读取元数据，按 Linux v6.12 的 [64 字节 Image 头](https://github.com/torvalds/linux/blob/v6.12/arch/riscv/include/asm/image.h)检查版本、`magic2`、小端标志、`text_offset` 与非零 `image_size`。校验 `max(文件长度, image_size)` 的物理范围；FDT header 各字段则按**大端**读取。核对 `totalsize`、所需余量、initrd 尾后地址与已保留固件范围，最后才一次提交装载和复位状态。反例：把 initrd 末端伸入 DTB，返回明确冲突的两段名称，原会话的 RAM 和 PC 不变。`FW_JUMP` 的 `a1` 若入参已等于目标地址则不再搬移，不能假定固件会替宿主修复无效 DTB。

**退出**：真实 OpenSBI 能交接给装在内核地址的 S 态探针；探针通过 SBI BASE 查询版本、经 TIME 定时后从 WFI 恢复、经 UART 中断读入一个由宿主发送的字节并输出变换后的结果。仅看到固件 banner、宿主回显或 earlycon 输出不算通过。

### L6 · 原生 Linux 首次端到端验收

- 此时才给现有 CLI 增加独立 `linux` 子命令，与 `apple1` 平级。宿主从清单读取 L1 镜像并验哈希，批量推进、收发串口字节，支持非 TTY 脚本和交互会话。错误区分装载失败、客体 trap 循环、预算耗尽与用户退出；不更改无参数的既有行为。
- 用固定 bootargs 冷启动。按里程碑定位：OpenSBI 交接 → 内核启用分页 → 正常 `ttyS0` 接管 → `/init` 成为 PID 1 → 可交互 shell。若卡死，冻结最后一次推进，输出 PC/特权级、trap CSR、`satp`、中断与 UART 状态、串口尾部；用同一镜像在参考机定位首次可见分歧。

**串口验收脚本只维护一份，L1c、L6、L7、L8 各自提供传输适配。** 镜像里的 `/init` 显式设置 `PS1='hesper# '`，从 `/dev/console` 接入 BusyBox `sh -i`，不借用 Apple I 键盘转换。驱动按字节累积（提示符可能跨批且没有换行）；见到提示才写下一行。发送以下**字面文本**给客体，不在宿主 shell 中提前展开：

```sh
uname -a
cat /proc/cpuinfo
printf 'HESPER_OK:%s\n' "$((6 * 7))"
```

保留原始串口记录。比较时只规范化 CRLF，要求 `uname` 结果行含清单中的内核 release、`/proc/cpuinfo` 有 hart 0 及相符的 ISA 字段、最后独立一行恰为 `HESPER_OK:42` 并再次收到提示符。回显中没有计算结果；把命令回显当输出的实现应失败。QEMU 和 Hesper 不比较整屏日志、bogomips 或墙钟耗时。给每个等待状态设截止条件；超限报最后完成的状态、串口尾部与预算。验收驱动见到最后的提示符才结束会话并回收进程/Worker，不依赖这块板上不存在的关机设备。

**首次分歧怎么找**：

| 最后可见的事实 | 先取什么证据 | 不要做什么 |
| --- | --- | --- |
| 没有固件输出 | 在 `fw_jump.elf` 的 `_start` 对照首批 PC/指令、`a0/a1`、FDT magic，查首个 trap；再查 UART THR 写 | 不要直接手工写 banner 或跳到内核入口。 |
| 固件打印完成，内核无输出 | 在 `0x80200000` 比较交接模式、`satp/a1` 与 DTB；保留未剥离 `vmlinux`，按符号定位第一次写 `satp` 后的取指 | 不要用常开 identity map 绕过 Sv39。 |
| 有 earlycon，正常控制台接管后不能输入 | 比较 UART 的 IER/IIR/LSR、PLIC 的 S enable/threshold/claim 和 `sip/sie`，再看 guest 是否配置 `console=ttyS0` | 不要把输入直接塞到 Linux tty 缓冲区。 |
| 内核起来但找不到 `/init`，或执行它时报错 | 对比交接树里的 initrd 起止与 cpio 清单、`/init` 模式位/解释器、`/bin/sh` 目标与 BusyBox ELF 的 ABI/是否需要动态加载器 | 不要为启动成功而把 shell 换成宿主进程。 |
| 有提示符但空闲后收不到下一字节 | 对照 WFI 前后的下一事件、TIME 重编程和 RX 超时；记录是哪一批不再归还控制 | 不要不断注入虚假中断或调大预算遮住挂起。 |

**退出**：真实 CLI/PTY 等待完整 shell 提示后，发送 L1 两条命令及带唯一标记的 `printf`，匹配返回的独立结果行、版本和 CPU 字段，而不是输入回显；非 TTY 跑同一脚本有界退出。另测缺文件、错误哈希及过大镜像能明确失败。在 `docs/verification.md` 记下具体散列、命令、指令/时间预算、峰值内存和串口记录。到这里才称“原生 Hesper 能启动这套 Linux”。

### L7 · 同一核心编译为 Wasm，接入 Worker

- 在 L6 通过后增加薄绑定，沿用现有 `wasm-bindgen` 打包方式。宿主接口只需装载、有限预算运行、发送/取出串口字节、结束实例与读取有限诊断；不把每条指令、每个字符或整块 RAM 封成 JS 对象。机器 RAM 一次分配；加载前检查镜像长度，避免在主线程、Worker 和 Wasm 同时保留多份大镜像。
- Linux 是独立工作区，不能复用 Apple I 的 40×24 字符屏或其键码过滤规则。Worker 内分批运行，回到事件循环后再发起下一批；不要在等待 shell 的单次同步 Wasm 调用中锁住 Worker。串口按字节成批传输，主线程负责终端显示、输入和取消/销毁。新会话不接收上一代 Worker 的迟到输出；暂停与恢复不能重装镜像或重置客体时钟。
- 浏览器只取可信清单指定的静态资源并校验；约定来源不可用或内存不足时的显式错误。首版不依赖 KVM、线程、SharedArrayBuffer 或跨源隔离响应头。镜像能否公开分发与本机能否构建是两个门禁；未满足再分发条件时使用本地选择的同哈希镜像验收，不擅自打包上线。

**具体接入顺序**：

1. 加 `hesper-riscv-linux-wasm` 薄包，延用 workspace 固定的 `wasm-bindgen`，不把它加入机器 crate 的依赖。先在[打包脚本](../../tools/build_wasm.ts)增加 web/nodejs 两种真实产物，注意 Rust 包名中的 `-` 与 Wasm 文件名中的 `_`。先用四指令探针做原生/JS 对照，再引入大镜像。RV64 的寄存器、地址、累计步数跨 JS 边界用 `bigint` 或十六进制字符串，不经 JS `number` 截断；单批预算可用 `u32`。
2. 增加 Linux 专用 Worker，在进入 Linux 工作区时才创建；不要把 Linux 包或镜像加进[现有 Worker](../../examples/web/src/emulator.worker.ts)的启动 `Promise.all`，否则 Linux 资源失败会连带破坏 Apple I/6502。复用[资源准备脚本](../../examples/web/scripts/prepare.ts)的构建产物路径与静态站子路径约定；不将 Linux 镜像内联进 JS，不手改 `src/generated/`。
3. 消息最小集：UI 发 `boot/sessionId`、`stdin/bytes`、`pause`、`resume`、`stop`；Worker 回 `ready`、`stdout/bytes`、`state`、`error`，每条都带会话 ID。字节用 `Uint8Array` 分批；若转移 `ArrayBuffer`，只转移该会话独占的 buffer。状态只需“装载中、运行、暂停、等待输入、失败、已结束”。等待输入不同于暂停：前者有合法设备事件就能被唤醒；后者只允许控制消息改变运行状态。
4. 用实际产物测三个反例：快速结束 A 再启动 B，A 的迟到输出不能出现在 B；损坏单个镜像字节，启动必须在执行客体前失败；暂停期间排入输入，恢复后仅消费一次。测试还要打开原 Apple I 工作区确认它在没有 Linux 镜像时照常使用。不要用假的 Worker、Node 截图或 UI 自造结果替代 L8。

**退出**：Node/Bun 对实际 Wasm 产物重放脚本，校验与 L6 相同的客体结果；不同批次预算不改变按同一虚拟时刻注入输入的观察。记录内存峰值与冷启动耗时；这一步仍不叫“浏览器验收”。

### L8 · 真实浏览器验收与范围冻结

- 在 HTTP 上打开生产构建，用真实浏览器冷启动到 shell，键盘输入 `uname -a`、`cat /proc/cpuinfo`，复验版本、hart 和 ISA；输出必须来自 Worker 内的客体，页面不能自行生成。验证启动中取消、输入、暂停/继续、销毁后从空白 RAM 重启和页面仍可操作。至少记下浏览器版本、宿主配置、镜像下载字节数、到提示符耗时与最长不可响应时间，不拿原生耗时推算浏览器性能。
- `make wasm-browser-test` 扩入 Linux 绑定对照；Web 工作台另跑 `bun run check`、`bun run build` 和实际页面检查。普通离线回归只用小探针；真实镜像走显式集成门禁，数据未准备必须失败，不能 skip 后标绿。

**门禁分层，不把 128 MiB 整机启动塞进每个单测**：

| 层 | 入口（除标为“已有”外，随对应卡实现） | 通过才可声称 |
| --- | --- | --- |
| L1 参考机 | `make riscv-linux-data` 显式准备/验哈希；`make riscv-linux-reference` 只消费已准备的镜像，运行参考串口脚本。 | 客体与板描述能在固定 QEMU 上工作；尚非 Hesper 证据。 |
| L2–L5 小探针 | 已有 `cargo test --locked -p hesper-riscv-linux`、`make verify`；实现新测试 target 后可加 `--test isa`、`--test privilege`、`--test sv39`、`--test devices` 或 `--test boot` 单跑。 | 特定 ISA、trap、分页、设备或装载行为正确；小探针不得依赖工具链下载。 |
| L5c/L6 真实镜像 | `make riscv-linux-native` 使用同一清单，先跑真实固件 + S 态探针，再跑 `hesper linux` 的完整脚本；小探针二进制不冒充 Linux `Image`。 | 本机真实固件与真实内核分别通过。 |
| L7/L8 浏览器 | 已有 `make wasm-browser-test` 用于扩展后的轻量绑定检查；Web 目录运行 `bun run check`、`bun run build`。另建 `make riscv-linux-browser`，在静态生产产物中启动真实 Worker/镜像，执行相同串口脚本。 | JS 绑定正确与网页能冷启动分别成立。大镜像门禁显式执行，资源不可用即非零退出。 |

三个真实镜像入口（reference/native/browser）都从清单读取同一组 SHA-256；允许比较各平台观测到的状态字段，不允许为特定宿主另编一个“易启动内核”。轻量门禁不得引入镜像网络下载；重门禁也不能因缺少镜像而返回成功。发布产物或部署 Pages 与通过这些门禁无关。

**退出**：固定镜像在 QEMU、原生 Hesper、浏览器 Hesper 三处均留有独立交互证据。浏览器无法通过时保留 L6 的原生结论，不把构建成功或 Node 的通过写成网页已跑通。部署、Release 与新增设备是后续独立决策。

## 3. 执行纪律与依据

- 每个可独立观察的增量一个提交。L0 的聚焦命令是 `cargo test --locked -p hesper-riscv-linux`；`make verify` 保持现有 workspace 门禁。自 L2 起把首个分歧缩为可离线重放的探针；既不修改 6502 指令/总线契约，也不拿现有 Apple I 测试当作 RISC-V 的验收。
- 性能先测后改：记录解释器每秒退休指令数、到 shell 的退休数/虚拟时间和宿主耗时，区分镜像下载、复制和解释执行。只有 profile 证实页表遍历等为瓶颈才考虑 TLB 等优化；必须保留相同的异常、失效语义和跨宿主结果。
- 随参考镜像提供 Linux/BusyBox 对应源码、config、构建脚本及适用许可材料，核对 OpenSBI、工具链运行库和其他客体文件的分发要求。缺少权利或来源记录时不进 CDN/Pages，不把用作参照的 QEMU 计为运行时依赖。
- 原始依据：[RISC-V 非特权/特权 ISA 手册](https://docs.riscv.org/reference/isa/)、[SBI 规范](https://docs.riscv.org/reference/sbi/)、[Linux RISC-V 启动 ABI](https://docs.kernel.org/arch/riscv/boot.html)、[OpenSBI FW_JUMP](https://github.com/riscv-software-src/opensbi/blob/v1.6/docs/firmware/fw_jump.md) 与 [generic 默认偏移](https://github.com/riscv-software-src/opensbi/blob/v1.6/platform/generic/objects.mk)、[QEMU virt 设备说明](https://www.qemu.org/docs/master/system/riscv/virt.html) 与 [v9.2.0 地址表](https://github.com/qemu/qemu/blob/v9.2.0/hw/riscv/virt.c)。实施阶段以固定修订和可重放记录为准；上述链接不是 Hesper 的兼容性证明。
- 寄存器布局对照：[QEMU v9.2.0 `virt.h`（PLIC context 与 UART IRQ）](https://github.com/qemu/qemu/blob/v9.2.0/include/hw/riscv/virt.h)、[Linux v6.12 `serial_reg.h`（8250 寄存器）](https://github.com/torvalds/linux/blob/v6.12/include/uapi/linux/serial_reg.h)。以架构规范作语义依据，参考实现的具体行为只用来解释选定板型。
