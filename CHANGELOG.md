# Changelog

记录 Hesper 各版本的用户可见变更。所有 Rust crate 统一版本；未发布的变更记在 `Unreleased`，发布时归入带日期的版本小节。

## [Unreleased]

### 新增

- **发布 CI**：推送版本 tag 后自动校验 workspace 版本与 changelog，运行常规／全量 CPU 验证，构建 Linux、macOS、Windows 的 x86_64／ARM64 CLI，并将 CPU／Apple I 的 web／nodejs Wasm 包、SHA-256 校验文件和对应 changelog 发布到 GitHub Release；支持只构建不发布的手动试跑。

### 修复

- **Windows CLI**：仅在 Unix 平台注册 `SIGHUP/SIGQUIT`，修复 Windows 缺少这些信号常量导致的编译失败。

## [0.2.0] - 2026-09-15

首次带 tag 的仓库发布，汇总此前 `0.1.0` 开发阶段的现有能力；`0.1.0` 未创建正式 Release。

### 新增

- **CPU**：机器无关的 NMOS 6502 核心，支持 151 个官方 opcode、二进制与十进制算术、逐周期／半周期总线观察、IRQ/NMI/BRK、RDY/SO，以及独立的宿主复位和物理 RESET 接口。
- **Apple I**：主板时钟、刷新停钟、PIA 键盘与显示握手、两组共 8 KiB RAM、40×24 字符显示和可选 `$1000–$1FFF` 扩展 RAM；数字视频链包含字符重放、像素移位与同步信号采样。
- **CLI / TUI**：中文启动中心、配置编辑、文件浏览、键盘和鼠标菜单、程序加载地址、暂停／恢复、RESET／CLEAR SCREEN／重建机器，以及有界执行预算和指令／总线 trace；重定向时支持文本宿主。
- **程序资源**：内置 Woz Monitor 和 42 个程序预置，支持 BASIC 与多段镜像加载；提供启动命令、来源、哈希、许可证记录和逐项兼容性矩阵。
- **Wasm / Web**：独立的 CPU 与 Apple I JS/Wasm 绑定、TypeScript 声明，以及通过 Worker 驱动的 React 工作台，提供字符屏、程序装载、CPU 单步、寄存器、内存和总线观察。
- **验证工具**：固定 SingleStep、Klaus、十进制和 Visual6502 revD 数据及重放入口，CPU 引脚／物理 RESET 对照，Woz Monitor 集成测试，以及 Bun、Node.js 和真实浏览器绑定检查。

### 修复

- **Apple I**：修正 PIA 寄存器选择与读回、键盘消费、PA7 固定高电平、RESET 与清屏分离，以及条件垂直重载和滚动帧长。
- **CLI / TUI**：修正 Enter 的 CR/LF 处理、程序加载地址与高位 RAM、配置粘贴和文件列表滚动、窄窗口布局、终端能力适配及会话预算／诊断边界。
- **Wasm 构建**：生成包的 README 从 workspace 读取版本号，避免继续输出硬编码的 `0.1.0`。

### 版本管理

- 6 个 Rust crate 从共同的开发版本 `0.1.0` 升至 `0.2.0`；新增本 changelog 与发布维护约定。
- 本次发布为 GitHub 源码发布；crate 仍设置 `publish = false`。

### 已知边界

- CPU 范围限于官方 NMOS 指令和固定 Visual6502 revD 观察；不包含非官方 opcode、65C02、NES 2A03 或所有芯片修订认证。
- Apple I 与浏览器前端尚未完成整体验收；42 个预置并非全部验证兼容。原板字模逐点认证、电气特性、真实上电状态、磁带接口、扩展卡和完整机器存档仍不在已验收范围，Apple II 尚未实现。
- 资源来源与许可证情况见[预置资源说明](https://github.com/VOD-Studio/hesper/blob/v0.2.0/crates/cli/assets/README.md)；硬件和验证范围见[路线图](https://github.com/VOD-Studio/hesper/blob/v0.2.0/docs/roadmap.md)与[验证记录](https://github.com/VOD-Studio/hesper/blob/v0.2.0/docs/verification.md)。

[Unreleased]: https://github.com/VOD-Studio/hesper/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/VOD-Studio/hesper/releases/tag/v0.2.0
