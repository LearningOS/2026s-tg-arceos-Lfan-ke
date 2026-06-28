# Zig 异步与协程

> **版本：Zig 0.16.0 / 0.17.0-dev**
> **重要历史**：0.15 移除了 `async`/`await`/`suspend`/`resume` 关键字，0.16 引入 `std.Io` 接口作为新并发模型基础，0.17-dev 正在落地 `io.async()`/`io.await()` 协程语义。

---

## 0. 历史演变：Zig 异步的三个时代

省流：尚未成熟

```
Zig 0.13/0.14 (旧时代)
  async fn, await, suspend, resume, nosuspend
  std.event.Loop
  ↓ 0.15 全部移除，重新设计

Zig 0.15/0.16 (过渡期)
  关键字全部删掉
  std.Io 接口作为 I/O + 并发统一抽象
  普通函数 + 手工状态机 + std.Thread 作为过渡

Zig 0.17-dev (新时代，落地中)
  io.async(frame)  — 启动协程帧
  io.await(handle) — 等待协程结果
  基于 std.Io 事件循环，无关键字，无栈切换魔法
```

> **如果你在写生产代码**：0.17 的 async API 仍在设计中，ABI 可能继续变化。
> **如果你在学原理**：本章覆盖三种策略——手工状态机、std.Thread、新 io.async。

---

## 1. 函数染色问题（Function Coloring）

### 1.1 什么是函数染色

Bob Nystrom 2015 年的文章《What Color is Your Function?》提出的概念：
当一门语言引入 async/await 后，函数被隐式分成两种颜色（两种类型）：

```
普通函数（"红色"）    async 函数（"蓝色"）
───────────────────    ──────────────────
可以调用普通函数  ✓    可以调用普通函数  ✓
可以调用 async ✗       可以调用 async   ✓
不会传染         ─     会传染上层调用方 ⚡
```

**传染性**是核心问题：一旦调用链最底层有 async 函数，整条链都必须变成 async。

```python
# Python 示例：染色传染
async def fetch_data(): ...          # async 函数（蓝色）

async def process():                  # 必须也是 async（被感染）
    data = await fetch_data()

async def main():                     # 必须也是 async（被感染）
    await process()

# 同步的 sync_worker() 无法调用 fetch_data()，必须用 asyncio.run()
```

### 1.2 各语言如何处理染色问题

| 语言 | 方案 | 结果 |
|------|------|------|
| Python / JavaScript / Rust（旧） | async/await 关键字修饰函数 | **有染色**，传染整条调用链 |
| Go | goroutine + channel | **无染色**，所有函数可在协程内调用 |
| Java（虚拟线程, JDK21） | 阻塞调用自动挂起 | **无染色**，普通函数在虚拟线程上运行 |
| Zig 0.13/0.14（旧） | `async fn` 关键字 | **有染色**（同 Rust） |
| Zig 0.16+（新） | 函数通过 `io` 参数注入调度器 | **无染色**，协程是普通函数 |

### 1.3 Zig 如何消除染色

旧设计（0.13/0.14）的问题：

```zig
// 旧 Zig：async fn 是特殊类型，染色传染
async fn fetchData() []u8 { ... }      // 蓝色函数
async fn processData() void {          // 被迫变蓝
    const data = await fetchData();
}
// 普通函数无法调用 processData()
```

新设计（0.16+）的解法：**协程能力通过 `io` 参数注入，函数本身不染色**：

```zig
// 新 Zig：普通函数，只要接受 io 参数就可以异步
fn fetchData(io: std.Io, url: []const u8) ![]u8 {
    return io.http.get(url); // io 决定是同步还是异步，函数本身不知道
}

fn processData(io: std.Io) !void {
    const data = try fetchData(io, "http://example.com");
    _ = data;
}

// 同一个函数，在不同 Io 后端下行为不同：
// - 单线程 Io → 顺序执行
// - 线程池 Io → 并发执行
// - io_uring Io → 异步 I/O
```

> **核心洞察**：旧设计把"是否异步"编码在**函数类型**里（染色）；新设计把"如何调度"编码在**参数**里（注入），函数本身是中性的。
>
> 吐槽：所以本质还是没有摆脱染色问题，只是把染色问题推迟到调用方决定是哪种调度方式

### 1.4 为什么移除了旧的 async/await？

旧设计的完整问题列表（Andrew Kelley 的总结）：

1. **Colored function problem**：`async fn` 传染整条调用链
2. **编译器复杂度极高**：async 实现占用大量编译器代码，难以维护
3. **无真正零成本**：每个 async 帧需要堆分配或调用者栈分配，帧大小难以静态计算
4. **与 comptime 交互复杂**：async + comptime 边界行为难以理解

新设计目标：
- 协程是普通函数，不是特殊类型
- 调度器通过 `std.Io` 接口注入，可替换
- 零关键字：`io.async()` 就是普通函数调用

---

## 2. 策略一：手工状态机（永远有效，裸机友好）

状态机是所有协程的本质。在任何 Zig 版本、任何平台都能用。

### 2.1 基础状态机

