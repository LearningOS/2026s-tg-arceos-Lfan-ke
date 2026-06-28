# Zig 构建系统与包管理器

> **版本：Zig 0.16.0**（参考笔记定位 0.16 stable）
> Zig 自带构建系统（`build.zig`）+ 包管理器（`build.zig.zon`），零外部依赖。
>
> ⚠️ **`std.Build` API 是高频变动区**：0.15/0.16 把 `addStaticLibrary`/`addSharedLibrary` 合并为 `b.addLibrary(.{ .linkage = .static/.dynamic, ... })`，`addExecutable`/`addTest` 改为接收 `.root_module = b.createModule(.{ .root_source_file, .target, .optimize })`。本篇示例展示**思路与步骤**，确切函数签名以官方 0.16 文档为准。

---

## 0. 总览

Zig 的构建系统是用 **Zig 本身写的**：

```
project/
├── build.zig         ← 构建脚本（Zig 代码）
├── build.zig.zon     ← 包声明 + 依赖（ZON = Zig Object Notation）
└── src/
    ├── main.zig
    └── lib.zig
```

构建流程：

```
zig build
  ↓
编译 build.zig（调用 std.Build API）
  ↓
生成依赖图（steps）
  ↓
并行执行编译步骤
  ↓
产物放入 zig-out/
```

常用命令：

```sh
zig build              # 默认步骤（通常 install）
zig build run          # 构建并运行
zig build test         # 运行所有测试
zig build -Doptimize=ReleaseFast  # 优化模式
zig build -Dtarget=riscv64-freestanding-none  # 交叉编译
zig build --help       # 查看所有可用步骤和选项
```

---

## 1. build.zig 基础

### 1.1 最小 build.zig

```zig
const std = @import("std");

pub fn build(b: *std.Build) void {
    // 目标平台（默认本机）
    const target   = b.standardTargetOptions(.{});
    // 优化模式（默认 Debug）
    const optimize = b.standardOptimizeOption(.{});

    // 定义可执行文件
    const exe = b.addExecutable(.{
        .name             = "myapp",
        .root_source_file = b.path("src/main.zig"),
        .target           = target,
        .optimize         = optimize,
    });

    // 安装到 zig-out/bin/
    b.installArtifact(exe);

    // 添加 `zig build run` 步骤
    const run = b.addRunArtifact(exe);
    const run_step = b.step("run", "Run the application");
    run_step.dependOn(&run.step);

    // 添加 `zig build test` 步骤
    const tests = b.addTest(.{
        .root_source_file = b.path("src/main.zig"),
        .target           = target,
        .optimize         = optimize,
    });
    const run_tests = b.addRunArtifact(tests);
    const test_step = b.step("test", "Run unit tests");
    test_step.dependOn(&run_tests.step);
}
```

### 1.2 编译静态库

```zig
const lib = b.addStaticLibrary(.{
    .name             = "mylib",
    .root_source_file = b.path("src/lib.zig"),
    .target           = target,
    .optimize         = optimize,
});
b.installArtifact(lib);
```

### 1.3 编译动态库

```zig
const dynlib = b.addSharedLibrary(.{
    .name             = "mylib",
    .root_source_file = b.path("src/lib.zig"),
    .target           = target,
    .optimize         = optimize,
    .version          = .{ .major = 1, .minor = 0, .patch = 0 },
});
b.installArtifact(dynlib);
```

---

## 2. 模块系统

### 2.1 模块（Module）概念

模块是 Zig 0.12+ 引入的概念，取代了旧的 `addPackage`：

```zig
// 创建模块
const utils_mod = b.addModule("utils", .{
    .root_source_file = b.path("src/utils.zig"),
});

// 将模块添加到可执行文件（在 Zig 代码中用 @import("utils") 导入）
exe.root_module.addImport("utils", utils_mod);
```

在 `src/main.zig` 中：
```zig
const utils = @import("utils"); // 对应 "utils" 模块
```

### 2.2 模块间依赖

```zig
// 模块 A
const mod_a = b.addModule("core", .{
    .root_source_file = b.path("src/core.zig"),
});

// 模块 B 依赖模块 A
const mod_b = b.addModule("net", .{
    .root_source_file = b.path("src/net.zig"),
    .imports = &.{
        .{ .name = "core", .module = mod_a },
    },
});

exe.root_module.addImport("net", mod_b);
```

---

## 3. 目标与优化

### 3.1 内置选项

