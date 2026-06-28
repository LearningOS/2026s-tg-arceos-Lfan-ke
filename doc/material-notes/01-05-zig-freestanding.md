# Zig 裸机（Freestanding / no_std）

> **版本：Zig 0.16.0 / 0.17.0-dev**
> 裸机 = 没有操作系统、没有 C 运行时、没有标准 I/O——ku-sbi 的运行环境。

---

## 0. 总览

| 特性 | 标准程序 (`std`) | 裸机 (`freestanding`) |
|------|-----------------|----------------------|
| OS 支持 | 有 | 无 |
| 内存分配器 | GPA / ArenaAllocator | 自己实现 / FixedBufferAllocator |
| I/O | `std.io` | 直接写寄存器 / SBI ecall |
| 入口点 | `main()` | `_start` / 自定义 |
| panic 处理 | 默认打印 + 退出 | 必须自己提供 |
| stdlib 可用部分 | 全部 | 大部分纯计算部分可用 |

---

## 1. 目标三元组（Target Triple）

Zig 用 `-target` 指定目标平台，格式：`arch-os-abi`

```sh
# RISC-V 64 裸机
zig build-exe main.zig -target riscv64-freestanding-none

# ARM Cortex-M4 裸机
zig build-exe main.zig -target thumb-freestanding-eabihf

# x86_64 裸机（比如 bootloader）
zig build-exe main.zig -target x86_64-freestanding-none
```

`build.zig` 中指定：

```zig
const target = b.resolveTargetQuery(.{
    .cpu_arch = .riscv64,
    .os_tag   = .freestanding,
    .abi      = .none,
});
const exe = b.addExecutable(.{
    .name   = "ku-sbi",
    .root_source_file = b.path("src/main.zig"),
    .target = target,
    .optimize = .ReleaseSmall,
});
```

---

## 2. 最小裸机程序

### 2.1 入口点

裸机没有 `main()`，入口是链接器指定的符号（通常 `_start`）：

```zig
// src/start.zig
const std = @import("std");

// 导出 _start 符号（链接器入口）
export fn _start() callconv(.naked) noreturn {
    // 设置栈指针（汇编）
    asm volatile (
        \\la sp, _stack_top
        \\j main_zig
        ::: "sp"
    );
}

export fn main_zig() noreturn {
    // 初始化 BSS
    clearBss();
    // 跳转到 Zig 业务逻辑
    @import("main.zig").run();
    unreachable;
}

fn clearBss() void {
    // _bss_start/_bss_end 由链接脚本定义
    const bss_start = @extern([*]volatile u8, .{ .name = "_bss_start" });
    const bss_end   = @extern([*]volatile u8, .{ .name = "_bss_end" });
    const len = @intFromPtr(bss_end) - @intFromPtr(bss_start);
    @memset(bss_start[0..len], 0);
}
```

### 2.2 RISC-V 裸机入口（SBI 固件场景）

```zig
// src/start.zig — ku-sbi 风格入口
export fn _start() callconv(.naked) noreturn {
    asm volatile (
        // 只让 hart 0 继续，其他 hart 等待
        \\csrr t0, mhartid
        \\bnez t0, .Lwait
        // 设置机器模式栈（M-mode stack）
        \\la   sp, _m_stack_top
        // 清 BSS
        \\call _clear_bss
        // 跳转到 Zig 初始化
        \\call sbi_init
        \\j    .Lwait
        \\.Lwait:
        \\  wfi
        \\  j .Lwait
        ::: "t0", "sp", "ra"
    );
}
```

---

## 3. Panic Handler（必须提供）

裸机程序必须提供 `panic` 函数，否则链接报错：

