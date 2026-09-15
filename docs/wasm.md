# WASM 与 JavaScript 绑定

`hesper-cpu6502-wasm` 与 `hesper-apple1-wasm` 分别生成可独立加载的 WASM、JS 和 TypeScript 声明。CPU 包提供 **CPU + 64 KiB RAM** 的宿主 `Cpu6502Ram`，Apple I 包提供完整固定配置机器 `Apple1`。原有 CPU 仍不持有内存，也没有第三方运行时依赖。

两个包通过内部 `hesper-wasm-support` 共用参数校验、错误和 CPU 观察快照转换。Apple I 包直接链接 Rust CPU，不需要加载 CPU 的 JS 包，也不在每个总线访问时调用 JS。CLI/TUI 不参与 WASM 构建。

## 构建

使用仓库 stable Rust 和 Bun 1.4.0；Node.js 包执行检查另需 Node.js：

```sh
make wasm-setup         # 联网：安装 wasm32 目标和匹配的 wasm-bindgen-cli
make wasm               # 库与绑定的目标编译检查
make wasm-build         # --locked release 编译，生成 web / nodejs 两套包
make wasm-test          # 构建、原生 Rust 参考执行、实际 nodejs 产物的 JS 测试
make wasm-typecheck     # 固定 TypeScript 7.0.2 检查生成声明的调用类型（首次联网）
make wasm-browser-test  # 上述测试和类型检查，再用独立无头 Chrome 验证 web 产物
```

`wasm-bindgen` 版本精确固定于根 Cargo.toml，构建脚本据此检查 CLI 版本。`wasm-setup` 把工具装到忽略目录 `.cache/wasm-tools`；也可通过 `WASM_BINDGEN=/path/to/wasm-bindgen` 指定已安装的同版本工具。浏览器检查默认寻找 macOS 的 Google Chrome 或 Linux 的 `google-chrome`，可用 `CHROME_BIN` 指定路径。浏览器使用临时独立 profile 和仅绑定 loopback 的临时 HTTP 服务，结束后清理。

产物：

```text
target/wasm-packages/
├── cpu6502/{web,nodejs}/hesper_cpu6502{.js,.d.ts,_bg.wasm,_bg.wasm.d.ts}
└── apple1/{web,nodejs}/hesper_apple1{.js,.d.ts,_bg.wasm,_bg.wasm.d.ts}
```

每个目录包含 README；Apple I README 同时保留内嵌 P-Lab 字模的作者、来源、CC BY 4.0 许可证和格式转换说明。**Woz Monitor / BASIC ROM 不内嵌、不下载、不随包分发**，由宿主提供。

输出由 `wasm-bindgen` 生成，不手改生成文件。构建和测试不要求根目录 `package.json`、前端构建器或应用框架。`make verify` 保持原有本机 Rust 检查；WASM 包执行在独立 CI job 中检查。

## CPU 示例

浏览器通过 HTTP 提供整个 `web` 产物目录，JS 与 `_bg.wasm` 保持相对位置：

```js
import init, { Cpu6502Ram } from './cpu6502/web/hesper_cpu6502.js';
await init();
const cpu = new Cpu6502Ram();
try {
  cpu.load(0x8000, Uint8Array.of(0xa9, 0x2a, 0x8d, 0x00, 0x02));
  cpu.load(0xfffc, Uint8Array.of(0x00, 0x80));
  cpu.reset();
  cpu.step(7); // LDA #$2A
  cpu.step(7); // STA $0200
  console.log(cpu.registers().a, cpu.readMemory(0x0200, 1)[0]); // 42, 42
} finally {
  cpu.free();
}
```

Node.js 使用对应 `nodejs` 产物，无需 `init()`：

```js
const { Cpu6502Ram } = require('./cpu6502/nodejs/hesper_cpu6502.js');
const cpu = new Cpu6502Ram();
try { console.log(cpu.registers()); } finally { cpu.free(); }
```

| 接口 | 约定 |
|---|---|
| `load(address, bytes)` | 非回绕、事务性装载，失败不修改 RAM |
| `readMemory(address, length)` | 纯 RAM 副本；不产生 CPU bus cycle |
| `registers()` / `debugState()` | 独立 JS 观察对象；寄存器 `status` 是状态字节；不是可恢复存档 |
| `halfCycle()` | Φ1 返回 `undefined`；Φ2 返回总线记录 |
| `cycle()` | 复用核心 cycle；返回读写地址、数据、SYNC、RDY 停顿与可选完成记录 |
| `step(cycleBudget)` | 完成当前或下一个指令/入口；预算指本次调用最多推进的周期，返回 `cycles` 是整个指令/入口的周期数 |
| `runCycles(cycleBudget)` | 执行指定数量 cycle 调用，返回 `{cycles, completedSteps}`；预算正常用完是成功，允许停在指令中间 |
| `beginReset()` / `reset()` | 宿主七周期 RESET 入口；`reset()` 预算耗尽后用 `cycle/step` 续跑，再次 `reset` 会重启入口 |
| `setResetLine` / `setIrqLine` / `setNmiLine` / `setSoLine` | `true` 表示逻辑 asserted；保留原有采样时序 |
| `setReady(ready)` | `false` 使 RDY 读周期停顿；不会省略重复总线读取 |

首版不提供 JS 自定义 Bus 或任意设备映射；使用 Rust `Bus` 扩展设备的能力仍在原库中。正常运行使用批量接口；逐周期和半周期接口用于有界观察。

## Apple I 示例与接口

