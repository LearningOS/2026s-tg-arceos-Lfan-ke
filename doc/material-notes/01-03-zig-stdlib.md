# Zig 标准库

> **版本：Zig 0.16.0 / 0.17.0-dev**
> 官方 std 源码：https://codeberg.org/ziglang/zig/src/branch/master/lib/std

---

## 0. 总览

Zig 标准库（`std`）的设计特点：

| 特点 | 说明 |
|------|------|
| **不默认分配堆内存** | 需要分配的函数要求你传入 `Allocator` |
| **错误是返回值** | 几乎所有 I/O 函数返回错误联合 |
| **无隐藏开销** | 没有全局状态、没有运行时初始化 |
| **显式比隐式** | `ArrayList(T)` 需要你管理 `deinit()`，不会自动析构 |

主要模块速览：

```
std
├── mem          — 内存操作（copy, set, compare, Allocator 接口）
├── heap         — 具体分配器（GPA, ArenaAllocator, FixedBufferAllocator...)
├── ArrayList    → 0.15+ 已是 unmanaged 默认（推荐；ArrayListUnmanaged 为其别名）
├── array_list.Managed — 旧的 managed 版（自带 allocator）
├── HashMap / ArrayHashMap — 哈希表
├── io           — I/O 抽象（Reader/Writer）
├── Io           — 0.16 新接口（文件 I/O 统一抽象）
├── fs           — 文件系统
├── fmt          — 格式化（print、format、parseInt...）
├── math         — 数学函数
├── sort         — 排序
├── meta         — 元编程工具
├── builtin      — 平台/编译配置查询
├── debug        — 调试工具（print、assert、panic）
├── testing      — 测试工具
├── process      — 进程参数/环境变量
├── Thread       — 线程
└── crypto       — 密码学
```

---

## 1. 内存分配器

### 1.1 Allocator 接口

`std.mem.Allocator` 是一个接口（胖指针），所有分配器都实现它：

```zig
pub const Allocator = struct {
    ptr: *anyopaque,
    vtable: *const VTable,

    pub const VTable = struct {
        alloc:   *const fn (*anyopaque, usize, u8, usize) ?[*]u8,
        resize:  *const fn (*anyopaque, []u8, u8, usize, usize) bool,
        remap:   *const fn (*anyopaque, []u8, u8, usize, u8, usize) ?[*]u8,
        free:    *const fn (*anyopaque, []u8, u8, usize) void,
    };
};
```

常用方法：
```zig
const alloc: std.mem.Allocator = ...;

// 分配
const buf: []u8 = try alloc.alloc(u8, 128);
const val: *u32 = try alloc.create(u32);

// 释放
alloc.free(buf);
alloc.destroy(val);

// 调整大小
const new_buf = try alloc.realloc(buf, 256);

// 零初始化
const zero_buf = try alloc.allocWithOptions(u8, 128, null, 0);
// 或
const zeroed = try alloc.alloc(u8, 128);
@memset(zeroed, 0);
```

### 1.2 常用分配器

#### GeneralPurposeAllocator（GPA）——开发/测试首选

```zig
const std = @import("std");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer {
        const status = gpa.deinit();
        if (status == .leak) std.debug.print("LEAK detected!\n", .{});
    }
    const alloc = gpa.allocator();

    const buf = try alloc.alloc(u8, 32);
    defer alloc.free(buf);

    // 使用 buf...
}
```

GPA 在 Debug 模式检测：
- 内存泄漏
- double-free
- use-after-free
- 越界访问

#### ArenaAllocator——批量分配、统一释放

```zig
var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
defer arena.deinit(); // 一次性释放所有分配

const alloc = arena.allocator();

// 分配很多东西，不需要逐个 free
const a = try alloc.alloc(u8, 64);
const b = try alloc.create(SomeStruct);
_ = a;
_ = b;
// arena.deinit() 统一回收
```

适用场景：编译器、解析器、请求处理（request-scoped 内存）

#### FixedBufferAllocator——栈上固定缓冲区（no_std 友好）

```zig
var buf: [4096]u8 = undefined;
var fba = std.heap.FixedBufferAllocator.init(&buf);
const alloc = fba.allocator();

const slice = try alloc.alloc(u8, 100);
_ = slice;
// 用完不需要 free（会失效）
// 超出 4096 字节会返回 error.OutOfMemory
```