```zig
// src/panic.zig
const std = @import("std");

// 必须导出此函数名，zig 运行时会调用它
pub fn panic(
    msg: []const u8,
    error_return_trace: ?*std.builtin.StackTrace,
    ret_addr: ?usize,
) noreturn {
    _ = error_return_trace;
    _ = ret_addr;

    // 裸机下无法用 std.io，直接写 UART 或 SBI console
    sbiConsolePuts("PANIC: ");
    sbiConsolePuts(msg);
    sbiConsolePuts("\n");

    // 停机
    while (true) {
        asm volatile ("wfi");
    }
}

fn sbiConsolePuts(s: []const u8) void {
    for (s) |c| {
        // SBI DBCN EID=0x4442434E, FID=0
        _ = sbiCall(0x4442434E, 0, 1, @intFromPtr(&c), 0, 0, 0, 0);
    }
}

fn sbiCall(eid: usize, fid: usize, a0: usize, a1: usize, a2: usize,
           a3: usize, a4: usize, a5: usize) struct { err: isize, val: usize } {
    var err: isize = undefined;
    var val: usize = undefined;
    asm volatile ("ecall"
        : [err] "={a0}" (err),
          [val] "={a1}" (val),
        : [a0]  "{a0}" (a0),
          [a1]  "{a1}" (a1),
          [a2]  "{a2}" (a2),
          [a3]  "{a3}" (a3),
          [a4]  "{a4}" (a4),
          [a5]  "{a5}" (a5),
          [fid] "{a6}" (fid),
          [eid] "{a7}" (eid),
        : "memory"
    );
    return .{ .err = err, .val = val };
}
```

在 `build.zig` 中注册 panic handler：

```zig
exe.root_module.addImport("panic", b.createModule(.{
    .root_source_file = b.path("src/panic.zig"),
}));
```

或者直接在根文件中：
```zig
// src/root.zig（主模块）
pub const panic = @import("panic.zig").panic;
```

---

## 4. 链接脚本

### 4.1 最小 RISC-V 链接脚本

```ld
/* linker.ld */
OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY {
    /* QEMU virt 机器: OpenSBI 占用 0x80000000-0x80200000 */
    /* S-mode 固件加载到 0x80200000 */
    RAM (rwx) : ORIGIN = 0x80200000, LENGTH = 128M
}

SECTIONS {
    . = ORIGIN(RAM);

    .text : {
        KEEP(*(.text.entry))   /* 入口必须在最前 */
        *(.text .text.*)
    } > RAM

    .rodata : {
        *(.rodata .rodata.*)
    } > RAM

    .data : {
        *(.data .data.*)
    } > RAM

    .bss (NOLOAD) : {
        _bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        _bss_end = .;
    } > RAM

    /* 栈：向下增长，4KB 对齐 */
    . = ALIGN(4096);
    _stack_bottom = .;
    . += 64K;
    _stack_top = .;
}
```

在 `build.zig` 中应用：
```zig
exe.setLinkerScriptPath(b.path("linker.ld"));
```

---

## 5. 内联汇编

Zig 的内联汇编语法（AT&T 风格）：

```zig
// 基本格式
asm volatile (
    \\<汇编指令>
    : [output] "=<constraint>" (output_var)     // 输出
    : [input]  "<constraint>"  (input_expr)      // 输入
    : "clobber1", "clobber2"                     // 破坏寄存器
);
```

### 5.1 RISC-V CSR 读写

```zig
// 读 mhartid
fn readHartId() usize {
    return asm volatile ("csrr %[ret], mhartid"
        : [ret] "=r" (-> usize)
    );
}

// 读 mstatus
fn readMstatus() usize {
    return asm volatile ("csrr %[ret], mstatus"
        : [ret] "=r" (-> usize)
    );
}

// 写 mstatus
fn writeMstatus(val: usize) void {
    asm volatile ("csrw mstatus, %[val]"
        :: [val] "r" (val)
        : "memory"
    );
}

// CSR set bits（csrs）
fn csrSet(comptime csr: []const u8, bits: usize) void {
    asm volatile ("csrs " ++ csr ++ ", %[bits]"
        :: [bits] "r" (bits)
        : "memory"
    );
}

// CSR clear bits（csrc）
fn csrClear(comptime csr: []const u8, bits: usize) void {
    asm volatile ("csrc " ++ csr ++ ", %[bits]"
        :: [bits] "r" (bits)
        : "memory"
    );
}
```

### 5.2 WFI / MRET / SFENCE

```zig
fn wfi() void {
    asm volatile ("wfi" ::: "memory");
}

fn mret() noreturn {
    asm volatile ("mret");
    unreachable;
}

fn sfenceVma() void {
    asm volatile ("sfence.vma" ::: "memory");
}

fn sfenceVmaAddr(vaddr: usize) void {
    asm volatile ("sfence.vma %[addr], zero"
        :: [addr] "r" (vaddr)
        : "memory"
    );
}
```

### 5.3 SBI ecall 封装（裸机调用 SBI）

