# Hesper

<p align="center">
  <img src="assets/hesper-logo.png" alt="Hesper logo" width="360">
</p>

Hesper 是一个使用 Rust 编写的经典计算机模拟器项目，包含机器无关、逐周期执行的 NMOS 6502 核心、Apple I 文本机器模型，以及中文 TUI 和可脚本化的 CLI。

CPU 的 M2 本地目标范围已验收；Apple I 已能运行 Woz Monitor、BASIC 和部分预置程序，**M3 整体验收仍未完成**。主板刷新停钟、CB2 显示握手、两端口读回路径，以及含条件垂直重载和滚动帧长变化的循环存储时序已实现；[数字视频链](docs/apple1/video.md)现可输出逐点亮度和同步。剩余边界包括原板字模逐点认证、电气特性和真实上电状态。具体范围见 [`docs/roadmap.md`](docs/roadmap.md) 和 [`硬件依据与差异`](docs/apple1/hardware-evidence.md)。CPU 与 Apple I 已提供 [WASM / JS 绑定](docs/wasm.md)，并提供 [React Web 工作台](examples/web/README.md)。Apple II 尚未开始，浏览器前端整体验收仍未完成。

无参数在 stdin/stdout 都是终端时进入中文 TUI 启动中心；重定向任一流时仍运行可脚本化的内置演示。Apple I 的 TUI 按键约定见 [`docs/apple1/examples.md`](docs/apple1/examples.md)：Ctrl-R 是物理 RESET，Ctrl-L 是键盘上的 CLEAR SCREEN 按钮，Ctrl-P 暂停/继续，Ctrl-N 重建机器，Ctrl-C/Ctrl-D 退出。

## 特性

### CPU 核心

- 151 个官方 NMOS 6502 opcode
- 单周期与半周期驱动接口
- 可观察的地址、数据、读写方向与 SYNC 总线事件
- IRQ、NMI、BRK、RDY、SO 和物理 RESET 时序
- 二进制及 NMOS 十进制 ADC/SBC
- 外部 `Bus` 接口；CPU 不持有内存或设备
- CPU 核心无第三方运行时依赖；整个 workspace 禁止 `unsafe`
- 有限步数、周期预算和有界诊断 trace

兼容范围不包括非官方 opcode、65C02 或 NES 2A03。Visual6502 对照固定于 revD；通过这些场景不等同于所有 NMOS 修订的硬件认证。

### Apple I 文本系统

- 固定 8 KiB RAM：`$0000–$0FFF` 与 `$E000–$EFFF` 两组独立可写内存；Woz Monitor ROM 位于 `$FF00–$FFFF`
- PIA 键盘／显示寄存器及地址镜像；`$D0F2` 与 `$D012` 共用 Port B 和读取副作用
- 14.31818 MHz 主时钟推进；每 65 个字符时钟有 4 个刷新槽抑制 CPU Φ2，不用 RDY 模拟刷新
- CB2→DA→PB7、RDA→CB1 显示握手；1024 槽循环存储投影为 40×24 字符屏，支持换行、清行和滚动，字符在光标槽被扫到时接收，无固定字符延时参数
- 分离物理 RESET、CLEAR SCREEN 与重新上电；暂停／恢复、输入排队、会话周期预算及有界指令／总线 trace
- TUI 程序选择、文件浏览、可编辑加载地址、下拉菜单与鼠标操作；管道和重定向使用文本宿主

机器层包含 2519 行重放、固定 P-Lab 2513 替换字模、74166 像素移位、视频光标及数字复合同步；TUI 仍显示宿主字符格。运行 `cargo run -p hesper-apple1 --release --example video -- /tmp/hesper-video-capture` 可从真实采样导出 PGM 图像、CSV 与 SVG 波形（目录须不存在）。原板字模逐点认证、模拟电压、DRAM 电荷保持、上电状态和单稳态容差不在已验收范围内；详见[视频说明](docs/apple1/video.md)。磁带接口、扩展卡和完整机器存档尚未实现。

## 快速开始

### WASM / JavaScript

```sh
make wasm-setup         # 安装 wasm32 目标及固定版本绑定工具（需要 Bun）
make wasm-build         # 生成 target/wasm-packages/ 下的 web / nodejs 包
make wasm-browser-test  # 原生对照、Bun / Node.js 和真实 Chrome 验证
```

两个独立包分别导出 `Cpu6502Ram` 和 `Apple1`，包含 `.wasm`、JS 与 TypeScript 类型声明。构建、接口和调用示例见 [WASM 文档](docs/wasm.md)。

