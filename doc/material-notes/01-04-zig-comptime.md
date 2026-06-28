# Zig Comptime、泛型与反射

> **版本：Zig 0.16.0 / 0.17.0-dev**
> 这是 Zig 最独特的部分——编译期运算不是模板/宏，而是直接运行 Zig 代码。

---

## 0. 总览

Zig 的编译期体系只有一个关键字：`comptime`，但由此衍生出整套泛型、元编程能力：

```
comptime
├── comptime 变量          — 编译期值
├── comptime 参数          — 泛型函数
├── comptime 块            — 编译期计算
├── inline for/while       — 编译期展开循环
├── @typeInfo              — 运行时类型反射（编译期执行）
├── @hasDecl / @hasField   — Duck typing 检查
├── @field / @tagName      — 动态字段访问
└── @Int/@Pointer/...      — 0.16 新：构造类型（替代旧 @Type）
```

---

## 1. comptime 基础

### 1.1 comptime 变量

```zig
// 编译期常量（等价于 C 的 constexpr）
comptime var counter: usize = 0;

// 在编译期运行的计算
const MAX_BUF = comptime blk: {
    const base = 1024;
    const pages = 4;
    break :blk base * pages; // = 4096
};
// MAX_BUF 是编译期常量

// 编译期字符串
const prefix = comptime "prefix_" ++ "suffix";
```

### 1.2 comptime 参数

```zig
// T 必须在调用时编译期已知
fn zeroed(comptime T: type) T {
    return @as(T, 0);
}

const z_u32 = zeroed(u32);    // 0
const z_f64 = zeroed(f64);    // 0.0
// 每种 T 都会生成独立的函数实例（monomorphization）

// comptime 非 type 参数
fn repeat(comptime n: usize, val: u8) [n]u8 {
    return [_]u8{val} ** n;
}

const five_as = repeat(5, 'a'); // [5]u8{ 'a', 'a', 'a', 'a', 'a' }
```

### 1.3 comptime 块

```zig
// 强制在编译期执行
const lookup_table = comptime blk: {
    var table: [256]u8 = undefined;
    for (&table, 0..) |*slot, i| {
        slot.* = if (i >= 'a' and i <= 'z')
            @intCast(i - 'a' + 'A')
        else
            @intCast(i);
    }
    break :blk table;
};
// lookup_table 是编译期计算出的 256 字节数组，运行期直接查表

// 在编译期报错
fn ensurePositive(comptime n: i32) void {
    if (n <= 0) @compileError("n must be positive");
}
```

---

## 2. inline for / inline while

`inline for`/`while` 在编译期展开，每次迭代使用不同的编译期值：

### 2.1 inline for

```zig
const types = .{ u8, u16, u32, u64 };

inline for (types) |T| {
    // 每次迭代 T 是不同的编译期类型
    const val: T = 0;
    std.debug.print("{s}: size={d}\n", .{ @typeName(T), @sizeOf(T) });
    _ = val;
}
// 展开为 4 次独立的代码块，T 各不同

// 对元组使用 inline for
const values = .{ 1, "hello", 3.14, true };
inline for (values, 0..) |v, i| {
    std.debug.print("[{d}] = {any}\n", .{ i, v });
}
```

### 2.2 comptime 字段数组迭代

```zig
const Foo = struct {
    x: u32,
    y: f32,
    name: []const u8,
};

const info = @typeInfo(Foo);
inline for (info.@"struct".fields) |field| {
    std.debug.print("{s}: {s}\n", .{ field.name, @typeName(field.type) });
}
// x: u32
// y: f32
// name: []const u8
```

---

## 3. @typeInfo — 类型反射

`@typeInfo(T)` 返回 `std.builtin.Type`，是一个 tagged union，字段全小写（0.14+ 改为小写）：