```zig
pub const SbiRet = struct {
    err: isize,
    val: usize,
};

pub fn ecall(eid: usize, fid: usize, args: struct {
    a0: usize = 0,
    a1: usize = 0,
    a2: usize = 0,
    a3: usize = 0,
    a4: usize = 0,
    a5: usize = 0,
}) SbiRet {
    var err: isize = undefined;
    var val: usize = undefined;
    asm volatile ("ecall"
        : [err] "={a0}" (err),
          [val] "={a1}" (val),
        : [a0]  "{a0}" (args.a0),
          [a1]  "{a1}" (args.a1),
          [a2]  "{a2}" (args.a2),
          [a3]  "{a3}" (args.a3),
          [a4]  "{a4}" (args.a4),
          [a5]  "{a5}" (args.a5),
          [fid] "{a6}" (fid),
          [eid] "{a7}" (eid),
        : "memory"
    );
    return .{ .err = err, .val = val };
}
```

---

## 6. 裸机可用的 stdlib 部分

没有 OS 不等于没有 stdlib。以下模块在 `freestanding` 下可用：

| 模块 | 可用性 | 说明 |
|------|--------|------|
| `std.mem` | ✅ 全部 | 内存操作不依赖 OS |
| `std.math` | ✅ 全部 | 纯计算 |
| `std.fmt` | ✅ 全部 | 格式化（需要 writer） |
| `std.sort` | ✅ 全部 | 排序 |
| `std.meta` | ✅ 全部 | 元编程 |
| `std.builtin` | ✅ 全部 | 编译期查询 |
| `std.heap.FixedBufferAllocator` | ✅ | 基于栈/静态缓冲区 |
| `std.ArrayListUnmanaged` | ✅ | 配合 FixedBufferAllocator |
| `std.AutoHashMap` | ✅ | 配合 FixedBufferAllocator |
| `std.io.getStdOut()` | ❌ | 需要 OS |
| `std.heap.page_allocator` | ❌ | 需要 OS mmap/brk |
| `std.Thread` | ❌ | 需要 OS |
| `std.fs` | ❌ | 需要 OS |
| `std.process` | ❌ | 需要 OS |

### 裸机格式化输出（写到自定义 Writer）

```zig
// 将 UART 包装为 Writer
const UartWriter = struct {
    pub fn write(_: UartWriter, bytes: []const u8) !usize {
        for (bytes) |b| {
            uartPutByte(b); // 直接写 UART FIFO
        }
        return bytes.len;
    }

    pub fn writeByte(self: UartWriter, b: u8) !void {
        _ = try self.write(&.{b});
    }

    // 实现 anytype writer 接口
    pub fn print(self: UartWriter, comptime fmt: []const u8, args: anytype) !void {
        return std.fmt.format(self, fmt, args);
    }
};

var uart_writer = UartWriter{};
uart_writer.print("hart {d} online\n", .{hart_id}) catch {};
```

---

## 7. volatile 与内存屏障

裸机编程必须防止编译器优化掉寄存器访问：

```zig
// MMIO 寄存器用 *volatile
const UART_BASE: usize = 0x10000000;
const uart_data = @as(*volatile u8, @ptrFromInt(UART_BASE));

fn uartPutByte(c: u8) void {
    uart_data.* = c;
}

fn uartGetByte() u8 {
    return uart_data.*;
}

// 内存屏障
fn memoryFence() void {
    asm volatile ("fence" ::: "memory");
}

fn fullFence() void {
    asm volatile ("fence rw, rw" ::: "memory");
}
```

---

## 8. 条件编译（平台分支）

```zig
const builtin = @import("builtin");

pub fn platformInit() void {
    switch (builtin.cpu.arch) {
        .riscv64 => riscv64Init(),
        .aarch64 => aarch64Init(),
        .x86_64  => x86_64Init(),
        else     => @compileError("unsupported architecture"),
    }
}

fn riscv64Init() void {
    // RISC-V 特有初始化
    csrSet("mstatus", 1 << 3); // MIE
}

// 在 freestanding 下禁用某些功能
fn getHeapAllocator() std.mem.Allocator {
    if (builtin.os.tag == .freestanding) {
        return fixed_buf_alloc.allocator();
    } else {
        return gpa.allocator();
    }
}
```

---

## 9. 段属性与 linksection

```zig
// 将变量放入特定段
const boot_info linksection(".boot_info") = struct {
    magic: u32 = 0xDEADBEEF,
    version: u32 = 1,
}{};

// 将函数放入特定段
export fn critical_isr() callconv(.naked) void linksection(".isr_vector") {
    // 中断服务例程
}

// 放入 .rodata.sbi_spec
const SBI_SPEC_VERSION: u32 linksection(".rodata.sbi_spec") = 0x00030000;
```

