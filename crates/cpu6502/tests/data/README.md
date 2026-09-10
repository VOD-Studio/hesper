# 自包含测试数据

`opcodes.txt` 是本项目依据 MOS 6500-50A 附录 B 独立手写的规格；不能从 CPU 解码器生成，以免共享实现错误。

`singlestep-decimal.txt` 是 [SingleStepTests/65x02](https://github.com/SingleStepTests/65x02) 提交 `2f6980a2d95757486c7bee24355c360e40e2a224` 中 `6502/v1/69.json` 和 `6502/v1/e9.json` 的 **8 条选定 NMOS 十进制用例**，各 4 条。按文件原始顺序选择 D=1 且 A 或操作数包含无效 BCD 数字的前 4 条，保留输入 A／操作数／C、输出 A／NVZC 以及用于定位记录的初始 PC。测试重新安置指令，I 固定为 1、D 固定为 1；其余存储标志输出由 NVZC 与 I/D 合成。

适用许可证 MIT，原文保存在 [LICENSE-SingleStepTests](LICENSE-SingleStepTests)。没有采用 `nes6502`、`65c02` 数据，也没有复制模拟器实现。

运行：`cargo test -p hesper-cpu6502 --test arithmetic selected_nmos_decimal_vectors`。同时包含在 debug／release workspace 测试中，无需联网。验证所选输入的寄存器结果及立即数指令 2 周期；**不比较原数据完整总线序列，也不表示通过两个 JSON 文件或整个套件**。更大范围的外部一致性验证仍属 M2。

## M2.1 原始格式样例

`singlestep/` 包含同一固定提交的 21 个 NMOS opcode 文件，各取原始顺序前 32 条，共 **672 条**。只改变 JSON 排版，不修改地址、寄存器、内存、预期结果或周期事件。选择规则、上游文件和夹具各自的 SHA-256、源／选定用例数量均记录在 [manifest.json](singlestep/manifest.json)。同样适用上面的 MIT 许可证；这批文件不能代表 21 个文件的全量或整个测试套件。

```sh
# 普通回归不联网，验证文件哈希、格式及全部 672 条结果
cargo test -p hesper-cpu6502 --test external
# 宿主工具重放全部夹具或其中一条，索引从 0 开始
cargo run -p hesper-cpu6502 --example singlestep
cargo run -p hesper-cpu6502 --example singlestep -- --opcode 69 --case-index 0
# 显式联网准备上游文件到忽略的缓存，并核对选取过程；不覆盖仓库文件
python3 tools/prepare_singlestep.py
```

验证维度：原始 PC/A/X/Y/SP、P 的六个存储标志、最终 RAM（含对未列入最终状态的写入检查）、周期数量。仅对 P 的 B/位 5 表示进行规范化；不删除 N/V/Z/C 比较。M2.3 起**逐周期比较地址、数据及读写方向**，并要求每次 `cycle` 返回的记录等于实际 Bus 活动。测试检查不在源用例 RAM 列表中的访问，失败报告包含数据提交、opcode、用例索引／名称、初始状态、首个差异及重放命令。

测试入口对 JSON 数值越界、重复内存地址、缺失 opcode、用例数量／文件哈希错误、缺失文件和零匹配选择明确失败。依赖 `serde`、`serde_json`、`sha2` 仅属于 CPU 包的开发依赖；CPU 库自身没有运行依赖。


## M2.2 全量官方指令与汇编测试

固定提交与 M2.1 相同。[full-manifest.json](singlestep/full-manifest.json) 列出全部 151 个官方 opcode 文件，每份 10000 条，共 **1510000 条**，原始字节不改写。完整 JSON 缓存在 `.cache/cpu6502/`，不提交到仓库；显式全量命令缺数据、哈希不符、清单缩减或零匹配都会失败，不自动跳过。

```sh
python3 tools/prepare_singlestep.py --full
cargo run -p hesper-cpu6502 --example singlestep --release -- --full
# 定位全量数据中的单条用例，不代表运行整个 corpus
cargo run -p hesper-cpu6502 --example singlestep --release -- --full --opcode 69 --case-index 0
python3 tools/prepare_klaus.py
cargo run -p hesper-cpu6502 --example functional --release
cargo run -p hesper-cpu6502 --example functional --release -- --decimal
```

Klaus 数据固定为 [`7954e2dbb49c469ea286070bf46cdd71aeb29e4b`](https://github.com/Klaus2m5/6502_65C02_functional_tests/tree/7954e2dbb49c469ea286070bf46cdd71aeb29e4b)。准备脚本校验源码、许可证、listing、镜像的 SHA-256，只写忽略的缓存。官方功能测试源码和镜像是 GPL-3.0-or-later；Bruce Clark 十进制源码明确为 public domain。这不决定 Hesper 自身许可证，不将这些测试代码链接进 CPU 库。

| 程序 | 构建与入口 | 宿主成功／失败判据 | 镜像 SHA-256 |
| --- | --- | --- | --- |
| Klaus functional | 使用上游 65536 字节镜像和对应 AS65 1.42 listing（`-l -m -s2 -w -h0`）；源码配置 `ROM_vectors=1, load_data_direct=1, I_flag=3, disable_selfmod=0, disable_decimal=0`；加载 `$0000`，PC=`$0400` | 仅 `$3469` 为成功；其他自循环为失败 | `fa12bfc761e6f9057e4cc01a665a7b800ff01ae91f598af1e39a1201d01953fd` |
| Bruce Clark decimal | `cputype=0, vld_bcd=0, chk_a/n/v/z/c=1`；零页 `$0000`，代码加载／入口 `$0200` | 到 `$024B`（DONE）且 `$000B`（ERROR）为 0；在结束宏 `$DB` 前停止 | `03798ab778456cc350044fdbe28b4078278648892712b994cdbdda09018674e7` |

两个 runner 使用上游 monitor 入口契约注入 PC、SP=`FD`、P=`20`，并非硬件 RESET；原创 CLI 演示仍从复位向量启动。每个程序限定 100000000 条指令／400000000 周期；任何错误或超限退出失败。

十进制源码仅转换汇编器伪指令并开启全部检查，原有运算及预期计算代码保留。脚本在缓存内构建 [cc65 V2.19](https://github.com/cc65/cc65/releases/tag/V2.19) 的 ca65/ld65，提交 `555282497c3ecf8b313d87d5973093af19c35bd5`，源码包 SHA-256=`62c77f00ef4141153a0ddecef06ca086c11c68f14d022beadeaf353d1d833ff1`。该版本工具实际报告 V2.18（上游发布说明已注明），BUILD_ID 固定为 `Git 55528249`；保留源码包内的 zlib 风格 LICENSE，不安装到系统。需要 Python 3.12+、make 和本地 C 编译器。转换后源码哈希为 `586f6f2da4fc8763630f73211356c6de5f8d47cbc01cc38fddcd761a6ed3ec39`；脚本检查最终镜像及 TEST/DONE/ERROR 符号地址，可在缓存查看完整 listing。

M2.2 历史通过范围见 [verification.md](../../../../docs/verification.md)：指令状态／内存和 SingleStep 周期数量；此时尚未比较逐周期总线序列，也未运行 Klaus 中断程序。


## M2.4 引脚与中断程序（本地已验收）

`visual6502/pins.json` 包含本项目原创的 246 个短程序／事件时间表，在 [Visual6502 revD](https://github.com/trebonian/visual6502/tree/d8ecc129b34e0eaf320e0400fcf33329475bdb1e) 晶体管模型上实际生成的 24 周期总线观察（共 5904 周期），事件以半周期索引给出 IRQ/NMI/RDY/SO 的低有效断言／释放；覆盖读写等待、等待中断以及 SO 与分支、CLV、ADC、SBC、BIT、PLP、RTI 的相位重叠。初始 RAM 未单列的地址为 `$EA`；引导代码通过真实指令设置寄存器，再停在目标取指。CPU 测试恢复相同状态及 RAM，预期不是由 Hesper 生成。fixture SHA-256：`c2913d4ab2c52af44ae9b69c3647c630215c90485e25f6f254fed6d24d516ac7`。

```sh
# 离线快速比较固定观察
cargo test -p hesper-cpu6502 --test pins
# 显式准备外部模型，随后 Node 本地重新运行并核对固定 trace
python3 tools/prepare_visual6502.py
node tools/verify_visual6502.cjs --suite pins
# 构建保留全部 505 条原指令语句的 Klaus 中断程序，运行明确延迟配置
python3 tools/prepare_klaus.py
cargo run -p hesper-cpu6502 --example interrupt --release -- --feedback-delay 4
```

模型文件哈希见 [manifest.json](visual6502/manifest.json)。上游 `chipsim.js`、`macros.js`、`wires.js`、`nodenames.js` 带 MIT 许可；`segdefs.js` 标明 CC BY-NC-SA 3.0，原作者 Greg James、来源 www.visual6502.org；`transdefs.js` 文件本身没有单独许可声明。模型文件及其原有声明仅保留在缓存，未复制进 CPU 或提交网表。新增 fixture 是原创程序的数字观察，并记录上述来源；不据此给 Hesper 自身选择许可证。Node 仅为显式外部验证工具，普通 Cargo 测试不需要它。

Klaus 中断源码沿用前述固定版本／GPL-3.0-or-later，采用相同 ca65/ld65。保留默认 `I_port=$BFFC, I_ddr=0, I_drive=1, IRQ_bit=0, NMI_bit=1, I_filter=$7F, D_clear=0, load_data_direct=1, report=0`。只改汇编伪指令并添加成功地址符号；独立检查转换前后全部 505 条指令语句一致，错误陷阱保留。转换源码 SHA-256=`f2ce31cba447eef9ad0a292a5d616b85e4b1ba1b947abaf168d3c00fcad0b1d9`；65536 字节镜像 SHA-256=`ecc829d494fd1f4b4262ac5c58e8cb3925f7aa7570ce815e9a3214b6f66a3c58`。加载 `$0000`，入口 `$0400`，唯一成功地址 `$06F5`，预算 1000000 周期；后面的手动 65C02 WAI/STP 测试不执行。

反馈寄存器写入立即可读；引脚从下一周期起经过 `--feedback-delay` 个额外周期传递，范围 0～32。**4 周期额外延迟**配置实际通过，1049 个 step、3013 周期，NMI/IRQ/BRK 顺序计数 `[1,3,2]`。这是明确的测试设备配置，CPU 没有为该程序添加特殊逻辑。

**默认 0 延迟没有通过这个外部程序**：NMI 与 BRK 重叠，进入 `$075C` 的 B 位检查陷阱，退出失败。上游 `nmi_trap` 附近的源码明确注明真实 NMOS 也可能在这里失败；revD 场景验证了向量被抢占但 B=1 的行为。因此保留失败和正确 NMOS 行为，不关闭陷阱、修改 B 预期或把此配置报告为通过。延迟 1～3 同样触发该陷阱；5～10 则超出上游另一组响应时序预期，不宣称这些配置通过。

### 物理 RESET 对照

`visual6502/reset.json` 单独保存 **419 组／26816 个总线周期**的物理 RESET 观察，每组固定 64 周期。参考模型仍为上面的 revD 固定提交；新增预期直接来自原模型，未由 Hesper 生成。原有 `pins.json` 的 246 组观察及哈希保持不变。RESET 夹具 SHA-256：`5b78b26bf4294609d54a5184b34655f3f85c08282b8808a6f9d31697b335d257`，数量及总周期另记于同一 manifest。

| 扫描范围 | 场景数 |
| --- | ---: |
| NOP、LDA/STA abs、INC abs、PHA、JSR、PLA/PLP、RTS/RTI、BRK、IRQ/NMI 入口；逐半周期断言，分别保持 8／9 个半周期以覆盖两种释放相位 | 280 |
| 非零 A/X/Y、D/C 置位及 SP=`$01` 的回绕 | 1 |
| 持续断言及较晚释放 | 6 |
| NOP／STA／INC／BRK 开始和结束位置的 1～3 半周期短脉冲 | 48 |
| 释放同步、栈读取、向量读取及取指期间再次断言 | 20 |
| RDY 与 NOP／STA／INC 的 RESET 重叠，含两个向量读取周期 | 36 |
| IRQ／NMI 与 RESET 同步、栈、向量及取指窗口重叠 | 28 |

事件 `[half, pin, asserted]` 中 `res` 是原模型的低有效 RESET 引脚名；`true` 为断言，CPU 侧对应 `set_reset_line(true)`。索引 `2n` 在模型时钟上升前注入，`2n+1` 在下降前注入；总线元组为 `[address, data, read/write, SYNC]`，在上升后采集。CPU 比较也执行同一真实引导程序建立寄存器及残留数据通路状态，再恢复记录的完整初始 RAM（未单列字节为 `$EA`），与模型在引导后施加 RAM 覆盖一致；不从寄存器快照伪造内部锁存器。RESET 向量为 `$B134`，处理程序先向 `$2010/$2013/$2014` 写出 A/X/Y，再 PHP 保存尚未被 TSX/INX 改写的 P，计算入口 SP 并写至 `$2011`，最后将 PHP 栈映像写至 `$2012`，进入 `$B147` 自循环。该见证程序是观察工具，不是 CPU 的完成条件或伪 HALT。

```sh
# 重现两个完整套件；分别报告 pins 和 reset，不合并成 CPU 通过数
node tools/verify_visual6502.cjs
# 只重现 RESET，或精确重放一组观察
node tools/verify_visual6502.cjs --suite reset
node tools/verify_visual6502.cjs --suite reset --case reset-registers-stack-wrap
# 实际 CPU 对照全部 419 组 RESET；普通 workspace 测试也包含它
cargo test -p hesper-cpu6502 --test pins fixed_visual6502_revd_reset_traces_match_actual_bus_cycles -- --exact
node tools/verify_visual6502.cjs --suite pins --case nop-irq-0
# 显式重新生成一整个套件到临时路径，不自动覆盖固定预期
node tools/verify_visual6502.cjs --suite reset --record /tmp/hesper-reset-reference.json
```

`--case` 按完整场景名选择，零匹配报错；重复／缺值参数、未知套件、未指定单套件的 `--record`、同时使用 `--case` 和 `--record` 均报错。验证先校验夹具哈希再运行模型；缺数据、哈希错误、场景／周期差异均退出 1。差异报告提供首个场景、周期、预期／实际值及单场景重放命令；不更新预期来吞掉失败。

#### 已实现并通过的执行器验证点

下表为固定输入下的 **revD 观察**，周期／半周期索引均从 0 开始，不是全部 NMOS 修订或模拟电气脉冲的保证：

| 重放场景 | 固定观察 | 已实现的执行器状态 |
| --- | --- | --- |
| `reset-nop-0-8` | h0 断言、h8 释放：c1～5 读 `$8001`，c6 SYNC，c8～10 栈读，c11/12 向量读，c13 取处理程序 opcode | 断言／释放同步和保持状态；不能在释放时立即套用宿主七周期入口 |
| `reset-pulse-nop-0-1`／`reset-pulse-nop-1-1` | h0→h1 脉冲未触发复位，h1→h2 触发 | 下降相位采样；短脉冲必须跨采样点，而非 setter 调用即触发 |
| `reset-store-2-8`／`reset-store-4-8` | 前者 c3 对 `$2000` 的写被变为读；后者 c3 写仍发生 | 写抑制有同步延迟；已发生写入不能撤销 |
| `reset-store-6-8`／`reset-jsr-4-8` | 分别出现 `$EA00→$EAE9` 与 `$EA00→$EAFB→$EAEA` 的过渡读取 | 中间 PC／地址锁存不能简单冻结；这些具体地址不是程序特判常量 |
| `reset-pha-2-8`／`reset-brk-4-8` | PHA 已写栈，但 RESET 从 `$01EA` 开始栈读，见证 SP=`$E7`；BRK 已写 `$01FD/$01FC`，RESET 却重新从 `$01FD` 读，见证 SP=`$FA` | 内部栈地址、SP 寄存器和提交时机必须分开；不能把每次栈访问后的软件 SP 当作全部硬件状态 |
| `reset-rdy-nop-0`／`reset-rdy-nop-24` | 前者出现 `$8000→$EA01→$EAEA`；后者 c12～16 重读向量高字节 `$FFFD` | RDY、RESET 同步和向量锁存的组合状态 |
| `reset-nmi-overlap-20`／`reset-nmi-overlap-22` | h20 的 NMI 未在观察期服务；h22 的 NMI 保留至 RESET 后，经 `$FFFA/B` 进入。RESET 均使用 `$FFFC/D` | 分阶段的 NMI 消费／保留窗口，不能一律清空或抢占 RESET 向量 |
| `reset-registers-stack-wrap` | A/X/Y=`$12/$34/$56` 保留；栈读 `$0101/$0100/$01FF`，入口 SP=`$FE`；PHP 见证 `$3D`（含 B，实际 P=`$2D`） | 保留 NMOS 的 D/C，设置 I，显式处理 8 位栈回绕 |

`reset-held-0/1` 在各自 64 周期观察内没有读取 RESET 向量；没有用有限观察宣称无限时长的模拟电气保证。未排除 PHA/JSR/BRK 的暂态异常地址或寄存器影响，也未拿手册中的 don't-care 标记删除这些预期。

参考基线增量曾按用户选择只固定观察；当前已接入实际物理 RESET。`pins.rs` 保留哈希、固定数量、格式、事件范围及唯一性检查，并在 CPU 上完整重放全部 419 组，逐周期比较总线及处理程序见证写入。同期原有 246 组引脚对照保持通过，没有修改或删减固定预期。M2.4／M2 本地目标范围已验收；这不等于所有 NMOS 修订或全部指令×引脚相位组合均已穷举。


## 快速回归、全量验证与失败诊断

普通 `cargo test --workspace` 使用仓库里的 672 条单步样例、246 组 IRQ/NMI/RDY/SO 观察、419 组物理 RESET 观察和本地穷举／边界测试；两组引脚数据均实际驱动 CPU。不读取外部下载缓存、不运行 Node 模型。当前 workspace 共 91 个测试。

完整官方 JSON、Klaus 镜像与 Visual6502 模型需要显式执行上述准备命令。全量 SingleStep、functional、decimal、interrupt（`--feedback-delay 4`）验证 CPU；`node tools/verify_visual6502.cjs` 验证原模型能重现固定观察；`cargo test -p hesper-cpu6502 --test pins --release` 在检查夹具完整性后实际对照 CPU 的两个引脚套件。不能用 Node 重放替代 CPU 对照，反之也不能省略来源和哈希验证。准备命令验证来源哈希，运行命令缺数据或校验失败直接报错。

[快速 CI](../../../../.github/workflows/ci.yml) 在拉取 Rust 开发依赖后离线运行固定测试；[全量 CI](../../../../.github/workflows/full-cpu.yml) 只在 GitHub Actions 中手动运行 **Full CPU conformance** 时下载和执行外部数据。Node 26、Python 3.12+、make／C 编译器仅用于数据准备和参考模型，不成为 CPU 运行依赖。两份配置尚未远程运行验证。

外部执行失败报告保留最后 32 个真实总线周期及其指令前后状态，并给出当前执行阶段、引脚和中断锁存；格式化不读取 Bus。SingleStep 报告继续附版本、opcode、用例索引和重放命令。Klaus 默认 0 延迟已知失败仍退出 1，不以“预期失败”替代外部程序通过的声明。