```zig
const State = enum { idle, running, waiting, done };

const Task = struct {
    state: State = .idle,
    progress: usize = 0,

    // 每次调用推进一步（协作式调度）
    pub fn poll(self: *Task) bool {
        switch (self.state) {
            .idle => {
                self.state = .running;
                self.progress = 0;
                return false; // 未完成
            },
            .running => {
                self.progress += 1;
                if (self.progress >= 10) {
                    self.state = .done;
                    return true; // 完成
                }
                return false;
            },
            .waiting => {
                // 检查外部条件
                if (checkCondition()) {
                    self.state = .running;
                }
                return false;
            },
            .done => return true,
        }
    }
};

fn checkCondition() bool { return true; } // 示例

// 调度器：轮询所有任务直到全部完成
fn runToCompletion(tasks: []Task) void {
    var all_done = false;
    while (!all_done) {
        all_done = true;
        for (tasks) |*task| {
            if (!task.poll()) all_done = false;
        }
    }
}
```

### 2.2 生成器模式（Producer）

```zig
// 模拟 Python yield：每次 next() 返回下一个值
const RangeGen = struct {
    current: usize,
    end: usize,

    pub fn init(start: usize, end: usize) RangeGen {
        return .{ .current = start, .end = end };
    }

    pub fn next(self: *RangeGen) ?usize {
        if (self.current >= self.end) return null;
        defer self.current += 1;
        return self.current;
    }
};

var gen = RangeGen.init(0, 5);
while (gen.next()) |val| {
    std.debug.print("{d} ", .{val}); // 0 1 2 3 4
}
```

### 2.3 Fibonacci 生成器

```zig
const FibGen = struct {
    a: u64 = 0,
    b: u64 = 1,

    pub fn next(self: *FibGen) u64 {
        const result = self.a;
        const next_b = self.a + self.b;
        self.a = self.b;
        self.b = next_b;
        return result;
    }
};

var fib = FibGen{};
for (0..10) |_| {
    std.debug.print("{d} ", .{fib.next()});
}
// 0 1 1 2 3 5 8 13 21 34
```

---

## 3. 策略二：std.Thread（OS 多线程）

适合有 OS 的环境，每个任务一个线程，共享内存通过互斥量保护。

### 3.1 基本线程

```zig
const std = @import("std");

fn workerFn(id: usize) void {
    std.debug.print("worker {d} start\n", .{id});
    std.time.sleep(10 * std.time.ns_per_ms); // 10ms
    std.debug.print("worker {d} done\n", .{id});
}

pub fn main() !void {
    const t1 = try std.Thread.spawn(.{}, workerFn, .{1});
    const t2 = try std.Thread.spawn(.{}, workerFn, .{2});
    const t3 = try std.Thread.spawn(.{}, workerFn, .{3});

    t1.join();
    t2.join();
    t3.join();
}
```

### 3.2 Mutex + 共享状态

```zig
const Counter = struct {
    mutex: std.Thread.Mutex = .{},
    value: u64 = 0,

    pub fn increment(self: *Counter) void {
        self.mutex.lock();
        defer self.mutex.unlock();
        self.value += 1;
    }

    pub fn get(self: *Counter) u64 {
        self.mutex.lock();
        defer self.mutex.unlock();
        return self.value;
    }
};

var counter = Counter{};

fn incrementMany(ctx: *Counter) void {
    for (0..1000) |_| ctx.increment();
}

pub fn main() !void {
    const t1 = try std.Thread.spawn(.{}, incrementMany, .{&counter});
    const t2 = try std.Thread.spawn(.{}, incrementMany, .{&counter});
    t1.join();
    t2.join();
    std.debug.print("counter = {d}\n", .{counter.get()}); // 2000
}
```

### 3.3 RwLock（读写锁）

```zig
var rwlock: std.Thread.RwLock = .{};
var shared_data: u32 = 0;

fn reader(id: usize) void {
    rwlock.lockShared();
    defer rwlock.unlockShared();
    std.debug.print("reader {d}: {d}\n", .{ id, shared_data });
}

fn writer(new_val: u32) void {
    rwlock.lock();
    defer rwlock.unlock();
    shared_data = new_val;
}
```

### 3.4 Semaphore（信号量）

```zig
var sema = std.Thread.Semaphore{ .permits = 0 };

fn producer() void {
    // 生产数据
    std.time.sleep(100 * std.time.ns_per_ms);
    sema.post(); // 通知消费者
}

fn consumer() void {
    sema.wait(); // 等待生产者
    // 消费数据
}
```

### 3.5 Channel（无锁消息队列，手工实现）

```zig
// 固定大小的无锁单生产者单消费者队列
fn SpscQueue(comptime T: type, comptime CAP: usize) type {
    return struct {
        const Self = @This();

        buf:  [CAP]T = undefined,
        head: std.atomic.Value(usize) = std.atomic.Value(usize).init(0),
        tail: std.atomic.Value(usize) = std.atomic.Value(usize).init(0),

        pub fn push(self: *Self, val: T) bool {
            const tail = self.tail.load(.monotonic);
            const next = (tail + 1) % CAP;
            if (next == self.head.load(.acquire)) return false; // full
            self.buf[tail] = val;
            self.tail.store(next, .release);
            return true;
        }

        pub fn pop(self: *Self) ?T {
            const head = self.head.load(.monotonic);
            if (head == self.tail.load(.acquire)) return null; // empty
            const val = self.buf[head];
            self.head.store((head + 1) % CAP, .release);
            return val;
        }
    };
}

var queue = SpscQueue(u32, 64){};
```

