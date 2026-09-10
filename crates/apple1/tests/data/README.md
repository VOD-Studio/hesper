# Apple I 外部资源：Woz Monitor ROM

Woz Monitor 是 Apple 自己的固件（Steve Wozniak 编写，最初发行于 1976 年
*Apple-1 Operation Manual*），不是本项目的代码。依照 [`AGENTS.md`](../../../../AGENTS.md)
与 [`docs/roadmap.md`](../../../../docs/roadmap.md) M3.1 的既定规则——「不提交
Apple ROM、商业软件或来源不明镜像」「公开可下载不等于有权再分发」——本仓库
**不下载、不内嵌、不提交**这份 256 字节镜像。任何需要真实 Woz Monitor 交互
的测试都从调用者显式提供的路径读取它。

## 获取 ROM

自行从你有权使用的来源获取一份 256 字节镜像（例如你拥有的 Apple-1 硬件、
复刻板固件，或公开转录并核对过的十六进制清单）。已核对的公开转录之一：
<https://github.com/alangarf/apple-one/blob/master/roms/wozmon.hex>（逐字节
核对自 *Apple-1 Operation Manual* 扫描件）。获取渠道公开并不代表你已有再
分发权，本项目也不因引用该链接而为你的副本背书；请自行确认使用权限。

## 校验

镜像必须恰好 256 字节；下面记录的 SHA-256 只是一个完整性指纹，用于确认你
拿到的文件和本项目引用、验证过的版本一致，**不代表本项目持有或分发该文件
的内容**：

```
e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25
```

```sh
python3 tools/verify_wozmon_hash.py /path/to/wozmon.bin
# 等价于：
make wozmon-verify ROM=/path/to/wozmon.bin
```

哈希不匹配或大小不对时脚本以退出码 1 失败，不静默通过。

## 向量与入口（来自该镜像，供核对用）

| 向量 | 地址内容 | 说明 |
| --- | --- | --- |
| RESET (`$FFFC/D`) | `$FF00` | Woz Monitor 入口；宿主 `Apple1::new` 加载的 256 字节镜像占满 `$FF00–$FFFF` |
| NMI (`$FFFA/B`) | `$0F00` | 未使用；Apple I 基础配置不产生 NMI，这只是镜像中该地址的原始字节，不是有效处理程序 |
| IRQ (`$FFFE/F`) | `$0000` | 未使用；基础配置没有向 IRQ 拉低的中断源 |

## 运行需要该资源的测试

普通 `cargo test --workspace` **不需要**这份 ROM：`crates/apple1/tests/wozmon.rs`
与 `crates/cli/tests/apple1.rs` 中依赖真实 Woz Monitor 交互的用例标记为
`#[ignore]`，默认离线跳过。显式运行时缺少资源或哈希不符会直接 panic 失败，
不是静默跳过：

```sh
export HESPER_APPLE1_ROM=/path/to/wozmon.bin
cargo test -p hesper-apple1 --test wozmon -- --ignored
cargo test -p hesper --test apple1 -- --ignored
# 等价于：
make wozmon-tests ROM=/path/to/wozmon.bin
```

加载与哈希校验逻辑见 [`crates/apple1/tests/support/wozmon_rom.rs`](../support/wozmon_rom.rs)
与 [`crates/cli/tests/support/wozmon_rom.rs`](../../../cli/tests/support/wozmon_rom.rs)（两个
crate 的测试各自编译，helper 各自保留一份小副本，不共享跨 crate 模块）。

不依赖真实 ROM 的原创总线／设备验证（地址译码、PIA 寄存器、显示/键盘握手、
批次一致性）留在 `crates/apple1/tests/machine.rs` 与 `crates/apple1/src/*.rs`
的单元测试中，使用仅设置了 RESET 向量的合成 256 字节镜像，随普通
`cargo test --workspace` 离线运行。