```js
import init, { Apple1 } from './apple1/web/hesper_apple1.js';
await init();
// romBytes 是宿主取得的 Uint8Array，长度必须为 256。
const machine = new Apple1(romBytes, false);
try {
  machine.reset();
  machine.typeText('FF00R\n'); // 适用于调用方提供的 Woz Monitor ROM
  const output = machine.runTicks(250_000);
  console.log(new TextDecoder().decode(output));
  const cells = machine.screen(); // 960 字节，行优先，40 x 24
  const cursor = machine.cursor(); // {row, column, visible}，零起点
  // 宿主按自己的调度继续调用 runTicks，并显示 cells。
} finally {
  machine.free();
}
```

- `new Apple1(rom, expansionRam)` 不自动 RESET。第二参数选择 `$1000–$1FFF` 扩展 RAM；低 RAM `$0000–$0FFF` 和高 RAM `$E000–$EFFF` 始终安装。
- `loadRam(address, bytes)` 复用现有机器装载校验，仅接受连续已安装 RAM；拒绝跨空洞、I/O、ROM 的装载，失败不部分写入。
- `typeText(text)` 先整体校验，只接受可打印 ASCII、CR、LF；CRLF/LF 归一化为 CR，ASCII 字母由机器转大写。`typeChar(byte)` 接受 `0..255`，保留原硬件宿主约定的七位掩码和大写转换，可输入控制字符。
- 键盘未消费队列最多 4096 字节，包含已呈现而尚未确认的键。超限拒绝整次输入，RESET 不清空未读输入。
- `runTicks(tickBudget)` 的单位是 **14.31818 MHz 主时钟 tick**，不是 CPU cycle。所有循环经过现有机器 `tick()`；刷新时 CPU Φ2 被抑制，主板与视频时钟继续推进。
- `tick()` 返回 `{video, cpu, refresh, frameCompleted}`；`cpu` 可以为 `null`。`video` 保留真实的 `luminance/sync/hsync/vsync/dotEdge` 布尔采样。
- `screen()` 是独立 ASCII 字符投影副本，不是像素帧；`cursor().visible` 为 false 时隐藏光标。逐点视频显示需要后续有界批量采样和浏览器渲染工作。
- `runTicks()` 成功时返回并清空已完成字符输出（包括此前 `tick/reset` 留下的输出）；出错时保留它，通过 `drainOutput()` 取回。连续 `tick()` 最多积累 4096 字节输出，满后返回 `OutputLimit` 并暂停推进，调用者 drain 后可继续。
- `reset()` 是有界物理主板 RESET，保留屏幕与时钟；`setResetLine(asserted)` 允许宿主控制引脚保持时间。`clearScreen()` 只清屏，无 CPU 周期、无时钟重置。
- `masterTicks()/cpuCycles()/videoFrames()` 返回累计 `bigint`。`registers()` 是只读 CPU 快照；`ioPending()` 表示键盘/显示握手等尚有工作。

没有隐式定时器、墙钟节流、DOM 访问或文件系统依赖。宿主可在主线程分批调用，也可以自己放入 Worker；不能把“每次推进 250,000 ticks”当成保证实时帧率。

## 类型、错误与生命周期

- 地址、字节和单次预算的 JS 参数是 `number`。绑定层用浮点参数先验证有限、整数、范围，再转换成 Rust 硬件宽度类型，避免自动截断/回绕。
- `step/runCycles/runTicks` 的单次预算为 `1..=1_000_000`；超范围在执行前报错。`readMemory` 长度为 `0..=65536`，同时检查地址与长度之和。
- 长期 `u64` 计数和 Step 的 `cycles` 返回 `bigint`。要序列化到 JSON，请显式转换成字符串；不要转成可能丢精度的 `number`。
- 成功返回的快照是普通 JS 数据，无需 `free()`。字节数组是独立副本，不暴露 WASM 内存视图。CPU/机器实例持有 WASM 内存，调用结束后用 `free()` 或 `Symbol.dispose` 释放。
- 预期失败抛出 JS `Error`，`name` 为 `HesperError`，`code` 为 `InvalidArgument/InvalidRange/InvalidLoad/InvalidRom/InvalidText/InputLimit/OutputLimit/UnsupportedOpcode/CycleBudgetExceeded`。CPU 错误另外携带 `address` 和 `opcode` 或 `budget`。
- 参数与装载错误发生在修改状态前。执行错误可能发生在部分推进之后，不回滚；`CycleBudgetExceeded` 保留 CPU 的可续跑状态，遇到非法 opcode 则由宿主明确处理。观察和生成错误消息不读取模拟设备。

## 验证范围

`make wasm-test` 用同版本原生 Rust 实时生成参考数据，在 Bun 与 Node.js 中测试实际生成的 `nodejs` JS/WASM：CPU 逐周期记录（包含 RMW、RDY、SO、NMI、物理 RESET）、单步续跑、半周期、参数检查、实例隔离，以及 Apple I 回显、主时钟与 CPU 周期、数字视频采样、不同分批方式、RESET/清屏、队列限制和错误时输出保存。

`make wasm-browser-test` 通过 HTTP 加载 `web` 产物，在真实 Chrome 上运行同一组检查。也可在仓库根目录运行 `python3 -m http.server 8000 --bind 127.0.0.1`，打开 `http://127.0.0.1:8000/tests/wasm/browser.html` 查看验证页；先执行 `make wasm-test` 生成包和参考数据。

这些是绑定与原生/WASM 一致性证据，不代替 CPU 全语料 conformance，也不代表 Apple I M3 或完整浏览器前端 M5 验收。核心 CPU 行为与原板兼容声明仍以现有文档为准。