```zig
// 允许用户通过命令行指定目标
const target = b.standardTargetOptions(.{
    .default_target = .{
        .cpu_arch = .riscv64,
        .os_tag   = .freestanding,
        .abi      = .none,
    },
});

// 允许用户通过命令行指定优化
const optimize = b.standardOptimizeOption(.{
    .preferred_optimize_mode = .ReleaseSmall,
});
```

### 3.2 手动指定目标

```zig
// 固定目标，不允许用户修改
const target = b.resolveTargetQuery(.{
    .cpu_arch    = .riscv64,
    .os_tag      = .freestanding,
    .abi         = .none,
    .cpu_model   = .{ .explicit = &std.Target.riscv.cpu.generic_rv64 },
    .cpu_features_add = std.Target.riscv.featureSet(&.{
        .m, .a, .f, .d, .c, // RISC-V 扩展
    }),
});
```

### 3.3 优化模式

| 模式 | 命令 | 说明 |
|------|------|------|
| `Debug` | 默认 | 所有安全检查，最慢 |
| `ReleaseSafe` | `-Doptimize=ReleaseSafe` | 保留运行期检查，优化速度 |
| `ReleaseFast` | `-Doptimize=ReleaseFast` | 关闭所有检查，最快 |
| `ReleaseSmall` | `-Doptimize=ReleaseSmall` | 优化体积（裸机首选）|

---

## 4. 自定义构建选项

```zig
pub fn build(b: *std.Build) void {
    // 布尔选项
    const enable_log = b.option(
        bool,
        "log",          // 命令行参数名：-Dlog=true
        "Enable debug logging",
    ) orelse false;

    // 枚举选项
    const log_level = b.option(
        enum { debug, info, warn, err },
        "log-level",
        "Log verbosity",
    ) orelse .info;

    // 整数选项
    const uart_base = b.option(
        u64,
        "uart-base",
        "UART MMIO base address",
    ) orelse 0x10000000;

    // 字符串选项
    const board = b.option(
        []const u8,
        "board",
        "Target board name",
    ) orelse "qemu-virt";

    // 将选项传入 Zig 代码（通过 options 模块）
    const options = b.addOptions();
    options.addOption(bool, "enable_log", enable_log);
    options.addOption(u64,  "uart_base",  uart_base);
    options.addOption([]const u8, "board", board);
    _ = log_level;

    exe.root_module.addOptions("config", options);
}
```

在 Zig 代码中使用：
```zig
const config = @import("config"); // 由 build.zig 生成

if (config.enable_log) {
    log("uart base: 0x{x}\n", .{config.uart_base});
}
```

---

## 5. 链接器选项

### 5.1 链接脚本

```zig
exe.setLinkerScriptPath(b.path("linker.ld"));
```

### 5.2 链接系统库

```zig
// 链接 C 标准库
exe.linkLibC();

// 链接系统库（通过 pkg-config）
exe.linkSystemLibrary("ssl");
exe.linkSystemLibrary("crypto");

// 添加头文件目录
exe.addIncludePath(b.path("include/"));

// 链接静态库（.a 文件）
exe.addObjectFile(b.path("lib/libfoo.a"));
```

### 5.3 C 源文件

```zig
// 编译 C 文件并链接
exe.addCSourceFile(.{
    .file  = b.path("src/c_helper.c"),
    .flags = &.{ "-std=c11", "-O2" },
});

// 批量添加
exe.addCSourceFiles(.{
    .files = &.{ "src/a.c", "src/b.c" },
    .flags = &.{"-std=c11"},
});
```

### 5.4 汇编文件

```zig
exe.addAssemblyFile(b.path("src/start.S"));
```

---

## 6. 构建步骤（Steps）

### 6.1 内置步骤

```zig
// 运行任意命令
const cmd = b.addSystemCommand(&.{ "python3", "scripts/gen.py" });
exe.step.dependOn(&cmd.step);

// 复制文件
const cp = b.addInstallFile(b.path("firmware.bin"), "bin/firmware.bin");
b.getInstallStep().dependOn(&cp.step);

// 创建目录
const mkdir = b.addMakeDir(b.path("zig-out/sbi"));
_ = mkdir;
```

### 6.2 自定义步骤（WriteFile）

```zig
// 在构建时生成文件
const gen = b.addWriteFiles();
_ = gen.add("generated/version.zig",
    \\pub const VERSION = "0.1.0";
    \\pub const GIT_HASH = "abcdef";
);
exe.root_module.addImport("version", b.createModule(.{
    .root_source_file = gen.getDirectory().path(b, "generated/version.zig"),
}));
```