适用：嵌入式/裸机，不需要 OS 支持。

#### page_allocator——直接向 OS 申请页

```zig
const alloc = std.heap.page_allocator;
// 开销大，但不需要初始化
const buf = try alloc.alloc(u8, 4096);
defer alloc.free(buf);
```

#### 分配器传递习惯

```zig
// 好的设计：接收 allocator 参数，不自己持有
fn processData(alloc: std.mem.Allocator, data: []const u8) ![]u8 {
    const result = try alloc.alloc(u8, data.len * 2);
    // ...
    return result; // 调用者负责 free
}
```

---

## 2. 动态数组：ArrayListUnmanaged

> **0.15+ 变更：** `std.ArrayList(T)` **本身已是 unmanaged 默认**（不存 `allocator`，每次调用显式传入，更省空间、控制更精确）。`std.ArrayListUnmanaged(T)` 现为其别名（仍可用）。需要旧的 managed 版（自带 `allocator`）请用 `std.array_list.Managed(T)`。下文示例用 `ArrayListUnmanaged` 写法（与 `ArrayList` 等价）。

```zig
const std = @import("std");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();

    var list = std.ArrayListUnmanaged(u32){};
    defer list.deinit(alloc);

    // 追加元素
    try list.append(alloc, 10);
    try list.append(alloc, 20);
    try list.append(alloc, 30);

    // 追加切片
    try list.appendSlice(alloc, &.{ 40, 50 });

    // 插入
    try list.insert(alloc, 0, 0); // 在索引 0 插入 0

    // 访问
    std.debug.print("{d}\n", .{list.items[0]});
    std.debug.print("len={d}\n", .{list.items.len});

    // 弹出最后一个（不释放内存，只缩短 len）
    const last = list.pop();
    std.debug.print("popped: {d}\n", .{last});

    // 删除（顺序保持）
    _ = list.orderedRemove(1);

    // 删除（不保持顺序，O(1)，用末尾元素填补）
    _ = list.swapRemove(0);

    // 预分配容量
    try list.ensureTotalCapacity(alloc, 100);

    // 清空（保留已分配内存）
    list.clearRetainingCapacity();

    // 转为拥有所有权的切片（调用者负责 free）
    const owned = try list.toOwnedSlice(alloc);
    defer alloc.free(owned);
}
```

### 与 ArrayList 的对比

```zig
// Managed 版（存储了 allocator，使用方便但多占 8/16 字节）— 0.15 后移到 std.array_list.Managed
var al = std.array_list.Managed(u32).init(alloc);
defer al.deinit();
try al.append(42);

// 默认的 ArrayList（= 旧 Unmanaged，不存储 allocator，每次显式传入）
var alu = std.ArrayList(u32){};      // 或 .empty
defer alu.deinit(alloc);
try alu.append(alloc, 42);
```

---

## 3. 哈希表

### 3.1 AutoHashMap

```zig
var map = std.AutoHashMap(u32, []const u8).init(alloc);
defer map.deinit();

try map.put(1, "one");
try map.put(2, "two");

if (map.get(1)) |val| {
    std.debug.print("{s}\n", .{val}); // "one"
}

// 检查存在
std.debug.print("{}\n", .{map.contains(2)}); // true

// 删除
_ = map.remove(1);

// 迭代
var it = map.iterator();
while (it.next()) |entry| {
    std.debug.print("{d} = {s}\n", .{ entry.key_ptr.*, entry.value_ptr.* });
}
```

### 3.2 StringHashMap（键为字符串）

```zig
var smap = std.StringHashMap(u32).init(alloc);
defer smap.deinit();

try smap.put("hello", 1);
try smap.put("world", 2);

std.debug.print("{d}\n", .{smap.get("hello").?}); // 1
```

### 3.3 ArrayHashMap（保持插入顺序）

```zig
var ordered = std.AutoArrayHashMap(u32, u32).init(alloc);
defer ordered.deinit();

try ordered.put(3, 30);
try ordered.put(1, 10);
try ordered.put(2, 20);

// 按插入顺序迭代：3→1→2
for (ordered.keys(), ordered.values()) |k, v| {
    std.debug.print("{d}={d}\n", .{ k, v });
}
```

---

## 4. 格式化

### 4.1 std.fmt.format 格式规范

