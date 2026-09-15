# Apple I 测试资源：Woz Monitor ROM

CLI 在编译时通过 `include_bytes!` 内置
[`crates/cli/assets/wozmon.bin`](../../../cli/assets/wozmon.bin)，TUI 和文本宿主默认使用它。
机器库仍由宿主传入 ROM 字节；机器集成测试复用这一份文件。
构建、启动及测试均不下载 ROM，不需要 Bun 或 `HESPER_APPLE1_ROM`。

## 来源与完整性

Woz Monitor 由 Steve Wozniak 编写，最初随 1976 年 *Apple-1 Operation Manual*
发布。镜像来自仓库已有的本地缓存，2026-09-15 按项目要求纳入内置资源；字节未修改。
原下载工具记录的 HEX 来源：
<https://github.com/alangarf/apple-one/blob/master/roms/wozmon.hex>。

文件大小：256 字节。SHA-256：

```text
e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25
```

这是第三方固件，保留其原有权利归属，不将项目自身的许可证声明套用于该镜像。
上述来源及哈希用于追溯文件身份，不是另行授予使用或再分发许可。

## 使用

```sh
cargo run --locked -p hesper -- apple1
```

可选的 `--rom /path/to/wozmon.bin` 仍要求文件长度及 SHA-256 匹配；显式指定
错误文件时直接失败。TUI 的 ROM 字段留空使用内置镜像，保存后会清除旧的外部路径。

## 向量与入口（来自该镜像，供核对用）

| 向量 | 地址内容 | 说明 |
| --- | --- | --- |
| RESET (`$FFFC/D`) | `$FF00` | Woz Monitor 入口；宿主 `Apple1::new` 加载的 256 字节镜像占满 `$FF00–$FFFF` |
| NMI (`$FFFA/B`) | `$0F00` | 未使用；Apple I 基础配置不产生 NMI，这只是镜像中该地址的原始字节，不是有效处理程序 |
| IRQ (`$FFFE/F`) | `$0000` | 未使用；基础配置没有向 IRQ 拉低的中断源 |

## 测试

真实 ROM 交互测试已取消 `#[ignore]`，随 `cargo test --workspace` 离线运行。
机器测试校验内置资源的 SHA-256；CLI 测试覆盖默认启动、命令、程序、周期预算、
trace 和可选外部镜像的校验，并比较内置与外部镜像的输出。

```sh
cargo test --locked -p hesper-apple1 --test wozmon
cargo test --locked -p hesper --test apple1
```

原创总线与设备验证继续使用各自的合成 ROM。