### 6.3 多步骤依赖

```zig
const gen_step   = b.step("gen",   "Generate files");
const build_step = b.step("fw",    "Build firmware");
const flash_step = b.step("flash", "Flash to device");

build_step.dependOn(gen_step);
flash_step.dependOn(build_step);

// 自定义 flash 命令
const flash_cmd = b.addSystemCommand(&.{
    "openocd", "-f", "interface/jlink.cfg",
    "-f", "target/riscv.cfg",
    "-c", "program zig-out/bin/ku-sbi.elf verify reset exit",
});
flash_step.dependOn(&flash_cmd.step);
```

---

## 7. 包管理器

### 7.1 build.zig.zon 格式

```zig
// build.zig.zon（ZON = Zig Object Notation，语法类似 Zig 的匿名结构体）
.{
    .name    = .my_project,
    .version = "0.1.0",
    .minimum_zig_version = "0.16.0",

    .dependencies = .{
        // 远程依赖（通过 URL + hash）
        .zap = .{
            .url  = "https://github.com/zigzap/zap/archive/refs/tags/v0.9.0.tar.gz",
            .hash = "1220...",  // zig fetch 自动填充
        },

        // 本地依赖（相对路径）
        .my_lib = .{
            .path = "../my_lib",
        },
    },

    // 导出的模块（其他包可以依赖）
    .paths = .{
        "build.zig",
        "build.zig.zon",
        "src/",
    },
}
```

### 7.2 添加依赖

```sh
# 方法 1：自动下载并填充 hash
zig fetch --save https://github.com/foo/bar/archive/main.tar.gz

# 方法 2：本地路径
# 手动编辑 build.zig.zon 添加 .path = "../other_proj"
```

### 7.3 在 build.zig 中使用依赖

```zig
pub fn build(b: *std.Build) void {
    // ...

    // 获取依赖
    const zap_dep = b.dependency("zap", .{
        .target   = target,
        .optimize = optimize,
    });

    // 使用依赖暴露的模块
    exe.root_module.addImport("zap", zap_dep.module("zap"));

    // 链接依赖的 artifact
    exe.linkLibrary(zap_dep.artifact("zap"));
}
```

### 7.4 将自己的库发布为可依赖包

```zig
// 你的库的 build.zig
pub fn build(b: *std.Build) void {
    const target   = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // 定义库
    const lib = b.addStaticLibrary(.{
        .name             = "mylib",
        .root_source_file = b.path("src/root.zig"),
        .target           = target,
        .optimize         = optimize,
    });

    // 暴露模块给外部（关键！）
    _ = b.addModule("mylib", .{
        .root_source_file = b.path("src/root.zig"),
    });

    b.installArtifact(lib);
}
```

---

## 8. ku-sbi 完整 build.zig

```zig
const std = @import("std");

pub fn build(b: *std.Build) void {
    // ===== 目标平台 =====
    const target = b.resolveTargetQuery(.{
        .cpu_arch = .riscv64,
        .os_tag   = .freestanding,
        .abi      = .none,
        .cpu_features_add = std.Target.riscv.featureSet(&.{
            .m, .a, .f, .d, .c,
        }),
    });

    // ===== 优化 =====
    const optimize = b.standardOptimizeOption(.{
        .preferred_optimize_mode = .ReleaseSmall,
    });

    // ===== 构建选项 =====
    const sbi_version = b.option(
        []const u8,
        "sbi-version",
        "Target SBI spec version (e.g. '3.0')",
    ) orelse "3.0";

    const uart_base = b.option(
        u64,
        "uart-base",
        "UART MMIO base address",
    ) orelse 0x10000000;

    const options = b.addOptions();
    options.addOption([]const u8, "sbi_version", sbi_version);
    options.addOption(u64, "uart_base", uart_base);

    // ===== 固件 ELF =====
    const fw = b.addExecutable(.{
        .name             = "ku-sbi",
        .root_source_file = b.path("src/main.zig"),
        .target           = target,
        .optimize         = optimize,
    });

    fw.setLinkerScriptPath(b.path("linker.ld"));
    fw.root_module.addOptions("config", options);

    // 无标准库
    fw.root_module.code_model = .medium;

    b.installArtifact(fw);

    // ===== 生成 .bin =====
    const objcopy = b.addObjCopy(fw.getEmittedBin(), .{
        .format = .bin,
    });
    const install_bin = b.addInstallBinFile(
        objcopy.getOutput(),
        "ku-sbi.bin",
    );
    b.getInstallStep().dependOn(&install_bin.step);

    // ===== 测试 =====
    const tests = b.addTest(.{
        .root_source_file = b.path("src/test_all.zig"),
        // 测试在宿主机上跑（非裸机目标）
        .target   = b.standardTargetOptions(.{}),
        .optimize = optimize,
    });
    const run_tests = b.addRunArtifact(tests);
    const test_step = b.step("test", "Run unit tests on host");
    test_step.dependOn(&run_tests.step);

    // ===== QEMU 运行 =====
    const qemu = b.addSystemCommand(&.{
        "qemu-system-riscv64",
        "-machine", "virt",
        "-bios",    "none",
        "-kernel",  "zig-out/bin/ku-sbi",
        "-nographic",
        "-serial",  "mon:stdio",
    });
    qemu.step.dependOn(b.getInstallStep());
    const qemu_step = b.step("qemu", "Run in QEMU");
    qemu_step.dependOn(&qemu.step);
}
```

