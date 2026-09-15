# Apple I 数字视频链

实现范围：六条 2504 数据轨道 → C4/C14 写入选择 → C3/2519 六条 40 级行缓冲 → D2/2513 字模 → D1/74166 串行像素 → C13 复合同步。`Apple1::tick().video` 每主时钟返回一次采样；核心只保存定长器件状态，不保存视频历史。TUI 的 `screen()` 和宿主光标仍是文本观察接口，不参与这条链。

## 编码与光标

`Display::memory` 存储 `B0..B4` 和原始 `B6`，打包为 `(data & 0x1f) | ((data & 0x40) >> 1)`。清屏、CR 擦除、VBL 擦除与输入空格的原始轨道均为 `00`。宿主 ASCII 另存，不作为字模输入。

C10 的 NOR 在 2519 输入端形成 A9：`!(raw_B6 || (cursor && blink))`，低五位直通。无光标时原始 `00` 解码为字模地址 `20`（空格），原始 `20` 解码为地址 `00`（@）。光标闪亮时把空白位置的 A9 拉低，产生 @ 字形，不是宿主终端下划线，也不是任意字形反色。

D13 使用原图 R10=10 kΩ、R11=10 kΩ、C7=25 µF 的标称 astable 近似：高电平 `.693*(R10+R11)*C7 = .3465 s`，低电平 `.693*R11*C7 = .17325 s`，向下量化至主时钟 tick。确定性初值为高；不模拟上电相位、RC 容差和温度。物理 RESET 保留视频及闪烁相位。原子宿主 CLEAR 清空行缓冲与像素寄存器，保留板时钟／闪烁相位；这不等同于物理按钮脉宽仿真。

## 时序与取样顺序

保留现有 `Timing` 的字符沿 phase 13 和 MEMΦ phase 3。C13 二分频后，每两个主时钟一个点时钟；D11 的同步预置／使能接线产生 `A B C D E F 0`，Q3 的 `0→A` 是字符沿。

| 同一字符内的 phase | 动作 |
|---|---|
| 0、2、4、6、8、10、12 | D11 稳态分别为 A、B、C、D、E、F、0 |
| 1、3、5、7、9、11、13 | 点时钟上升沿，D1 移位或装载 |
| 3 | 原有 MEMΦ 选中当前物理槽，执行写入／擦除与握手，保存 C4/C14 选出的数据，之后推进物理读头 |
| 11 | D1 读取沿前的字模输出；`H6 && old_TC` 时并行装载；其后 C3 在派生 LINEΦ 上升沿采样 |
| 13 | 字符计数沿；H18 按旧写控制、VBL 和 D6.Q3 重载垂直计数器 |

`LINEΦ = NAND(H6, D11.Q2)`。H6 高的 H120–159 范围每扫描线产生 40 次上升沿；RC 接 `/LINE7`，扫描行地址 7 装入，其他扫描行循环。C3 的新输出可在下一次 D1 装载前驱动组合字模，但不会穿透本次 D1 装载。模型保留此先后关系，不模拟纳秒传播／毛刺。

因此一行数据有一整个字符行的流水延迟：在 V0–7 期间装入的第一行，V8–15 重放；字模行 0 空白，第一条有字形的扫描线是 V9。最后一行会在 V192–199 输出，**不能用存储控制的 `vbi` 直接抹掉视频**。VBL 使 C4/C14 清零，并在扫描行 7 向 C3 装入空格，即使该字符槽没有 MEMΦ。

D2 的 Q5..Q1 接 D1 的 H..D，A/B/C 和串行输入接地。并行装载后依次输出五个像素和两个零；第三个接地位在下一次装载时被替换。非对称 F、斜线和 CPU 写入测试锁定位序。H18 的重扫与 `$00` 重载由同一个计数器驱动所有级，不另设固定 262 行渲染时钟。

## 同步、采样与电平

`VideoSample` 的布尔量均用 true 表示有效：

- `luminance`：D1 QH 的串行像素；同步期间宿主按 `sync` 优先解释。
- `hsync`：C9 `H4 OR H6` 为低，正常每行 H100–109，共 140 master tick。
- `vsync`：D15 Q3 为低；正常帧持续 8 个水平周期。
- `sync`：C15 → C13 的复合同步，等于采样到的 `hsync || vsync`。
- `dot_edge`：本 tick 是否发生点时钟上升沿，可供宿主以点频采图。

C13 每主时钟取旧计数器译码，因此 phase 13 提交的计数器变化在下一 tick 的同步采样中可见。正常帧 262 行，存储 VBL 为 70 行；这和 8 行垂直同步不是同一个信号。

