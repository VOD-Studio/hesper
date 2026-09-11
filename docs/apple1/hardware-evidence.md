# Apple I 一手硬件依据核对

核对日期：2026-09-11。逐项对照 Apple I 原始操作手册（1976）、三张原理图、MC6820 原厂数据表（1976）与 MC6821 原厂数据表（1985）的指定页，及 Hesper `crates/apple1/` 当前实现。不新增测试或修改运行行为。

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

OCR 全文（`_djvu.txt`）仅用于定位；地址、芯片脚号、反相符号、连线交点以扫描图为准。LLM 核对时因当前模型不支持图像输入，原理图事实源自本计划执行前已完成的目视确认，并在下方逐项标明。

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
| 49      | 50        | Table 4–5（CA2/CB2 输入／输出） |
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

Apple I 主板允许将每个 4 KiB 区域跳线为 RAM、ROM 或 I/O。Hesper 固定一组基线配置（`$0000–$0FFF` RAM、`$D010–$D013` PIA、`$FF00–$FFFF` ROM），不建模可变跳线。

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

**当前实现**：`bus.rs::decode` 仅匹配 `0xD010..=0xD013`，`pia.rs::Reg::from_addr` 仅使用地址低两位。**`$D014` 等别名区间当前返回开路总线而非 PIA**。这是已知差异，不在本轮修改译码。

### PROM/RAM 译码

- PROM（`$FF00–$FFFF`）：A15–A8 全部为 1 时选中（256 字节窗口）。当前 `decode` 仅匹配 `0xFF00..=0xFFFF`，未考虑不足 16 条地址线的 PROM 在更大地址空间中的别名可能性——原理图未显示高位地址线参与 PROM 片选译码，但两片 256×4 PROM 仅有 8 条地址输入，无法响应 A8–A14，因此 `$FF00–$FFFF` 在 `$0000–$7FFF` 的理论镜像取决于未在原理图中明示的总线缓冲器行为。当前实现保守地将所有非 `$FF00–$FFFF` 访问视为开路。
- RAM（`$0000–$0FFF`）：当前实现正确对应 4 KiB 已装区域；板上 8 KiB 容量未启用。

## 主张—证据—实现矩阵

每行：来源标识／定位 → 代码路径 → 来源状态／实现关系。

### H01：PIA 原始型号与 MC6820/MC6821 差异

| 维度 | 内容 |
|------|------|
| **一手证据** | M76 leaf 14（Sheet 2/3）板位 A4 标 "6820"；印刷页 7 Hardware Notes 均提 "PIA" 未标具体型号。P20 ©1976 为同期 MC6820 原厂数据表；P21 ©1985/1994 为后期 MC6821 数据表。 |
| **关键差异** | MC6821 文本明确：RESET 清零全部寄存器；Port A 读实际引脚、Port B 读输出锁存；Port A 有内部上拉（输入模式）、Port B 三态浮空。MC6820 同章包含 E 周期条件的中断边沿网络与 RESET 期间控制线电平注意事项。两块芯片的寄存器模型、端口读回行为和 RESET 清零范围兼容，但时序参数和 RESET 期间的控制线电平不能无条件互换。 |
| **实现位置** | `crates/apple1/src/lib.rs` 模块文档和 `pia.rs` 模块文档称 "MC6821"。 |
| **来源状态** | **一手原文**（M76 标 6820；P20/P21 分别定位）。 |
| **实现关系** | **模拟约定**：代码以 MC6821 命名但其行为综合了二级资料转录的寄存器规则。历史硬件是 MC6820，命名和历史精确性存在差异。Port A/B 读回行为当前对称（均用 `(OR & DDR) | (pins & !DDR)`），实际 Port B 输出模式应读输出锁存而非引脚。 |

### H02：RAM 片选与区间

