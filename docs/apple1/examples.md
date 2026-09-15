# Apple I 使用示例

Apple I 的硬件架构、历史背景和 Woz Monitor 全面介绍见 [`apple-1-overview.md`](apple-1-overview.md)。本文档给出 `crates/apple1`（`hesper-apple1`）机器模型与 Hesper TUI / `hesper apple1` 文本宿主的可复现使用示例。机器模型与地址映射见 [`crates/apple1/src/lib.rs`](../../crates/apple1/src/lib.rs)；CLI 分流、会话与 TUI 分别在 [`crates/cli/src/main.rs`](../../crates/cli/src/main.rs)、[`crates/cli/src/apple1.rs`](../../crates/cli/src/apple1.rs) 和 [`crates/cli/src/tui.rs`](../../crates/cli/src/tui.rs)；来源与许可证见 [`references.md`](../references.md#apple-i)。

所有输出片段均为本地实际运行结果，不是编造的示意输出。

## 1. 内置 ROM

Hesper 已内置 256 字节 Woz Monitor，启动无需下载或配置路径。
来源、SHA-256 与权利归属见 [`资源说明`](../../crates/apple1/tests/data/README.md)。
可选 `--rom <path>` 用于提供同一版本的本地镜像，长度和哈希仍会校验。

## 2. CLI 快速启动

```sh
cargo run -p hesper -- apple1 --help
```

| 选项 | 说明 |
| --- | --- |
| `--rom <path>` | 可选，本地镜像覆盖内置 Woz Monitor；要求 256 字节且哈希匹配。TUI 的 ROM 字段留空使用内置镜像 |
| `--program <path>` | 可选，启动时加载原始二进制程序，默认地址 `$0000` |
| `--preset <id>` | 加载内置预置程序（42 个，见下节）并使用其固定地址；与 `--program`、`--program-address` 互斥 |
| `--list-presets` | 列出全部内置程序的分类、id、大小、载入范围、启动命令、来源页与许可证，无需 ROM |
| `--expansion-ram` / `--no-expansion-ram` | 开启／关闭 `$1000–$1FFF` 的 4 KiB 扩展 RAM；默认关闭，TUI 可保存此配置 |
| `--program-address <N>` | 指定程序加载地址；支持十进制、`0xE000` 或引号包裹的 `'$E000'`。整个文件必须落在连续已安装 RAM 内（默认 `$0000–$0FFF` 或 `$E000–$EFFF`） |
| `--max-cycles <N>` | 真实 CPU 周期预算上限（不含刷新停钟的板级时间），用完即退出 |
| `--trace` / `--bus-trace` | 指令／总线诊断，运行结束后写 stderr（不进入机器画面） |
| `--trace-limit <N>` | 保留最近 N 条诊断记录，1..4096，默认 64；两种 trace 共用这个上限 |

`--max-cycles` 计的是真实 CPU 总线周期；总线诊断每行以 `M=<会话主板时钟>` 开头，并按 `C<CPU 序号>` 续接，因此刷新停钟在 trace 里表现为 M 的间隔而不是伪造的读写记录。显示没有速度参数：终端固定按原板时序在光标槽接受字符。

加载地址只决定文件放在哪里，不改变 RESET 向量，也不自动运行程序。TUI 配置页可编辑加载地址；CLI 指定的地址用于预填，未指定时预填 `0x0000`。启动和替换会话使用表单中的地址，重新上电沿用已确认的原始程序字节和地址；物理 RESET 保留两块 RAM。

### 内置程序库（42 个，来自 apple1software.com）

`crates/cli/assets/programs/` 内置了 [The Apple-1 Software Library](https://apple1software.com/)
在 2026-09-14 发布的全部程序：4 个分类、42 个程序页、56 个 RAM 数据块，按站点自己的
Wozmon 传输格式逐块保存（一个 RAM 块一个 `.bin`，文件名带加载地址）。逐文件 SHA-256、
来源页、启动命令与许可证说明见 [`预置资源说明`](../../crates/cli/assets/README.md)；
程序分类、作者、年份、许可证与来源页同时写在 `crates/cli/src/presets.rs` 的表里。

```sh
cargo run --locked -p hesper -- apple1 --list-presets
```

程序列表按站点分类分组（Games 游戏 / Fun 娱乐 / Programming 编程 / Utilities 工具），
每行给出 `id - 名称 - 作者, 年份 - 字节数 - 载入范围 - 启动命令`，下一行是来源页与许可证。
多数程序在 Woz Monitor 提示符下按载入地址启动，例如 `--preset 15-puzzle` 后输入 `0300R`：

```sh
cargo run --locked -p hesper -- apple1 --preset 15-puzzle
cargo run --locked -p hesper -- apple1 --preset basic-huston
```

内置的 `basic-huston`（以及 `basic-c`／`basic-d`／`basic-pagetable`）是站点发布的 4096
字节 BASIC 镜像，加载到 `$E000–$EFFF`，进入 Woz Monitor 后输入 `E000R`。

八个 BASIC 语言程序（Blackjack、Dobble、Hamurabi、Mini-Startrek、Lunar Lander ASCII
Graphics、Twinkle、Resistor Calculator、Stopwatch）在站点上总是与 BASIC 一起传输，因此
预置按同样方式载入：`programming/basic-huston-e000.bin` 到 `$E000`，站点列出的 `$004A`
磁带头与程序块各自到自己的地址。启动命令是站点列表末尾的 `E2B3R`——BASIC 的热入口，
不会清掉刚载入的程序——随后输入 `RUN` 即可运行：

```sh
cargo run --locked -p hesper -- apple1 --preset hamurabi
# 进入 Woz Monitor 后：E2B3R，然后 RUN
```

`little-tower` 占用 `$0300–$14CD`，需在 TUI 启动配置中开启“扩展 RAM”，或添加 `--expansion-ram`：

```sh
cargo run --locked -p hesper -- apple1 --preset little-tower --expansion-ram
# 进入 Monitor 后输入 0300R，再按 1 开始游戏
```

扩展 RAM 默认关闭；TUI 用 Tab／鼠标选中后按 Enter／空格切换，“校验并保存”或“启动”保存到配置文件的 `[apple1] expansion_ram`。CLI 显式的 `--expansion-ram`／`--no-expansion-ram` 覆盖保存的 TUI 偏好，脚本模式不读取个人配置。RESET 保留扩展 RAM；重新上电沿用已确认的 RAM 配置并恢复原程序。

`memory-test-1000-1fff` 在扩展关闭时仍可运行并报告目标 RAM 缺失；开启后检测已安装的 RAM。
全部 42 个预置的加载条件、局部证据与未验收功能见[兼容性矩阵](../../crates/cli/assets/README.md#compatibility-matrix)。

TUI 启动中心按 `C` 打开配置页，按 `F3` 打开程序列表：列表按站点四个分类分组，↑↓ 移动、
←→ 切换分类，下方详情栏显示所选程序的载入范围、**启动命令**、兼容性限制、作者与来源页；Enter 选中后
配置页的“程序”字段下方同样显示该启动命令，启动成功后状态栏也会提示（例如
`Apple BASIC (Huston) 已加载 · 启动后输入 E000R`）。列表也可切换为“本地二进制文件”或
“不加载程序”；Esc 关闭列表并保留原选择。替换已有会话沿用原有确认流程，RESET 保留 RAM，
重新上电恢复所选程序的原始字节。

不使用预置时，任何本地 4096 字节 BASIC 镜像仍可按地址加载：

```sh
cargo build --locked -p hesper --release
./target/release/hesper apple1 \
  --program ~/Downloads/basic-c.bin --program-address 0xE000
```

也可以完全在 TUI 中配置，无需传入这两个程序参数：

1. 运行 `./target/release/hesper tui`，在启动中心按 `C` 打开配置页。
2. ROM 路径留空使用内置镜像，再按 `Tab` 到“程序路径”，填写二进制文件的绝对路径，或按 `F4` 选择文件。
3. 按 `Tab` 到“程序加载地址”，按 `Ctrl+U` 清空默认值，输入 `0xE000`（也接受 `$E000` 或 `57344`）。支持退格和单行粘贴。
4. 按 `Tab` 到“校验并保存”，或再按一次 `Tab` 到“启动”，按 `Enter` 执行；地址格式错误、超出 16 位或文件跨越 RAM 边界都会显示错误并保留当前会话。

程序路径与地址在本次 TUI 进程中保留；“校验并保存”保存的是 ROM 路径。

进入 Woz Monitor 后输入 `E000R`，看到 BASIC 的 `>` 提示符后输入 `PRINT 1+2`，结果为 `3`。带行号的程序可以用 `RUN` 执行：

```basic
10 FOR I=1 TO 3
20 PRINT I*I
30 NEXT I
40 END
RUN
```

该程序依次输出 `1`、`4`、`9`。预置和本地文件都直接加载二进制，不经过磁带接口；预置选择与本地程序路径、地址只在本次 TUI 进程中保留，不写入配置文件。

不带 `--max-cycles` 时正常启动 Apple-1 TUI：

```sh
cargo run -p hesper -- apple1
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

TUI 配置固定使用用户主目录下的 `~/.config/hesper/config.toml`，只保存已校验的绝对 ROM 路径和显示偏好；路径或 TOML 损坏不会被自动覆盖，也不会影响 `demo` 或文本/管道 Apple-1 路径。升级前若使用 macOS 的 `~/Library/Application Support/hesper/config.toml`，可将原配置复制到新位置；新版本只读取新路径。

顶栏菜单在对应标题下方展开。鼠标左键点击标题打开或收起菜单；展开后移动鼠标可切换分类、高亮菜单项，点击项目执行，点击外部收起。F10／F2 和方向键、Enter、Esc 仍可完整操作菜单。鼠标菜单默认开启；已有配置中的 `ui.mouse = false` 会保留，可按 F10 进入「显示 → 显示设置」，用上下键选中「鼠标菜单」、Enter 开启，再选择「保存设置」。鼠标仅操作菜单，确认框仍需键盘确认；退出 TUI 时会关闭鼠标捕获。

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
  | cargo run -p hesper -- apple1 --max-cycles 2000000
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
