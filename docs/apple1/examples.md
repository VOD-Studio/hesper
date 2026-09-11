# Apple I 使用示例

Apple I 的硬件架构、历史背景和 Woz Monitor 全面介绍见 [`apple-1-overview.md`](apple-1-overview.md)。本文档给出 `crates/apple1`（`hesper-apple1`）机器模型与 Hesper TUI / `hesper apple1` 文本宿主的可复现使用示例。机器模型与地址映射见 [`crates/apple1/src/lib.rs`](../../crates/apple1/src/lib.rs)；CLI 分流、会话与 TUI 分别在 [`crates/cli/src/main.rs`](../../crates/cli/src/main.rs)、[`crates/cli/src/apple1.rs`](../../crates/cli/src/apple1.rs) 和 [`crates/cli/src/tui.rs`](../../crates/cli/src/tui.rs)；来源与许可证见 [`references.md`](../references.md#apple-i)。

所有输出片段均为本地实际运行结果，不是编造的示意输出。

## 1. 准备 ROM

ROM 不内嵌、不提交；请先确认使用权限。安装 Bun 后，`make wozmon` 调用 `tools/prepare_wozmon.ts` 从 <https://github.com/alangarf/apple-one/blob/master/roms/wozmon.hex> 下载 HEX、转换并校验 256 字节和 SHA-256，通过后写入被 Git 忽略的 `.cache/apple1/wozmon.bin`。普通构建和测试不会自动下载，详见 [`资源说明`](../../crates/apple1/tests/data/README.md)。

```sh
make wozmon
export HESPER_APPLE1_ROM="$PWD/.cache/apple1/wozmon.bin"
```

校验：

```sh
bun tools/prepare_wozmon.ts --verify "$HESPER_APPLE1_ROM"
```

## 2. CLI 快速启动

```sh
cargo run -p hesper -- apple1 --rom "$HESPER_APPLE1_ROM" --help
```

| 选项 | 说明 |
| --- | --- |
| `--rom <path>` | 文本/管道模式必填；TUI 中可在配置页输入，256 字节 Woz Monitor ROM |
| `--program <path>` | 可选，启动时额外加载到 `$0000` 的程序 |
| `--max-cycles <N>` | 真实 CPU 周期预算上限（不含刷新停钟的板级时间），用完即退出 |
| `--trace` / `--bus-trace` | 指令／总线诊断，运行结束后写 stderr（不进入机器画面） |
| `--trace-limit <N>` | 保留最近 N 条诊断记录，1..4096，默认 64；两种 trace 共用这个上限 |

`--max-cycles` 计的是真实 CPU 总线周期；总线诊断每行以 `M=<会话主板时钟>` 开头，并按 `C<CPU 序号>` 续接，因此刷新停钟在 trace 里表现为 M 的间隔而不是伪造的读写记录。显示没有速度参数：终端固定按原板时序在光标槽接受字符。

不带 `--max-cycles` 时正常启动 Apple-1 TUI：

```sh
cargo run -p hesper -- apple1 --rom "$HESPER_APPLE1_ROM"
```

```
\
```

`\` 是 Woz Monitor 复位后的提示符。

**stdin 与 stdout 都是真实终端时**，CLI 进入 Apple-1 TUI：机器的 40×24 屏幕只从 `screen()`/`cursor()` 投影，状态、菜单和 RESET 通知在网格外绘制。F1 帮助、F2 会话菜单、F3 侧栏、F10 顶栏菜单；小于 `44×30` 时会暂停机器并保留可退出入口。按键无需按 Enter 就进入模拟键盘，monitor 命令仍以 Enter（CR）执行。无参数 `cargo run -p hesper` 则先显示中文启动中心，可配置或继续本次进程内保留的 Apple-1 会话。

保留的宿主命令（真实 Apple I 键盘产生不了 Ctrl 组合）：

| 按键 | 作用 |
| --- | --- |
| Ctrl-C / Ctrl-D | 退出（退出码 0，终端恢复） |
| Ctrl-R | 物理 RESET：保留 RAM、屏幕与未读按键，受同一周期预算约束 |
| Ctrl-L | CLEAR SCREEN：Apple I 键盘的第二个按钮，清机器 40×24 屏幕，不跑任何周期 |
| Ctrl-P | 暂停／继续：暂停期间不跑自由批次，按键仍排队；Ctrl-R／Ctrl-N 仍会执行并保持暂停 |
| Ctrl-N | 用原始 ROM／程序字节重建机器（新 RAM、空屏），会话周期计数与预算保留 |

TUI 配置使用系统配置目录中的 `hesper/config.toml`，只保存已校验的绝对 ROM 路径和显示偏好；路径或 TOML 损坏不会被自动覆盖，也不会影响 `demo` 或文本/管道 Apple-1 路径。

**stdout 被重定向时**（`> file`、管道）仍是纯字符流，绝不写入光标／清屏等控制序列。

**仿真机输入输出固定大写**：键盘入队先取七位，再将 ASCII `a–z` 转为 `A–Z`；显示完成时对七位字符做同样转换，屏幕与输出流一致。库 API、管道、逐键输入和 bracketed paste 共用设备层规则，因此可输入小写 `300r`。数字、标点和控制字符的既有行为不变；这不是 Unicode 大写转换或完整字符 ROM 仿真，也不转换 CPU 写总线、PIA 显示读回、ROM、路径或宿主日志。

## 3. 交互示例：内存检查（examine）

在 `\` 提示符后输入 `FF00.FF0F` 并按 Enter，转储 ROM 起始 16 字节：

```
FF00.FF0F
FF00: D8 58 A0 7F 8C 12 D0 A9
FF08: A7 8D 11 D0 8D 13 D0 C9
```

`D8 58`＝`CLD` `CLI`，是 Woz Monitor 自身复位入口的头两条指令。

## 4. 交互示例：写入并回读 RAM（deposit → examine）

```
300: AB CD EF

300.302
0300: AB CD EF
```

`300: AB CD EF` 把三个字节写入 `$0300‑$0302`（deposit 语法不产生额外输出）；`300.302` 回读同一区间，确认写入生效。

## 5. 交互示例：写入并运行一段小程序（deposit → run）

程序：`LDA #'*'`（`A9 2A`）→ `STA $D012`（`8D 12 D0`，写显示口）→ `JMP $0305`（`4C 05 03`，自旋在 `JMP` 指令上）：

```
300: A9 2A 8D 12 D0 4C 05 03

300R
0300: A*
```

输出里的 `*` 就是程序通过 PIA Port B 写到显示的字符。

**注意（真实 Apple 1 行为，不是本项目的限制）**：Woz Monitor 的 `R` 命令是 `JMP` 到目标地址，不是 `JSR`。上面用 `JMP` 自旋跳回自身正是官方操作手册里的标准写法。如果程序末尾改用 `RTS` 而没有配对的 `JSR`，PC 会跳到未定义地址，Woz Monitor 不会自动收回控制权——这与真实硬件一致，需要靠自旋、显式跳回 `$FF00`，或重新拉物理 RESET 才能回到 monitor。

## 6. 非交互 / 自动化：管道喂命令

CLI 通过管道读取 stdin 时同样有效，适合脚本或 CI smoke check。命令行必须以真实的 CR（`\r`，`0x0D`）结束——Woz Monitor 只认 CR 作为行结束；`printf` 需要显式写 `\r`：

```sh
printf 'FF00.FF0F\r\n300: A9 2A 8D 12 D0 4C 05 03\r\n300R\r\n' \
  | cargo run -p hesper -- apple1 --rom "$HESPER_APPLE1_ROM" --max-cycles 2000000
```

从真实交互式终端敲 Enter 同样能正确工作：终端的 canonical 模式会把物理 Enter（CR）翻译成 LF 再交给读取进程，CLI 在把整行内容送进模拟键盘前会先剥掉这个行尾，统一补发一次 CR（`crates/cli/src/apple1.rs`）；因此第 3～5 节里的交互示例在真实终端里逐字敲同样命令即可复现，不需要额外操心行结束符。

## 7. 直接使用 `hesper-apple1` 库 API

不经过 CLI，直接用 `Apple1` 驱动机器（适合宿主自定义 I/O、批量场景，或写自己的集成测试）：

```rust
use hesper_apple1::Apple1;

/// 一个视频帧的主板时钟数：262 条扫描线 × 65 个字符时钟 × 14 个主时钟。
const FRAME_TICKS: u64 = 262 * 65 * 14;

fn run(rom: &[u8; 256]) -> Result<(), Box<dyn std::error::Error>> {
    let mut machine = Apple1::new(rom)?;
    machine.reset()?; // 拉物理 RESET 线、保持、释放并跑完真实复位时序

    // 时间为主板时钟。终端每圈循环存储（一帧）才接受一个字符，所以
    // 推进量要按帧算，不能按"CPU 周期够了"估。
    let boot_output = machine.run_ticks(8 * FRAME_TICKS)?;
    assert!(boot_output.contains(&b'\\'));

    // 逐字符输入一条命令（monitor 只认 CR 结尾，不是 LF）
    machine.type_str("FF00.FF0F\r");

    // 等一个**具体结果**，不要用"某一批没有输出"当作完成：机器空转时
    // 同样没有输出。这里等完整的 16 字节转储。
    let mut output = Vec::new();
    for _ in 0..40 {
        output.extend_from_slice(&machine.run_ticks(FRAME_TICKS)?);
        let text = String::from_utf8_lossy(&output);
        if text.contains("FF00: D8 58") && text.contains("FF08: ") {
            return Ok(());
        }
    }
    Err("dump did not complete within the board-time budget".into())
}
```

主机侧另有几个只读观察：`master_ticks()`（主板时钟总数）、`cpu_cycles()`（真实 CPU 总线周期数，含刷新停钟造成的缺口）、`video_frames()`（垂直终止计数产生的完整帧数）、`io_pending()`（未消费键盘输入、未完成握手、B3 脉冲或视频控制序列是否仍在进行）。`Apple1::tick()` 返回的 `Tick { cpu, refresh, frame_completed }` 中，`cpu` 只在真实 Φ2 完成时给出 `Cycle`。

这与 `crates/apple1/tests/wozmon.rs` 里的测试用的是同一套 API；测试里的 `run_until(machine, budget, predicate)` 辅助函数就是上面这种"等具体结果"的封装，并在超预算时带最近周期轨迹、CPU 状态与屏幕内容失败。

## 8. 可以手动输入运行的完整程序

第 3～5 节只是单条命令。下面 1～4 号文件是本项目为演示技巧编写的教学示例，完整的、逐字节手抄进 monitor 就能跑的小程序（拿到一份十六进制清单，用 `addr: byte byte ...` 一行行敲进去，再 `addrR` 跑），每个都在真实 Hesper Apple1 模拟器上实测过，覆盖字符串输出、键盘轮询、子程序+十六进制运算、无限循环四类基础技巧；5 号文件不是本项目编写的，是逐字节复刻自 1976 年原始 *Apple-1 Operation Manual* 的官方 TEST PROGRAM——当年真实买家接好硬件后第一个手敲运行的程序，来源与核对过程见该文档内注明的原始扫描与两份独立转录：

1. [`01-hello-string.md`](01-hello-string.md) — 打印一行字符串
2. [`02-keyboard-echo.md`](02-keyboard-echo.md) — 绕开 monitor，直接读键盘回显
3. [`03-hex-adder.md`](03-hex-adder.md) — 两数相加，打印十六进制结果
4. [`04-counting-loop.md`](04-counting-loop.md) — 无限循环打印 0123456789
5. [`05-manual-test-program.md`](05-manual-test-program.md) — 1976 官方 Operation Manual 的整机测试程序（历史真实程序，非本项目原创）

## 参考

- 地址映射与设备职责：[`crates/apple1/src/lib.rs`](../../crates/apple1/src/lib.rs)
- CLI 参数解析与交互循环：[`crates/cli/src/apple1.rs`](../../crates/cli/src/apple1.rs)、[`crates/cli/src/main.rs`](../../crates/cli/src/main.rs)
- 集成测试（含本文档示例的可执行版本）：[`crates/apple1/tests/wozmon.rs`](../../crates/apple1/tests/wozmon.rs)、[`crates/cli/tests/apple1.rs`](../../crates/cli/tests/apple1.rs)
- ROM 来源与许可证声明：[`references.md`](../references.md#apple-i)
- 里程碑范围与验收记录：[`roadmap.md`](../roadmap.md#m3apple-i-文本系统)、[`verification.md`](../verification.md)