```zig
// 基本格式符
std.debug.print("{d}\n", .{42});        // 十进制整数
std.debug.print("{x}\n", .{255});       // 十六进制小写 (ff)
std.debug.print("{X}\n", .{255});       // 十六进制大写 (FF)
std.debug.print("{o}\n", .{8});         // 八进制
std.debug.print("{b}\n", .{5});         // 二进制
std.debug.print("{s}\n", .{"hello"});   // 字符串
std.debug.print("{c}\n", .{'A'});       // 字符
std.debug.print("{f}\n", .{3.14});      // 浮点（小数）
std.debug.print("{e}\n", .{3.14});      // 科学计数
std.debug.print("{}\n", .{true});       // 默认格式
std.debug.print("{any}\n", .{@as(u8, 5)}); // 任意类型

// 宽度和填充
std.debug.print("{d:>10}\n", .{42});    // 右对齐，宽度 10
std.debug.print("{d:<10}\n", .{42});    // 左对齐
std.debug.print("{d:0>5}\n", .{42});    // 前补零 (00042)
std.debug.print("{x:0>8}\n", .{0xAB}); // 0x000000ab

// 指针
std.debug.print("{*}\n", .{&x});        // 地址
```

### 4.2 格式化到字符串

```zig
// 格式化到固定缓冲区
var buf: [64]u8 = undefined;
const s = try std.fmt.bufPrint(&buf, "x = {d}", .{42});
// s 是 []u8，指向 buf 的一部分

// 格式化到分配的字符串（调用者 free）
const heap_s = try std.fmt.allocPrint(alloc, "x = {d}, y = {d}", .{ 1, 2 });
defer alloc.free(heap_s);
```

### 4.3 自定义 format 方法

```zig
const Point = struct {
    x: f32,
    y: f32,

    // 实现 format 方法，则 {} 会调用它
    pub fn format(
        self: Point,
        comptime fmt_str: []const u8,
        options: std.fmt.FormatOptions,
        writer: anytype,
    ) !void {
        _ = fmt_str;
        _ = options;
        try writer.print("({d:.2}, {d:.2})", .{ self.x, self.y });
    }
};

const p = Point{ .x = 1.0, .y = 2.0 };
std.debug.print("{}\n", .{p}); // (1.00, 2.00)
```

### 4.4 数字解析

```zig
const n = try std.fmt.parseInt(i32, "-42", 10);
const u = try std.fmt.parseInt(u32, "0xFF", 0);  // 自动检测基数
const f = try std.fmt.parseFloat(f64, "3.14");
```

---

## 5. I/O

> ⚠️ **0.15「Writergate」+ 0.16 大改：** I/O 是 Zig 当前**变动最剧烈**的部分。`std.io.getStdOut()`/`bufferedReader` 等是 **0.15 前的旧写法**；0.15 引入新的 `std.Io.Reader`/`std.Io.Writer`，0.16 进一步把文件 I/O 收进 `std.Io` 接口（`std.Io.File`、`std.Io.Writer.print`，文件操作需传 `io` 参数）。**本节下方示例多为旧形态，仅供理解概念**；当前确切 API 以官方 0.16 文档为准，跨版本最稳的纯输出请用 `std.debug.print`（写 stderr）。

### 5.1 基本输出

```zig
// Debug 输出（任何时候都可用，输出到 stderr）
std.debug.print("value = {d}\n", .{42});

// stdout 写入
const stdout = std.io.getStdOut();
const writer = stdout.writer();
try writer.print("Hello {s}\n", .{"world"});
try writer.writeAll("no format\n");
try writer.writeByte('\n');
```

### 5.2 main 接收 std.process.Init（0.16+ 推荐）

```zig
pub fn main(io: std.process.Init) !void {
    // io.stdout, io.stdin, io.stderr 均为 std.Io.File
    try io.stdout.writer().print("Hello from Init!\n", .{});
    
    // 读取输入
    var buf: [256]u8 = undefined;
    const n = try io.stdin.read(&buf);
    const line = buf[0..n];
    _ = line;
}
```

### 5.3 文件读写

