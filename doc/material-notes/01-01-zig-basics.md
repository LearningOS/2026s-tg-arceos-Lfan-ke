# Zig 基础语法

> **版本：Zig 0.16.0 / 0.17.0-dev**（本笔记所有语法基于此版本，拒绝 0.15 及以前的旧写法）
> 官方文档：https://ziglang.org/documentation/0.16.0/
> 源码仓库：https://codeberg.org/ziglang/zig

---

## 0. 全局速览

Zig 的核心设计哲学，五条原则：

| 原则 | 含义 |
|------|------|
| **无隐式控制流** | 无运算符重载、无析构函数、无异常抛出 |
| **无隐式内存分配** | 分配器必须显式传入，stdlib 不会悄悄分配堆内存 |
| **comptime = 编译期 Zig** | 编译期和运行期用 **同一套语法**，comptime 是第一公民 |
| **错误是值** | `error.Foo` 是枚举值，用 `!T` 类型表达错误联合 |
| **C 互操作一等公民** | `@cImport`、`extern`、ABI 兼容结构体，零成本 |

一个典型的 Zig 程序结构：

```zig
const std = @import("std");

pub fn main() void {
    // 0.15 Writergate 后,stdout 写法变动较大;演示输出用跨版本稳定的 std.debug.print(写 stderr)
    std.debug.print("Hello, {s}!\n", .{"Zig"});
}
```

---

## 1. 变量与常量

### 1.1 基本声明

```zig
const x: u32 = 42;       // 不可变，编译期或运行期均可
var y: i32 = -1;          // 可变
var z: bool = undefined;  // 显式推迟初始化（故意留空，不是 null）
```

- `const` 绑定不可修改其值（对指针：不可重定向，但指向内容可变）
- `undefined` 是 Zig 特有的"故意未初始化"标记，Debug 模式会填充 0xAA 帮助检测

### 1.2 类型推断

```zig
const a = 42;          // 类型推断为 comptime_int（编译期整数）
const b = 3.14;        // comptime_float
const c = true;        // bool
const s = "hello";     // *const [5:0]u8（字符串字面量）
```

> **comptime_int / comptime_float**：字面量在编译期没有固定位宽，只有赋值给具体类型时才确定。

### 1.3 容器级变量

```zig
const std = @import("std");

// 文件（容器）顶层的变量拥有静态生存期，惰性分析
var global_counter: usize = 0;
const MAX: u32 = 4096;
```

### 1.4 threadlocal

```zig
threadlocal var tls_val: u32 = 0;  // 每个线程独立副本
```

---

## 2. 基本类型

### 2.1 整数

| 类型 | 位宽 | 范围 |
|------|------|------|
| `u8` | 8 | 0 – 255 |
| `i8` | 8 | -128 – 127 |
| `u16/i16` | 16 | ... |
| `u32/i32` | 32 | ... |
| `u64/i64` | 64 | ... |
| `u128/i128` | 128 | ... |
| `usize` | 平台字长 | 指针大小 |
| `isize` | 平台字长 | 有符号指针大小 |
| `u7`, `i3`, `u42`... | 任意 | 任意位宽整数！ |

```zig
const a: u8 = 0xFF;
const b: i64 = -9_223_372_036_854_775_808;
const c: u7 = 127;       // 任意位宽
const d: u32 = 0b1010_1010;
const e: u32 = 0o755;
const f: u32 = 1_000_000; // 下划线分隔符
```

**溢出运算符**（不会 panic，有包装或饱和语义）：

```zig
const x: u8 = 255;
const wrap  = x +% 1;  // 0    （回绕）
const sat   = x +| 1;  // 255  （饱和）
const shift = x << 1;  // 编译期检查移位量
```

### 2.2 浮点

```zig
const pi: f64 = 3.141592653589793;
const e: f32  = 2.71828;
const big: f128 = 1.23e300;
const hex_float: f32 = 0x1.8p+1; // 十六进制浮点
```

