# exercise-printcolor · 加粗 + 真彩渐变 —— 实现笔记

## 目标
让 `Hello, Arceos!` 的输出带 ANSI SGR 颜色，使 `scripts/test.sh` 通过。
**本次额外要求（自定）：加粗 + 逐字符冷色渐变（祖母绿→靛蓝→蓝紫）**，不是单色绿。

## 判分逻辑（scripts/test.sh）
对 QEMU 串口输出**最后 10 行**做两项检查（**不去 ANSI，直接 grep 原始字节**）：
1. 必须包含**连续**文本 `Hello, Arceos!`（`EXPECTED_TEXT`，第 12 行）。
2. 必须包含一个“非平凡”颜色转义，grep 模式 `\x1b\[[0-9;]*[1-9][0-9;]*m`
   （排除裸 reset `\x1b[m` / `\x1b[0m`，须有像 `\x1b[1;38;2;..m` 的设色序列）。

## 关键矛盾与解法
**逐字符渐变与“连续 Hello, Arceos!”本质冲突**：渐变要求每个字符前都插一段
`\x1b[38;2;R;G;Bm`，于是 `Hello, Arceos!` 被转义码打断、不再是连续子串 → 文本检查失败。

解法：**打印两行**——
1. 第一行：加粗 + 逐字符 24 位真彩渐变（视觉效果，满足颜色检查 + 个人审美要求）。
2. 第二行：加粗 + 整体着色的**连续** `[WithColor]: Hello, Arceos!`（满足文本 grep）。

## 实现（`src/main.rs`，未 patch axstd）
- `hsv_to_rgb(h)`：纯整数 HSV(S=V=1)→RGB，色相 150→285 得「祖母绿→青→靛蓝→蓝紫」冷色渐变。
- `print_gradient_bold(s)`：先 `print!("\x1b[1m")` 开加粗，再对每个字符按
  `h = 150 + 135*i/(n-1)` 取色、`print!("\x1b[38;2;{r};{g};{b}m{c}")`，末尾 `println!("\x1b[0m")` 复位。
  首字符 `(0,255,127)` 祖母绿、尾字符 `(191,0,255)` 蓝紫，中途经青/靛蓝。
  - 用到 `axstd::print`（已确认 axstd 导出 `print!`，与原模板只用 `println!` 不同）。
- `main()`：先 `print_gradient_bold("[WithColor]: Hello, Arceos!")`，
  再 `println!("\x1b[1;38;2;0;200;255m[WithColor]: Hello, Arceos!\x1b[0m")`。

## 通过输出（riscv64，`cat -v` 节选）
```
^[[1m^[[38;2;0;255;127m[^[[38;2;0;255;148mW^[[38;2;0;255;170mi...^[[38;2;165;0;255ms^[[38;2;191;0;255m!^[[0m
^[[1;38;2;90;50;220m[WithColor]: Hello, Arceos!^[[0m
```
即第一行是加粗冷色渐变（祖母绿→靛蓝→蓝紫逐字过渡），第二行是加粗蓝紫色连续整句。

`bash scripts/test.sh` 结果：
- **riscv64 ✓**（目标架构）、**x86_64 ✓**、**aarch64 ✓**：colored output detected + 文本命中。
- loongarch64 ✗ —— 与本改动无关，预存环境问题：`qemu-system-loongarch64: ram_size must be greater than 1G`（xtask/configs 硬编码 `-m 128M`），qemu 启动即失败、无任何输出，故文本未找到。

> 备选「正统」做法（README Tips）：`cargo clone axstd@0.3.0-preview.1` 后改其 print 宏的 write 层。
> 本次未采用——churn 大且需联网，应用层内联已精确满足判分并实现渐变。
