# Apple I 一手硬件依据核对

一手资料核对始于 2026-09-11，范围为 Apple I 原始操作手册（1976）、三张原理图、MC6820 原厂数据表（1976）与 MC6821 原厂数据表（1985）的指定页。2026-09-14 补核 H18 的共享预置网络、同步重载与计数使能，并实现其数字时序；2026-09-15 按印刷页 7 的接线修复 H09 首次按键前的 PA7 电平。以下区分原件事实、原图推导、已验证行为和保留的模拟约定。

## 原件与页码索引

### M76 — Apple-1 Operation Manual (1976)

- 馆藏：Internet Archive `Apple-1_Operation_Manual_1976_Apple_a`
- 归档 SHA-1（元数据，非独立校验）：`d0708a286041cbb8ce97f31827b8d1b255d214b4`
- 原始 PDF：1774213 字节
- IIIF 入口：`https://iiif.archive.org/iiif/Apple-1_Operation_Manual_1976_Apple_a%24{N}/full/1200,/0/default.jpg`（N = scan leaf，从 0 计数）
- 15 个 scan leaf（0–14）；leaf 编号不等于印刷页码。

| 印刷页 | scan leaf | 内容 |
|--------|-----------|------|
| 封面   | 0         | 封面 |
| 未编号（SPECIFICATIONS）| 1 | 规格表 |
| 1–6     | 2–7       | Section I（键盘、显示、电源、测试程序） |
| 7       | 8         | HARDWARE NOTES / KBD/DSP Interface |
| 8       | 9         | Section III / REFRESH / DMA / SOFTWARE CONSIDERATIONS |
| —       | 10        | Dwg 00101 Rev A Sheet 3/3 POWER SUPPLY |
| —       | 11–12     | Section II 系统监控程序清单 |
| —       | 13        | Dwg 00101 Rev A Sheet 1/3 TERMINAL SECTION |
| —       | 14        | Dwg 00101 Rev A Sheet 2/3 PROCESSOR SECTION |

OCR 全文（`_djvu.txt`）仅用于定位；地址、芯片脚号、反相符号、连线交点以扫描图为准。H18 本轮使用 leaf 13 原尺寸裁剪追线，而非仅根据 OCR 或图中的数值标注判断。

### P20 — MC6820 PIA（M6800 Microcomputer System Design Data, Second Printing, ©1976）

- 馆藏：Internet Archive `bitsavers_motorola68erSystemDesignData1976_23834957`
- MC6820 章：PDF 第 41–50 页（scan leaf 42–51）
- IIIF 入口：`https://iiif.archive.org/iiif/bitsavers_motorola68erSystemDesignData1976_23834957%24{N}/full/1400,/0/default.jpg`

| 印刷页 | scan leaf | 内容 |
|--------|-----------|------|
| 45      | 46        | MPU Interface、内部寄存器寻址 |
| 46      | 47        | Expanded Block Diagram |
| 47      | 48        | Peripheral Interface、RESET 注意事项 |
| 48      | 49        | Table 1–3（内部寻址、控制字、CA1/CB1） |
| 47      | 48        | Table 5（CA2/CB2 输入／输出模式；本次实现逐条核对，此前索引偏后） |
| 50      | 51        | Table 6（CA2 输出模式） |

### P21 — MC6821 PIA（©1985, DS9435R5, 末页标注 1994 印刷）

- 馆藏：Internet Archive `Motorola_MC6821_NMOS_Peripheral_Interface_Adapter_1985_Motorola`
- 归档 SHA-1（元数据，非独立校验）：`7a4c78be2f027c46252edb0425c94c09d83fa343`
- OCR 全文（`_djvu.txt`）辅助搜索；表格位值与电气特性以原图为准。

| 印刷页 | scan leaf | 内容 |
|--------|-----------|------|
| 8       | 7         | Table 1 内部寄存器选择、Port A–B Hardware Characteristics |
| 9       | 8         | Figure 17 两端口等效电路 |
| 10      | 9         | Figure 18 控制字格式与输出模式 |

### H18 补充器件依据与独立旁证