| 维度 | 内容 |
|------|------|
| **一手证据** | M76 规格页（leaf 1）："8K bytes (4K supplied)"，"16-pin, 4K Dynamic, type 4096 (2104)"。Sheet 2/3（leaf 14）：跳线 Z 至 CS0 选择 RAM 区间。 |
| **实现位置** | `bus.rs::Apple1Bus::decode` → `0x0000..=0x0FFF => Device::Ram`。 |
| **来源状态** | **原图推导**：RAM 起始地址由跳线 Z 决定；出厂跳线对应 `$0000`。未装第二组 4 KiB 时高 4 KiB 不存在。 |
| **实现关系** | **吻合**（固定配置 4 KiB RAM 于 `$0000`，不镜像，高 4 KiB 未装当开路）。 |

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
| **实现位置** | `bus.rs::decode` → `0xD010..=0xD013 => Device::Pia`；`pia.rs::Reg::from_addr` → 只看 `addr & 3`。 |
| **来源状态** | **原图推导**：选择条件完整，别名区间确认存在。 |
| **实现关系** | **已知差异**：`$D014–$D01F` 中满足 `(addr & 0xF010) == 0xD010` 的地址在当前实现返回开路总线而非 PIA 寄存器镜像。Woz Monitor 仅使用 `$D010–$D013`，不影响当前功能验证。 |

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
| **实现位置** | `lib.rs` 文档说明时钟但不建模刷新；`machine.rs::Apple1::cycle` 每次推进一个真实 CPU 周期。 |
| **来源状态** | **一手原文**：刷新使用 Φ2 门控（非 RDY），每 65 周期占 4 周期。 |
| **实现关系** | **已知差异**：不建模 DRAM 刷新周期；每个 `cycle()` 均为真实 CPU 周期，无 Φ2 被抑制的周期。这是用户选定的保留近似，不能被当作整机逐周期准确。 |

### H09：键盘数据线、PA7 与选通

| 维度 | 内容 |
|------|------|
| **一手证据** | 印刷页 2–3 Keyboard 节（leaf 3–4）：键盘连接器 B4 有七条 DATA 线（B6–B0），一条 STROBE 线。"Any ASCII encoded keyboard, with positive DATA outputs." "The strobe can be either positive or negative." 印刷页 7 Hardware Notes 标注键盘数据进入 PA0–PA6，CA1 为键盘选通输入。印刷页 8 SOFTWARE CONSIDERATIONS："KBD Data D010 — High order bit equals 1." 即 PA7 固定为 1（来自上拉或外部接线）。 |
| **实现位置** | `keyboard.rs::Keyboard::type_char` → `(c & 0x7F).to_ascii_uppercase()`；`Keyboard::tick` → 将 `(七位字符 | 0x80)` 送 Port A，CA1 高电平维持一个 tick。 |
| **来源状态** | **一手原文**：七位数据 + bit 7 固定为 1，选通极性可正可负。 |
| **实现关系** | **模拟约定**：宿主 FIFO + 一个周期选通 近似于外接键盘编码器锁存行为；真实键盘协议不包含宿主端预输入的队列。 |

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
| **一手证据** | P21 PORT A-B HARDWARE CHARACTERISTICS（OCR 第 2007–2035 行）："When reading Port A, the actual pin is read, whereas the B side read comes from an output latch, ahead of the actual pin." Figure 17（scan leaf 8）等效电路图证实：Port A 读路径来自引脚（输入或输出模式均读引脚），Port B 输出模式读来自输出锁存。 |
| **实现位置** | `pia.rs::read_port_a_data` / `read_port_b_data` → 当前两端口均使用 `(OR & DDR) | (pins & !DDR)`。 |
| **来源状态** | **一手原文**：Port A 读实际引脚，Port B 输出模式读输出锁存。 |
| **实现关系** | **已知差异**：Port B 输出模式应读 ORA/ORB 锁存值而非引脚。Apple I 显示器仅使用 Port B 输出（DDRB 低 7 位置 1），而 PB7 被设为输入（DDRB bit 7=0）以读 DA 反馈——在此配置下 `(OR & DDR) | (pins & !DDR)` 对 PB7 读引脚（正确），对 PB6–PB0 读 OR（碰巧正确，因为 DDR 已设为输出）。这是一种巧合吻合，不是 Port A/B 差异的正确建模。 |

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
| **实现位置** | `pia.rs`：控制寄存器位 5/4/3 可读写，但 CA2/CB2 输出电平未根据 CR 设置和内部事件实际驱动。`display.rs::Display::on_write` 直接设置内部忙标志并启动倒计时，不经过 CB2 输出状态。 |
| **来源状态** | **一手原文 + 原图推导**：Apple I 使用 CB2 输出模式（CRB-5=1），具体子模式待对照 CRB-4/3 值与原理图时序。 |
| **实现关系** | **已知差异**：CA2/CB2 输出电平未建模；显示握手绕过 CB2 输出直接操作 PB7 和 CB1。这是功能级近似，不等同于 PIA 输出模式实现。 |

