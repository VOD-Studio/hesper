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

验证维度：原始 PC/A/X/Y/SP、P 的六个存储标志、最终 RAM（含对未列入最终状态的写入检查）、周期数量。仅对 P 的 B/位 5 表示进行规范化；不删除 N/V/Z/C 比较。当前**不比较完整总线序列**。测试检查不在源用例 RAM 列表中的访问，失败报告包含数据提交、opcode、用例索引／名称、初始状态、首个差异及重放命令。

测试入口对 JSON 数值越界、重复内存地址、缺失 opcode、用例数量／文件哈希错误、缺失文件和零匹配选择明确失败。依赖 `serde`、`serde_json`、`sha2` 仅属于 CPU 包的开发依赖；CPU 库自身没有运行依赖。