类型：`f16`、`f32`、`f64`、`f80`、`f128`（均 IEEE-754）

### 2.3 bool

```zig
const t: bool = true;
const f: bool = false;
const logic = t and f or !t; // and/or/! 短路求值
```

### 2.4 void 与 noreturn

```zig
fn nothing() void {}

fn forever() noreturn {
    while (true) {}
}
```

### 2.5 type（类型作为值）

```zig
const T: type = u32;
const U: type = []const u8;

fn makeSlice(comptime Item: type, n: usize) []Item {
    // ...
    _ = n;
    unreachable;
}
```

---

## 3. 复合类型

### 3.1 数组

```zig
// 固定长度，编译期已知
const arr: [5]u32 = .{ 1, 2, 3, 4, 5 };
const zeros = [_]u8{0} ** 16;    // 16 个 0
const combined = arr ++ [_]u32{6, 7}; // 拼接（++ 操作符）

std.debug.print("{d}\n", .{arr.len}); // 5
std.debug.print("{d}\n", .{arr[0]});  // 1

// Sentinel-terminated（哨兵终止数组）
const cstr: [5:0]u8 = "hello".*;  // 末尾有隐式 0
```

### 3.2 切片（Slice）

切片 = 指针 + 长度，是 Zig 中最常用的序列类型：

```zig
const arr = [_]u32{ 1, 2, 3, 4, 5 };
const s: []const u32 = arr[1..4]; // { 2, 3, 4 }，长度 3
const all: []const u32 = &arr;    // 整个数组的切片

// 可变切片
var buf: [16]u8 = undefined;
const mutable: []u8 = &buf;
mutable[0] = 0xFF;
```

- `[]T` — 可变切片
- `[]const T` — 不可变切片
- `[:0]const u8` — 哨兵终止切片（字符串的常见表示）

### 3.3 指针

```zig
var x: u32 = 42;

// 单值指针
const ptr: *u32 = &x;
ptr.* = 100;              // 解引用赋值

// 多值指针（不知道长度）
const many: [*]u32 = &arr; // 不推荐直接使用，改用切片
many[0] = 1;              // 可以索引，但无边界检查

// const 指针
const cptr: *const u32 = &x;
// cptr.* = 1;  // 编译错误
```

**可选指针**（指针的 null 版本）：

```zig
var opt_ptr: ?*u32 = null;
opt_ptr = &x;
if (opt_ptr) |p| {
    p.* = 999;
}
```

### 3.4 字符串

Zig 没有内置 String 类型，字符串是字节切片：

```zig
const s: []const u8 = "hello, world";
const c_str: [*:0]const u8 = "c-style";  // C 兼容
const zig_str: [:0]const u8 = "zig-style"; // Zig 推荐

// 多行字符串
const multi =
    \\line one
    \\line two
    \\line three
;

// UTF-8 字符
const emoji = "🦎";                       // []const u8，4 字节
const char: u21 = '🦎';                   // Unicode 码点
```

---

## 4. 结构体

### 4.1 基本结构体

```zig
const Point = struct {
    x: f32,
    y: f32 = 0.0, // 默认值

    // 方法（第一个参数是 self）
    pub fn distance(self: Point, other: Point) f32 {
        const dx = self.x - other.x;
        const dy = self.y - other.y;
        return @sqrt(dx * dx + dy * dy);
    }

    // 关联函数（类似构造函数）
    pub fn origin() Point {
        return .{ .x = 0, .y = 0 };
    }
};

const p1 = Point{ .x = 3.0, .y = 4.0 };
const p2 = Point.origin();
const d = p1.distance(p2); // 5.0
```

### 4.2 匿名结构体（元组风格）

```zig
const tuple = .{ 1, "hello", true };
// 访问：tuple[0], tuple[1], tuple[2]

// 类型为匿名结构体
const named = .{ .x = 1, .y = 2 };
```