```zig
// 打开文件
const file = try std.fs.openFileAbsolute("/tmp/test.txt", .{ .mode = .read_only });
defer file.close();

// 读取全部内容
const content = try file.readToEndAlloc(alloc, 1024 * 1024);
defer alloc.free(content);

// 逐行读取
var buf_reader = std.io.bufferedReader(file.reader());
const reader = buf_reader.reader();
var line_buf: [4096]u8 = undefined;
while (try reader.readUntilDelimiterOrEof(&line_buf, '\n')) |line| {
    std.debug.print("{s}\n", .{line});
}
```

```zig
// 创建并写入文件
const out = try std.fs.createFileAbsolute("/tmp/out.txt", .{});
defer out.close();

var buf_writer = std.io.bufferedWriter(out.writer());
const writer = buf_writer.writer();
try writer.print("line 1\n", .{});
try writer.print("line 2\n", .{});
try buf_writer.flush(); // 必须 flush！
```

### 5.4 相对路径操作

```zig
// 相对于 cwd
const cwd = std.fs.cwd();
const file = try cwd.openFile("relative/path.txt", .{});
defer file.close();

// 创建目录
try cwd.makeDir("new_dir");

// 列目录
var dir = try cwd.openDir("some_dir", .{ .iterate = true });
defer dir.close();

var it = dir.iterate();
while (try it.next()) |entry| {
    std.debug.print("{s}: {s}\n", .{
        entry.name,
        @tagName(entry.kind), // file, directory, symlink...
    });
}
```

### 5.5 Writer 接口

Zig 的 `Writer`（0.15+ Writergate 后）是一个接口（anytype）：

```zig
// 接受任意 writer 的泛型函数
fn writeHeader(writer: anytype) !void {
    try writer.writeAll("# Header\n");
    try writer.print("date: {s}\n", .{"2026-05-02"});
}

// 调用时传入具体的 writer
const f = try std.fs.createFileAbsolute("/tmp/x.txt", .{});
defer f.close();
try writeHeader(f.writer());

// 或者传 stderr
try writeHeader(std.io.getStdErr().writer());
```

### 5.6 内存中的 I/O

```zig
// 写入内存（ArrayList 作为缓冲区）
var buf = std.ArrayListUnmanaged(u8){};
defer buf.deinit(alloc);

const writer = buf.writer(alloc); // 返回 Writer
try writer.print("data: {d}\n", .{42});

// 读取字节流
var fbs = std.io.fixedBufferStream(buf.items);
const reader = fbs.reader();
var tmp: [16]u8 = undefined;
const n = try reader.readAll(&tmp);
_ = n;
```

---

## 6. 内存操作（std.mem）

```zig
const std = @import("std");

// 比较
std.debug.print("{}\n", .{std.mem.eql(u8, "hello", "hello")}); // true
std.debug.print("{}\n", .{std.mem.startsWith(u8, "hello", "hel")}); // true
std.debug.print("{}\n", .{std.mem.endsWith(u8, "hello", "llo")}); // true

// 查找
const idx = std.mem.indexOf(u8, "hello world", "world"); // ?usize = 6
const last = std.mem.lastIndexOf(u8, "abcabc", "abc"); // ?usize = 3

// 复制
var dst: [5]u8 = undefined;
@memcpy(&dst, "hello");

// 字节序转换
const le = std.mem.nativeToLittle(u32, 0x12345678);
const be = std.mem.nativeToBig(u32, 0x12345678);

// 对齐
const aligned = std.mem.alignForward(usize, 13, 8); // 16

// 字符串分割
var it = std.mem.splitScalar(u8, "a,b,c", ',');
while (it.next()) |part| {
    std.debug.print("{s}\n", .{part});
}

// trim
const trimmed = std.mem.trim(u8, "  hello  ", " "); // "hello"
```

---

## 7. 排序

```zig
const std = @import("std");

var arr = [_]i32{ 5, 2, 8, 1, 9, 3 };

// 排序（原地）
std.mem.sort(i32, &arr, {}, std.sort.asc(i32));
// 降序
std.mem.sort(i32, &arr, {}, std.sort.desc(i32));

// 自定义比较
const Ctx = struct {
    fn lessThan(_: void, a: i32, b: i32) bool {
        return @abs(a) < @abs(b);
    }
};
std.mem.sort(i32, &arr, {}, Ctx.lessThan);

// 稳定排序
std.mem.sortUnstable(i32, &arr, {}, std.sort.asc(i32));
```

---

## 8. 数学（std.math）