```zig
const builtin = @import("builtin");
const Type = std.builtin.Type;

fn describeType(comptime T: type) void {
    const info = @typeInfo(T);
    switch (info) {
        .int => |i| {
            // i.signedness: .signed / .unsigned
            // i.bits: u16（位宽）
            std.debug.print("int: {s}{d}\n", .{
                if (i.signedness == .signed) "i" else "u",
                i.bits,
            });
        },
        .float => |f| {
            std.debug.print("float: f{d}\n", .{f.bits});
        },
        .bool => std.debug.print("bool\n", .{}),
        .void => std.debug.print("void\n", .{}),
        .type => std.debug.print("type\n", .{}),
        .optional => |o| {
            std.debug.print("?", .{});
            describeType(o.child);
        },
        .error_union => |eu| {
            std.debug.print("{}!{s}\n", .{ eu.error_set, @typeName(eu.payload) });
        },
        .pointer => |p| {
            switch (p.size) {
                .One  => std.debug.print("*{s}\n", .{@typeName(p.child)}),
                .Many => std.debug.print("[*]{s}\n", .{@typeName(p.child)}),
                .Slice => std.debug.print("[]{s}\n", .{@typeName(p.child)}),
                .C    => std.debug.print("[*c]{s}\n", .{@typeName(p.child)}),
            }
        },
        .array => |a| {
            std.debug.print("[{d}]{s}\n", .{ a.len, @typeName(a.child) });
        },
        .@"struct" => |s| {
            std.debug.print("struct {{\n", .{});
            for (s.fields) |field| {
                std.debug.print("  {s}: {s}\n", .{ field.name, @typeName(field.type) });
            }
            std.debug.print("}}\n", .{});
        },
        .@"enum" => |e| {
            std.debug.print("enum({s}) {{\n", .{@typeName(e.tag_type)});
            for (e.fields) |field| {
                std.debug.print("  .{s} = {d}\n", .{ field.name, field.value });
            }
            std.debug.print("}}\n", .{});
        },
        .@"union" => |u| {
            std.debug.print("union {{\n", .{});
            for (u.fields) |field| {
                std.debug.print("  .{s}: {s}\n", .{ field.name, @typeName(field.type) });
            }
            std.debug.print("}}\n", .{});
        },
        .@"fn" => |f| {
            std.debug.print("fn(", .{});
            for (f.params, 0..) |param, i| {
                if (i > 0) std.debug.print(", ", .{});
                if (param.type) |t| std.debug.print("{s}", .{@typeName(t)});
            }
            std.debug.print(") {s}\n", .{@typeName(f.return_type orelse void)});
        },
        else => std.debug.print("other: {s}\n", .{@typeName(T)}),
    }
}
```

> **注意 0.14+ 字段名全小写**：`.int`（非`.Int`）、`.@"struct"`（非`.Struct`）、`.@"fn"`（非`.Fn`）

`.@"struct"` 中的 `@"..."` 是 Zig 的标识符转义语法（因为 `struct` 是关键字）。

---

## 4. @hasDecl / @hasField — Duck Typing

```zig
// 检查类型是否有某个声明（方法、常量、嵌套类型...）
fn callInit(comptime T: type, x: *T) void {
    if (@hasDecl(T, "init")) {
        x.init();
    }
}

// 检查结构体是否有某个字段
fn hasXField(comptime T: type) bool {
    return @hasField(T, "x");
}

const A = struct { x: u32, y: u32 };
const B = struct { a: u32, b: u32 };

comptime {
    std.debug.assert(hasXField(A) == true);
    std.debug.assert(hasXField(B) == false);
}
```

### 实用模式：接口鸭子检查

```zig
// 检查是否实现了"Allocator 接口"（有 alloc/free 方法）
fn isAllocator(comptime T: type) bool {
    return @hasDecl(T, "alloc") and @hasDecl(T, "free");
}

// 检查是否有 format 方法（用于 std.fmt 自定义格式化）
fn isFormattable(comptime T: type) bool {
    if (@typeInfo(T) != .@"struct") return false;
    return @hasDecl(T, "format");
}
```

---

## 5. 泛型数据结构

### 5.1 泛型结构体