### 4.3 extern struct（C ABI 兼容）

```zig
const CStruct = extern struct {
    a: u32,
    b: u64,
    // 字段顺序和填充严格按 C ABI
};
```

### 4.4 packed struct（位精确布局）

```zig
const Flags = packed struct(u8) {
    read:    bool,
    write:   bool,
    execute: bool,
    _pad:    u5 = 0,
};

const f = Flags{ .read = true, .write = false, .execute = true };
const raw: u8 = @bitCast(f); // 转为整数
```

---

## 5. 枚举

### 5.1 基本枚举

```zig
const Direction = enum {
    north,
    south,
    east,
    west,
};

const dir = Direction.north;

switch (dir) {
    .north => std.debug.print("N\n", .{}),
    .south => std.debug.print("S\n", .{}),
    .east  => std.debug.print("E\n", .{}),
    .west  => std.debug.print("W\n", .{}),
}
```

### 5.2 显式 tag 类型和值

```zig
const Status = enum(u8) {
    ok      = 0,
    err     = 1,
    pending = 2,
};

const s: Status = @enumFromInt(0); // Status.ok
const n: u8 = @intFromEnum(Status.err); // 1
```

### 5.3 枚举方法

```zig
const Color = enum {
    red, green, blue,

    pub fn isWarm(self: Color) bool {
        return switch (self) {
            .red => true,
            else => false,
        };
    }
};
```

### 5.4 非穷举枚举

```zig
const BigEnum = enum(u16) {
    a = 1,
    b = 2,
    _,  // 允许其他值（非穷举）
};
```

---

## 6. 联合体（Union）

### 6.1 Tagged Union（常用）

```zig
const Value = union(enum) {
    int:   i64,
    float: f64,
    text:  []const u8,
    empty: void,
};

const v = Value{ .int = 42 };

switch (v) {
    .int   => |n| std.debug.print("int: {d}\n",   .{n}),
    .float => |f| std.debug.print("float: {d}\n", .{f}),
    .text  => |s| std.debug.print("text: {s}\n",  .{s}),
    .empty => std.debug.print("empty\n", .{}),
}
```

### 6.2 extern union / packed union

```zig
const Raw = extern union {
    as_u32: u32,
    as_bytes: [4]u8,
};

var r = Raw{ .as_u32 = 0xDEAD_BEEF };
std.debug.print("{x}\n", .{r.as_bytes[0]}); // 平台字节序
```

---

## 7. 可选类型（Optional）

```zig
var opt: ?u32 = null;
opt = 42;

// 解包方式 1：if
if (opt) |val| {
    std.debug.print("got: {d}\n", .{val});
} else {
    std.debug.print("null\n", .{});
}

// 解包方式 2：orelse（提供默认值）
const val = opt orelse 0;

// 解包方式 3：.?（panic if null）
const forced = opt.?; // 确定不为 null 时用

// 解包方式 4：while 迭代器模式
var iter: ?u32 = getNext();
while (iter) |v| : (iter = getNext()) {
    _ = v;
}
```

可选指针 `?*T` 内部实现为零开销（null 用空指针表示）。

---

## 8. 错误处理

### 8.1 错误集

```zig
const MyError = error{
    OutOfMemory,
    InvalidInput,
    Overflow,
};

// anyerror：全局错误集（合并所有错误，类似 anytype）
```

### 8.2 错误联合类型

```zig
fn parse(s: []const u8) MyError!u32 {
    if (s.len == 0) return error.InvalidInput;
    // ...
    return 0;
}
```

`MyError!u32` 读作"要么是 MyError 中的某个错误，要么是 u32"

### 8.3 try / catch / orelse

