# Hesper Web Lab

Apple-1 与 NMOS 6502 的浏览器工作台。React 19.3、shadcn/ui（Base UI）、Tailwind CSS 4 和 Vite 8；实际执行来自仓库的两个独立 Wasm 包。

## 启动

需要仓库的 stable Rust、Bun，以及 `wasm32-unknown-unknown` 和匹配版本的 wasm-bindgen。首次配置 Wasm 工具时，在仓库根目录运行 `make wasm-setup`。

```sh
cd examples/web
bun install --frozen-lockfile
bun run dev
```

`dev` 和 `build` 自动构建 Wasm，并导出内置资源。Vite 默认只监听 `127.0.0.1`，按终端显示的本地地址访问。

```sh
bun run build       # 生成独立静态站点 dist/
bun run preview     # 预览生产构建
bun run check       # 类型检查、lint、实际 Wasm 会话回归
```

新检出后，单独运行 `check` 前先运行 `bun run prepare:assets`。生成文件位于忽略的 `src/generated/` 和 `public/machine/`；不要手工编辑或提交。生产产物支持子路径托管，JS/Worker/Wasm 和 machine 资源需一起提供，通过 HTTP 访问。

## 使用

### Apple-1

- 内置 Woz Monitor，默认选中 BASIC (Huston)。点击“启动 Apple-1”，然后发送 `E000R` 进入 BASIC。
- “启动配置”可选择自己的 256-byte ROM、42 个预置之一、本地 `.bin`，或仅启动 Monitor。加载地址支持十进制、`$E000`、`0xE000`。
- 预置保留完整的多个 RAM 块、依赖、启动指令、来源和许可证说明；Little Tower 需要扩展 RAM。BASIC 程序使用各自显示的热启动指令，不能统一用 `E000R`。
- 点击屏幕连接键盘；Enter 发送 CR，Backspace 发送 `_`，Esc 发送 `0x1B`。Tab 可离开屏幕。移动设备可使用“粘贴 / 输入文本”。
- 粘贴统一换行、过滤非 ASCII 字符并提示；队列最多 4096 个字符，超限整次拒绝。
- RESET 为物理主板复位，保留 RAM、屏幕和未读键盘输入。重新上电重新装载原始 ROM/程序；替换或结束会话需在界面确认。
- 弹窗、后台标签页和切换到 CPU 工作区会暂停 Apple-1 自由运行，回到页面恢复之前的运行意图；手动暂停不会自动取消。
- `Alt + Shift + P` 暂停/继续。其余操作使用按钮，避免覆盖浏览器快捷键。
- 显示偏好保存在当前浏览器；自选文件仅保留在当前会话，刷新后需重新选择。

### CPU 6502

使用 `Cpu6502Ram` 加载与 CLI 完全相同的计数演示。支持一键运行、重新运行、指令单步、周期单步、RESET、寄存器、状态位、内存窗口和最近 256 个周期的记录。

完成结果：`$0200–$0209 = 00–09`，`PC=$800F`，54 条指令，147 个指令周期加 7 个 RESET 周期，共 154 cycles。

## 数据流与边界

- React → Worker 消息 → `runtime.ts` → Apple1 / Cpu6502Ram。两个工作区使用独立实例。
- CPU core、Apple-1 时序及 Wasm public API 均沿用现有代码；本例没有 JS 总线替代或设备直接读取。
- `crates/cli/examples/web_assets.rs` 使用现有公开的 `presets` 和 demo 常量，通过 TOML 输出给 Bun 构建脚本。不解析 Rust 源码、不维护第二份程序目录，不给 CPU 引入依赖。
- Worker 分批执行，按有限时间片向主线程让出控制。批次预算不是实时性能承诺；14.31818 MHz 为模拟主时钟基准，实际推进速度取决于设备性能。
- 屏幕使用 Wasm `screen()` 的 40×24 字符投影，非可打印值显示为空白。CSS 辉光和光标闪烁是宿主显示效果，不是完整视频信号模拟。
- 配置替换先创建并校验新机器，成功才释放旧机器；失败保留旧机器。观察快照不能用于恢复存档。
- 程序来源和现有兼容性说明来自 `crates/cli/assets/README.md`；ROM 来源来自 `crates/apple1/tests/data/README.md`。它们复制进静态产物，页面“帮助”提供链接。Wasm 字模署名也保留在产物中。

## 验证

`src/runtime.test.ts` 直接运行实际生成的 web Wasm 包及与 Worker 共用的会话控制器，覆盖：

- CLI 同源计数程序、周期/指令单步、总线写入、RESET 保留 RAM。
- 真实 Woz Monitor / Huston BASIC 执行、`PRINT 123+456`、暂停/后台/工作区隔离。
- CLEAR 与 RESET、重新上电的不同效果。
- 装载失败保留原会话、输入超限原子拒绝。
- 全部 42 个预置的块大小和加载、扩展 RAM 要求、Blackjack 多段 BASIC 的 LIST。
- 地址校验、粘贴归一化和固定 40×24 字符投影。

根目录 `make wasm-browser-test` 另行验证 Bun、Node.js 和真实 Chrome 中的 8 项绑定检查；`make verify` 检查本机 Rust 工作区。这些不等于 42 个程序的完整玩法或硬件 conformance 验收。

### 2026-09-15 本轮 QA

- `bun run check`：类型检查、lint、5 项实际 Wasm 会话回归通过。
- `bun run build`：生产构建通过，产物包含 Worker、两个 Wasm 和所有程序资源。
- 生产 Worker 冒烟：在 Node worker 中加载实际构建产物及两个 Wasm，确认 CPU 154 cycles 和 Woz Monitor `0000: 00`；该检查不替代浏览器 UI QA。
- `make wasm-browser-test`：Bun / Node.js / Chrome 各 8 项绑定检查通过。
- `make verify`：Rust debug/release 工作区测试、格式、check 和 Clippy 全部通过。
- 浏览器 UI 自动化连接报错 `Unable to load browser request-header policy`，内置浏览器不可用；新页面的鼠标操作、截图及桌面/窄屏视觉验收尚未完成。