```zig
// 返回类型即可实现泛型
fn Stack(comptime T: type) type {
    return struct {
        const Self = @This();

        items: std.ArrayListUnmanaged(T) = .{},

        pub fn push(self: *Self, alloc: std.mem.Allocator, val: T) !void {
            try self.items.append(alloc, val);
        }

        pub fn pop(self: *Self) ?T {
            if (self.items.items.len == 0) return null;
            return self.items.pop();
        }

        pub fn peek(self: *const Self) ?T {
            if (self.items.items.len == 0) return null;
            return self.items.items[self.items.items.len - 1];
        }

        pub fn deinit(self: *Self, alloc: std.mem.Allocator) void {
            self.items.deinit(alloc);
        }
    };
}

// 使用
var stack = Stack(u32){};
defer stack.deinit(alloc);

try stack.push(alloc, 1);
try stack.push(alloc, 2);
std.debug.print("{d}\n", .{stack.pop().?}); // 2
```

### 5.2 @This() — 获取当前容器类型

```zig
const Node = struct {
    val: u32,
    next: ?*@This() = null, // 递归引用自身
};

const MyUnion = union(enum) {
    a: u32,
    b: f32,

    pub fn tag(self: @This()) std.meta.Tag(@This()) {
        return self;
    }
};
```

### 5.3 泛型接口（comptime 协议）

```zig
// 定义"接口"规范：必须有 read 函数
fn Reader(comptime Impl: type) type {
    // 编译期检查
    if (!@hasDecl(Impl, "read")) {
        @compileError(@typeName(Impl) ++ " must implement read(self, []u8) !usize");
    }

    return struct {
        impl: Impl,

        pub fn read(self: *@This(), buf: []u8) !usize {
            return self.impl.read(buf);
        }

        pub fn readByte(self: *@This()) !u8 {
            var b: [1]u8 = undefined;
            _ = try self.read(&b);
            return b[0];
        }
    };
}
```

---

## 6. 构造类型：0.16 新 @Int/@Pointer 等

> **0.16 重大变更**：`@Type(.{.Int = ...})` 被废弃，拆分为独立内置函数。

### 6.1 @Int — 动态生成整数类型

```zig
// OLD（0.15 及以前，0.16 已废弃）：
// const T = @Type(.{ .Int = .{ .signedness = .unsigned, .bits = 32 } });

// NEW（0.16+）：
const T = @Int(.unsigned, 32);  // u32
const S = @Int(.signed, 16);    // i16

// 动态位宽整数（生成任意位宽类型）
fn intType(comptime bits: u16) type {
    return @Int(.unsigned, bits);
}
const u12 = intType(12);
const u42 = intType(42);
```

### 6.2 @Pointer — 动态生成指针类型

```zig
// NEW（0.16+）：
const PtrU8 = @Pointer(.{
    .size = .One,         // .One/.Many/.Slice/.C
    .is_const = true,
    .is_volatile = false,
    .alignment = 1,
    .address_space = .generic,
    .child = u8,
    .is_allowzero = false,
    .sentinel = null,
});
// PtrU8 = *const u8
```

### 6.3 其他类型构造器（0.16+）

```zig
// 动态生成结构体类型
const DynStruct = @Struct(.{
    .layout = .auto,
    .fields = &.{
        .{ .name = "x", .type = u32, .default_value_ptr = null, .is_comptime = false, .alignment = 4 },
        .{ .name = "y", .type = f32, .default_value_ptr = null, .is_comptime = false, .alignment = 4 },
    },
    .decls = &.{},
    .is_tuple = false,
});

// 动态生成枚举
// @Enum、@Union、@Fn、@Tuple、@EnumLiteral 均遵循类似模式
```

### 6.4 实用：动态生成位字段结构体