```zig
// try：错误时直接向上传播（= catch |e| return e）
const result = try parse("123");

// catch：捕获并处理
const safe = parse("bad") catch |err| blk: {
    std.debug.print("Error: {}\n", .{err});
    break :blk 0;
};

// catch 仅返回默认值
const default = parse("") catch 0;

// if 解构错误联合
if (parse("abc")) |val| {
    _ = val;
} else |err| {
    _ = err;
}
```

### 8.4 errdefer

```zig
fn init() !*Resource {
    const r = try allocate();
    errdefer r.deinit(); // 仅在本函数以错误返回时执行

    try r.setup();
    return r;
}
```

---

## 9. 控制流

### 9.1 if

```zig
const x = 5;

// 普通 if
if (x > 3) {
    // ...
} else if (x > 1) {
    // ...
} else {
    // ...
}

// if 作为表达式
const label = if (x > 3) "big" else "small";

// 解包可选
if (maybe_val) |v| {
    _ = v;
}

// 解包错误联合
if (result_val) |v| {
    _ = v;
} else |err| {
    _ = err;
}
```

### 9.2 while

```zig
var i: usize = 0;
while (i < 10) : (i += 1) {
    std.debug.print("{d} ", .{i});
}

// 无限循环
while (true) {
    if (condition) break;
    if (skip_case) continue;
}

// while 解包可选（迭代器模式）
while (iterator.next()) |item| {
    _ = item;
}
```

### 9.3 for

```zig
const items = [_]u32{ 10, 20, 30 };

// 迭代元素
for (items) |item| {
    std.debug.print("{d}\n", .{item});
}

// 迭代元素 + 索引
for (items, 0..) |item, idx| {
    std.debug.print("[{d}] = {d}\n", .{ idx, item });
}

// 同时迭代多个切片（长度必须相同）
for (a_slice, b_slice) |a, b| {
    _ = a + b;
}

// 迭代范围（不含终止）
for (0..10) |i| {
    _ = i; // 0, 1, ..., 9
}

// break/continue 带标签
outer: for (0..5) |i| {
    for (0..5) |j| {
        if (i == j) continue :outer;
        std.debug.print("{d},{d}\n", .{ i, j });
    }
}
```

### 9.4 switch

```zig
const x: u32 = 3;

switch (x) {
    0 => std.debug.print("zero\n", .{}),
    1, 2 => std.debug.print("one or two\n", .{}),
    3...7 => std.debug.print("3 to 7\n", .{}),
    else => std.debug.print("other\n", .{}),
}

// switch 作为表达式
const label = switch (x) {
    0 => "zero",
    1 => "one",
    else => "many",
};

// switch 枚举（穷举，不需要 else）
const dir = Direction.north;
const name = switch (dir) {
    .north => "N",
    .south => "S",
    .east  => "E",
    .west  => "W",
};
_ = name;
```

### 9.5 defer 和 errdefer

```zig
fn example() !void {
    std.debug.print("start\n", .{});
    defer std.debug.print("always end\n", .{}); // 函数退出时执行

    const file = try std.fs.openFileAbsolute("/tmp/x", .{});
    defer file.close();  // RAII 模式

    errdefer cleanup();  // 仅错误路径执行

    try doWork();
    // defer 按 LIFO 顺序执行
}
```

### 9.6 块表达式（labeled block）

```zig
const result: u32 = blk: {
    const a = 10;
    const b = 20;
    break :blk a + b; // 块的值
};
// result == 30
```

---

## 10. 函数

### 10.1 基本函数

```zig
fn add(a: i32, b: i32) i32 {
    return a + b;
}

// 推断返回类型（不推荐在库代码中使用）
// fn add(a: i32, b: i32) @TypeOf(a + b) { ... }

// 错误联合返回
fn tryAdd(a: u32, b: u32) error{Overflow}!u32 {
    return std.math.add(u32, a, b);
}
```

### 10.2 comptime 参数

```zig
// T 在调用时必须是编译期已知的类型
fn makeZero(comptime T: type) T {
    return @as(T, 0);
}

const z_u32 = makeZero(u32);  // 0 as u32
const z_f64 = makeZero(f64);  // 0.0 as f64
```