### H15：PIA IRQ 与 CPU IRQ/NMI 连线

| 维度 | 内容 |
|------|------|
| **一手证据** | Sheet 2/3（leaf 14）：IRQ 和 NMI 引出至边缘连接器，但主板本身无设备驱动这两个引脚。PIA 的 IRQA/IRQB 同样引出至边缘连接器，未回接 CPU。印刷页 8 确认："The sequences listed below are the routines used to read the keyboard or output to the display."——均用轮询（`BIT`/`BPL`/`BMI`），不依赖中断。 |
| **实现位置** | `pia.rs`：`set_ca1`/`set_cb1` 注释注明 "IRQ 输出未连接"。`machine.rs::Apple1::cycle` 不处理 PIA 中断。 |
| **来源状态** | **原图推导**：基线 Apple I 无中断源；PIA IRQ 输出仅到边缘连接器。 |
| **实现关系** | **吻合**（未建模 PIA→CPU IRQ 路径，符合基线配置）。 |

### H16：视频数据位、2513 字符发生器与字符编码

| 维度 | 内容 |
|------|------|
| **一手证据** | Sheet 1/3 TERMINAL SECTION（leaf 13）：PB0–PB6（7 位 ASCII）经移位寄存器送 2513 字符发生器（64×7×5 字符 ROM）。印刷页 7：显示数据 B6–B0（高位在前）。印刷页 8 SOFTWARE CONSIDERATIONS："Lower seven bits are data output." 第 7 位（PB7）为 DA 输入。手册 Section I："The system monitor accepts only uppercase alpha." |
| **实现位置** | `display.rs::Display::commit_char` → 字母大写化（`.to_ascii_uppercase()`），CR（`$0D`）换行，其余字节占一个格子。 |
| **来源状态** | **一手原文 + 原图推导**：7 位字符码（PB6–PB0 至 2513），大写 ASCII+数字+标点子集。 |
| **实现关系** | **模拟约定**：大写化所有 ASCII 字母匹配手册描述；2513 实际字库可能包含小写字形但 Apple I 不区分（仅 64 字符集）。当前 ASCII 投影（CR 和普通字符）是功能级近似，未建模不可打印字符在 2513 上的实际字模对应。 |

### H17：显示数据／DA／RDA／PB7／CB1／CB2 信号路径

| 维度 | 内容 |
|------|------|
| **一手证据** | 印刷页 7 Hardware Notes + Sheet 2/3（leaf 14）：CB2 取反成 DA送往显示板；DA 回接 PB7。RDA 经 74123 单稳态 3.5 μs 送 CB1。监控程序清单（OCR 第 1276 行）：`BIT DSP`（测试 PB7）→ `BMI ECHO`（PB7=1 时等待）。即 PB7=1 → 忙（等待），PB7=0 → 就绪。 |
| **实现位置** | `pia.rs::Pia6821::display_data` → CRB bit 2=1、DDRB 低 7 位全输出、RESET 无效时返回 true。`display.rs::Display::tick` → 完成时对 CB1 连续调用 true/false（产生上升沿）。`machine.rs::Apple1::cycle` → 在 `$D012` 写时若 `display_data` 则调用 `Display::on_write`。 |
| **来源状态** | **一手原文 + 原图推导**：CB2→DA（反相）→PB7，RDA（3.5 μs）→CB1。PB7=1=忙。 |
| **实现关系** | **模拟约定**：PB7 极性正确（DA=1 时忙、DA=0 时就绪），但 PB7 由显示模型直接控制而非经 CB2 输出反相驱动。CB1 上升沿时序由固定 `cycles_per_char` 近似而非 74123 单稳态 3.5 μs RDA 脉冲。CB2 输出状态本身未实现。 |