---

## 4. 策略三：新 io.async / io.await（0.17-dev）

> **状态**：0.17-dev 中正在落地，API 可能有微调，以实际编译通过为准。

### 4.1 核心模型

新模型基于 `std.Io` 接口：

```
std.Io
├── 提供 I/O 能力（文件读写、网络...）
└── 提供协程调度能力（io.async/await）
    ├── io.async(frame) → 启动一个协程帧，立即返回
    └── io.await(handle) → 等待协程帧完成，获取返回值
```

协程帧是**普通的 Zig 函数**，没有特殊关键字修饰：

```zig
// 0.17-dev 预期语法（以实际版本为准）
pub fn main(io: std.process.Init) !void {
    // 启动并发任务
    const handle1 = io.async(fetchData, .{ io, "url1" });
    const handle2 = io.async(fetchData, .{ io, "url2" });

    // 等待两个任务都完成
    const result1 = try io.await(handle1);
    const result2 = try io.await(handle2);

    std.debug.print("got: {s}, {s}\n", .{ result1, result2 });
}

fn fetchData(io: std.process.Init, url: []const u8) ![]u8 {
    // 普通函数，使用 io 做非阻塞 I/O
    _ = url;
    const file = try io.fs.openFile("/tmp/data.txt", .{});
    defer file.close(io);
    return file.readToEndAlloc(io, 4096);
}
```

### 4.2 Io 接口的层级

```
std.Io（接口，由运行时注入）
├── 单线程 Io（zig build — 默认）
│   └── 不支持 io.async 并发，顺序执行
├── 多线程 Io（std.Io.thread_pool）
│   └── 线程池调度 async 帧
└── 事件循环 Io（io_uring / epoll / kqueue）
    └── 事件驱动，真正异步 I/O
```

在 `main()` 中接收的 `io: std.process.Init` 由 Zig 运行时在启动时注入，可通过环境变量或 build 选项切换后端。

### 4.3 结构化并发（Structured Concurrency）

新模型强制遵守**结构化并发**：
- `io.async()` 启动的帧必须在**当前作用域**结束前被 `io.await()`
- 不允许"fire and forget"（帧可能访问已释放的栈变量）

```zig
pub fn processAll(io: std.process.Init, items: []const Item) !void {
    var handles = try std.ArrayListUnmanaged(/* Handle类型 */){};
    defer {
        // 作用域结束：等待所有未完成的帧
        for (handles.items) |h| io.await(h) catch {};
        handles.deinit(io.allocator);
    }

    for (items) |item| {
        const h = io.async(processOne, .{ io, item });
        try handles.append(io.allocator, h);
    }
    // defer 确保全部完成后才退出
}
```

---

## 5. 原子操作（std.atomic）

无论哪种并发模型，原子操作都是基础：

```zig
const std = @import("std");

// 原子整数（无锁计数器）
var atomic_counter = std.atomic.Value(u64).init(0);

fn atomicIncrement() u64 {
    return atomic_counter.fetchAdd(1, .monotonic);
}

fn atomicGet() u64 {
    return atomic_counter.load(.acquire);
}

// 内存序（从弱到强）
// .unordered  — 最弱，只保证原子性（不推荐用于同步）
// .monotonic  — 不提供跨线程可见性保证（用于计数器）
// .acquire    — 读屏障（load）
// .release    — 写屏障（store）
// .acq_rel    — 读写屏障（RMW 操作）
// .seq_cst    — 全序，最强（最安全，性能最低）

// CAS（Compare And Swap）
fn casExample(ptr: *std.atomic.Value(u32)) bool {
    var expected: u32 = 0;
    return ptr.cmpxchgWeak(
        expected,  // 期望值
        1,         // 新值
        .acq_rel,  // 成功时内存序
        .monotonic // 失败时内存序
    ) == null; // null = 成功
}

// 常用 RMW 操作
var val = std.atomic.Value(i32).init(10);
_ = val.fetchAdd(5,  .monotonic); // 返回旧值 10，现在是 15
_ = val.fetchSub(3,  .monotonic); // 返回 15，现在是 12
_ = val.fetchAnd(~1, .monotonic); // 清 bit0
_ = val.fetchOr(8,   .monotonic); // 置 bit3
_ = val.swap(0,      .seq_cst);   // 原子替换，返回旧值
```

---

## 6. 协程模式速查

### 6.1 哪种方案选哪种