### Web 工作台

完成上述 Wasm 工具安装后：

```sh
cd examples/web
bun install --frozen-lockfile
bun run dev
```

React 19.3 + shadcn/ui，提供 Apple-1 40×24 屏幕、内置 Woz Monitor、42 个预置及本地程序装载，以及独立 CPU 6502 单步、寄存器、内存和总线记录。通过 Worker 运行真实 Wasm；使用说明与验证边界见 [examples/web](examples/web/README.md)。

### 本机 CLI / TUI

需要当前稳定版 Rust。仓库中的 `rust-toolchain.toml` 会安装最小工具链及 `rustfmt`、`clippy` 组件。

```sh
cargo run -p hesper                 # 终端中打开 TUI 启动中心
cargo run -p hesper -- demo         # 显式运行内置演示（适合脚本）
```

内置程序从 RESET 向量启动，将数字 `0..9` 写入 `$0200..$0209`，然后由宿主在完成地址停止。

查看指令或总线 trace：

```sh
cargo run -p hesper -- demo --trace
cargo run -p hesper -- demo --bus-trace --trace-limit 4
cargo run -p hesper -- demo --help
```

Apple I Woz Monitor 交互（ROM 已内置，无需下载；`--rom` 可选，用于提供 256 字节、哈希匹配的本地镜像）：

```sh
cargo run -p hesper -- apple1
```

也可以先运行 `cargo run -p hesper`，在启动中心选择 Apple-1 即可启动。ROM 字段留空使用内置镜像；已保存外部路径的用户可清空该字段并保存，切回内置镜像。配置页用方向键或 Tab / Shift+Tab 选择字段，Enter 进入编辑，再按 Enter 确认，Esc 放弃本次编辑；路径可按 F4 浏览。编辑时支持左右键、Home / End、Ctrl+U 清空和单行粘贴。完成后选择“校验并保存”或“启动”。TUI 配置只保存 ROM 路径和显示偏好，不保存机器内存或会话。