### H17 附：手册内部矛盾

印刷页 7 的 Hardware Notes 原理图与监控程序清单一致指示 PB7=1 为忙（`BIT DSP / BMI ECHO`——位 7=1 时分支等待）。但印刷页 8 SOFTWARE CONSIDERATIONS 文字描述却写 "high order bit is 'display ready' input (1 equals ready, 0 equals busy)"。两者矛盾。本核对以连线（CB2 反相→DA→PB7）、所选 PIA 握手模式（CB2 输出低有效确认）和实际 monitor 机器码（`BIT DSP / BMI ECHO`）交叉推导的 PB7=1=忙 为准；页 8 文字视为原件内部冲突记录，不作为 PB7 极性依据。

### H18：40 列 × 24 行、折行与滚动

| 维度 | 内容 |
|------|------|
| **一手证据** | 规格页（leaf 1）："Format: 40 characters/line, 24 lines; with automatic scrolling." 印刷页 1 介绍："The output format is 40 characters/line, 24 lines/page, with auto scrolling." |
| **实现位置** | `display.rs::COLUMNS` = 40, `ROWS` = 24；`commit_char` → 满 40 列或 CR 换行；`advance_line` → 末行时整体上移一行、末行清空。 |
| **来源状态** | **一手原文**：40×24，自动滚动。 |
| **实现关系** | **吻合**（原文确认尺寸与滚动，但具体滚动机制——指针移动还是物理复制——来自移位寄存器设计推理而非原文逐行描述）。 |

### H19：CR 与控制字符行为

| 维度 | 内容 |
|------|------|
| **一手证据** | Woz Monitor 清单仅使用 CR（`$0D`）和常规可打印字符。印刷页 2："The system monitor accepts only uppercase alpha (A-F, R)." 手册未列出任何其他控制码的支持。2513 字符 ROM 为 64 字符集（大写字母、数字、基础标点）。 |
| **实现位置** | `display.rs::Display::commit_char` → CR 换行，其余字节占一个格子（含不可打印字符）。 |
| **来源状态** | **一手原文**：无退格、无光标寻址、无其他控制字符。 |
| **实现关系** | **模拟约定**：非 CR 的全部字节占一个格子（含控制字符）。这与 "无其他控制字符" 同义——真实 2513 对不可打印码可能显示特定字形或空白，但手册未规定。 |

### H20：上电屏幕状态与清屏效果

| 维度 | 内容 |
|------|------|
| **一手证据** | 印刷页 1："上电时移位寄存器未初始化" 不在 OCR 中精确出现，但源于移位寄存器无硬件清零电路的已知特性。CLEAR SCREEN 按钮清屏归位光标。印刷页 3 Section I：按 RESET 后 "\" 提示符被显示并光标下移一行，不提及清屏。 |
| **实现位置** | `display.rs::Display::new` → 屏幕初始为空格；`clear_screen` → 清空并光标归 (0,0)。 |
| **来源状态** | **一手原文**：CLEAR SCREEN 空白屏幕；RESET 不清屏。上电初始内容原文未逐字规定。 |
| **实现关系** | **模拟约定**：上电初始为全空格（非真实移位寄存器随机图案）。CLEAR SCREEN 行为吻合。 |

### H21：字符接收速率与视频帧周期关系