---

## 9. 项目结构推荐（ku-sbi）

```
ku-sbi/
├── build.zig
├── build.zig.zon
├── linker.ld
├── specs/
│   └── riscv-sbi-v3.0.pdf
└── src/
    ├── main.zig         ← 固件入口（调用 sbi_init）
    ├── start.zig        ← _start 汇编 + BSS 清零
    ├── panic.zig        ← panic handler
    ├── test_all.zig     ← 宿主机测试入口
    ├── spec/            ← SBI 规范类型定义
    │   ├── version.zig  ← SbiVersion + comptime 比较
    │   ├── eid.zig      ← EID/FID 常量
    │   └── ret.zig      ← SbiRet + 错误码
    ├── base/            ← Base Extension (0x10)
    │   └── mod.zig
    ├── time/            ← TIME Extension (0x54494D45)
    │   └── mod.zig
    ├── dbcn/            ← Debug Console (0x4442434E)
    │   └── mod.zig
    ├── hal/             ← 平台抽象层
    │   ├── timer.zig    ← TimerHal interface
    │   ├── console.zig  ← ConsoleHal interface
    │   └── ipi.zig      ← IpiHal interface
    ├── platform/        ← 具体平台实现
    │   ├── qemu_virt.zig
    │   └── sifive_u.zig
    └── trap/            ← M-mode 陷阱处理
        ├── handler.zig
        └── ecall.zig    ← ecall 分发
```

---

## 10. build.zig.zon 完整示例

```zig
.{
    .name    = .ku_sbi,
    .version = "0.1.0",
    .minimum_zig_version = "0.16.0",

    .dependencies = .{
        // 目前 ku-sbi 无外部依赖
        // 如果将来需要，例如：
        // .zig_serial = .{
        //     .url  = "https://...",
        //     .hash = "1220...",
        // },
    },

    .paths = .{
        "build.zig",
        "build.zig.zon",
        "src/",
        "linker.ld",
        "specs/",
    },
}
```

---

## 练习

### 练习 16：构建系统

```
1. zig build        → 编译 riscv64-freestanding 固件 ELF 到 zig-out/bin/ku-sbi
2. zig build test   → 编译并运行宿主机单元测试
3. zig build qemu   → 用 QEMU virt 机器运行固件（-nographic）
4. 自定义选项 -Duart-base=0x10000000 传入代码
```

### 练习 17：包依赖

```
TODO: 将 src/spec/ 目录抽成独立的 Zig 包（sbi-spec），
使 ku-sbi 和假想的 sbi-test-harness 都能依赖它。
需要：
1. 在 src/spec/ 创建独立的 build.zig + build.zig.zon
2. ku-sbi 的 build.zig 通过本地路径依赖 sbi-spec
3. 验证 @import("sbi-spec") 能访问 SbiVersion 等类型
```

---

> **全部笔记结构**：
> - [01-01-zig-basics.md](01-01-zig-basics.md) — 基础语法
> - [01-03-zig-stdlib.md](01-03-zig-stdlib.md) — 标准库
> - [01-04-zig-comptime.md](01-04-zig-comptime.md) — Comptime & 泛型
> - [01-05-zig-freestanding.md](01-05-zig-freestanding.md) — 裸机环境
> - [01-06-zig-async.md](01-06-zig-async.md) — 异步与协程
> - [01-02-zig-build.md](01-02-zig-build.md) — 构建系统（本文）