```zig
// 根据 comptime 参数生成不同布局的寄存器结构体
fn RegisterBits(comptime fields: []const struct { name: []const u8, bits: u8 }) type {
    // 计算总位宽
    comptime var total_bits: u16 = 0;
    for (fields) |f| total_bits += f.bits;

    // 构造 packed struct 字段
    comptime var struct_fields: [fields.len]std.builtin.Type.StructField = undefined;
    comptime {
        for (&struct_fields, fields) |*sf, f| {
            sf.* = .{
                .name = f.name,
                .type = @Int(.unsigned, f.bits),
                .default_value_ptr = null,
                .is_comptime = false,
                .alignment = 0,
            };
        }
    }

    return @Struct(.{
        .layout = .@"packed",
        .backing_integer = @Int(.unsigned, total_bits),
        .fields = &struct_fields,
        .decls = &.{},
        .is_tuple = false,
    });
}

const MyReg = RegisterBits(&.{
    .{ .name = "enable", .bits = 1 },
    .{ .name = "mode",   .bits = 3 },
    .{ .name = "addr",   .bits = 12 },
});
// 生成：packed struct(u16) { enable: u1, mode: u3, addr: u12 }
```

---

## 7. @field / @tagName / std.meta

### 7.1 @field — 按名字字符串访问字段

```zig
const Point = struct { x: f32, y: f32 };
var p = Point{ .x = 1.0, .y = 2.0 };

const field_name = "x";
std.debug.print("{f}\n", .{@field(p, field_name)}); // 1.0

// 常用于泛型序列化
fn printAllFields(val: anytype) void {
    const T = @TypeOf(val);
    inline for (@typeInfo(T).@"struct".fields) |field| {
        std.debug.print("{s} = {any}\n", .{ field.name, @field(val, field.name) });
    }
}
```

### 7.2 @tagName — 枚举/联合 tag 名字

```zig
const Dir = enum { north, south, east, west };

const d = Dir.north;
std.debug.print("{s}\n", .{@tagName(d)}); // "north"

// 联合体
const Val = union(enum) { a: u32, b: f32 };
const v = Val{ .a = 1 };
std.debug.print("{s}\n", .{@tagName(v)}); // "a"
```

### 7.3 std.meta 工具

```zig
const meta = std.meta;

// 获取 tagged union 的 tag 枚举类型
const ValTag = meta.Tag(Val); // enum { a, b }

// 字段名称列表
const field_names = meta.fieldNames(Point); // []const []const u8

// 枚举字段数量
const enum_count = @typeInfo(Dir).@"enum".fields.len;

// 类型 eql
comptime std.debug.assert(meta.eql(u32, u32));
comptime std.debug.assert(!meta.eql(u32, i32));
```

---

## 8. 实战：类型安全的 SBI 版本门控

这是一个 Zig comptime 在 ku-sbi 中的真实应用模式：

```zig
// src/spec/version.zig

pub const Signedness = enum { pre_release, released };

pub const SbiVersion = struct {
    major: u16,
    minor: u16,
    pre:   ?PreRelease = null,

    pub const PreRelease = union(enum) {
        rc: u8,
    };

    // 编译期比较
    pub fn ge(comptime self: SbiVersion, comptime other: SbiVersion) bool {
        if (self.major != other.major) return self.major > other.major;
        return self.minor >= other.minor;
    }

    pub const v1_0 = SbiVersion{ .major = 1, .minor = 0 };
    pub const v2_0 = SbiVersion{ .major = 2, .minor = 0 };
    pub const v3_0 = SbiVersion{ .major = 3, .minor = 0 };
};

// 版本门控：只在目标版本 >= 2.0 时编译此扩展
fn nacl_extension(comptime target_ver: SbiVersion) type {
    if (!target_ver.ge(SbiVersion.v2_0)) {
        return struct {}; // 空类型，零开销
    }
    return struct {
        pub const eid: u32 = 0x4E41434C;

        pub fn probe() bool {
            return true;
        }
        // ... NACL 实现
    };
}

// 使用
const NaclV3 = nacl_extension(SbiVersion.v3_0); // 包含实现
const NaclV1 = nacl_extension(SbiVersion.v1_0); // 空类型
```

---

## 9. anytype 的局限与 comptime 接口模式