### 10.3 anytype（Duck Typing）

```zig
// anytype：编译期根据实际参数推断类型
fn printAny(val: anytype) void {
    std.debug.print("{any}\n", .{val});
}

printAny(42);
printAny("hello");
printAny(true);
```

### 10.4 可变参数

```zig
// Zig 没有 C 风格 vararg，用元组 + comptime 代替
fn printAll(args: anytype) void {
    inline for (0..args.len) |i| {
        std.debug.print("{any} ", .{args[i]});
    }
}

printAll(.{ 1, "two", 3.0 });
```

### 10.5 函数指针

```zig
const FnPtr = *const fn(u32) u32;

fn double(x: u32) u32 { return x * 2; }

const f: FnPtr = &double;
const r = f(21); // 42
```

### 10.6 内联函数

```zig
inline fn fastOp(x: u32) u32 {
    return x * x;
}
```

---

## 11. 内置函数速查（常用）

| 函数 | 用途 |
|------|------|
| `@import("std")` | 导入模块 |
| `@TypeOf(x)` | 获取表达式的类型 |
| `@typeInfo(T)` | 获取类型信息（返回 `std.builtin.Type`，字段 **全小写**）|
| `@typeName(T)` | 获取类型名字符串 |
| `@sizeOf(T)` | 类型字节大小 |
| `@alignOf(T)` | 类型对齐要求 |
| `@offsetOf(S, "field")` | 结构体字段偏移 |
| `@as(T, x)` | 类型强制（必须兼容） |
| `@intCast(x)` | 整数转换（运行期检查溢出） |
| `@floatCast(x)` | 浮点转换 |
| `@ptrCast(ptr)` | 指针类型转换 |
| `@bitCast(x)` | 位模式重解释 |
| `@enumFromInt(n)` | 整数转枚举 |
| `@intFromEnum(e)` | 枚举转整数 |
| `@errorFromInt(n)` | 整数转错误 |
| `@intFromError(e)` | 错误转整数 |
| `@truncate(x)` | 截断整数 |
| `@memcpy(dst, src)` | 内存复制 |
| `@memset(dst, val)` | 内存填充 |
| `@compileError("msg")` | 编译期报错 |
| `@compileLog(...)` | 编译期打印（调试） |
| `@hasDecl(T, "name")` | 编译期检查类型是否有某声明 |
| `@hasField(T, "name")` | 编译期检查结构体是否有某字段 |
| `@field(x, "name")` | 按名字字符串访问字段 |
| `@abs(x)` | 绝对值 |
| `@sqrt(x)` | 平方根 |
| `@min(a, b)` / `@max(a, b)` | 最小/最大值 |
| `@popCount(x)` | 置位比特数 |
| `@clz(x)` / `@ctz(x)` | 前导零/尾随零 |
| `@byteSwap(x)` | 字节序翻转 |
| `@atomicLoad` / `@atomicStore` / `@atomicRmw` | 原子操作 |
| `@panic("msg")` | 运行期 panic |
| `unreachable` | 断言不可达（非内置，是关键字） |

> **0.16 重要变化**：`@Type` 系列被拆分为独立的 `@Int`、`@Pointer`、`@Struct` 等，详见 `01-04-zig-comptime.md`

---

## 12. 测试

```zig
const std = @import("std");
const testing = std.testing;

fn add(a: u32, b: u32) u32 {
    return a + b;
}

test "add works" {
    try testing.expectEqual(@as(u32, 5), add(2, 3));
}

test "string comparison" {
    const s = "hello";
    try testing.expectEqualStrings("hello", s);
}
```

运行：
```sh
zig test src/main.zig
```

---

## 练习

> 在下方空白处补全代码，使得 `zig test` 通过。

### 练习 1：基本类型