- [M76 Sheet 1/3 原图](https://iiif.archive.org/iiif/Apple-1_Operation_Manual_1976_Apple_a%2413/full/2400,/0/default.jpg)：D6／C10 预置源、D8／D9 预置脚与 C9 重载门、D15 使能反馈，以及 C7／C8 的 CR／写控制。原图坐标裁剪 `850,620,1450,850`、`1580,590,1750,530`、`800,2060,1560,1100`、`3730,1720,1370,1100` 可复查这些区域；H6 标注为低有效 HBL 的反相信号。
- [TI SDLS060，74161 系列数据表](https://www.ti.com/lit/ds/symlink/sn74ls161a.pdf) 第 1–2 页：并行 LOAD 为同步操作，不受计数使能阻止；CLEAR 为异步操作。这里使用数字功能表，不把 LS 器件传播参数套到原板。
- [TI SDLS068A，74174 系列数据表](https://www.ti.com/lit/ds/symlink/sn74ls174.pdf) 第 1 页：正沿触发 D 触发器；不据此声称当前 C7 行为模型已覆盖所有亚字符传播相位。
- [Mike Willegal 原板走线勘误](https://www.willegal.net/appleii/apple1-hardware.htm)：图上若干共网实际浮空（含 D6 pins 3/5、D7 pins 3/6/11、D8/D9 pin 1），VINH 两种标法为同网。不得按印刷图的假共网推断上电状态。
- [Myndale/Apple1Display 独立数字重建](https://github.com/Myndale/Apple1Display/blob/master/impl1/Apple1Display.vhd) 的 `vert_in`、`load_v`、`vinh_start` 可交叉检查门级方程；它是重建旁证，不是原板认证，也不取代上述走线勘误。

## 出厂配置与地址译码

### 板级芯片选型（Sheet 2/3, leaf 14）

- 微处理器：MOS Technology 6502（板位 A1）
- PIA：MC6820（板位 A4；标称 "6820"，非 6821）
- ROM：两片 256×4 位 PROM（板位 A1、A2，原理图标 MMI 6301），并行组成 256×8 位于 `$FF00–$FFFF`
- RAM：8 颗 4096×1 位 DRAM（类型 4096/2104），4 KiB 已装，插座支持最大 8 KiB
- 晶振：14.31818 MHz（NTSC 色副载频 ×4），经分频产生 1.023 MHz CPU 时钟

### 跳线区与固定配置

原理图 Sheet 2/3 标注出厂跳线：
- Z 跳接至 CS0（RAM 片选区间选择）
- W 跳接至 CS1、X 跳接至 CS0（PROM 片选）
- R、S、T 为用户可选芯片选择跳线

Apple I 主板允许将每个 4 KiB 区域跳线为 RAM、ROM 或 I/O。Hesper 固定两组 RAM 均已安装的配置（`$0000–$0FFF` 与 `$E000–$EFFF` RAM、`$D010–$D013` PIA、`$FF00–$FFFF` ROM），不建模可变跳线。

### PIA 地址译码推导

从 Sheet 2/3（leaf 14）原理图与印刷页 7 Hardware Notes 交叉推导：

1. **RS0 ← A0、RS1 ← A1**。PIA 内部寄存器选择由地址线最低两位决定。
2. **CS0 ← A4**。A4 直接接 CS0（高有效）。
3. **CS1 ← +5V**。CS1 固定接高电平（高有效）。
4. **CS2 ← 低有效**，来自 A15–A12 译码。印刷页 7 Hardware Notes 标注 "Decode A15, A14, A13, A12 to $DXXX"。即 A15=1、A14=1、A13=0、A12=1 时，CS2 有效（低）。

MC6820/MC6821 片选条件（P21 OCR）：CS0=1、CS1=1、CS2=0 三者同时满足时芯片被选中。

**推导 PIA 选择条件**：
```
(addr & 0xF010) == 0xD010
即 A15=1, A14=1, A13=0, A12=1, A4=1; A11–A5 未参与译码
```

**寄存器地址**：`addr & 3`（RS1=bit1, RS0=bit0）。

**地址别名（镜像）**：
- `$D010`、`$D014`、`$D018`、`$D01C` … 均选择 Port A 数据/DDR（取决于 CRA-2）
- `$D011`、`$D015`、`$D019`、`$D01D` … 均选择 CRA
- `$D012`、`$D016`、`$D01A`、`$D01E` … 均选择 Port B 数据/DDR（取决于 CRB-2）
- `$D013`、`$D017`、`$D01B`、`$D01F` … 均选择 CRB

**当前实现**：`bus.rs::decode` 使用 `(addr & 0xF010) == 0xD010` 选择 PIA，`pia.rs::Reg::from_addr` 使用地址低两位。`$D014`、BASIC 使用的 `$D0F2` 等别名共用寄存器及读取副作用。

### PROM/RAM 译码

- PROM（`$FF00–$FFFF`）：A15–A8 全部为 1 时选中（256 字节窗口）。当前 `decode` 仅匹配 `0xFF00..=0xFFFF`，未考虑不足 16 条地址线的 PROM 在更大地址空间中的别名可能性——原理图未显示高位地址线参与 PROM 片选译码，但两片 256×4 PROM 仅有 8 条地址输入，无法响应 A8–A14，因此 `$FF00–$FFFF` 在 `$0000–$7FFF` 的理论镜像取决于未在原理图中明示的总线缓冲器行为。当前实现保守地将所有非 `$FF00–$FFFF` 访问视为开路。
- RAM：当前固定安装 8 KiB，第一组位于 `$0000–$0FFF`，第二组位于 `$E000–$EFFF`，用于加载 BASIC 等程序；两组互不镜像。

## 主张—证据—实现矩阵

每行：来源标识／定位 → 代码路径 → 来源状态／实现关系。

### H01：PIA 原始型号与 MC6820/MC6821 差异

| 维度 | 内容 |
|------|------|
| **一手证据** | M76 leaf 14（Sheet 2/3）板位 A4 标 "6820"；印刷页 7 Hardware Notes 均提 "PIA" 未标具体型号。P20 ©1976 为同期 MC6820 原厂数据表；P21 ©1985/1994 为后期 MC6821 数据表。 |
| **关键差异** | MC6821 文本明确：RESET 清零全部寄存器；Port A 读实际引脚、Port B 读输出锁存；Port A 有内部上拉（输入模式）、Port B 三态浮空。MC6820 同章包含 E 周期条件的中断边沿网络与 RESET 期间控制线电平注意事项。两块芯片的寄存器模型、端口读回行为和 RESET 清零范围兼容，但时序参数和 RESET 期间的控制线电平不能无条件互换。 |
| **实现位置** | `crates/apple1/src/lib.rs` 模块文档和 `pia.rs` 模块文档称 "MC6821"。 |
| **来源状态** | **一手原文**（M76 标 6820；P20/P21 分别定位）。 |
| **实现关系** | **模拟约定**：历史硬件是 MC6820，代码以 MC6821 命名；CB2 数字规则已按 MC6820/MC6821 数据表核对，不将两者的电气参数视为相同。两端口读回路径已按该数据表的 A 读引脚、B 输出位读锁存分别建模（H12）；A 侧上拉、B 侧浮空的电气特性不建模。 |

### H02：RAM 片选与区间

| 维度 | 内容 |
|------|------|
| **一手证据** | M76 规格页（leaf 1）："8K bytes (4K supplied)"，"16-pin, 4K Dynamic, type 4096 (2104)"。Sheet 2/3（leaf 14）：跳线 Z 至 CS0 选择 RAM 区间。 |
| **实现位置** | `bus.rs::Apple1Bus::decode` → `$0000–$0FFF` 和 `$E000–$EFFF` 两组独立 RAM。 |
| **来源状态** | **原图推导**：RAM 起始地址由跳线 Z 决定；出厂跳线对应 `$0000`。未装第二组 4 KiB 时高 4 KiB 不存在。 |
| **实现关系** | **固定扩展配置**：两组 4 KiB 均已安装，分别映射在 `$0000`、`$E000`，不镜像；区别于出厂仅安装第一组 RAM 的配置，不建模可变跳线。 |

### H03：PROM 容量与地址窗口

| 维度 | 内容 |
|------|------|
| **一手证据** | Sheet 2/3（leaf 14）：两片 PROM 于板位 A1/A2，类型 MMI 6301（256×4 位），A0–A7 接 CPU 地址线，片选出厂跳线 W→CS1、X→CS0。监控程序清单（OCR 第 1077–1278 行）始于 `$FF00`，RESET 向量 `$FFFC/D` 指向 `$FF00`。 |
| **实现位置** | `bus.rs::decode` → `0xFF00..=0xFFFF => Device::Rom`；ROM 数组 256 字节。 |
| **来源状态** | **原图推导**：W/X 跳线选通区间 + 8 条地址线 = 256 字节窗口；高位地址线别名取决于未标明的总线缓冲器行为。 |
| **实现关系** | **吻合**（256 字节 ROM 于 `$FF00–$FFFF`，ROM 写入静默忽略）。 |

### H04：PIA 完整选择条件与寄存器镜像

| 维度 | 内容 |
|------|------|
| **一手证据** | M76 leaf 14 + 印刷页 7：RS0←A0、RS1←A1、CS0←A4、CS1←+5V、CS2←低有效来自 A15–A12 译码 $DXXX。P21 Table 1（OCR 第 1982–2005 行）：RS1/RS0 + CRA-2/CRB-2 选择六个内部寄存器。 |
| **推导** | 片选条件 `(addr & 0xF010) == 0xD010`；寄存器 `addr & 3` + CRA-2/CRB-2。`$D014`（`0xD014 & 0xF010 == 0xD010`，`0xD014 & 3 == 0`）应选中 Port A 数据/DDR，与 `$D010` 同。`$D013` 和 `$D017` 均选中 CRB。 |
| **实现位置** | `bus.rs::decode` → `(addr & 0xF010) == 0xD010`；`pia.rs::Reg::from_addr` → 只看 `addr & 3`。 |
| **来源状态** | **原图推导**：选择条件完整，别名区间确认存在。 |
| **实现关系** | **吻合（地址译码）**：满足上述片选条件的地址均路由到同一个 PIA。别名读取清除同一中断标志并触发同一键盘读取事件；未满足片选条件的 `$Dxxx` 地址保持开路。 |

### H05：未选中数据总线状态

| 维度 | 内容 |
|------|------|
| **一手证据** | M76 手册未规定未映射地址读值。Sheet 2/3 无总线终端电阻标注。真实 NMOS 数据总线在无人驱动时浮空至不确定电平。 |
| **实现位置** | `bus.rs::Apple1Bus::new` → `last_read: 0`；`Bus::read`（Open 分支）返回 `self.last_read`；`Bus::write` 更新 `last_read`。模块注释原称 "the high byte of the address if nothing was driven since the last opcode fetch"，此说与代码不符（代码初值为 0，不追踪最后取指地址）。 |
| **来源状态** | **原件未规定**（手册与原理图均未定义未映射读值）。 |
| **实现关系** | **模拟约定**：确定性返回最后总线值（初始 0）。这是可重复模拟的工程选择，真实硬件无此保证。ROM 写入静默忽略同样是模拟约定——真实 PROM 为只读熔丝器件。 |

### H06：RESET 网络连接

| 维度 | 内容 |
|------|------|
| **一手证据** | Sheet 3/3 POWER SUPPLY（leaf 10）：键盘连接器 B4 的 RESET（B4-11）经按钮接地，同时标注 "RESET (B4-11)"。Sheet 2/3（leaf 14）：6502 RES 引脚、PIA RESET 引脚（低有效）及 B4-11 共接同一 RESET 线。规格页描述 RESET 按钮功能为 "enter monitor"。 |
| **实现位置** | `machine.rs::Apple1::set_reset_line` → 同时设置 CPU 和 PIA 的 RESET 线；`keyboard.rs::Keyboard::resync` 处理跨 RESET 的已排队按键。 |
| **来源状态** | **原图推导**：RESET 线连接 6502 RES、PIA RESET、键盘 B4-11。视频板和键盘编码器不在此线上。 |
| **实现关系** | **吻合**（RESET 清除 PIA 寄存器、重启 CPU、保留已排队按键）。 |

### H07：CLEAR SCREEN 连接

| 维度 | 内容 |
|------|------|
| **一手证据** | Sheet 3/3 POWER SUPPLY（leaf 10）：键盘连接器 B4-12 标注 "CLEAR SCREEN"，按钮接 +5V。Sheet 1/3 TERMINAL SECTION（leaf 13）：CLEAR SCREEN 信号进入视频板，清移位寄存器和光标。印刷页 2 Keyboard 节明确 RESET 与 CLEAR SCREEN 为两个独立按钮。 |
| **实现位置** | `display.rs::Display::clear_screen` → 清空 40×24 网格、光标归 (0,0)；`machine.rs::Apple1::clear_screen` → 调用 Display 方法。 |
| **来源状态** | **原图推导**：CLEAR SCREEN 为纯视频板输入，不经过 CPU 或 PIA，不清 PIA 寄存器。 |
| **实现关系** | **吻合**（功能级模型：一次动作清屏归位，不建模按钮脉冲宽度，不影响 CPU/PIA/键盘状态）。 |

### H08：时钟、刷新与 Φ2 抑制

| 维度 | 内容 |
|------|------|
| **一手证据** | 规格页（leaf 1）：Microprocessor Clock Frequency 1.023 MHz，Effective Cycle Frequency 0.960 MHz（Including Refresh Waits）。印刷页 8 REFRESH 节（leaf 9）："Four out of every 65 clock cycles is dedicated to memory refresh. … Φ2 is inhibited during a refresh cycle, and the processor is held in Φ1." 同页：RDY 用于 "single stepping, or slow ROM applications"，与刷新 Φ2 门控无关。 |
| **实现位置** | `timing.rs`：14.31818 MHz 主时钟，D11 ÷14 得字符时钟；D6/D7 级联给出 65 槽水平序列，`H6 && H10` 在计数 129/139/149/159（槽 34/44/54/64）选刷新。`machine.rs::Apple1::tick` 在刷新槽跳过 Φ2 相位推进：CPU 停在 Φ2、PIA 无 E、无总线访问，板时钟与视频继续推进。 |
| **来源状态** | **一手原文**：刷新使用 Φ2 门控（非 RDY），每 65 周期占 4 周期。 |
| **实现关系** | **吻合（数字模型）**：一个水平周期 910 master tick，其中 61 次真实 CPU 总线访问、4 次刷新停钟；刷新不经 `Bus::read/write`，不改变 open bus 或 PIA 侧效应，也不产生伪造的 `stalled`。刷新不参与 RDY。未建模 DRAM 单元衰减本身（仅建模门控）。 |

### H09：键盘数据线、PA7 与选通

| 维度 | 内容 |
|------|------|
| **一手证据** | 印刷页 2–3 Keyboard 节（leaf 3–4）：键盘连接器 B4 有七条 DATA 线（B6–B0），一条 STROBE 线。"Any ASCII encoded keyboard, with positive DATA outputs." "The strobe can be either positive or negative." 印刷页 7 Hardware Notes 的 KBD/DSP Interface 图明确画出 PA0–PA6 接键盘数据、PA7 接 +5V、CA1 接键盘选通。印刷页 8 SOFTWARE CONSIDERATIONS："KBD Data D010 — High order bit equals 1." |
| **实现位置** | `bus.rs::Apple1Bus::new` 在构造机器总线时将 Port A 外部输入初始化为 `$80`，在首次按键前建立 PA7 固定高电平；`Keyboard::type_char` 规范化七位字符，`Keyboard::tick` 将 `(七位字符 | $80)` 送 Port A 并经 CA1 选通。 |
| **来源状态** | **一手原文**：七位数据 + bit 7 固定为 1，选通极性可正可负。 |
| **实现关系** | **数字接线吻合（2026-09-15 修复）**：Port A 配置为输入时，首次按键前、物理 RESET 后与机器重建后，PA7 都为 1；RESET 保留已呈现的键盘数据。修复前首次按键前误读为 `$00`。PA7 固定接高不依赖 PIA 内部上拉模型；通用 PIA 默认输入不变。宿主 FIFO、初始低七位为零与字符时钟选通仍是模拟约定，非真实键盘上电状态认证。 |

### H10：键盘接口反馈与消费语义

| 维度 | 内容 |
|------|------|
| **一手证据** | M76 手册未描述计算机向键盘发送任何反馈信号。印刷页 8：`LDA KBD CR (D011) / BPL ... / LDA KBD DATA (D010)`——读取数据寄存器消费按键。同页："Reading key clears flag." |
| **实现位置** | `keyboard.rs::Keyboard::acknowledge_read` → 仅在 Port A 数据寄存器被读且此前有按键呈现时消费；`Keyboard::resync` → RESET 后重新选通未读键。 |
| **来源状态** | **一手原文**：无键盘反馈；读数据寄存器清标志并消费键值。 |
| **实现关系** | **吻合**（功能级）：消费语义匹配 PIA 清标志规则；宿主 FIFO 跨 RESET 保留是模拟约定（真实键盘编码器也不在 RESET 线上）。 |

### H11：PIA 寄存器选择（RS1/RS0/CR bit 2）

| 维度 | 内容 |
|------|------|
| **一手证据** | P21 Table 1（OCR 第 1982–2005 行）：RS1/RS0 + CRA-2/CRB-2 决定访问目标。Bit 2=0 → DDR，Bit 2=1 → 外设数据寄存器。读写双方同受 bit 2 控制。P21 OCR 第 2081–2085 行："A '1' in bit 2 allows access of the Peripheral Interface Register, while a '0' causes the Data Direction Register to be addressed." |
| **实现位置** | `pia.rs::Pia6821::read` / `write` → 通过 `Reg::from_addr` 和当前 CRA-2/CRB-2 路由到正确寄存器。 |
| **来源状态** | **一手原文**：bit 2 对读写双方选择相同寄存器。 |
| **实现关系** | **吻合**（此前 bit 2 仅控制写入的 bug 已在 2026-09-11 修复）。 |

### H12：Port A 与 Port B 读回行为

| 维度 | 内容 |
|------|------|
| **一手证据** | P21 PORT A-B HARDWARE CHARACTERISTICS（OCR 第 3168–3190 行；早前记录引用的 2007–2035 行指向同一节）："Notice the differences between a Port A and Port B read operation when in the output mode. When reading Port A, the actual pin is read, whereas the B side read comes from an output latch, ahead of the actual pin." 同节另述：A 侧按 CMOS 30%–70% 电平驱动并带内部上拉（输入模式仍连接），B 侧为三态 NMOS 缓冲、无上拉、输入模式浮空。Figure 17（scan leaf 8）等效电路图：Port A 读路径来自引脚（输入或输出模式均读引脚），Port B 输出模式读来自输出锁存。 |
| **实现位置** | `pia.rs::pin_a_levels`（Port A 读路径＝实际引脚电平）与 `pia.rs::port_b_read_levels`（Port B 读路径＝输出位读锁存、输入位读引脚）；`read_port_a_data` / `read_port_b_data` 分别调用它们。 |
| **来源状态** | **一手原文**：Port A 读实际引脚，Port B 输出模式读输出锁存（差异限定在输出模式那一句）。 |
| **实现关系** | **吻合（限定于无外部驱动的数字模型）**：Port A 读引脚——输出位由 PIA 输出缓冲驱动，故引脚电平即 ORA；输入位为外设电平（Apple I 上即键盘数据线）。Port B 输出位读输出锁存、输入位读引脚，PB7 由 DA 驱动。两种读法的表达式形状相同，只有在 PIA 之外另有器件争用同一输出线时才会出现可观察差别：本模型不表示该争用，也不表示 A 侧内部上拉与 B 侧浮空／驱动电平。回归：`port_a_read_returns_the_pin_driven_by_ora_and_by_the_keyboard`、`port_b_read_returns_the_output_latch_while_bit_7_reads_the_pin`。 |

### H13：CA1/CB1 标志与中断使能独立性

| 维度 | 内容 |
|------|------|
| **一手证据** | P21 OCR 第 2087–2102 行："The four interrupt flag bits are set by active transitions … These bits cannot be set directly from the MPU Data Bus and are reset indirectly by a Read Peripheral Data Operation." Bit 0（中断使能）仅控制是否拉低 IRQ 输出，"不影响标志位本身的置位行为。" P20 第 48 页 Table 3 定义 CA1/CB1 沿选择（CRA-1/CRB-1）。 |
| **实现位置** | `pia.rs::Pia6821::set_ca1` / `set_cb1` → 标志位独立于中断使能置位；读外设数据寄存器清标志。 |
| **来源状态** | **一手原文**：标志独立于使能位置位，读数据寄存器清标志。 |
| **实现关系** | **吻合**（此前标志绑定在使能位上的 bug 已在 2026-09-11 修复）。 |

### H14：CA2/CB2 握手模式与 Apple I 实际使用

| 维度 | 内容 |
|------|------|
| **一手证据** | P21 Figure 18（scan leaf 9）：CB2 输出模式由 CRB-5/4/3 选择。Apple I 手册印刷页 7：CB2 反相成为 DA（显示确认）并回接 PB7；RDA（接收数据确认）经 74123 单稳态 3.5 μs 送 CB1。P21 OCR 第 2136–2195 行："When CRA-5 (CRB-5) is high, CA2 (CB2) becomes an output signal." |
| **实现位置** | `pia.rs`：`cb2_mode()` 由 CRB 位 5/4/3 解码；100 = 写 ORB 后第一个 E 上升沿拉低、CB1 有效沿释放；101 = 下一个 E 释放；110/111 = 手动低／高；0xx = 输入（不产生输出 strobe）。`e_rising_edge()` 由机器在每个真实 Φ2 调用。|
| **来源状态** | **一手原文**：MC6820 Table 5 / MC6821 Figure 18 的 CB2 输出模式与 Apple I 引脚用法。 |
| **实现关系** | **吻合**：Apple I 的 `CRB=$27`（或等效的 `$A7`，位 7 只读）选中模式 100，握手由 CB1 有效沿完成；写 ORB 后不在当拍拉低，而在下一个 E。CRB7 未被数据读清时，该次应答不推进握手。 |

### H15：PIA IRQ 与 CPU IRQ/NMI 连线

| 维度 | 内容 |
|------|------|
| **一手证据** | Sheet 2/3（leaf 14）：IRQ 和 NMI 引出至边缘连接器，但主板本身无设备驱动这两个引脚。PIA 的 IRQA/IRQB 同样引出至边缘连接器，未回接 CPU。印刷页 8 确认："The sequences listed below are the routines used to read the keyboard or output to the display."——均用轮询（`BIT`/`BPL`/`BMI`），不依赖中断。 |
| **实现位置** | `pia.rs`：`set_ca1`/`set_cb1` 注释注明 "IRQ 输出未连接"。`machine.rs::Apple1::tick` 不处理 PIA 中断。 |
| **来源状态** | **原图推导**：基线 Apple I 无中断源；PIA IRQ 输出仅到边缘连接器。 |
| **实现关系** | **吻合**（未建模 PIA→CPU IRQ 路径，符合基线配置）。 |

### H16：视频数据位、2513 字符发生器与字符编码

| 维度 | 内容 |
|------|------|
| **一手证据** | Sheet 1/3 TERMINAL SECTION（leaf 13）：PB0–PB6（7 位 ASCII）经移位寄存器送 2513 字符发生器（64×7×5 字符 ROM）。印刷页 7：显示数据 B6–B0（高位在前）。印刷页 8 SOFTWARE CONSIDERATIONS："Lower seven bits are data output." 第 7 位（PB7）为 DA 输入。手册 Section I："The system monitor accepts only uppercase alpha." |
| **实现位置** | `display.rs`：`memory` 保存六条原始数据轨道（B0–B4 与原始 B6；B5 不入库），`host` 另行保存宿主投影字符且不参与控制或视频；`video.rs` 实现 C10 字符地址门控、C3 行重放、D2 字模和 D1 串行输出。 |
| **来源状态** | **原图推导**：六位存储（六片 2504）；（D6/D7 的若干输入浮空、VINH 两处标法同网）另有原板走线勘误。 |
| **实现关系** | **数字视频模型（2026-09-15）**：修正提前反相 B6 与擦除值不一致的问题，A9 在 2519 输入端由 `!(raw_B6 || cursor_and_blink)` 得到。行缓冲、固定 P-Lab 2513 替换字模、74166、光标及数字复合同步均接入 `Tick.video`。具体相位、字模授权、器件资料中的 RC 极性冲突与独立认证边界见 [视频依据](video.md)。宿主字符投影继续独立保存。 |

### H17：显示数据／DA／RDA／PB7／CB1／CB2 信号路径

| 维度 | 内容 |
|------|------|
| **一手证据** | 印刷页 7 Hardware Notes + Sheet 2/3（leaf 14）：CB2 取反成 DA送往显示板；DA 回接 PB7。RDA 经 74123 单稳态 3.5 μs 送 CB1。监控程序清单（OCR 第 1276 行）：`BIT DSP`（测试 PB7）→ `BMI ECHO`（PB7=1 时等待）。即 PB7=1 → 忙（等待），PB7=0 → 就绪。 |
| **实现位置** | `machine.rs`：`da = !pia.cb2_level()`；每个真实 Φ2 把 DA 写回 PB7（`set_display_ready`）。视频终端在光标槽接受字符时给出 RDA，机器据此启动 B3 单稳态（`B3_PULSE_TICKS = 51`，即 `ceil(3.5µs × 14.31818MHz)`），CB1 拉低、51 master tick 后回高。`pia.rs::data_lines` 把未驱动线解析为 TTL 高。 |
| **来源状态** | **一手原文 + 原图推导**：CB2→DA（反相）→PB7，RDA（图上标称 3.5 µs）→CB1。PB7=1=忙。 |
| **实现关系** | **数字模型**：完整信号路径按 CB2 输出电平驱动，PB7 不再是显示模型直控的内部标志；CB1 脉冲长度为标称值量化到整主时钟周期，非 74123 的元件容差认证。B3 为可重触发，脉冲期间再次有效不产生额外沿，且在刷新停钟期间照常计时。 |

### H17 附：手册内部矛盾

印刷页 7 的 Hardware Notes 原理图与监控程序清单一致指示 PB7=1 为忙（`BIT DSP / BMI ECHO`——位 7=1 时分支等待）。但印刷页 8 SOFTWARE CONSIDERATIONS 文字描述却写 "high order bit is 'display ready' input (1 equals ready, 0 equals busy)"。两者矛盾。本核对以连线（CB2 反相→DA→PB7）、所选 PIA 握手模式（CB2 输出低有效确认）和实际 monitor 机器码（`BIT DSP / BMI ECHO`）交叉推导的 PB7=1=忙 为准；页 8 文字视为原件内部冲突记录，不作为 PB7 极性依据。

### H18：40 列 × 24 行、折行与滚动

| 维度 | 内容 |
|------|------|
| **一手证据** | 规格页（leaf 1）及印刷页 1 明确 40 characters/line、24 lines、automatic scrolling。Sheet 1/3：D8/D9 为 74161，共享预置源接 C10 pin 10，`P = NOR(D6.Q3, /VBL)`；D8 的四位及 D9 的位 7/5/4 接 P，位 6 接地，因此 P=1 预置 `$BF`，P=0 预置 `$00`，不是恒定 191。C9 pin 8 驱动两只 `/LOAD`，等于 `/WC1 OR /VBL`。D15 预置 A，`/LOAD = VBL AND V5 AND NOT(V3 OR V4)`，Q1 反馈计数使能；LAST H 驱动行末递增。 |
| **实现位置** | `timing.rs::Timing::tick(write_control)`：同步 LOAD 优先于 LAST H／D15 使能；自然 FF→00 才完成一帧，加载 00 不伪造帧完成。`display.rs`：CR／写控制保留至后续 MEMΦ 并反馈 `/WC1`；物理 `head` 仅随 MEMΦ 前进，`origin` 由 head 与重载后的光栅偏移推导，不再有 `scroll_pending` 或独立的加 40 操作。 |
| **来源状态** | **原图与器件功能表推导**：D15 使正常 256 计数包含六条保持扫描线，共 262 行。MEMΦ 位于每字符行扫描线 7：可见区 H120–159 共 40 次，消隐区仅在 V199、207、…、255 的 H128/129、138/139、148/149、158/159 各 8 次。正常帧共 1024 次真实存储移位。主时钟相位约定为字符计数沿 phase 13、MEMΦ phase 3；不是零延迟模拟器中的同时调用。 |
| **实现关系** | **已验证的数字模型**：正常帧 238420 tick／262 行／1024 MEMΦ；在 V192/H95 同步加载 BF 会重扫扫描线 191，得到 239330 tick／263 行／1064 MEMΦ。实际 PIA 回归覆盖隔离的底行 CR、末列可打印折行、连续 CR 和重载前后 CLEAR，验证完整屏幕、光标、输出顺序及后续正常帧。263 是这些隔离条件的结果，不是给所有滚动设置的常量；未测实板或认证所有 C7 亚字符传播相位。 |

### H19：CR 与控制字符行为

| 维度 | 内容 |
|------|------|
| **一手证据** | Woz Monitor 清单仅使用 CR（`$0D`）和常规可打印字符。印刷页 2："The system monitor accepts only uppercase alpha (A-F, R)." 手册未列出任何其他控制码的支持。2513 字符 ROM 为 64 字符集（大写字母、数字、基础标点）。 |
| **实现位置** | `display.rs::Display::accept`：低七位数据的位 5/6 均为 0 时为控制码；其中 `$0D` 解码为 CR，确认后逐槽清到 LAST H 并推进光标，其余控制码完成握手但不写存储、不移动光标。 |
| **来源状态** | **一手原文**：无退格、无光标寻址、无其他控制字符。**原图推导**：非打印字符的写入门控与 CR 清行电路。 |
| **实现关系** | **原图推导**：控制码仍完成握手（否则软件会卡在等待 PB7），但不占字符格；CR 的清行是逐槽序列而非瞬时赋值，且清行期间循环存储的写入口被清行电路占用，此时送入的字符会等到光标下一个槽（即下一行行首）才被接受。 |

### H20：上电屏幕状态与清屏效果

| 维度 | 内容 |
|------|------|
| **一手证据** | 印刷页 1："上电时移位寄存器未初始化" 不在 OCR 中精确出现，但源于移位寄存器无硬件清零电路的已知特性。CLEAR SCREEN 按钮清屏归位光标。印刷页 3 Section I：按 RESET 后 "\" 提示符被显示并光标下移一行，不提及清屏。 |
| **实现位置** | `display.rs::Display::new` → 屏幕初始为空格；`clear_screen` → 清空并光标归 (0,0)。 |
| **来源状态** | **一手原文**：CLEAR SCREEN 空白屏幕；RESET 不清屏。上电初始内容原文未逐字规定。 |
| **实现关系** | **模拟约定**：上电初始为全空格，非真实移位寄存器随机图案。CLEAR SCREEN 原子清空存储，按当前光栅重新对齐读头并归位光标，取消 CR／写控制；不复位板时钟或 PIA，不模拟物理按钮脉宽和清屏传播过程。 |

### H21：字符接收速率与视频帧周期关系

| 维度 | 内容 |
|------|------|
| **一手证据** | 规格页（leaf 1）：Line Rate 15734 Hz，Frame Rate 60.05 Hz，Format 40 characters/line。印刷页 8 软件考虑中输出例程等待 DA 清零。原图：PB0–PB6 为**并行**七条数据线（不是串行位流），另一路 CB2→DA→PB7 为握手线；六片 2504 组成 1024 槽循环存储，字符时钟只在需要显示时成串给出（每字符行 40 拍）。 |
| **来源状态** | **原图推导**：字符按 MEMΦ 逐槽被接受，接受的时刻是光标槽被扫到的时刻；因此一字符的等待取决于写入时刻与光标槽的相对位置，而不是固定的「若干位串行移位」时间。此前的「每字符 7/1024 帧 ≈ 0.4 ms」推导把并行数据轨道误当作串行位流，已删除。 |
| **实现关系** | **数字模型**：字符在光标槽扫过时接受。正常、无滚动的一帧恰为一圈 1024 槽，同一行内连续两个字符的回归差值为「正常帧 + 一个字符时钟」。滚动重扫会多推进存储，不能再普遍假定一帧等于一圈；CR 清行期间的字符等待下一行行首。宿主不得设置「字符延时」参数。 |

### H22：忙时改写、未配置握手与清屏/RESET 重叠

| 维度 | 内容 |
|------|------|
| **一手证据** | 印刷页 8 输出例程先等待 DA 清零再写入，即 Woz Monitor 遵守先检查后写入的协议。手册未定义在不经握手直接写入时硬件行为——是否丢弃、覆写或破坏在途数据。P21 数据表未覆盖不配置控制寄存器直接操作端口的行为。 |
| **实现位置** | `display.rs`：DA 上升置起待接收请求，接受后清除，DA 撤销也取消未接收请求；CR 清行期间不接受。`pia.rs`：ORB 写只在 CRB 选择输出寄存器时发生；未驱动数据线按 TTL 高解析。RESET 清 ORB/DDRB、取消待发 E 操作并释放 DA。 |
| **来源状态** | **原件未规定**：忙时写入行为、未配置握手下的端口行为、RESET 期间与显示的交叠均未在手册或数据表中规定。 |
| **实现关系** | **模拟约定（可复现）**：DA 持续有效只接收一次，接收时读取实时端口数据而非保存早先副本。CLEAR SCREEN 不撤销 PIA 中仍有效的字符请求；RESET 释放 DA，使未被接收的字符取消，不能让遗留请求在稍后的 MEMΦ 把已浮空的数据线写成字符。RESET 不取消已经开始的 CR 清行或重置视频计数。 |

## 原件冲突与未闭合项

1. **PB7 极性（印刷页 7 vs 页 8）**：页 7 原理图连线 + monitor 机器码一致指示 PB7=1=忙；页 8 文字摘要相反（"1 equals ready, 0 equals busy"）。本核对以连线+机器码为准；页 8 文字记为原件内部冲突。实现侧以 `$FFEF` 处的真实 ROM 机器码（`2C 12 D0 / 30 FB`，即 `BIT`/`BMI`）复核了该极性。

2. **PIA 端口的电气特性与数字解析范围**：读回路径已按资料分别建模（H12：Port A 读引脚，Port B 输出位读锁存、输入位读引脚）。当前不区分外部驱动低与未驱动，也不解析多个驱动源的冲突；A 侧内部上拉、B 侧浮空及负载相关电平未建模。争用电流、电压与器件容差属于模拟电气范围，驱动／高阻语义则是尚未实现的数字模型扩展，两者不能混为一谈。H09 的 PA7 板级固定接高已修复，不属于这些保留边界；B3 仍按标称 3.5 µs 量化为 51 master tick，不宣称 74123 容差认证。

3. **DRAM 单元衰减**：只建模 Φ2 门控，不建模刷新周期之外的电荷保持特性，也不建模真实上电随机态。

4. **上电屏幕初始状态**：当前为确定性空白（模拟器初始化选择），非移位寄存器随机图案。

5. **开路总线初始值**：`last_read` 初始 0 为模拟约定，真实硬件浮空无此保证。

6. **MC6820 vs MC6821 命名**：历史硬件为 6820；代码以 6821 命名，并以 MC6820 印刷页 47（scan leaf 48）的 Table 5 复核 CB2 数字规则。电气参数与 RESET 细节不能互换。

7. **滚动边沿的电气范围**：H18 的条件 BF／00 同步重载和帧长变化已实现；C7／TTL 亚字符传播及物理 CLEAR SCREEN 脉宽仍未认证。数字回归不等于全部滚动相位的实板对照。

8. **视频的剩余边界**：已实现 2519 逐字符行重放、固定替换字模、D1 像素移位、视频光标和数字复合同步；原板字模全部像素的独立认证、模拟电压、负载和串扰仍未覆盖。标称电平是输出约定；TUI 字符格不作为像素证据。详见 [数字视频链](video.md)。

以上未闭合项均为独立可追踪缺口，不合并为一。补齐任意一项不自动关闭其余；M3 整体验收直到全部资料核对完成且所有已知差异已解决或明确记录为保留近似后才勾选。