```zig
// anytype 可以接受任意类型，但没有编译期类型检查提示
fn process(x: anytype) void {
    // 在函数体内用 @typeInfo/@hasDecl 做检查
    const T = @TypeOf(x);
    if (!@hasDecl(T, "required_method")) {
        @compileError(@typeName(T) ++ " does not have required_method");
    }
    x.required_method();
}

// 更好的模式：显式 comptime 检查函数
fn assertHasAllocInterface(comptime T: type) void {
    if (!@hasDecl(T, "alloc"))   @compileError(@typeName(T) ++ ": missing alloc");
    if (!@hasDecl(T, "free"))    @compileError(@typeName(T) ++ ": missing free");
    if (!@hasDecl(T, "resize"))  @compileError(@typeName(T) ++ ": missing resize");
}

fn MyContainer(comptime AllocImpl: type) type {
    comptime assertHasAllocInterface(AllocImpl);
    return struct {
        alloc_impl: AllocImpl,
        // ...
    };
}
```

---

## 10. 编译期计算极限

```zig
// Zig 默认允许 1000 次分支（comptime 循环迭代）
// 对于大循环需要提升配额
pub fn fibonacci(comptime n: u32) u64 {
    @setEvalBranchQuota(100_000);
    if (n <= 1) return n;
    return fibonacci(n - 1) + fibonacci(n - 2);
}

const fib50 = comptime fibonacci(50); // 编译期算出 fibonacci(50)
```

---

## 11. Builtin 函数与自定义工具函数

### 11.1 @ 函数是编译器保留前缀

Zig 的 `@xxx` 前缀是**编译器内置函数**，用户不能新增 @ 函数。所有内置函数都在编译器里硬编码，不能通过语言本身扩展。

```zig
// 常用 @ 内置函数速查
@import("std")            // 导入模块
@typeInfo(T)              // 类型反射（返回 std.builtin.Type）
@typeName(T)              // 类型名字符串（编译期）
@compileError("msg")      // 触发编译错误（用于断言）
@compileLog(val)          // 编译期打印（不报错，调试用）
@setEvalBranchQuota(n)    // 提升 comptime 迭代次数上限
@panic("msg")             // 运行时 panic
@ptrCast(ptr)             // 指针类型转换（unsafe）
@bitCast(val)             // 位重解释（unsafe）
@sizeOf(T)                // 类型字节大小
@alignOf(T)               // 类型对齐要求
@offsetOf(T, "field")     // 字段偏移量
@field(obj, "name")       // 按字符串名访问字段
@hasDecl(T, "name")       // 编译期检查 T 是否有声明
@hasField(T, "name")      // 编译期检查 T 是否有字段
```

### 11.2 用 comptime 函数模拟 @ 工具函数

虽然不能创建 @ 函数，但可以用普通 comptime 函数达到同样效果：

```zig
// 检查类型是否有某个方法（@hasDecl 的封装）
pub fn hasMethod(comptime T: type, comptime name: []const u8) bool {
    return switch (@typeInfo(T)) {
        .@"struct", .@"union", .@"enum", .@"opaque" => @hasDecl(T, name),
        else => false,
    };
}

// 获取结构体所有字段名（编译期数组）
pub fn fieldNames(comptime T: type) []const []const u8 {
    const fields = @typeInfo(T).@"struct".fields;
    comptime var names: [fields.len][]const u8 = undefined;
    inline for (fields, 0..) |f, i| names[i] = f.name;
    return &names;
}

// 用例
const S = struct { x: i32, y: f64, z: bool };
const names = comptime fieldNames(S); // ["x", "y", "z"] 编译期常量
comptime std.debug.assert(!hasMethod(S, "deinit"));
```

### 11.3 编译期安全保障

comptime 评估完全在编译期运行，有多重安全保障：

