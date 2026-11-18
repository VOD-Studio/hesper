# RISC-V Linux 下一步：在固定内核上启动自己的 `/init`

**状态：U1–U3 已在本地验收；CLI 可更换启动根，U4 浏览器门禁尚未全部完成。** [固定板启动](riscv-linux.md)和[数据盘 D1–D5](riscv-linux-data-disk.md)已在本地验收。这一轮只换启动链的最后一件东西：用户提供 `initramfs.cpio`，不用重编 Hesper、OpenSBI 或内核，就能在串口运行自己的程序。

不先加 hart：多核既不能改变镜像锁，也不能让现有 RV64IMAC 核心执行发行版常见的 `lp64d` 程序。这一轮不开放任意内核、DTB、bootargs、压缩归档、分区系统盘、UEFI 或网络；不发布镜像，不部署 Pages。

## 用户契约：只替换根文件系统的初始内容

`--images` 仍选择原有板配置；`--initramfs` 指向另一个本地普通文件（文件名不限），不修改 `--images` 目录。

```sh
hesper linux --images .cache/riscv-linux/images --initramfs ./my-root.cpio
hesper linux --images .cache/riscv-linux/images-blk --initramfs ./my-root.cpio \
  --disk-in seed.raw --disk-out next.raw
```

| 选择 | 启动前必须验证什么 | 客体看到什么 |
| --- | --- | --- |
| 不指定替换包 | 保留现状：按是否带盘选择 `serial` / `blk` 锁，校验原版四文件的大小与 SHA-256。 | 原版 `/init`；原有命令和验收不变。 |
| 显式指定替换包 | 原锁仍是信任根。校验原始 `fw_jump.bin`、`Image`、`board.dtb`；第四份输入取用户选定的 cpio，不要求原版 cpio 存在，不要求自定义包命中白名单。 | 同一个内核从 RAM 执行新包的 `/init`。移除参数再冷启动即恢复原版。 |
| 替换包并带数据盘 | 同上，但三件基础文件必须来自 `blk` 锁，数据盘仍按已有 raw 规则检查。磁盘绝不自动充当 initramfs。 | `/` 仍在 RAM；只有挂载、卸载并显式导出的 `/dev/vda` 能跨会话保留。 |

原生 `manifest.json` 在两种模式都必须与内置锁的 JSON 值一致；不接受用户修改清单里的第四项来“批准”自制镜像。网页继续使用内置锁，不额外索要 manifest。用户选择的归档是**待执行的客体代码**：算出 SHA-256 可用于重放，不等于项目认证。替换模式失败直接报错，不回退至默认包。反过来，默认模式也不能因哈希不对而猜测用户想换包。

**首版格式。** 接受单个未压缩 `newc`（`070701`）归档，允许 `TRAILER!!!` 后有全零填充。不接受 gzip、`070702`、串接多个归档或 raw/ext2。建议将替换包的产品上限定为 **16 MiB（含填充）**：避免在浏览器为用户选择的大文件分配过多内存；这是本轮的取舍，与数据盘上限分别维护。物理窗口其实是 `[0x84000000, 0x87800000)`，共 56 MiB；两种边界不可互相替代。保证能装载不等于保证能运行：受支持的验收样例用小端 RV64IMAC、`lp64` 软浮点静态程序；脚本的解释器、可执行的 `/init` 及所需设备节点由制作者放进归档。宿主不递归检查每个 ELF，也不替用户补 `/bin/sh`。

## U1 · 制作一份能辨别真假的测试根，在参考机启动

1. 从[已有客体构建工具](../../tools/riscv-linux/build_guest.py)复用已锁的 Buildroot 工具链、静态 BusyBox 和可复现的打包条件。测试 overlay 与[默认 `/init`](../../tools/riscv-linux/rootfs/init)分开；不改默认构建输入、两个镜像锁或已准备的 `images*/`。产物和原始日志放到 `.cache/riscv-linux/` 的独立子目录；仓库只存源码、构建步骤和验收脚本。
2. 新 `/init` 挂载 devtmpfs、proc、sysfs，导出仅它会设置的 `HESPER_ROOT_ID=custom-v1`，从 `/dev/console` 进入交互 shell，保留 `hesper# ` 供现有等待器使用。用同一工具链生成静态 `/bin/root-proof`：接收两个整数，读取该环境变量，打印标识和乘积；缺标识不能返回成功。`readelf -h -A -lW` 核对 ELF64/RISC-V、软浮点、ISA 与无 `INTERP`。归档含可执行的两文件和字符设备 `/dev/console`（5:1）。使归档**长度与默认值不同**，才能检出沿用旧设备树的问题。独立生成两回比对哈希。
3. 复用 [QEMU 参考脚本](../../tools/riscv-linux/reference.py)的单 hart、128 MiB、固定 CPU、固定地址 raw loader。用构建工具在临时目录按新包实际长度生成参考 DTB，**不用 Hesper 待测的 DTB 修改逻辑**。QMP 取 OpenSBI 交接后的树，核对 initrd 起止、原 bootargs 和固件 `no-map` 保留区。这里的 manifest 仍记录基础锁；新归档长度、指纹和派生树另记，不伪造一份能通过原锁的目录。

