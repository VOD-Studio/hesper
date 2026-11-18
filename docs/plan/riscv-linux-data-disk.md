# RISC-V Linux 下一步：能在第二次冷启动读回的数据盘

**状态：待实施。** [L8 验收记录](../verification.md#risc-v-linux-l8生产网页与范围冻结)已经证明固定镜像能在浏览器启动真实 Linux。下一项不是再证明一遍能进 shell，而是让用户在 BusyBox 里写一个脚本，结束会话，在全新的机器上挂载同一份磁盘内容并运行它。第一站是原生 CLI，第二站是浏览器与 CLI 互换磁盘。QEMU 只作独立参照。

**第一张工作卡是 D1，不是写设备模拟。** 先证明锁定版本的 Linux 驱动、设备树、参考机插槽和文件系统确实匹配；否则 Hesper 上「没有 `/dev/vda`」无法定位。下文新增的参数、配置名与 Make 目标均为拟议接口，不是当前可运行的入口。

## 交付边界

| 决策 | 首版契约 |
| --- | --- |
| 系统启动 | 原有 `serial` 配置不改：128 MiB、单 hart RV64IMAC、OpenSBI → Linux → initramfs、`ttyS0`。增加一个明确的 `blk` 配置；根仍在 RAM，只有挂在 `/mnt/data` 的盘上文件能导出。不是从 `/dev/vda` 启动。 |
| 盘格式 | 一块 raw 字节镜像，512 字节扇区，长度为正、整扇区且不超过 16 MiB（首版产品上限，不是 virtio 上限）。客体示例用无分区表的整盘 ext2。核心不解释文件系统，不接受 qcow2、目录映射、自动扩容或热插拔。 |
| 所有权 | 输入文件只读。会话拥有一个可写内存副本；显式导出才生成下一次可用的 raw 盘。`sync` 和 `umount` 把客体缓存写回虚拟盘，**不等于**宿主已经保存文件。非正常中止可以丢失本次修改，不能损坏输入。 |
| 兼容性 | `serial` 继续校验现有四个启动文件；`blk` 单独锁定它的四个启动文件。数据盘是用户数据，不是第五个固件，不受固定 SHA-256 白名单限制。不得为接入磁盘而放开内核或 DTB 校验。 |

不做网络、F/D、多核、任意发行版、机器快照、IndexedDB 自动保存或镜像发布。若目标改为「首次访问网页无需自建镜像」，单独优先处理参考镜像分发：核对来源、许可、对应源码和构建配置后再决定分发位置；这不是数据盘的前置条件。

## D1 · 在参考机上冻结构建输入和接线

- 从现有[构建脚本](../../tools/riscv-linux/build_guest.py)、[内核片段](../../tools/riscv-linux/linux.fragment)、[设备树](../../tools/riscv-linux/board.dts)派生 `blk` 配置。当前片段明确关闭 `CONFIG_BLOCK` 和 `CONFIG_VIRTIO_MENU`；新配置须在**最终 `.config`** 中确认块层、`VIRTIO_MMIO`、`VIRTIO_BLK`、ext2 可挂载的内建驱动，不靠运行时加载模块。验证 BusyBox 有 `mount`、`umount`、`sync` 和 `sh`；不要为了装数据盘改 `root=` 或让 `/init` 自动格式化。
- 保留[原产物锁](../../tools/riscv-linux/images.lock.json)、原缓存和旧入口。新配置复用源码锁、工具链和构建逻辑，但隔离 build stamp、产物目录和日志；同名 `Image`、`board.dtb` 不得覆盖旧配置。新锁仍记录四文件大小、SHA-256、装载区间及配置指纹；不同目录干净重建比对后才签入。不按文件名混搭两个配置。
- 参照固定 QEMU 9.2.0 `virt` 的第一个 virtio-mmio 窗口，目标接线为 `0x1000_1000`、跨度 `0x1000`、PLIC source **1**；UART 仍为 `0x1000_0000`、source **10**。扩展[参考机脚本](../../tools/riscv-linux/reference.py)：给 `virtio-blk-device` 显式绑定对应 transport/bus，要求现代 MMIO（version 2），不能依赖 QEMU 自动选槽。记录实际总线、DTB 的 `reg`/`interrupts` 和客体探测结果；三者不符先修参考机，不绕过驱动探测。
- 用宿主 `mke2fs -t ext2` 在缓存中建立一份 **8 MiB** 整盘测试种子，记录工具版本、生成参数、种子 SHA-256；各宿主从该种子的独立副本开始。它不随四个启动文件一起锁定。独立验证以盘内文件字节为准，不要求不同运行写回相同的 ext2 时间戳。

**退出：**固定 QEMU 用 `blk` 四文件和种子副本冷启动；串口中 `/dev/vda` 容量为 16,384 个 512 B 扇区。执行下文的写入脚本，卸载、结束 QEMU，以新进程重开写过的副本，通过读回脚本。保留配置、串口原始字节、磁盘前后哈希及原种子未变的证据。此时不能声称 Hesper 支持磁盘。

## D2 · 接通第二条中断线，不改变纯串口板

[板级 MMIO](../../crates/riscv-linux/src/board.rs) 目前只分发 CLINT、PLIC、UART；[PLIC](../../crates/riscv-linux/src/interrupt_devices.rs) 的 priority、enable、pending 和 in-service 都按单个 UART 编写。先在现有实现内支持固定的两个 source，不引入设备注册表。

- 两个 source 各自保存优先级、线电平、pending 和 in-service；M/S 两个 context 分别保存 enable 掩码及 threshold。通知按 threshold 门控；claim 在已使能的非零优先级 pending 中选最高，同优先级取较小 ID。complete 后如果对应线仍高，该源重新 pending，不改变另一源的状态。
- 扩展[设备测试](../../crates/riscv-linux/tests/devices.rs)：两线同时置位、不同优先级和同优先级、不同 context 屏蔽、先 claim 一个仍能收到另一个、设备清线后 complete 不重复上报；保留原 UART 及 WFI 唤醒场景。

**退出：**`cargo test --locked -p hesper-riscv-linux --test devices`、`make verify`；带旧锁的 `make riscv-linux-native` 仍通过。这里不创建虚假的块设备来提前暴露 `/dev/vda`。

## D3 · 在 Rust 核心实现一条可验证的块请求路径

在 `crates/riscv-linux` 内加入一个可选的内存数据盘与固定 MMIO 窗口。盘随机器创建传入，原无盘入口不映射新窗口；不在核心打开文件、调用 JS、查看客体路径或识别 Linux 指令位置。按 [Virtio 1.2](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html) 的 MMIO transport、split virtqueue 与 block device 章节实施：

1. **发现与协商。** 实现身份/version/device ID、双页 feature 选择、status/FEATURES_OK、容量、队列 0 的大小/地址/ready、notify 及 IRQ status/ack。仅现代 `VIRTIO_F_VERSION_1`，一条 split queue，最大 128 项。仅公布实现且已测的特性；不宣告 packed ring、间接描述符、event index、写回缓存、多队列、discard 或 write-zeroes。不要假定 Linux 会使用 QEMU 协商到的全部特性。
2. **读写与完成。** 通过客体**物理 DRAM** 读取 avail ring 和描述符链，处理 `IN`、`OUT` 以及驱动查询磁盘标识的 `GET_ID`。支持跨多个数据描述符，按设备读/写方向校验，容量以 512 B 扇区计。盘数据就在会话内存中，完成写请求时字节已进入该介质；不宣告 `FLUSH` 或可配置写缓存。合法但未实现的请求返回 `UNSUPP`，越过容量返回 `IOERR`，不能报成功。先写数据及 status，再公布 used 元素、实际设备写入长度和 used index，按 split ring 通知规则置 IRQ；清中断与 PLIC complete 是两个动作。
3. **预算和失败。** 入队前核对队列大小、对齐和地址范围；每条链在修改盘之前检查全链、总长度、方向及扇区范围，用 checked arithmetic。限制遍历到队列项数，按 16 位 ring index 的回绕差计算待处理数；单批工作有界，不能因一个 notify 无限执行。DMA 不准穿过 DRAM 边界或触发 UART/MMIO 副作用。若连 status/used 都不能安全写，报告明确的设备错误并要求 reset，绝不向猜测的地址补写。设备 reset 丢弃队列和中断，不擦已写扇区。
4. **小探针先于 Linux。** 离线 Rust 测试构造真实 RAM 环：末扇区写后回读、合法分段链、多个请求与 index 回绕、IRQ ack/complete、reset 后重协商；反例覆盖未 ready、链成环、队列地址越界、方向颠倒和跨盘尾写。失败写须证明目标扇区和邻接哨兵未变；不同 fuel 切片重放同一事件得到相同盘内容和中断结果。

**退出：**新增聚焦测试目标 `cargo test --locked -p hesper-riscv-linux --test virtio_blk`，再跑 `make verify`。两者都不需要真实镜像。协议依据另对照 [Linux 6.12 virtio-blk 驱动](https://github.com/torvalds/linux/blob/v6.12/drivers/block/virtio_blk.c)，不把探针通过当作 Linux 驱动已能挂载。

## D4 · CLI 交付第一份持久化证明

在[原生串口宿主](../../crates/cli/src/linux.rs)加入成对参数。建议命令如下（`$BLK_IMAGES` 为 D1 产物目录）：

```sh
hesper linux --images "$BLK_IMAGES" --disk-in seed.raw --disk-out first.raw
hesper linux --images "$BLK_IMAGES" --disk-in first.raw --disk-out second.raw
```

- 不给磁盘参数：保持现有串口配置；给齐参数：选择内置 `blk` 锁，不接受调用方自带的未校验清单。缺参数、配置混用、输入不是普通文件、长度无效、输出已存在（含与输入同路径或别名）在执行客体前报错。不在工作目录猜文件，不就地改盘。
- 每次只读入一份输入并移交机器。用户在客体 `sync`、成功 `umount` 后，用现有 **Ctrl+]** 请求正常结束并保存。正常结束时才将工作副本写入目标目录的临时文件，完成写入及同步后以**不覆盖**方式原子发布；输出在检查后被另一进程创建也必须失败。EOF、信号、fuel 耗尽、客体错误不自动保存；保存失败非零退出并清理临时文件，不留下半份目标。无盘会话沿用原有退出语义。
- `umount` 由用户完成：宿主不能仅凭暂停或 `sync` 就宣称文件系统一致。验收等待每条命令完成的结果行和下一次完整提示符，不能只匹配串口回显。

**所有宿主共用的客体验收。** 第一次启动运行下列命令；用 `&&` 保证写入或卸载失败时不会打印成功标记：

```sh
mkdir -p /mnt/data && mount -t ext2 /dev/vda /mnt/data && \
  printf '%s\n' 'echo "DISK_OK:$((6 * 7))"' > /mnt/data/proof.sh && \
  sync && umount /mnt/data && echo DISK_UNMOUNTED
```

等独立结果行 `DISK_UNMOUNTED` 后保存。第二次从空白 RAM 冷启动，输入**第一次的输出盘**：

```sh
mkdir -p /mnt/data && mount -t ext2 -o ro /dev/vda /mnt/data && \
  sh /mnt/data/proof.sh && umount /mnt/data
```

**退出：**管道和真实 PTY 各完成两次冷启动，第二次串口出现独立的 `DISK_OK:42` 并成功卸载；宿主用只读 ext2 工具核对导出盘脚本的精确内容，并核对种子哈希未变。反例至少含输出重名、单字节损坏的启动文件、无效盘长、预算中止、导出写入失败。旧 `make riscv-linux-native` 仍用原锁，新增显式重门禁 `make riscv-linux-blk-native`，不能以缺盘为由跳过。

## D5 · 同一盘在浏览器与 CLI 间往返

- Wasm 绑定只负责把原始盘字节送入同一核心、按显式请求取回；不得在 JS 重新实现队列或 ext2。[Linux Worker](../../examples/web/src/linux.worker.ts)仍持有机器；在[消息协议](../../examples/web/src/linux-messages.ts)中给启动增加独立可选的盘输入，并给导出请求及回复附 `sessionId`。有盘/无盘使用与 CLI 相同的锁选择规则。先检查 `File.size` 再读内容；不上传、不把盘缓冲放进 React 状态逐帧复制。
- 在[Linux 工作区](../../examples/web/src/LinuxWorkspace.tsx)分开选择四个启动文件和可写数据盘。提示「先在客体卸载，再暂停并导出」；只在有盘且暂停时允许导出，下载单独的 `.raw` 文件，不改本地原件。导出用独立缓冲，不分离 Wasm 线性内存。停止会丢弃工作副本：有尚未导出的写入时先确认；迟到的上一代回复不能触发下一代会话下载。
- 真浏览器验收用 D4 的两段脚本：**CLI 写 → 网页读；网页写 → 下载 → 新 CLI 进程读**。用两份独立种子及不同标记，防止误读前一轮的文件也通过。另测下载后继续写入、暂停中不推进、取消选盘、过大文件、停止后旧导出回复无效，以及无盘 Linux 与 Apple I 启动不受影响。

**退出：**绑定跑 `make wasm-browser-test`；网页跑 `bun run check`、`bun run build` 及无盘 `make riscv-linux-browser`。新增 `make riscv-linux-blk-browser` 对生产构建执行真实文件选择、串口和下载，将下载文件交给原生 CLI 验收。单测、Node 读回或浏览器显示「已导出」均不能替代跨宿主读回。

## 证据与停止条件

- D1 → D2/D3 → D4 → D5；D2 的中断测试可单独提交，D4 通过即得到可用的原生增量，不等到 UI 完成才验收。每个独立可观察的改动按仓库规则提交；CLI/Web 开始对用户可用时再写 `Unreleased` 和使用说明。本计划不把任何新能力标为已交付。
- 镜像和大日志只放 `.cache/riscv-linux/`。各阶段在[验证记录](../verification.md)记命令、配置锁、宿主版本、输入/输出盘哈希、期望与实际串口行、退出原因、fuel/墙钟预算。失败保留首个不同的设备状态或串口行与最小重放命令；缺工具、镜像或超限即失败，不改期望来通过。
- **性能是单独一张测量卡。** 用同一台宿主、同一构建，分别测纯串口和带盘配置：读取/校验文件、创建机器、运行至首个提示符、导出磁盘的墙钟耗时，以及启动退休数和内存峰值。分清新内核驱动的成本和宿主拷贝成本；浏览器内存指标取不到就注明，不用原生 RSS 代替。只有测量能指向具体瓶颈才安排优化，不把 TLB、JIT 或镜像分发塞进数据盘验收。
