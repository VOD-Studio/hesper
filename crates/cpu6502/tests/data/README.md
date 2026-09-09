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