**退出证据：**等待完整提示，输入 `/bin/root-proof 6 7`，要求独立结果行 `ROOT_OK:custom-v1:42` 和下一次提示；再以不同参数得到不同乘积。串口输入不含期望结果。`uname -r` 仍是 `6.12.0-hesper`；对照用默认根运行同一路径不能成功。后续 CLI 与浏览器使用这份 cpio 的**同一 SHA-256**，不各造一份。建议新增 `make riscv-linux-initramfs-reference`；这一关只证明客体与参考板相容。

准备好 `make riscv-linux-data` 和 `make riscv-linux-qemu` 的产物后，运行
`make riscv-linux-initramfs-reference`。可沿用[构建容器](riscv-linux.md)和
`RISCV_LINUX_CACHE`；通过后将归档及报告写入该缓存的 `initramfs/`，
每回编译与串口证据另存于其 `reference-*/`。见[实测记录](../verification.md#risc-v-linux-u1固定-qemu-启动替换根)。

## U2 · 在 Rust 装载路径派生 DTB，不放松默认校验

[装载器](../../crates/riscv-linux/src/boot.rs)在默认路径仍只检查 cpio 魔数；[FDT 检查器](../../crates/riscv-linux/src/fdt.rs)要求 `/chosen/linux,initrd-end` 与实际装载长度**精确相等**。两套锁里的默认 cpio 均为 932864 字节，尾址 `0x840e3c00`。显式替换使用 `Machine::boot_with_initramfs` / `reboot_with_initramfs`，不通过跳过哈希或放松 FDT 检查来接受另一个长度：

1. 宿主按原始字节认证基础 DTB，再把原锁的 initrd 长度及“显式替换”意图传给共用的 Rust 装载路径。默认路径仍用原树，长度不合就拒绝；不把自动修正变成所有 `boot` / 重启调用的默认行为。
2. 替换路径对有界切片顺序检查 `newc` 结构：110 字节头、十六进制字段、含结尾 NUL 的名称、4 字节对齐、数据区、终止项与尾部填充。偏移和对齐用 checked arithmetic，每条记录严格推进；不按条目大小再分配内存，不解包到宿主文件系统。只检查可装载的容器，不复制 Linux 的文件覆盖、链接和 `exec` 语义。结构损坏应在启动前拒绝；结构合法但缺解释器是**客体启动失败**，保留真实内核诊断。
3. 以 `INITRAMFS_ADDRESS + 实际字节数` 计算尾后地址。沿用完整的区域校验：内核有效范围、initrd、DTB 与 OpenSBI 的 `FDT_GROWTH` 不重叠且在 DRAM 内。在已校验的 FDT 结构中定位唯一 `/chosen/linux,initrd-end`，先对照锁内原值，保留 start、bootargs 和 `reserved-memory` 检查。仅在供本次启动使用的树中写该属性的 **8 个大端字节**；不搜索裸数值，不重新序列化整树，不改输入文件。有效树须通过现有 FDT 语义检查。原生和 Wasm 不各写一套 DTB/CPIO 解析。
4. 全部验证成功后才提交装载。复用现有 DTB 缓冲或改动装入 RAM 的那一份；不要为改 8 字节复制内核、归档或 128 MiB DRAM。失败不能留下半次重启。

**聚焦验证：**扩展[现有启动测试](../../crates/riscv-linux/tests/boot.rs)。用比默认短、长、等长但内容不同的合法归档，检查实际装入的末字节和客体树尾址；树在该属性外逐字节不变，源 DTB 未改。覆盖空包、压缩包、截断头/数据、畸形名称、缺 trailer、产品上限与上限加一；保留原有“initrd 跨入 DTB”的独立物理边界用例。重复 chosen 属性、错误原尾址不能被修成合法树。拒绝重启后原 PC 与 RAM 哨兵不变。先跑 `cargo test --locked -p hesper-riscv-linux --test boot`，再跑 `make verify`。不需要下载镜像。

## U3 · 用原生 CLI 交付第一个可用版本

在[原生宿主](../../crates/cli/src/linux.rs)增加 `--initramfs` 和帮助文字。保持按有无盘选锁；不需要新的板型、第三份锁或 `--no-verify`。文件打开后检查是普通文件，复用有界读取，再按实际读到的字节数校验；拒绝缺值、重复参数、空包、超限。不按文件扩展名决定模式。诊断区分基础文件的哈希错误、自定义包格式/大小错误、装载冲突、执行中无提示或耗尽预算。自定义 `/init` 可以不启动 shell，产品本身不能等待特定提示符才允许运行。

**原生验收（`make riscv-linux-initramfs-native`）：**使用 U1 的缓存以及按锁准备的 `images-blk`。若块设备镜像存于另一缓存，可通过 `RISCV_LINUX_BLK_IMAGES` 指定该 `images-blk` 目录。见[实测记录](../verification.md#risc-v-linux-u3原生-cli-启动替换根)。

- 用 U1 的包通过管道和真实 PTY 冷启动，复验 `root-proof` 和内核版本。至少一次从**没有默认 cpio**、仍有原版 manifest 和另外三文件的目录启动。退出后不带参数从完整默认目录冷启，证明没有自定义程序残留。
- `blk` 也走一次组合验收：新根内运行 `root-proof`，挂载 `/dev/vda` 写脚本，成功 `sync`、卸载、Ctrl+] 导出；第二个进程用默认 `blk` 根挂载输出盘并执行脚本。由此区分“换启动根”和“保存数据”；输入种子哈希不变。
- 分别损坏基础 DTB 一字节、混用 `serial` 内核和 `blk` 清单、给坏归档。这三种情况都应在执行固件前失败。另做一份结构合法但没有 `/init`、备用 init 或 shell 的最小包：预期固件和内核已执行，串口留下内核启动失败的诊断，不能误判为宿主哈希错误。驱动在预算内结束，不请求保存，目标盘不存在。原有 `make riscv-linux-native` 与 `make riscv-linux-blk-native` 仍按原锁通过。

**到此可声明：CLI 在固定内核上能更换用户空间。** 在 U4 通过之前，不把这一结论写成浏览器支持。

## U4 · 浏览器选择同一份 cpio，运行同一个程序

[Linux 工作区](../../examples/web/src/LinuxWorkspace.tsx)将「基础启动文件」「自定义 initramfs（可选）」「可写数据盘（可选）」分开。默认仍需要四个原版文件；显式替换时只需要固件、内核、DTB。允许继续选择原来的四文件集合，但标明默认 cpio **不参与此次启动**，不要再读入它。用户清除替换选择后恢复四文件要求；无效选择要显示错误并禁止启动，不能静默退回默认。运行中不热换根。

沿[消息协议](../../examples/web/src/linux-messages.ts)给启动消息加可选 `File`，沿用 `sessionId`。由 [Worker](../../examples/web/src/linux.worker.ts)先判 `File.size`、再读本地字节并校验基础文件；在异步读、哈希和加载后处理取消，不让旧会话覆盖新会话。归档不上传，不放入 React 大缓冲。[Wasm 绑定](../../crates/riscv-linux-wasm/src/lib.rs)在分配或复制前核对长度，调用 U2 的同一路径，同时保留直接调用默认绑定时的严格行为。

**网页验收（拟新增 `make riscv-linux-initramfs-browser`）：**生产构建、真实 Chromium 文件选择与键盘输入，以 U1 的包跑同一串口命令；`blk` 组合至少验证新程序和挂载读回。取消加载 A 并启动默认 B，B 不出现 A 的串口结果；清除替换后缺默认包、选择超限文件、基础文件损坏均不能启动。检查客体文件 HTTP 传输为零。另跑 `make wasm-browser-test`、`examples/web` 下 `bun run check` 和 `bun run build`，以及原有 `make riscv-linux-browser`、`make riscv-linux-blk-browser`。Node 结果或网页显示「已加载」都不能替代实际客体输出。

## 证据、停线与后续决策

- 沿用[串口验收驱动](../../tools/riscv-linux/serial_acceptance.py)：按完整提示推进、独立结果行判定，不按命令回显判定。建议维持现有量级：参考机首提示 20 秒，原生 90 秒，浏览器 180 秒；每条命令 20 秒 / 256 KiB，Hesper 总 fuel 先沿用 450000000。超限记录最后状态和串口尾部，不调大预算冒充通过。
- 每卡在[验证记录](../verification.md)追加命令、工具版本、profile、基础锁和替换包 SHA-256、包长度、预期/实测 initrd 尾址、实际结果与退出原因；重型验收保留原始输入/串口、设备树和耗时。工具、参考镜像或浏览器缺失即失败，不 skip。只将实际通过的环境列为支持；U3/U4 交付时同步更新对应使用说明和 `CHANGELOG.md`。
- **U4 后再选方向，不并行加三类兼容性。** 若要换内核，选定第二个 `Image`，先验证其 ISA、板级驱动和 QEMU 交接；若要持久化整个系统，单开从 `/dev/vda` 挂根的方案；若要跑某个发行版，先点名发行版、版本和镜像，核对用户态 ABI、启动方式与 RAM 需求。任何一项都不是本轮验收的隐含承诺。