| 场景 | 推荐方案 |
|------|---------|
| 裸机/no_std | 手工状态机 |
| 简单顺序逻辑 | 直接函数调用，不需要协程 |
| CPU 密集并行 | `std.Thread` + 线程池 |
| I/O 密集（文件/网络） | `io.async`/`io.await`（0.17+）|
| 嵌入式 RTOS 风格 | 手工状态机 + 轮询调度器 |
| 生成器/流式处理 | 手工状态机（next() 模式）|

### 6.2 轻量级协程调度器（裸机）

```zig
// 适合 SBI/RTOS 的轮询调度器
const MAX_TASKS = 8;

const TaskFn = *const fn (*anyopaque) bool; // 返回 true = 完成

const TaskEntry = struct {
    func: TaskFn,
    ctx:  *anyopaque,
    done: bool = false,
};

var task_pool: [MAX_TASKS]TaskEntry = undefined;
var task_count: usize = 0;

fn spawnTask(func: TaskFn, ctx: *anyopaque) void {
    if (task_count >= MAX_TASKS) @panic("task pool full");
    task_pool[task_count] = .{ .func = func, .ctx = ctx };
    task_count += 1;
}

fn runScheduler() void {
    var running = true;
    while (running) {
        running = false;
        for (task_pool[0..task_count]) |*t| {
            if (!t.done) {
                t.done = t.func(t.ctx);
                if (!t.done) running = true;
            }
        }
        // 空闲时 wfi
        asm volatile ("wfi");
    }
}
```

---

## 7. 实战：异步 SBI 控制台（状态机版）

```zig
// 非阻塞 UART 发送状态机
const ConsoleTx = struct {
    const Self = @This();

    buf:    []const u8 = &.{},
    offset: usize = 0,

    pub fn send(self: *Self, data: []const u8) void {
        self.buf    = data;
        self.offset = 0;
    }

    // 返回 true = 发送完成
    pub fn poll(self: *Self) bool {
        if (self.offset >= self.buf.len) return true;

        // 尝试发送一个字节（检查 UART THRE）
        if (uartTxReady()) {
            uartPutc(self.buf[self.offset]);
            self.offset += 1;
        }
        return self.offset >= self.buf.len;
    }
};

fn uartTxReady() bool {
    const LSR = @as(*volatile u8, @ptrFromInt(0x10000000 + 5));
    return (LSR.* & (1 << 5)) != 0; // THRE bit
}

fn uartPutc(c: u8) void {
    const THR = @as(*volatile u8, @ptrFromInt(0x10000000));
    THR.* = c;
}
```

---

## 8. 有栈协程与上下文切换机制

### 8.1 并发模型全景

```
上下文切换机制
│
┌───────────────┼───────────────┐
▼               ▼               ▼
抢占式调度       协作式调度        编译器魔法
（时间片中断）   （显式让出）     （状态机+poll）
│               │               │
┌─────┴─────┐  ┌────┴────┐  ┌────┴────┐
▼           ▼  ▼         ▼  ▼         ▼
进程/线程  goroutine 有栈协程  无栈协程  Generator
(内核调度) (Go runtime) (Lua/Erlang) (Rust async) (Python yield)

切换对象：    切换对象：   切换对象：    切换对象：
PC+SP+寄存器  PC+SP+寄存器 PC+SP+寄存器  「无」—只是改变状态机的当前状态
+页表基址     (无页表切换) (无页表切换)
```

### 8.2 四种并发机制核心对比

| 维度 | 进程/线程（内核） | goroutine | 有栈协程 | 无栈协程（async） |
|------|-----------------|-----------|---------|-----------------|
| 触发者 | 操作系统内核 | Go runtime | 用户态调度器 | 调用方轮询 |
| 权限级别 | 用户态 ↔ 内核态 | 纯用户态 | 纯用户态 | 纯用户态（无切换） |
| 保存什么 | PC+SP+全寄存器+页表+信号掩码 | PC+SP+callee-saved | PC+SP+callee-saved | 无——编译为状态机 |
| 切换开销 | ~1–10 μs（TLB刷新+特权切换） | ~100 ns | ~10–100 ns | ~ns（函数调用级） |
| 每协程栈 | 内核栈（固定大） + 用户栈 | 动态增长（初始 2–8 KB） | 固定用户栈（需预分配） | 无独立栈（帧在堆上） |
| 栈溢出风险 | 内核保护 | 自动扩容 | 需手动设置大小 | 不存在（无栈） |

> **核心洞察**：有栈协程的原理本质上就是用户态的上下文切换，
> 和操作系统进程/线程调度的机制高度一致——都是保存和恢复执行上下文（主要是 SP 和寄存器），
> 只是发生在用户空间，没有内核参与，也没有页表切换（无 TLB 刷新开销）。

### 8.3 上下文切换的汇编实现（RISC-V）

有栈协程切换只需保存 **callee-saved 寄存器**（按 ABI，被调用方负责保存的寄存器）：