`nominal_millivolts()` 定义同步 0 mV、黑 300 mV、白 1000 mV，只是便于宿主消费的标称输出约定。原图 R1/R2/Q5、电位器、负载、TTL 输出特性、传播与串扰没有做电路求解。不能把这些值称作测得的原板电压。[原板调查](https://www.willegal.net/appleii/apple1-hardware.htm)记录的视频串扰额外亮点不在本模型内。

## 字模来源与许可证

固定使用 [P-Lab 的 Apple-1 2513 替换字模](https://p-l4b.github.io/2513/)；下载链接为 [`2513_Apple-1.bin`](https://p-l4b.github.io/2513/2513_Apple-1.bin)，作者 P-Lab，页面声明 [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)。保留作者、来源和许可证标记；只做去除重复银行及 Rust 数组格式转换，不修改像素。这是有独立授权的替换器件字模，不嵌入 Woz Monitor 或 Apple 软件 ROM。

- 2026-09-15 下载长度 2048 字节，SHA-256 `8b98c5ef224cffd56716e55b25e559ace99132dd4165cb36f7c80974feb37ffc`。
- 四个 512 字节银行完全相同；`src/video/font.rs` 保存首银行的 64×8 字节。
- 索引为 `character_address * 8 + row_address`；位 4..0 对应 Q5..Q1，1 表示亮点。全部 64 个字符的行 0 均为零，地址 `00` 是 @，`20` 是空格。
- 按 Signetics CM2141 原厂字符格式图核对了行地址与 Q5..Q1 顺序，并以 F 和 `/` 做显式字形断言。**尚未完成全部 64 个字形与目标原板 2513 掩膜的逐点独立认证**；实现的固定字模身份是上述 P-Lab 文件，不宣称任意 2513 修订均相同。

## 一手资料定位

- [Apple-1 Operation Manual，terminal sheet，leaf 13](https://iiif.archive.org/iiif/Apple-1_Operation_Manual_1976_Apple_a%2413/full/2400,/0/default.jpg)：C3/D2/D1、C10/C12/C14、C13/D11/D15。原尺寸 `5797×3751`；本轮直接查看整页，以及 `1500,1180,1920,1900`、`1050,840,1230,780` 区域。
- [Signetics 2518/2519](https://www.applefritter.com/files/signetics2519.pdf)，PDF 页 1 真值表／引脚，页 2 时序图：RC=1 循环、RC=0 装入，输入相对时钟上升沿有 setup/hold。该扫描合集页 3 的说明文字反而写 RC=0 循环，与页 1 真值表冲突；采用页 1 真值表及原板 `/LINE7` 接线，不把冲突文字作为同极性的第二份证据。SHA-256 `cd16e87303d54b1c78ac257bd6c7ee68ae0c6e43487b8ab888380cdd94d0d44f`。
- [Signetics 2513](https://meatfighter.com/mad-turings-maze/signetics-2513.pdf)，PDF 页 1–3（印刷页 54–56）、页 7–8（60–61）：引脚、静态 ROM、行 0 空白、Q5..Q1、CM2141 字形图。SHA-256 `30fe9bac16aa6dc4acc52316a13be6b9480fb7f81d26aec383ef25c6332bb42f`。
- [TI SN74166 / SN74LS166A](https://www.ti.com/lit/gpn/sn74ls166a)，PDF 页 1–2（October 1976 / revised March 1988）：同步装载、上升沿、H 位先输出，以及两路时钟输入之一固定低、另一输入作时钟的允许行为。原图 D1 pin 6 接点时钟、pin 7 接地。SHA-256 `31f40876159e83acfdfdd65b9fd2f1af5d9602b67efc9a71c8fcb6a6a87c121a`。

## 可重现采样与验证

```sh
cargo test --locked -p hesper-apple1 --lib
cargo test --locked -p hesper-apple1 --test video
cargo run --locked -p hesper-apple1 --release --example video -- /tmp/hesper-video-capture
# 可选 TEXT（1..255 字节 ASCII，可含 CR）和从第几个自然帧结束后开始采集：
cargo run --locked -p hesper-apple1 --release --example video -- /tmp/hesper-video-write F 1
make verify
cargo test --locked -p hesper-apple1 --test wozmon
cargo test --locked -p hesper --test apple1
```

输出目录必须不存在。示例运行原创 6502 程序，通过 PIA 握手写字；默认等写入完成并经过两个空闲自然帧，再从真实 HSYNC 沿开始采集。提供 FRAME 则可捕获写入／滚动中的画面。准备及采样均有预算；没有完整帧、行不完整、文本非法或目录已存在都会报错。

- `frame.pgm`：每点时钟采集真实亮度，455 像素宽；高度取实际同步沿间的扫描线数。
- `video.csv`：每 master tick 的亮度、H/V/复合同步及标称 mV，保留完整相位。
- `sync.svg`：同一段采样生成的同步波形，可直接用浏览器查看。

宿主示例最多保存 2048 行；导出不读取 `screen()`，不在帧末反推字形。PGM 包含真实消隐边界，macOS 可用 `sips -s format png frame.pgm --out frame.png` 转为 PNG。默认单元／集成回归自包含、无联网或 ROM 依赖；Woz Monitor 联调使用已有外部 ROM，验收状态单独记录。