配置页按 **F3** 打开程序列表，共 **42 个内置程序**，按 [The Apple-1 Software Library](https://apple1software.com/) 的四个分类（Games 游戏 / Fun 娱乐 / Programming 编程 / Utilities 工具）分组；列表上用 ↑↓ 移动、←→ 切换分类，下方详情栏给出该程序的载入范围、**启动命令**（如 `0300R`）、来源页与许可证，Enter 选中后配置页会显示同样的启动命令。也可选择本地二进制文件或不加载程序；预置选择仅在本次进程中保留。

例如选中 **BASIC (Huston)** 会自动加载到 `$E000`，启动后输入 `E000R` 进入 BASIC，再输入 `PRINT 1+2` 可得到 `3`。八个 BASIC 语言程序（如 Hamurabi、Dobble）会连同 BASIC 一起载入，启动命令是站点给出的 `E2B3R`（BASIC 热入口，保留刚载入的程序），再输入 `RUN` 运行。

```sh
cargo run -p hesper -- apple1 --list-presets
cargo run -p hesper -- apple1 --preset basic-huston
cargo run -p hesper -- apple1 --preset hamurabi
```

预置程序随可执行文件内置，无需保留原始下载文件；Woz Monitor ROM 也随可执行文件内置。**收录 42 个预置不代表全部兼容**：[逐项兼容性矩阵](crates/cli/assets/README.md#compatibility-matrix) 区分加载条件、局部运行证据和未验收功能。`little-tower` 需开启“扩展 RAM”（CLI：`--expansion-ram`），增加 `$1000–$1FFF` 的 4 KiB RAM；默认关闭时仍拒绝加载。`memory-test-1000-1fff` 在未开启时预期报告诊断错误。其余预置通过加载范围校验，不据此宣称功能兼容。

Little Tower 启动示例：

```sh
cargo run --locked -p hesper -- apple1 --preset little-tower --expansion-ram
# 进入 Monitor 后输入 0300R，再按 1 开始游戏
```

TUI 启动配置中可用鼠标或 Tab 选中“扩展 RAM”，按 Enter／空格切换；“校验并保存”或“启动”会保存到配置文件 `[apple1]` 下的 `expansion_ram = true`。`--no-expansion-ram` 可覆盖 TUI 保存的设置；脚本模式只使用命令行参数，默认关闭。

镜像来源、逐文件 SHA-256、启动命令与许可证说明见 [`预置资源说明`](crates/cli/assets/README.md)。其中 8 个来源页面声明了许可证，34 个未声明；公开下载、记录来源和哈希不等于再分发授权已明确。

## Workspace 结构

```text
crates/
├── cpu6502/   # NMOS 6502 CPU、Bus/Ram、周期执行器及一致性测试
├── cli/       # 中文 TUI、文本 CLI、演示宿主、程序预置与资源加载
└── apple1/    # Apple I 主板时钟、地址译码 Bus、PIA、显示与键盘

tools/         # 固定外部数据准备及 Visual6502 重放工具
docs/          # 架构、opcode、路线图、来源和验证记录
```

根目录 Cargo 默认成员是 `crates/cli`。检查整个仓库时必须显式使用 `--workspace`。

## 架构

依赖方向为：

```text
CLI / TUI
├─ 内置演示宿主 → hesper-cpu6502 → Ram
└─ Apple I 会话 → hesper-apple1 → hesper-cpu6502 → Apple1Bus
```

CPU 核心不负责程序加载、设备所有权、停止条件、日志或展示，通过外部 `Bus` 访问内存和设备。演示宿主直接驱动 CPU；Apple I 机器层持有 CPU、Bus 和设备，由 `Apple1::tick` 统一推进板级时钟。CLI／TUI 负责资源加载、输入输出、宿主调度和停止条件。

每个 CPU 执行阶段对应一次真实总线访问。dummy read、RMW 的两次写入、RDY 重读、引脚采样和 RESET 同步都是可观察契约，不能仅按最终寄存器结果简化。Apple I 的刷新停钟不推进 CPU 总线阶段，但视频和板级时钟继续运行。

CPU 公共 API 由 [`crates/cpu6502/src/lib.rs`](crates/cpu6502/src/lib.rs) 导出，Apple I 入口见 [`crates/apple1/src/lib.rs`](crates/apple1/src/lib.rs)。详细行为边界见 [`docs/architecture.md`](docs/architecture.md)。

## 开发与验证

完整的日常本地检查：

```sh
make verify
```

它依次执行格式检查、全目标编译、debug/release 测试、Clippy、CLI smoke run 和 Git 空白检查。也可以单独运行：

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
```

普通 workspace 测试使用仓库内固定夹具，获取 Cargo 依赖后可离线运行。测试包括：

- 本地算术与边界测试
- 672 条固定 SingleStep 样例
- 246 组 IRQ/NMI/RDY/SO revD 总线观察
- 419 组物理 RESET revD 总线观察
- Apple I 内存映射、PIA 握手、主板刷新、屏幕与 RESET 的原创程序回归
- CLI 加载／诊断，以及 TUI 输入、布局、程序选择和会话生命周期回归

`cargo test --workspace` 已包含真实 Woz Monitor 测试，不包含验证脚本自身的回归和全量外部一致性。`tools/` 下 4 个脚本（3 个数据准备、Visual6502 重放驱动）的回归测试用 Bun 运行，冷缓存时会真实下载固定上游数据，因此不并入 `make verify`：

```sh
make tools-test        # 等价于 bun test tools/
```

真实 Woz Monitor 联调使用内置镜像，已纳入普通 workspace 测试，也可单独运行：

```sh
cargo test --locked -p hesper-apple1 --test wozmon
cargo test --locked -p hesper --test apple1
```

本地验证包括监控程序读写／执行、BASIC 算术与行号循环、部分预置启动和真实 PTY 交互。实际执行范围和结果见 [`docs/verification.md`](docs/verification.md) 的对应日期记录；普通测试通过不代表全部预置、全部硬件窗口或跨平台终端都已验收。

## 全量 CPU 一致性验证

全量验证会联网准备固定版本的数据，并运行 151 万条 SingleStep 用例、Klaus 功能/十进制/中断程序、Visual6502 原模型重放和 CPU 引脚对照：

```sh
make data
make full
```

额外需要 Bun、`make` 和本地 C 编译器。下载及构建结果只写入已忽略的 `.cache/cpu6502/`。

Visual6502 的 Bun 重放与 CPU 验证是两项独立证明：

```sh
bun tools/verify_visual6502.ts
cargo test -p hesper-cpu6502 --test pins --release
```

前者证明固定上游模型能重现仓库观察，后者证明 Hesper CPU 与这些观察一致。详细数据来源、哈希、许可证、成功条件和单用例重放命令见 [`crates/cpu6502/tests/data/README.md`](crates/cpu6502/tests/data/README.md)。本地执行结果见 [`docs/verification.md`](docs/verification.md) 的相关记录；CPU 全量验证与 Apple I 联调分别报告。

## 版本与发布

当前版本为 **0.2.0**，变更记录见 [CHANGELOG.md](CHANGELOG.md)，发布见 [GitHub Releases](https://github.com/VOD-Studio/hesper/releases)。

所有 Rust crate 使用统一版本，由根 `Cargo.toml` 的 `workspace.package.version` 管理；一次发布对应一个 `vX.Y.Z` tag 和一份 changelog。通过 GitHub Release 分发源码、CLI 和 Wasm，保留 `publish = false`，不发布到 crates.io；`examples/web` 是私有示例应用，随源码发布。

用户可见变更先记入 changelog 的 `Unreleased`，注明受影响的组件和兼容性变化。发布时更新 workspace 版本、`Cargo.lock` 和此处版本，将条目移入 `## [X.Y.Z] - YYYY-MM-DD` 小节，并更新底部版本链接。运行 `make verify`、`make full`、`make wasm-browser-test`，以及 Web 示例的 `bun run check` / `bun run build`，提交并推送后创建版本 tag：

```sh
# 以未来的 0.2.1 为例；Cargo.toml 和 changelog 必须已更新到对应版本。
git tag -a v0.2.1 -m 'Hesper v0.2.1'
git push origin v0.2.1
```

[`Release` 工作流](.github/workflows/release.yml) 接收 `v*` tag：先检查 tag 与 workspace 版本一致，且 changelog 有且仅有一段非空、带日期的对应版本说明；再运行常规检查、全量 CPU 一致性及平台构建。全部通过后上传完整产物并发布 Release，说明取自对应 changelog 小节，不包含 `Unreleased` 或其他版本。`vX.Y.Z-rc.N` 等预发布 tag 会标记为 prerelease。发布说明提取工具使用 Python 3.11+ 标准库，回归命令是 `python3 -m unittest discover -s tests/release`，已纳入常规 CI。

CLI 产物命名为 `hesper-vX.Y.Z-<Rust target>.<格式>`：

| 系统 | Rust target | 格式 |
| --- | --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | `.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `.zip` |
| Windows ARM64 | `aarch64-pc-windows-msvc` | `.zip` |

Linux 在 Ubuntu 22.04 上构建，使用 glibc，不是 musl 静态包；macOS 和 Windows 包未做发行者签名／公证。各平台运行宿主库测试与 CLI demo，这不代替真实终端的完整交互验收。CLI 压缩包包含可执行文件、changelog、使用文档和内置资源来源说明。

Wasm 产物为 `hesper-{cpu6502,apple1}-vX.Y.Z-wasm32-unknown-unknown-{web,nodejs}.tar.gz`，共 4 个包，包含实际验证过的 `.wasm`、JS、TypeScript 声明与说明文件。所有 10 个压缩包的哈希写入 `SHA256SUMS`，与产物一起上传。下载后可运行 `sha256sum -c SHA256SUMS` 校验。

在 Actions 中手动运行 `Release`（或 `gh workflow run release.yml --ref master`）会验证并构建全部产物，只保存在 Actions artifacts，不创建或修改 Release。发布失败可重跑原 tag 的工作流；未发布的 draft 可补传产物，已经正式发布的 Release 不会被覆盖。不要移动已发布 tag；修复后使用新版本。`v0.2.0` 已将初次源码发布补齐为上述 CLI／Wasm 产物发布。详细验证结果继续记录在 `docs/verification.md`。

## 文档

- [`CHANGELOG.md`](CHANGELOG.md)：按版本维护的用户可见变更

- [`docs/architecture.md`](docs/architecture.md)：架构、状态、总线和引脚时序契约
- [`docs/opcodes.md`](docs/opcodes.md)：官方 opcode 支持与测试矩阵
- [`docs/roadmap.md`](docs/roadmap.md)：已完成范围与后续里程碑
- [`docs/references.md`](docs/references.md)：硬件资料及外部测试来源
- [`docs/verification.md`](docs/verification.md)：按时间记录的本地验证证据
- [`docs/apple1/apple-1-overview.md`](docs/apple1/apple-1-overview.md)：Apple I 硬件、Woz Monitor 与历史背景全面介绍
- [`docs/apple1/examples.md`](docs/apple1/examples.md)：Apple I CLI 与库 API 使用示例
- [`docs/apple1/hardware-evidence.md`](docs/apple1/hardware-evidence.md)：一手资料核对、实现对应关系与已知硬件差异
- [`crates/cli/assets/README.md`](crates/cli/assets/README.md)：内置程序的来源、加载范围、哈希与许可证记录
- [`AGENTS.md`](AGENTS.md)：面向代码助手和贡献者的仓库规则