---

## 10. 裸汇编：全裸函数与全局汇编

### 10.1 为什么需要裸函数

普通函数被编译器包装了 prologue/epilogue（保存 callee-saved 寄存器、分配局部变量栈帧、恢复寄存器、ret）。裸函数没有任何包装——函数体里的汇编就是最终机器码。

两个典型场景：
- **Boot 入口 `_start`**：CPU 上电时没有合法栈，必须用汇编手动设置 `sp` 再调用 Zig
- **陷阱入口 `trapEntry`**：中断/ecall 时寄存器全是 S-mode 内核状态，必须手动保存全部寄存器后才能进入 Zig

### 10.2 callconv(.naked)

```zig
export fn _start() callconv(.naked) noreturn {
    asm volatile (
        \\csrr t0, mhartid        // 读当前 hart ID
        \\bnez t0, .Lwait         // 非 boot hart 挂起等待
        \\la   sp, _stack_top     // 设置 M-mode 栈
        \\call zigStart           // 调用 Zig 初始化
        \\.Lwait:
        \\  wfi
        \\  j .Lwait
        ::: "t0", "sp", "ra"
    );
    unreachable;
}
```

**关键约束：**
- `callconv(.naked)` 函数体**只能包含内联汇编**，不能有任何 Zig 语句（变量声明、`if`、函数调用等）
- 必须是 `noreturn` 或通过汇编跳走（不能依赖编译器生成 `ret`）
- LLVM 会报错若你在裸函数体里写了非汇编代码

### 10.3 陷阱帧：完整寄存器保存


```zig
export fn trapEntry() callconv(.naked) noreturn {
    asm volatile (
        \\ addi sp, sp, -256      // 分配陷阱帧（32 寄存器 × 8 字节）
        \\ sd ra,   0*8(sp)
        \\ sd t6,  30*8(sp)       // 先保存 t6，腾出来当临时寄存器
        \\ addi t6, sp, 256       // t6 = 进入前的原始 sp（-256 之前）
        \\ sd t6,   1*8(sp)       // 保存正确的 sp（不是 -256 后的值！）
        \\ sd gp,   2*8(sp)
        \\ sd tp,   3*8(sp)
        // ... 其余寄存器 ...
        \\ mv   a0, sp            // 陷阱帧指针作为第一参数
        \\ csrr a1, mepc          // 异常 PC 作为第二参数
        \\ csrr a2, mcause        // 原因作为第三参数
        \\ call trapHandler       // 调用 Zig 处理函数（返回新 mepc）
        \\ csrw mepc, a0          // 更新返回地址
        \\ ld ra,   0*8(sp)
        // ... 恢复所有寄存器 ...
        \\ ld t6,  30*8(sp)       // 恢复真实 t6
        \\ ld sp,   1*8(sp)       // 最后恢复原始 sp
        \\ mret
    );
    unreachable;
}
```

> **sp 顺序陷阱**：必须先 `sd t6, 30*8(sp)` 保存真实 t6，
> 再 `addi t6, sp, 256` 计算原始 sp，最后 `sd t6, 1*8(sp)`。
> 若直接 `sd sp, 1*8(sp)` 则保存的是 `-256` 后的错误值，
> 每次 ecall 返回 Linux 的 sp 就少 256 字节，最终 stack overflow。

### 10.4 全局汇编（comptime asm）

在模块级别注入汇编，不属于任何函数：

```zig
// 在 .text.entry 段注入跳转指令
// 背景：QEMU ROM 固定跳到 0x80000000，但 LLVM 会把 compiler-rt 放在 .text 开头
// 通过在 .text.entry 段注入 j _start，再由链接脚本把它排第一，确保 0x80000000 处是合法入口
comptime {
    asm (
        \\.pushsection .text.entry, "ax", @progbits
        \\.balign 4
        \\j _start
        \\.popsection
    );
}
```

链接脚本配合：
```ld
SECTIONS {
    .text : {
        KEEP(*(.text.entry))   /* 排第一，确保 0x80000000 是 j _start */
        *(.text .text.*)
    }
}
```

注意：`comptime { asm(...) }` 不能有输入/输出约束（无法传递变量），只用于静态代码注入。

### 10.5 三种汇编方式对比