```asm
# RISC-V ABI callee-saved: ra, sp, s0–s11（共 14 个寄存器 = 112 字节）
# context_switch(old_ctx: *Context, new_ctx: *Context)

context_switch:
    # 保存当前协程（a0 = old Context 指针）
    sd ra,   0*8(a0)   # 保存返回地址（即恢复时的 PC）
    sd sp,   1*8(a0)   # 保存栈指针
    sd s0,   2*8(a0)
    sd s1,   3*8(a0)
    sd s2,   4*8(a0)
    sd s3,   5*8(a0)
    sd s4,   6*8(a0)
    sd s5,   7*8(a0)
    sd s6,   8*8(a0)
    sd s7,   9*8(a0)
    sd s8,  10*8(a0)
    sd s9,  11*8(a0)
    sd s10, 12*8(a0)
    sd s11, 13*8(a0)

    # 恢复下一个协程（a1 = new Context 指针）
    ld ra,   0*8(a1)
    ld sp,   1*8(a1)
    ld s0,   2*8(a1)
    ld s1,   3*8(a1)
    ld s2,   4*8(a1)
    ld s3,   5*8(a1)
    ld s4,   6*8(a1)
    ld s5,   7*8(a1)
    ld s6,   8*8(a1)
    ld s7,   9*8(a1)
    ld s8,  10*8(a1)
    ld s9,  11*8(a1)
    ld s10, 12*8(a1)
    ld s11, 13*8(a1)
    ret  # 跳到 ra（新协程上次 yield 时保存的地址）
```

对比：进程切换需要额外保存 **satp（页表基址）+ sstatus + sepc**，并执行 `sfence.vma` 刷新 TLB，这正是协程比进程快的根本原因。

### 8.4 无栈协程：状态机 + Poll（对比）

```zig
// Zig async（无栈）
async fn example() u32 {
    const a = step1().await;  // 状态 0：等待 step1
    const b = step2(a).await; // 状态 1：等待 step2
    return b + 1;
}

// 编译后等价状态机（概念）：
const ExampleFrame = union(enum) {
    state0,           // 未开始
    state1: Step1Handle,  // 等待 step1
    state2: Step2Handle,  // 等待 step2
    done: u32,
};
// 每次 poll() 根据当前状态执行一段，返回 Pending 或 Ready
```

关键区别：无栈协程**不保存/恢复栈**，只是在同一个调用栈上跳转状态机节点。
所有局部变量必须放入帧结构体（堆分配）。

### 8.5 Zig 生态：无栈协程库

Zig 0.16/0.17 已移除内置 async 关键字，社区库：

| 库 | 模式 | 状态 |
|----|------|------|
| `std.io.async` (0.17-dev) | 无栈协程 | 官方，落地中 |
| `mitchellh/libxev` | 事件循环（回调式无栈异步） | 活跃维护 |
| `kprotty/zap` | 有栈纤程调度器 | 已存档，参考价值高 |
| `rsepassi/zigcoro` | 有栈协程（汇编 context_switch） | 活跃维护 |

#### mitchellh/libxev — 跨平台事件循环

底层后端：Linux=io_uring，macOS=kqueue，Windows=IOCP，纯 Zig 无 C 依赖。
异步模型：**回调式**（Completion-based），每个 I/O 操作附带一个回调，本质是无栈的事件驱动状态机。

```zig
const xev = @import("xev");
const std = @import("std");

pub fn main() !void {
    var loop = try xev.Loop.init(.{});
    defer loop.deinit();

    // 定时器：1000ms 后触发回调
    var timer = try xev.Timer.init();
    defer timer.deinit();

    var fired = false;
    var c: xev.Completion = undefined;
    timer.run(&loop, &c, 1000, bool, &fired, struct {
        fn cb(
            ud: ?*bool,
            _: *xev.Loop,
            _: *xev.Completion,
            r: xev.Timer.RunError!void,
        ) xev.CallbackAction {
            _ = r catch unreachable;
            ud.?.* = true;
            return .disarm;  // 不重复触发
        }
    }.cb);

    try loop.run(.until_done);
    std.debug.print("timer fired: {}\n", .{fired}); // true
}
```

多个 I/O 操作并发：

```zig
// TCP 连接示例：两个连接并发进行，均通过回调汇合
var c1: xev.Completion = undefined;
var c2: xev.Completion = undefined;

tcp1.connect(&loop, &c1, addr1, void, null, onConnect);
tcp2.connect(&loop, &c2, addr2, void, null, onConnect);

try loop.run(.until_done); // 事件循环驱动两个并发连接
```

> libxev 的并发单位是 **Completion 回调**，不是帧/栈；是回调地狱的工程化版本，适合高并发 I/O 密集场景。

### 8.6 Zig 生态：有栈纤程库

#### rsepassi/zigcoro — 有栈协程（推荐，活跃维护）

底层使用平台专用汇编实现 `context_switch`，支持 x86_64 / aarch64 / RISC-V。
原理与 8.3 节手写汇编完全一致，只是封装成了 Zig API。

