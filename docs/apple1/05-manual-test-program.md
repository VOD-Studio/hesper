# 程序 5：1976 年原始 Operation Manual 测试程序（历史真实程序，非本项目原创）

这一篇跟 01～04 号不一样：那四篇是本项目为了演示"跑一段自己的机器码"这套技巧而编写的教学示例；这一篇是逐字节复刻自 Apple-1 官方 *Operation Manual* (1976) 第一节 "GETTING THE SYSTEM RUNNING" 里的 **TEST PROGRAM**——真实历史上，1976 年买到 Apple 1 裸板、自己接好键盘/显示器/电源之后,手册要求你做的第一件事就是手敲这 11 个字节进去跑一遍,以验证整机（键盘输入、显示输出、电源）接对了没有,不是本项目原创的例子。

## 来源与核对

- 原始出处：*Apple-1 Operation Manual* (1976)，仓库已在 [`references.md`](../references.md#apple-i) 里引用同一份手册。OCR 全文见 <https://archive.org/download/Apple-1_Operation_Manual_1976_Apple_a/Apple-1_Operation_Manual_1976_Apple_a_djvu.txt>（"TEST PROGRAM" 一节）。
- 原始扫描是打字机排版，OCR（tesseract）把不少十六进制数字识别错了（比如把 `0` 认成 `@`/`G`/`O`），所以下面的字节序列不是直接照抄 OCR，而是与两份独立转录交叉核对后确认的：
  - Applefritter 论坛帖子，Mike Willegal 指出该程序在手册第 2 页，只做"examine（查内存）、modify（改内存）、run（跑程序）"三件事：<https://www.applefritter.com/content/programming-woz-monitor>
  - KIM Uno 项目的 Apple-1 使用说明，逐字节给出同一段程序：<https://obsolescence.wixsite.com/obsolescence/kim-uno-apple-1>
- 手册原文对预期行为的描述（英文原文）：

  > THE PROGRAM SHOULD THEN PRINT OUT ON THE DISPLAY A CONTINUOUS STREAM OF ASCII CHARACTERS. TO STOP THE PROGRAM AND RETURN TO THE SYSTEM MONITOR, HIT THE "RESET" BUTTON. TO RUN AGAIN, TYPE: R (RET).

  这段描述已经在 Hesper 上实际复现，见下方"实测输出"，行为完全对得上——程序会连续不断地滚屏输出字节值，只能靠 RESET 打断，`R` 命令能再跑一次。

## 汇编列表

| 地址 | 字节 | 指令 | 说明 |
| --- | --- | --- | --- |
| `0300` | `A9 00` | `LDA #$00` | A ← 0，作为要打印的第一个字节值 |
| `0302` | `AA` | `TAX` | 循环入口：X ← A（当前要打印的值） |
| `0303` | `20 EF FF` | `JSR $FFEF` | 调用 Woz Monitor 的 `ECHO`，打印 A |
| `0306` | `E8` | `INX` | X 加 1 |
| `0307` | `8A` | `TXA` | A ← X，准备打印下一个值 |
| `0308` | `4C 02 03` | `JMP $0302` | 回到循环入口，没有任何退出条件 |

X 是 8 位寄存器，加到 `$FF` 后再 `INX` 会自动回卷到 `$00`——这就是手册说的"连续不断的 ASCII 字符流"：程序会不断打印 `$00, $01, ..., $FF, $00, $01, ...`，包括控制字符，永远不停，直到你按 RESET。

## 输入步骤

在 `\` 提示符后输入（按 Enter 结束）：

```
300: A9 00 AA 20 EF FF E8 8A 4C 02 03
```

（原始手册用 `@` 表示十六进制的零，专门注明"不是字母 O"；本仓库和 Hesper 的十六进制都用阿拉伯数字 `0`，字节本身不受影响。）

## 运行

```
300R
```

## 实测输出（用 `--max-cycles 700000` 截断；`cat -v` 显示不可见控制字符）

```
\
300: A9 00 AA 20 EF FF E8 8A 4C 02 03

0300: 00
300R

0300: A9^@^A^B^C^D^E^F^G^H	
^K^L
^N^O^P^Q^R^S^T^U^V^W^X^Y^Z^[^\^]^^^_ !"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\]^_`abcdefghijklmnopqrstuvwxyz{|}~^?^@^A^B^C...
[max cycles reached: 706000]
```

`^@`～`^_` 是 `$00`～`$1F` 的控制字符（`cat -v` 用 caret 记法把不可打印字节显示出来；真实终端上这些字节大多不可见或会触发副作用，比如 `$09`＝Tab、`$0A`＝换行、`$0D`＝CR）；随后是 `$20`（空格）到 `$7E`（`~`）的可打印 ASCII，`^?` 是 `$7F`（DEL）；打印完 `$FF` 后回卷到 `$00` 重新开始，循环不止，跟手册原文完全一致。

## 说明：这是官方给"新机器测试"用的，不是给你学编程用的示例

手册原文很直白：接好键盘、显示器、电源之后，敲这 11 个字节、`300R` 跑一下，如果屏幕上真的滚出一片字符，就说明键盘输入、显示输出、电源三者都接对了——它不测试 CPU 的大部分指令，只是当年"整机能不能用"的最低验收，所以手册管它叫 "TEST PROGRAM"，不是教学代码。跟 01～04 号一样，退出方式只有物理 RESET（Hesper 里是关掉 CLI 进程或用 `--max-cycles`/`Ctrl+C` 截断）。

上一个：[程序 4：无限循环打印 0123456789](04-counting-loop.md)