```zig
// 1. @compileError：精确的编译错误（用于接口约束）
fn assertReadable(comptime T: type) void {
    if (!@hasDecl(T, "read")) {
        @compileError(@typeName(T) ++ " must implement pub fn read(*Self) ![]u8");
    }
}

// 2. 越界/溢出在 comptime 中是编译错误，不是运行时崩溃
const arr = [_]u8{ 1, 2, 3 };
const x = comptime arr[5];          // compile error: index out of bounds
const y = comptime @as(u8, 255) + 1; // compile error: integer overflow

// 3. 类型约束：编译期检查代替运行时断言
pub fn safeDiv(comptime T: type, a: T, b: T) T {
    switch (@typeInfo(T)) {
        .int, .float, .comptime_int, .comptime_float => {},
        else => @compileError("safeDiv: numeric type required, got " ++ @typeName(T)),
    }
    return a / b;
}

// 4. 编译期生成类型安全代码（零运行时开销）
pub fn RegFile(comptime n: usize) type {
    return struct {
        regs: [n]u64 = [_]u64{0} ** n,
        pub fn read(self: *const @This(), idx: usize) u64 {
            std.debug.assert(idx < n); // debug 模式运行时检查
            return self.regs[idx];
        }
    };
}
const RV64RegFile = RegFile(32); // RISC-V 32个通用寄存器，编译期确定大小
```

### 11.4 comptime 常用模式速查

```zig
// 枚举所有字段并批量初始化
inline for (@typeInfo(MyStruct).@"struct".fields) |field| {
    @field(instance, field.name) = std.mem.zeroes(field.type);
}

// 编译期字符串拼接（生成错误信息/标识符）
const msg = comptime "expect_" ++ @typeName(T) ++ "_got_non_numeric";

// 平台分支（编译期常量，无运行时开销）
const RegWidth: type = if (@sizeOf(usize) == 8) u64 else u32;

// comptime 计算链接期常量（裸机常用）
const PAGE_SHIFT: comptime_int = std.math.log2_int(u64, 4096); // 12
const PTE_COUNT: comptime_int   = 4096 / @sizeOf(usize);       // 512（sv39）
```

---

## 练习

### 练习 8：泛型 Stack

```zig
// TODO: 实现泛型 Stack(T)，要求：
// - push(alloc, val) !void
// - pop() ?T
// - peek() ?T
// - len() usize
// - deinit(alloc) void
fn Stack(comptime T: type) type {
    // 填充这里
    return struct {};
}

test "Stack(u32)" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const a = gpa.allocator();

    var s = Stack(u32){};
    defer s.deinit(a);

    try s.push(a, 1);
    try s.push(a, 2);
    try s.push(a, 3);

    try std.testing.expectEqual(@as(usize, 3), s.len());
    try std.testing.expectEqual(@as(?u32, 3), s.pop());
    try std.testing.expectEqual(@as(?u32, 2), s.peek());
    try std.testing.expectEqual(@as(usize, 2), s.len());
}
```

### 练习 9：类型反射

```zig
// TODO: 实现 structToJson，将任意结构体（无嵌套）转为 JSON 字符串
// 仅支持字段类型：u32, i32, f64, bool, []const u8
// 例如 Point{.x=1.0, .y=2.0} → {"x":1.0,"y":2.0}
fn structToJson(alloc: std.mem.Allocator, val: anytype) ![]u8 {
    // 填充这里（提示：用 inline for + @typeInfo + @field）
    _ = alloc;
    _ = val;
    return "";
}
```

### 练习 10：comptime 查找表

```zig
// TODO: 在编译期生成 sin 查找表（0°到359°，整数角度）
// 精度到小数点后 4 位（存为 i16，单位 0.0001）
const SIN_TABLE = comptime blk: {
    // 填充这里
    break :blk [360]i16{};
};

test "SIN_TABLE" {
    // sin(0°) = 0
    try std.testing.expectEqual(@as(i16, 0), SIN_TABLE[0]);
    // sin(90°) ≈ 1.0000 → 10000
    try std.testing.expectApproxEqAbs(@as(f32, 10000.0), @as(f32, @floatFromInt(SIN_TABLE[90])), 5.0);
}
```

---

> 下一节：[01-05-zig-freestanding.md](01-05-zig-freestanding.md) — 裸机/no_std 环境