```zig
const coro = @import("coro");
const std   = @import("std");

pub fn main() !void {
    const alloc = std.heap.page_allocator;

    // 分配协程独立栈（默认 64 KB）
    var stack = try coro.stackAlloc(alloc, null);
    defer coro.stackDealloc(alloc, stack);

    // 创建协程帧，绑定入口函数
    var frame = try coro.Coro.init(worker, stack);

    std.debug.print("main: first resume\n", .{});
    coro.xresume(frame);   // 切入 worker，执行到 xsuspend 返回

    std.debug.print("main: second resume\n", .{});
    coro.xresume(frame);   // 继续 worker 到函数末尾

    std.debug.print("main: done={}\n", .{frame.isDone()}); // true
}

fn worker() void {
    std.debug.print("worker: start\n", .{});
    coro.xsuspend();  // 让出给调用方（context_switch 回 main）
    std.debug.print("worker: resumed\n", .{});
    // 函数返回 = 协程自然结束
}
```

带返回值的协程：

```zig
// xresume 传入值，xsuspend 返回值（双向通信）
var frame = try coro.CoroT(u32, u32).init(counter, stack);

coro.xresumeT(frame, 10);   // 传入 10，运行到下一个 xsuspend
const v = coro.xresumeT(frame, 20); // 传入 20，取回协程 yield 的值

fn counter(input: u32) u32 {
    const doubled = coro.xsuspendT(input * 2); // yield input*2，等待下次 resume
    return doubled + 1;
}
```

多协程调度（手工 round-robin）：

```zig
var frames = [_]*coro.Coro{ f1, f2, f3 };
var i: usize = 0;
while (true) {
    var all_done = true;
    for (&frames) |f| {
        if (!f.isDone()) {
            coro.xresume(f);
            all_done = false;
        }
    }
    if (all_done) break;
}
```

#### kprotty/zap — 有栈纤程调度器（已存档）

比 zigcoro 更高层：内置调度器 + work-stealing，API 类似 Go。
虽已停止维护，但代码量少，汇编 context_switch 清晰，适合阅读学习。

```zig
const zap = @import("zap");
const std  = @import("std");

pub fn main() !void {
    // 启动带调度器的运行时，入口函数在调度器内执行
    try zap.Scheduler.run(.{}, struct {
        pub fn run(sched: *zap.Scheduler) !void {
            const f1 = try sched.spawn(fiberTask, .{1}, .{});
            const f2 = try sched.spawn(fiberTask, .{2}, .{});
            const f3 = try sched.spawn(fiberTask, .{3}, .{});
            try sched.join(&.{ f1, f2, f3 }); // 等全部完成
        }
    }.run);
}

fn fiberTask(id: usize) !void {
    std.debug.print("fiber {d}: start\n", .{id});
    zap.Fiber.yield(); // 协作式让出调度权，切换到其他纤程
    std.debug.print("fiber {d}: done\n", .{id});
}
```

| 对比维度 | zigcoro | zap |
|---------|---------|-----|
| 状态 | 活跃维护 | 已存档 |
| 调度器 | 无（需手工 resume） | 内置 work-stealing |
| API 风格 | 底层（显式 resume/suspend） | 高层（类 goroutine spawn/join） |
| 适合场景 | 嵌入式/裸机/定制调度 | 学习 M:N 调度器实现 |
| 源码可读性 | 好 | 极好（代码量少） |

### 8.7 Rust 生态：有栈协程库

Rust 学习参考（理解有栈协程工程实现）：

#### Corosensei — 跨平台有栈协程
```toml
# Cargo.toml
corosensei = "0.2"
```
```rust
use corosensei::{Coroutine, CoroutineResult};

let mut co = Coroutine::new(|yielder, input: i32| {
    println!("coroutine start, input={input}");
    let x = yielder.suspend(input * 2);   // 暂停，返回值给调用方
    println!("resumed with {x}");
    x + 1
});

// 启动并首次运行
match co.resume(10) {
    CoroutineResult::Yield(v) => println!("yielded: {v}"),   // 20
    CoroutineResult::Return(v) => println!("returned: {v}"),
}
// 恢复
match co.resume(99) {
    CoroutineResult::Return(v) => println!("done: {v}"),     // 100
    _ => {}
}
```
底层使用 RISC-V/x86/ARM 汇编实现 `context_switch`，与 8.3 节原理完全一致。

#### May — Go-like Goroutines in Rust
```toml
may = "0.3"
```
```rust
use may::go;

// 启动 goroutine 风格的有栈协程
go!(|| {
    println!("hello from coroutine");
    may::coroutine::sleep(std::time::Duration::from_millis(10));
    println!("woke up");
});

// M:N 调度（多线程运行协程）
may::config().set_workers(4);  // 4 个 OS 线程
```
May 实现了 M:N 调度（多个 OS 线程运行多个用户态协程），类似 Go runtime。

### 8.8 Rust 生态：无栈协程（futures 0.3）

```toml
# Cargo.toml
futures = "0.3.32"
tokio = { version = "1", features = ["full"] }
```

futures 0.3 定义了 Rust async 生态的核心 trait：

```rust
// 核心 trait：可被轮询的异步计算
pub trait Future {
    type Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}

// Poll 是两种状态的枚举（对应有栈协程的 yield/return）
pub enum Poll<T> {
    Ready(T),    // 完成，返回结果
    Pending,     // 未完成，请稍后再轮询
}
```