| 特性 | `asm volatile(...)` | `callconv(.naked)` | `comptime { asm(...) }` |
|------|--------------------|--------------------|------------------------|
| 位置 | 函数体内 | 函数体就是它 | 模块顶层（全局） |
| 调用约定 | 编译器管理栈帧 | 无栈帧，纯汇编 | 直接注入目标文件 |
| 寄存器保存 | 编译器 + clobber 列表 | 完全手动 | 无（直接汇编） |
| 输入/输出变量 | 支持（约束语法） | 支持（asm 在裸函数内） | 不支持 |
| 典型用途 | CSR 读写，fence，单条指令 | `_start`, `trapEntry`, 中断向量 | 段注入，链接器技巧 |
| 返回 | 正常函数返回 | 汇编 `ret`/`mret` 或跳走 | N/A |

---

## 11. 完整裸机程序框架（ku-sbi 骨架）

```zig
// src/main.zig
const std     = @import("std");
const builtin = @import("builtin");

// 确保在裸机目标上编译
comptime {
    if (builtin.os.tag != .freestanding) {
        @compileError("ku-sbi must be compiled for freestanding target");
    }
}

// 导出 panic handler
pub const panic = @import("panic.zig").panic;

// 固定缓冲区分配器（全局，裸机用）
var heap_buf: [256 * 1024]u8 = undefined; // 256KB 堆
var fba = std.heap.FixedBufferAllocator.init(&heap_buf);
pub const allocator = fba.allocator();

// SBI 初始化入口（从 _start 汇编调用）
export fn sbi_init(hart_id: usize, fdt_addr: usize) noreturn {
    _ = fdt_addr;

    if (hart_id == 0) {
        primaryHartInit();
    } else {
        secondaryHartInit(hart_id);
    }

    // 跳转到 S-mode 内核（由 HSM 扩展管理）
    unreachable;
}

fn primaryHartInit() void {
    consoleInit();
    log("ku-sbi: primary hart online\n", .{});
    // TODO: 初始化各 SBI 扩展
}

fn secondaryHartInit(hart_id: usize) void {
    log("ku-sbi: hart {d} online\n", .{hart_id});
}

fn consoleInit() void {
    // TODO: 初始化 UART
}

var log_buf: [256]u8 = undefined;
fn log(comptime fmt: []const u8, args: anytype) void {
    const s = std.fmt.bufPrint(&log_buf, fmt, args) catch return;
    for (s) |c| {
        // SBI 规范 v3.0 DBCN EID=0x4442434E 写字节
        _ = @import("sbi/ecall.zig").ecall(0x4442434E, 0, 1, @intFromPtr(&c), 0, .{});
    }
}
```

---

## 练习

### 练习 11：CSR 操作

```zig
// TODO: 实现以下 CSR 操作函数
// mstatus 寄存器的 MIE 位（bit 3）控制 M-mode 全局中断使能

const MSTATUS_MIE: usize = 1 << 3;

// 使能 M-mode 全局中断
fn enableMachineInterrupt() void {
    // 填充这里（使用 csrs 指令置位 MIE）
}

// 禁用 M-mode 全局中断，返回旧值
fn disableMachineInterrupt() usize {
    // 填充这里（读取 mstatus，然后用 csrc 清除 MIE 位，返回旧的 mstatus）
    return 0;
}

// 恢复中断状态（配合 disableMachineInterrupt 使用）
fn restoreMachineInterrupt(old_mstatus: usize) void {
    // 填充这里
    _ = old_mstatus;
}
```

### 练习 12：MMIO 访问

```zig
// TODO: 为 NS16550A UART 实现最简单的初始化和字节收发
// UART 基地址（QEMU virt）：0x10000000
// 寄存器偏移：
//   +0: RBR（读）/ THR（写）— 数据
//   +3: LCR — 线控（bit7=DLAB，bits1:0=字长 0b11=8bit）
//   +5: LSR — 状态（bit5=THRE 发送空，bit0=DR 数据就绪）

const UART_BASE: usize = 0x10000000;

fn uartInit() void {
    // 填充这里：设置 8-N-1 模式（8位数据，无奇偶校验，1停止位）
}

fn uartPutc(c: u8) void {
    // 填充这里：等待 THRE，然后写 THR
    _ = c;
}

fn uartGetc() u8 {
    // 填充这里：等待 DR，然后读 RBR
    return 0;
}
```

---

> 下一节：[01-06-zig-async.md](01-06-zig-async.md) — 异步与协程