```zig
const math = std.math;

// 常量
const pi = math.pi;      // f64
const e  = math.e;

// 安全运算（返回 error.Overflow）
const sum = try math.add(u32, 100, 200);
const max = math.maxInt(u32); // 4294967295
const min = math.minInt(i8);  // -128

// 浮点
const x = math.sqrt(2.0);
const y = math.pow(f64, 2.0, 10.0); // 2^10
const z = math.log2(1024.0);
const s = math.sin(math.pi / 2.0);
const c = math.cos(0.0);

// 位运算辅助
const log2_floor = math.log2_int(u32, 1024); // 10
```

---

## 9. 进程与环境

```zig
pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();

    // 命令行参数
    const args = try std.process.argsAlloc(alloc);
    defer std.process.argsFree(alloc, args);

    for (args) |arg| {
        std.debug.print("arg: {s}\n", .{arg});
    }

    // 环境变量
    const path = std.process.getEnvVarOwned(alloc, "PATH") catch null;
    if (path) |p| {
        defer alloc.free(p);
        std.debug.print("PATH: {s}\n", .{p});
    }

    // 退出
    std.process.exit(0);
}
```

---

## 10. builtin（编译配置查询）

```zig
const builtin = @import("builtin");

// 目标平台
const arch = builtin.cpu.arch;             // .riscv64, .x86_64, .aarch64...
const os   = builtin.os.tag;              // .linux, .macos, .freestanding...
const abi  = builtin.abi;                  // .gnu, .musl, .none...

// 编译模式
const mode = builtin.mode; // .Debug, .ReleaseFast, .ReleaseSafe, .ReleaseSmall

// 是否在测试
const is_test = builtin.is_test;

// 字节序
const endian = builtin.cpu.arch.endian(); // .little / .big

// 指针大小
const ptr_size = @sizeOf(usize);           // 4 或 8

// 条件编译
if (builtin.os.tag == .freestanding) {
    // 裸机代码
}
```

---

## 11. 调试工具

```zig
// assert（仅 Debug/ReleaseSafe 模式检查）
std.debug.assert(x > 0);

// panic
if (bad_state) std.debug.panic("bad state: {d}", .{x});

// 打印到 stderr
std.debug.print("debug: {d}\n", .{x});

// 打印堆栈跟踪
std.debug.dumpCurrentStackTrace(null);
```

---

## 练习

### 练习 5：分配器使用

```zig
const std = @import("std");

// TODO: 实现一个函数，接收 allocator 和整数切片，
// 返回一个新的已排序的切片（调用者负责 free）
fn sortedCopy(alloc: std.mem.Allocator, input: []const i32) ![]i32 {
    // 填充这里
    _ = alloc;
    _ = input;
    return &.{};
}

test "sortedCopy" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();

    const result = try sortedCopy(alloc, &.{ 3, 1, 4, 1, 5, 9, 2, 6 });
    defer alloc.free(result);

    try std.testing.expectEqualSlices(i32, &.{ 1, 1, 2, 3, 4, 5, 6, 9 }, result);
}
```

### 练习 6：哈希表

```zig
// TODO: 统计一个字符串中每个字符出现的次数
// 返回 AutoHashMap(u8, usize)，调用者负责 deinit
fn countChars(alloc: std.mem.Allocator, s: []const u8) !std.AutoHashMap(u8, usize) {
    // 填充这里
    _ = s;
    return std.AutoHashMap(u8, usize).init(alloc);
}

test "countChars" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();

    var map = try countChars(alloc, "aabbbc");
    defer map.deinit();

    try std.testing.expectEqual(@as(usize, 2), map.get('a').?);
    try std.testing.expectEqual(@as(usize, 3), map.get('b').?);
    try std.testing.expectEqual(@as(usize, 1), map.get('c').?);
}
```

### 练习 7：格式化

```zig
// TODO: 实现 hexDump，将字节切片按以下格式打印：
// 00000000: 48 65 6c 6c 6f  Hello
// 00000005: 20 57 6f 72 6c  _Worl
// ...（每行 5 字节，十六进制 + ASCII 可打印字符，不可打印用 '.'）
fn hexDump(writer: anytype, data: []const u8) !void {
    // 填充这里
    _ = data;
    _ = writer;
}
```

---

> 下一节：[01-04-zig-comptime.md](01-04-zig-comptime.md) — Comptime、泛型、反射