`futures` 库还提供：
- `futures::stream::Stream` — 异步迭代器（对应 Generator）
- `futures::sink::Sink` — 异步写入器
- `futures::select!` — 同时等待多个 Future（类似 Go select）
- `futures::join!` — 并发等待多个 Future

### 8.9 Generator 模式

#### Python 风格 yield（Rust nightly）
```rust
// Rust nightly: Generator trait（正在稳定化）
#![feature(coroutines, coroutine_trait)]
use std::ops::{Coroutine, CoroutineState};
use std::pin::Pin;

let mut gen = |_: ()| {
    yield 1u32;
    yield 2u32;
    yield 3u32;
    // 返回时 CoroutineState::Complete
};

// 手动驱动 Generator
match Pin::new(&mut gen).resume(()) {
    CoroutineState::Yielded(v) => println!("{v}"),  // 1
    CoroutineState::Complete(_) => {}
}
```

#### genawaiter — stable Rust Generator
```toml
genawaiter = "0.99"
```
```rust
use genawaiter::{sync::gen, yield_};

let fib = gen!({
    let (mut a, mut b) = (0u64, 1u64);
    loop {
        yield_!(a);
        (a, b) = (b, a + b);
    }
});

for val in fib.take(10) {
    print!("{val} ");  // 0 1 1 2 3 5 8 13 21 34
}
```

#### itertools 0.14 — 迭代器增强工具
```toml
itertools = "0.14.0"
```
```rust
use itertools::Itertools;

// 滑动窗口（类 Generator 的惰性求值）
let windows: Vec<_> = (1..=5).tuple_windows::<(_, _)>().collect();
// [(1,2), (2,3), (3,4), (4,5)]

// 笛卡尔积
let pairs: Vec<_> = (0..3).cartesian_product(0..3).collect();

// chunk：惰性分组
for chunk in &(0..100).chunks(10) {
    let sum: i32 = chunk.sum();
    println!("{sum}");
}
```
itertools 是 Rust 迭代器的瑞士军刀，常与 `futures::stream` 配合实现流式处理。

### 8.10 Rust 异步运行时（执行器）

> 本地路径：`/home/heke/tgln/stage2/material/async/tokio`、`async/monoio`

Rust async/await 是**无栈协程**语法，但本身不能运行——必须搭配一个**运行时（执行器）**驱动 `Future::poll`。

| 运行时 | 线程模型 | I/O 后端 | 适合场景 |
|--------|---------|---------|---------|
| `tokio` | 多线程工作窃取 | epoll/kqueue/IOCP | 服务端通用，生态最大 |
| `monoio` | 单线程 | io_uring（Linux Only） | 极低延迟，零拷贝 |
| `async-std` | 多线程 | 同 tokio | API 镜像 std，学习友好 |
| `smol` | 多线程 | async-io（poll/epoll） | 极简实现（~1500行），适合嵌入 |

#### tokio — 主流多线程异步运行时

```toml
# Cargo.toml
tokio = { version = "1", features = ["full"] }
```

```rust
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

#[tokio::main]  // 宏展开为：Runtime::new().block_on(async { ... })
async fn main() {
    // spawn：启动独立异步任务（类似 goroutine）
    let h1 = tokio::spawn(async {
        sleep(Duration::from_millis(100)).await;
        42u32
    });
    let h2 = tokio::spawn(async {
        sleep(Duration::from_millis(50)).await;
        99u32
    });

    // join!：并发等待（不是顺序 await）
    let (r1, r2) = tokio::join!(h1, h2);
    println!("{} {}", r1.unwrap(), r2.unwrap()); // 99 42（h2先完成但join!等两个）

    // select!：等第一个完成
    tokio::select! {
        _ = sleep(Duration::from_millis(10)) => println!("timeout"),
        v = async { 42u32 } => println!("got {v}"),
    }

    // mpsc Channel（类 Go channel，async 版）
    let (tx, mut rx) = mpsc::channel::<u32>(32);
    tokio::spawn(async move {
        for i in 0..5u32 { tx.send(i).await.unwrap(); }
    });
    while let Some(v) = rx.recv().await {
        print!("{v} "); // 0 1 2 3 4
    }
}
```

tokio 调度原理：

```
tokio Runtime
├── thread pool（默认 CPU 核数个 OS 线程）
│   └── 每个线程有自己的任务队列（work-stealing）
├── reactor（io_uring/epoll）— 监听 I/O 事件，唤醒对应任务
└── timer wheel — 管理所有 sleep/timeout
```

#### monoio — io_uring 完成式单线程运行时

完成式 I/O（Completion-based）和就绪式 I/O（Readiness-based）的核心区别：

```
就绪式（epoll/tokio）：
  epoll_wait 说"fd 可读了" → 你去 read() → 内核把数据从内核缓冲区拷贝到你的 buf

完成式（io_uring/monoio）：
  提交 read 请求 + 你的 buf → 内核异步执行 → 完成时通知你 buf 已填好（零额外拷贝）
```

```toml
monoio = { version = "0.2", features = ["default"] }
```