| 维度 | 内容 |
|------|------|
| **一手证据** | 规格页（leaf 1）：Line Rate 15734 Hz，Frame Rate 60.05 Hz。印刷页 8 软件考虑中输出例程等待 DA 清零。移位寄存器 1024 位（7×1024），960 位可见（40×24），64 位不可见。每字符需移位 7 位，约 7/1024 帧 ≈ 0.4 ms。但手册 "每帧最多一字" 来自哪里？——原手册未直接给出此断言，它来自移位寄存器时序的二级推导（全部 40 字符排满一线需要约 1/15734 × 7 × 40 ≈ 17.8 ms，一帧约 16.7 ms）。 |
| **实现位置** | `display.rs::DEFAULT_CYCLES_PER_CHAR` = 1000（约 1 ms at 1 MHz）。 |
| **来源状态** | **原图推导**：实际延迟取决于移位寄存器位置和字符位数；手册未给出固定微秒值。"每帧最多一字" 的断言源于爱好者对移位寄存器带宽的保守估计，非手册原文。 |
| **实现关系** | **模拟约定**：固定 1000 周期近似（约 1 ms），不随移位寄存器位置动态变化。 |

### H22：忙时改写、未配置握手与清屏/RESET 重叠

| 维度 | 内容 |
|------|------|
| **一手证据** | 印刷页 8 输出例程先等待 DA 清零再写入，即 Woz Monitor 遵守先检查后写入的协议。手册未定义在不经握手直接写入时硬件行为——是否丢弃、覆写或破坏在途数据。P21 数据表未覆盖不配置控制寄存器直接操作端口的行为。 |
| **实现位置** | `display.rs::Display::on_write` → 若忙则覆盖当前字符（不丢弃、不排队）；`display_data` 要求 CRB bit 2=1 且 DDRB 设置正确。RESET 后至 Woz Monitor 初始化 PIA 之间约 13–20 周期——当前实现中若在此期间写入 `$D012`（DDR 被 RESET 清零），`display_data` 返回 false 不触发显示。 |
| **来源状态** | **原件未规定**：忙时写入行为、未配置握手下的端口行为、RESET 期间与显示的交叠均未在手册或数据表中规定。 |
| **实现关系** | **模拟约定**：忙时覆写、未配置时不触发显示、RESET 清零 DDR 自然阻止显示写入选通。均为合理工程选择但非原件保证。 |

## 原件冲突与未闭合项

1. **PB7 极性（印刷页 7 vs 页 8）**：页 7 原理图连线 + monitor 机器码一致指示 PB7=1=忙；页 8 文字摘要相反（"1 equals ready, 0 equals busy"）。本核对以连线+机器码为准；页 8 文字记为原件内部冲突。

2. **PIA 地址别名（$D014 等）**：原图推导确认 `$D014` 应选中 Port A。当前实现未支持。不影响 Woz Monitor 功能验证但阻止声明地址译码完整。

3. **Port A/B 读回差异**：MC6820/MC6821 两端口读回行为不同，当前对称实现。Apple I 具体配置下碰巧吻合（Port B=输出读 OR，Port A=输入读引脚），但不是正确建模。

4. **CB2 输出模式**：Apple I 使用 CB2 作为输出握手，当前实现绕过 CB2 直接操作 PB7/CB1。

5. **DRAM 刷新 Φ2 门控**：用户选定的保留近似，不与 RDY 混淆。

6. **固定显示延迟**：`cycles_per_char` 近似不随移位寄存器实际位置变化。

7. **上电屏幕初始状态**：当前为空格而非移位寄存器随机图案。

8. **开路总线初始值**：`last_read` 初始 0 为模拟约定，真实硬件浮空无此保证。

9. **MC6820 vs MC6821 命名**：历史硬件为 6820；代码以 6821 命名。寄存器模型兼容但时序参数和 RESET 细节不能互换。

以上未闭合项均为独立可追踪缺口，不合并为一。补齐任意一项不自动关闭其余；M3 整体验收直到全部资料核对完成且所有已知差异已解决或明确记录为保留近似后才勾选。