```zig
const std = @import("std");
const testing = std.testing;

// TODO: 写一个函数，接收两个 ?u32，
// 返回两者之和（任一为 null 则返回 null）
fn addOptional(a: ?u32, b: ?u32) ?u32 {
    if (a == null or b == null) return null;
    return a.? + b.?;
}

test "addOptional" {
    try testing.expectEqual(@as(?u32, 7), addOptional(3, 4));
    try testing.expectEqual(@as(?u32, null), addOptional(null, 4));
    try testing.expectEqual(@as(?u32, null), addOptional(3, null));
}
```

### 练习 2：错误处理

```zig
const ParseError = error{
    Empty,
    InvalidCharacter,
    Overflow,
};

// TODO: 实现简单的 u8 解析（只处理十进制数字，字符串为空返回 Empty，
// 含非数字字符返回 InvalidCharacter，结果超过 255 返回 Overflow）
fn parseU8(s: []const u8) ParseError!u8 {
    if (s.len == 0) return error.Empty;

    var result: u16 = 0;
    for (s) |c| {
        if (c < '0' or c > '9') return error.InvalidCharacter;
        const digit = c - '0';
        result = result * 10 + digit;
        if (result > 255) return error.Overflow;
    }
    return @intCast(result);
}

test "parseU8" {
    try std.testing.expectEqual(@as(u8, 42), try parseU8("42"));
    try std.testing.expectError(ParseError.Empty, parseU8(""));
    try std.testing.expectError(ParseError.InvalidCharacter, parseU8("12x"));
    try std.testing.expectError(ParseError.Overflow, parseU8("256"));
}
```

### 练习 3：结构体与方法

```zig
// TODO: 实现一个 Vec2 结构体，包含：
// - x, y: f32
// - add(self, other: Vec2) Vec2
// - scale(self, factor: f32) Vec2
// - dot(self, other: Vec2) f32
const Vec2 = struct {
    x: f32,
    y: f32,

    fn add(self: Vec2, other: Vec2) Vec2 {
        return Vec2{ .x = self.x + other.x, .y = self.y + other.y };
    }

    fn scale(self: Vec2, factor: f32) Vec2 {
        return Vec2{ .x = self.x * factor, .y = self.y * factor };
    }

    fn dot(self: Vec2, other: Vec2) f32 {
        return self.x * other.x + self.y * other.y;
    }
};

test "Vec2" {
    const a = Vec2{ .x = 1.0, .y = 2.0 };
    const b = Vec2{ .x = 3.0, .y = 4.0 };
    const sum = a.add(b);
    try std.testing.expectApproxEqAbs(@as(f32, 4.0), sum.x, 1e-5);
    try std.testing.expectApproxEqAbs(@as(f32, 6.0), sum.y, 1e-5);
    try std.testing.expectApproxEqAbs(@as(f32, 11.0), a.dot(b), 1e-5);
}
```

### 练习 4：Tagged Union

```zig
// TODO: 实现一个简单的 JSON 值类型：
// - null_val
// - bool_val: bool
// - int_val: i64
// - float_val: f64
// - string_val: []const u8
// 并实现 format 方法，打印对应的字面量表示
const JsonValue = union(enum) {
    bool_val: bool,
    int_val: i64,
    float_val: f64,
    string_val: []const u8,
    null_val,

    pub fn format(self: JsonValue, comptime _: []const u8, _: std.fmt.FormatOptions, writer: anytype) !void {
        switch (self) {
            .null_val => try writer.writeAll("null"),
            .bool_val => |b| try writer.print("{}", .{b}),
            .int_val => |i| try writer.print("{d}", .{i}),
            .float_val => |f| try writer.print("{d}", .{f}),
            .string_val => |s| try writer.print("\"{s}\"", .{s}),
        }
    }
};
```

---

> 下一节：[01-03-zig-stdlib.md](01-03-zig-stdlib.md) — 标准库：分配器、集合、I/O