```rust
use monoio::fs::File;
use monoio::io::AsyncReadRentExt;

#[monoio::main]
async fn main() -> std::io::Result<()> {
    // 完成式读取：buf 所有权交给内核，完成后归还
    let file = File::open("/etc/hostname").await?;
    let buf = vec![0u8; 64];
    let (res, buf) = file.read_exact(buf).await; // buf 转移所有权，完成后归还
    res?;
    println!("{}", String::from_utf8_lossy(&buf).trim());
    Ok(())
}
```

monoio 的所有权模型与 tokio 不同：

```rust
// tokio（就绪式）：buf 在 read 期间借用
let mut buf = [0u8; 64];
file.read(&mut buf).await?; // &mut buf，调用期间借用

// monoio（完成式）：buf 必须转移所有权（内核持有期间不能访问）
let buf = vec![0u8; 64];
let (res, buf) = file.read_exact(buf).await; // 移动进去，返回时移动回来
```

#### smol — 极简运行时（可嵌入）

```toml
smol = "2"
async-std = "1"  # 基于 smol，提供类 std 的 async API
```

```rust
// smol: 最小运行时，约 1500 行，适合嵌入自定义项目
fn main() {
    smol::block_on(async {
        let handle = smol::spawn(async { 42u32 });
        println!("{}", handle.await); // 42
    });
}
```

```rust
// async-std: API 镜像 std，迁移成本低
use async_std::fs;
use async_std::task;

fn main() {
    task::block_on(async {
        let content = fs::read_to_string("/etc/hostname").await.unwrap();
        println!("{content}");
    });
}
```

#### 运行时选型速查

| 需求 | 选择 |
|------|------|
| 通用服务端（HTTP/gRPC/数据库） | tokio |
| Linux 极低延迟 I/O（io_uring） | monoio |
| 学习 async 执行器内部原理 | smol（代码量最少） |
| 从同步代码迁移，保留 std 风格 | async-std |
| 嵌入式 / no_std async | embassy（在 `/rtos/embassy/`）|

### 8.11 学习路径建议

```
无栈协程（理解状态机）
  → Zig 手工状态机（本笔记第2章）
  → futures::Future + poll 模型（8.8节）
  → libxev 回调式事件循环（8.5节）
  → Zig io.async（0.17，实验中，第4章）

有栈协程（理解上下文切换）
  → 手写 RISC-V 汇编 context_switch（8.3节）
  → zigcoro 源码（汇编封装，RISC-V/x86/aarch64，8.6节）
  → zap 源码（M:N 调度器，代码量少，8.6节）
  → Corosensei（跨平台，8.7节）

Generator / 流式处理
  → Python yield → genawaiter（8.9节）
  → futures::Stream → itertools（8.9节）

Rust 异步运行时（执行器层）
  → smol 源码（~1500行，理解最小执行器，8.10节）
  → tokio（工业级多线程运行时，8.10节）
  → monoio（io_uring，完成式 I/O 所有权模型，8.10节）
```

> **本地材料目录**：
> - `/home/heke/tgln/stage2/material/async/tokio` — tokio 源码
> - `/home/heke/tgln/stage2/material/async/monoio` — monoio 源码
> - zigcoro / zap / Corosensei / May：按需克隆到 `async/` 下

---

## 练习

### 练习 13：状态机

```zig
// TODO: 实现一个简单的 HTTP 响应解析状态机
// 输入是逐字节的字节流（每次 feed 一个字节）
// 状态：读状态行 → 读头部 → 读空行 → 读 body
// 输出：解析完成时返回 status_code 和 body 起始偏移

const HttpParser = struct {
    // 填充 state, buffer 等字段

    pub fn feed(self: *HttpParser, byte: u8) ?struct { status: u16, body_offset: usize } {
        // 填充这里
        _ = self;
        _ = byte;
        return null;
    }
};

test "HttpParser basic" {
    var parser = HttpParser{};
    const response = "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHello";
    var result: ?struct { status: u16, body_offset: usize } = null;
    for (response, 0..) |b, _| {
        result = parser.feed(b);
        if (result != null) break;
    }
    try std.testing.expect(result != null);
    try std.testing.expectEqual(@as(u16, 200), result.?.status);
}
```

### 练习 14：无锁队列

```zig
// TODO: 测试上面的 SpscQueue 实现
// 启动一个生产者线程和一个消费者线程
// 生产者发送 0..1000，消费者接收并累加，验证总和

test "SpscQueue producer-consumer" {
    // 填充这里
}
```

### 练习 15：生成器

```zig
// TODO: 实现 PrimesGen，按顺序生成素数
// 每次调用 next() 返回下一个素数

const PrimesGen = struct {
    // 填充这里

    pub fn next(self: *PrimesGen) u64 {
        // 填充这里
        _ = self;
        return 0;
    }
};

test "PrimesGen" {
    var gen = PrimesGen{};
    const expected = [_]u64{ 2, 3, 5, 7, 11, 13, 17, 19, 23, 29 };
    for (expected) |want| {
        try std.testing.expectEqual(want, gen.next());
    }
}
```

---

> 下一节：[01-02-zig-build.md](01-02-zig-build.md) — 构建系统与包管理器
