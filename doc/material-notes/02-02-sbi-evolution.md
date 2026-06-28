# SBI 演化史 — 从 v0.1 到 v3.0 的大图景

> 读完此笔记，你会知道：SBI 规范长什么样、从哪来、到哪去、M-mode 固件启动时做了什么、每加一个扩展固件多了什么代码。

---

## 1. 一图定全局 — SBI 究竟是什么

```
┌──────────────────────────────────────────────────────────┐
│  S-mode OS (Linux / tg-rcore)                            │
│                                                          │
│  需要 → 获取 SBI 版本？  Probe BASE FID 0                │
│  需要 → 设闹钟？         ecall EID=TIME FID=0            │
│  需要 → 通知其他核？     ecall EID=IPI FID=0              │
│  需要 → 关机？           ecall EID=SRST FID=0            │
│  需要 → 打印调试？       ecall EID=DBCN FID=0            │
│        ↓ 都是 ecall，参数用 a7(EID)+a6(FID)+a0-a5        │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  _start → 初始化 CSR → 设 mtvec → mret 到 S-mode         │
│           ↓                                              │
│  trapEntry (S-mode ecall 进入) → trapHandler → dispatch  │
│    EID+TIME → 写 CLINT mtimecmp                          │
│    EID+IPI  → 写 CLINT MSIP                              │
│    EID+SRST → 写 SiFive test device                      │
│    EID+DBCN → 写/读 NS16550A UART                        │
│           ↓                                              │
│  mret 返回 S-mode（返回值在 a0/a1）                      │
├──────────────────────────────────────────────────────────┤
│  Hardware: CLINT / PLIC / UART / test device / PMU ...   │
└──────────────────────────────────────────────────────────┘
```

**一句话：SBI = S-mode OS 与 M-mode 硬件之间的标准化桥梁。OS 只调 ecall，不用管硬件地址。**

---

## 2. 版本演化线 — SBI 从哪来、到哪去

```
2019 ─ v0.1     9个 Legacy 函数，无 EID 概念。a7=0~8 直接是函数号。
                   最简：set_timer + console_putchar/getchar + ipi + fence + shutdown
                   
2022 ─ v1.0     正式规范。EID/FID 体系建立。
      rc1       BASE(7)  TIME(1)  IPI(1)  RFNC(7)  HSM(4)  SRST(1)
      rc3       +PMU(8)     ← 性能监控
      
2024 ─ v2.0     Ratified 2024-02-01
      rc1       +DBCN(3)  SUSP(1)    ← 调试控制台、系统挂起
      rc2       +CPPC(4)             ← 协作电源管理
      rc3       +NACL(5)             ← 嵌套加速
      rc5       +STA(1)              ← 被偷时间记账（steal-time）
      
2025 ─ v3.0     最新 Ratified
      rc2       +SSE(11)             ← 监控者软件事件（最复杂扩展）
      rc3       +MPXY(6)             ← 消息代理
      rc4       +DBTR(8)             ← 调试触发器
      rc5       +FWFT(2)             ← 固件特性查询
```

**关键节点：**
- v1.0-rc1 是一道分水岭——EID/FID 体系取代了 Legacy 的 a7 直接编号
- 一次引入了 6 个主力扩展 (TIME/IPI/RFNC/HSM/SRST 加上 BASE)，这 6 个覆盖了 OS 90% 的需求
- v2.0 和 v3.0 都是新增，不删不改已有的

---

## 3. 所有扩展一览（17个扩展 = 全部 SBI 规范）

### 3.1 主力扩展（v1.0-rc1 引入，每个 OS 都会用到）

| EID | 名称 | FID数 | 一句话功能 |
|-----|------|-------|-----------|
| 0x10 | BASE | 9 | **强制实现。** 版本信息 + 扩展探测 + CPU ID |
| 0x54494D45 | TIME | 1 | 设闹钟：告诉硬件"在 deadline 时刻中断我" |
| 0x735049 | IPI | 1 | 核间通知：告诉另一个核"该干活了" |
| 0x52464E43 | RFNC | 7 | TLB/缓存远程刷新：多核 MMU 一致性 |
| 0x48534D | HSM | 4 | Hart 生命周期：启动/停止/挂起/查状态 |
| 0x53525354 | SRST | 1 | 关机/重启 |

### 3.2 高级扩展（v1.0-rc3 ~ v2.0-rc5 引入）

| EID | 名称 | FID数 | 版本 | 功能 |
|-----|------|-------|------|------|
| 0x504D55 | PMU | 8 | v1.0-rc3 | 硬件性能计数器（指令数/cycle/分支预测命中...） |
| 0x4442434E | DBCN | 3 | v2.0-rc1 | 调试串口（write/read/write_byte） |
| 0x53555350 | SUSP | 1 | v2.0-rc1 | 系统挂起到内存/磁盘 |
| 0x43505043 | CPPC | 4 | v2.0-rc2 | 协作电源管理（DVFS，ARM SCMI 对应物） |
| 0x4E41434C | NACL | 5 | v2.0-rc3 | 嵌套虚拟化加速（Guest ↔ Hypervisor 直通 CSR） |
| 0x535441 | STA | 1 | v2.0-rc5 | steal-time：虚拟化中告诉 VM "你被偷了多少 CPU 时间" |

### 3.3 v3.0 新增扩展（2025 Ratified）

| EID | 名称 | FID数 | 版本 | 功能 |
|-----|------|-------|------|------|
| 0x535345 | SSE | 11 | v3.0-rc2 | Supervisor Software Events：固件向 S-mode 注入事件 |
| 0x4D505859 | MPXY | 6 | v3.0-rc3 | Message Proxy：核间消息通道（CPPC/SSE 底层传输） |
| 0x44425452 | DBTR | 8 | v3.0-rc4 | Debug Triggers：M-mode 调试触发器的 S-mode 访问接口 |
| 0x46574654 | FWFT | 2 | v3.0-rc5 | Firmware Features：查询/设置固件行为开关 |

> **以上 17 个扩展就是 SBI 规范的全部。** 除此之外没有秘密扩展。


**已实现 9：** BASE / TIME / IPI / RFNC / HSM / SRST / DBCN / SUSP / FWFT
**缺 8：** Legacy 完整版 / PMU / CPPC / NACL / STA / SSE / MPXY / DBTR



|------|------|------|-------------|-------------|-----------|
| **AIA** | Advanced Interrupt Architecture（APLIC + IMSIC + IMSIC indirect CSRs）| 2024 ratified | ✅ | ✅（2026-05 加）| 必须 |
| **CoVE** | Confidential VM Extension（covh + covg）| 草案 | ❌ | ✅（2026-05 加）| 必须 |
| **Smrnmi** | Resumable Non-Maskable Interrupt | 2024 ratified | ✅（2026-05 加）| ❌ | 必须 |
| **H 扩展** | Hypervisor Extension | ratified | ✅ | 部分 | 必须（HS-mode 适配）|
| **AIA 嵌套** | AIA H 扩展协同 | ratified | ✅ | 部分 | 必须 |

---

## 4. M-mode _start 固件代码——一步一步走


### 4.1 _start 汇编（裸函数，callconv(.naked)）

```asm
_start:
    csrr t0, mhartid          ; 1. 读当前 hart ID（QEMU 启动时 a0 传入）

    la   sp, __stack_top      ; 2. 设每 hart 独立栈
    li   t1, 0x10000          ;    sp = __stack_top - hartid * 64KB
    mul  t1, t0, t1
    sub  sp, sp, t1

    bnez t0, .Lsecondary      ; 3. boot hart (id=0) 清 BSS，secondary 直接等 IPI

    la   t1, __bss_start      ; 4. boot hart：BSS 段清零（全局变量初始化）
    la   t2, __bss_end
.Lbss_loop:
    beq  t1, t2, .Lbss_done
    sd   zero, 0(t1)
    addi t1, t1, 8
    j    .Lbss_loop
.Lbss_done:
    mv   a0, a1               ; 5. FDT 地址作为第一个参数传给 zigStart
    call zigStart

.Lsecondary:
    call zigSecondary         ; 6. 次级 hart 进入等待循环
```

**为什么是裸函数？** `callconv(.naked)` 意味着编译器不生成序言/尾声——不用 push/pop ra/sp/fp。因为这是 CPU 复位后的第一段代码，栈还没设好。

**为什么要在 .text.entry 注入 `j _start`？** QEMU ROM 固定跳到 `0x80000000`。Zig/LLVM 会把 compiler-rt 放在 `.text` 开头，导致 `_start` 不在首地址。注入一条无条件跳转确保 `0x80000000` 处是可执行指令。

### 4.2 zigStart（M-mode 初始化，完成后 mret 到 S-mode）

这是 M-mode 固件的核心初始化。按顺序走：

**Step 1 — 设置 mtvec（M-mode trap vector）**
```zig
csrw("mtvec", @intFromPtr(&trapEntry) & ~@as(usize, 3));
// mtvec = trapEntry 地址，Mode = Direct（最低位 = 0）
// trapEntry 是 S-mode ecall 进入 M-mode 的第一站
```

**Step 2 — 设置 mstatus（权限 + 浮点 + 中断）**
```zig
const mstatus: usize = (1 << 11) | (1 << 7) | (1 << 13);
// bit 11 (MPP) = 01 → mret 后 CPU 进入 S-mode（不是 U-mode）
// bit  7 (MPIE) = 1  → mret 后自动使能 M-mode 中断
// bit 13 (FS)  = 01 → FPU 状态 = Initial（防止 Linux 执行 FP 指令时报 IllegalInstruction）
```
这是整个启动过程中最关键的三行之一。MPP 决定了 mret 后 OS 运行在哪个特权级。

**Step 3 — 设置 menvcfg（S-mode 环境功能）**
```zig
const menvcfg: usize = (1 << 63) | (1 << 7) | (1 << 6) | (1 << 4);
// bit 63 (STCE) = 1  → S-mode 可以直接写 stimecmp CSR（Sstc 扩展）
// bit  7 (CBZE) = 1  → S-mode 可用 cbo.zero 指令（Zicboz 扩展）
// bit  6 (CBCFE)= 1  → S-mode 可用 cbo.clean/flush（Zicbom）
// bit  4 (CBIE) = 01 → cbo.inval 行为 = invalidate
```
没有 STCE=1，Linux 写 stimecmp 会被直接忽略，定时器不工作。

**Step 4 — PMP 直通（允许 S-mode 访问全部物理内存）**
```zig
csrw("pmpaddr0", ~@as(usize, 0) >> 10);  // 地址范围 = 全部 64 位空间
csrw("pmpcfg0", 0x1f);   // A=01(TOR) X=1 W=1 R=1 L=0 → 直通，不锁
```
默认 PMP 规则是拒绝一切。不设这条，S-mode 连内存都读不了。

**Step 5 — 异常/中断委托（哪些 trap 直接给 S-mode，M-mode 不拦截）**
```zig
csrw("medeleg", 0x00f0b509);  // 委托 页面错误/断点/U-ecall 给 S-mode
csrw("mideleg", 0x00001666);  // 委托  STIP/SSIP/SEIP 给 S-mode
// 不委托 S-ecall → S-mode 的 ecall 始终进 M-mode（这是 SBI 调用的入口！）
// 不委托 IllegalInstruction → M-mode 捕获非法指令，防止 OS 执行危险指令
```

**Step 6 — Sstc 定时器安全初始化**
```zig
asm volatile ("csrw 0x14d, %[v]" : : [v] "r" (~@as(usize, 0)));
// 设 stimecmp = UINT64_MAX，防止 mret 后立即触发定时器中断
```

**Step 7 — mscratch = M-mode SP**
```zig
asm volatile ("csrw mscratch, sp");
// 关键！trapEntry 用 csrrw sp, mscratch, sp 原子交换
// → 得到 M-mode 物理栈，同时把 S-mode 的 sp 存到 mscratch 供返回时恢复
```

**Step 8 — mret 进入 S-mode**
```zig
csrw("mepc", build_options.os_entry);   // 默认 0x80200000
asm volatile (
    \\ mv   a1, %[fdt]     // a1 = FDT 地址（Linux 靠它发现硬件）
    \\ csrr a0, mhartid    // a0 = 当前 hart ID
    \\ mret                // CPU: PC=mepc, mode=MPP(S-mode), MIE=MPIE
);
```

**mret 后 CPU 状态：**
- PC = `0x80200000`（os_entry，OS 内核入口）
- 特权级 = S-mode（由 MPP 决定）
- a0 = hartid（boot hart = 0）
- a1 = FDT 地址（设备树二进制）
- M-mode 中断使能（MPIE 恢复）

### 4.3 trapEntry + trapHandler（运行时，S-mode ecall 处理）

```
S-mode 执行 ecall
    │
    ▼
CPU 硬件自动：
  mepc = (S-mode ecall 指令的 PC)
  mcause = 9 (Environment call from S-mode)
  mstatus.MPP = S-mode
  mstatus.MPIE = mstatus.MIE
  mstatus.MIE = 0                    ← 关 M-mode 中断
  PC = mtvec (trapEntry)
    │
    ▼
trapEntry (汇编，callconv(.naked)):
  csrrw sp, mscratch, sp   ← 原子交换：sp=M-mode栈, mscratch=S-mode栈
  保存 31 个通用寄存器到 TrapFrame（在 M-mode 栈上）
  设 sp 指向 M-mode 栈
  a0 = &TrapFrame
  a1 = mepc（S-mode 触发地址）
  a2 = mcause（原因代码）
  call trapHandler(frame, mepc, mcause)
    │
    ▼
trapHandler (Zig):
  if (mcause == interrupt):
    timer → delegate STIP to S-mode; return mepc（不修改）
    IPI   → clear MSIP; process RFNC fence / HSM start
  if (mcause == S-ecall):
    eid = frame.a7（扩展 ID）
    fid = frame.a6（函数 ID）
    args = SbiArgs{ frame.a0-a5 }
    ret = MySbi.dispatch(eid, fid, args)  ← comptime 分发
    frame.a0 = ret.error
    frame.a1 = ret.value
    return mepc + 4（跳过 ecall 指令）
    │
    ▼
恢复寄存器（从 TrapFrame）
csrrw sp, mscratch, sp   ← 再次原子交换：恢复 S-mode sp
mret
```

**dispatch 内部（comptime 生成）：**
```zig
// 编译期生成的大 switch：
fn dispatch(eid: u32, fid: u32, args: SbiArgs) SbiRet {
    return switch (eid) {
        0x10 => switch (fid) {
            0 => baseGetSpecVersion(),
            1 => baseGetImplId(),
            3 => baseProbeExtension(args.a0),
            ...
        },
        0x54494D45 => switch (fid) {
            0 => timeSetTimer(args.a0),
        },
        ...
        else => .not_supported,
    };
}
```

---

## 5. 7 个扩展的代码解剖 — 每加一个扩展，M-mode 多了什么

### 5.1 BASE — 唯一强制扩展（SbiRet = 版本 + CPU ID + 探测）

**代码量：~40 行 Zig**

```zig
fn baseGetSpecVersion() SbiRet {
    // Wire 格式: major << 24 | minor
    return .{ .value = @as(u64, ver.major()) << 24 | ver.minor() };
}
fn baseGetImplId() SbiRet {
}
fn baseProbeExtension(eid: u32) SbiRet {
    // 编译期决定：这个 eid 是否被当前配置支持
    return switch (eid) {
        0x10 => .{ .value = 1 },           // BASE 永远支持
        0x54494D45 => if (enable_time) ...  // 看 Kconfig/-D 设了什么
        else => .{ .value = 0 },            // 不支持
    };
}
```

**作用：** OS 通过 base probe 发现 M-mode 固件提供了哪些扩展。Linux 启动时第一个 SBI 调用就是 probe TIME、IPI、HSM，决定用 SBI 路径还是 fallback。

**BASE 所有 9 个函数：**
| FID | 函数 | 返回 | 说明 |
|-----|------|------|------|
| 0 | get_spec_version | a1=major<<24\|minor | SBI 规范版本 |
| 2 | get_impl_version | a1=0x1 | 固件自己的版本号 |
| 3 | probe_extension(eid) | a1=0或非0 | 问"这个扩展你有吗？" |
| 4 | get_mvendorid | CSR mvendorid | CPU 厂商 JEDEC ID |
| 5 | get_marchid | CSR marchid | CPU 微架构 ID |
| 6 | get_mimpid | CSR mimpid | CPU 实现版本 |
| 7 | get_mhartid (v2.0+) | a1=hartid | 当前核的编号 |
| 8 | get_features (v3.0+) | 特性位图 | 固件支持哪些可选行为 |

### 5.2 TIME — 定时器（1 个函数，FID=0 的 set_timer）

**代码量：~15 行 Zig**

```zig
// platform/qemu_virt.zig
pub fn setTimer(deadline: u64) void {
    if (HAS_SSTC) {
        asm volatile ("csrw 0x14d, %[v]"  // 直接写 stimecmp CSR
            : : [v] "r" (deadline));
    } else {
        // 传统路径：写 CLINT mtimecmp，M-mode 代理转发 STIP
        mmioWrite64(CLINT_MTIMECMP_BASE + hartid * 8, deadline);
    }
}
```

**硬件行为：** 当 mtime（硬件自动递增的计数器）>= deadline（stimecmp/mtimecmp）时，硬件自动置 STIP 位。S-mode 看到 STIP=1 就知道闹钟响了，处理定时器中断（调度/超时/preempt）。

**Sstc vs 传统路径：**
- Sstc（有 stimecmp CSR）：S-mode 直接写 stimecmp，无需 M-mode 参与 → 快、简单
- 传统（无 Sstc）：S-mode ecall 到 M-mode → M-mode 写 mtimecmp → 在 M-mode 捕获定时器中断 → 设 STIP → mret → S-mode 处理 → 慢、多一次往返

**zigStart 中对应代码：** `menvcfg.STCE = 1` 就是为 Sstc 路径开的门。

### 5.3 IPI — 核间中断（1 个函数，FID=0 的 send_ipi）

**代码量：~15 行 Zig**

```zig
pub fn sendIpi(mask: HartMask) void {
    // 对 mask 中每个 hart，写它的 CLINT MSIP 寄存器
    var i: usize = 0;
    while (i < max_hart) : (i += 1) {
        if (mask.contains(i)) {
            mmioWrite32(CLINT_MSIP_BASE + i * 4, 1);  // 写1=触发软件中断
        }
    }
}
```

**硬件行为：** `mmioWrite32(MSIP, 1)` → 目标 hart 的 MSIP 位被置 1 → 如果目标 hart 未屏蔽，进入 M-mode trap handler → M-mode 检查 mcause=MSIP → 转发 SSIP 给 S-mode（如果 mideleg 委托）→ S-mode 处理 IPI。

**什么时候用 IPI？**
- Linux scheduler：某个核决定把任务迁移到另一个核 → send_ipi 通知目标核重新调度
- RFNC：执行完本地 fence，发 IPI 通知其他核也 fence
- HSM：启动一个 hart 时，发 IPI 唤醒目标 hart

### 5.4 HSM — Hart 生命周期管理（4 个函数）

**代码量：~60 行 Zig，因为要维护 hart_ctrl 状态数组**

```zig
// 每个 hart 一个控制块
const HartCtrl = struct {
    state: std.atomic.Value(u32) = ...,  // stopped / start_pending / started / suspended
    start_addr: u64 = 0,                  // 启动后跳到哪里
    priv: u64 = 0,                        // 传给 hart 的 opaque 值
};
pub var hart_ctrl: [MAX_HARTS]HartCtrl = ...;  // BSS 中，不占 FLASH

pub fn hartStart(hartid, start_addr, priv_val) SbiError {
    // CAS: stopped → start_pending
    ctrl.state.cmpxchgStrong(stopped, start_pending, ...);
    ctrl.start_addr = start_addr;
    ctrl.priv = priv_val;
    sendIpi(target_hart);  // 发 IPI 唤醒目标 hart
}
```

**4 个 HSM 函数：**
| FID | 函数 | 做什么 |
|-----|------|--------|
| 0 | hart_start(id, addr, priv) | 让 hart N 从 addr 开始执行，priv 传给它 |
| 1 | hart_stop() | 当前 hart 自我停止（wfi 循环） |
| 2 | hart_get_status(id) | 查询 hart N 当前状态 |
| 3 | hart_suspend(type, addr, priv) | 挂起 hart（retentive/non-retentive） |

**Linux 怎么用：** Linux SMP bringup → `hart_start(N, entry, hartid)` → M-mode 设 ctrl[N] → send_ipi → 目标 hart 收到 IPI → checkHsmIpi() 发现 start_pending → mret 到 start_addr → 目标 hart 进入 Linux 内核。

### 5.5 RFNC — 远程 TLB/缓存刷新（7 个函数）

**代码量：~40 行 Zig**

```zig
pub fn remoteFenceI(mask: HartMask) void {
    asm volatile ("fence.i");         // 先在本地执行
    rfncRequest(mask, .fence_i, ...); // 再通知远端
}
pub fn remoteSfenceVma(mask: HartMask, start, size: usize) void {
    asm volatile ("sfence.vma %[s], zero" : : [s] "r" (start));  // 本地TLB刷新
    rfncRequest(mask, .sfence_vma, start, size, 0);                // 通知远端
}
```

**7 个 RFNC 函数（3 个简单版 + 4 个复杂版）：**
| FID | 函数 | 刷新什么 | 场景 |
|-----|------|----------|------|
| 0 | remote_fence_i | 指令缓存 | JIT/自修改代码 |
| 1 | remote_sfence_vma | TLB 页表项 | 缺页处理 |
| 2 | remote_sfence_vma_asid | TLB（按 ASID 过滤）| 进程切换 |
| 3 | remote_hfence_gvma_vmid | G-stage TLB（按 VMID 过滤）| Hypervisor |
| 4 | remote_hfence_gvma | G-stage TLB 全部 | Hypervisor |
| 5 | remote_hfence_vvma_asid | VS-stage TLB（按 ASID）| 嵌套虚拟化 |
| 6 | remote_hfence_vvma | VS-stage TLB 全部 | 嵌套虚拟化 |

**为什么需要远程刷新？** 多核运行时，一个核改了页表，其他核的 TLB 里还缓存着旧映射。必须通知它们刷新 TLB，否则读到过期数据。

### 5.6 SRST — 系统复位（1 个函数）

**代码量：~10 行 Zig**

```zig
pub fn systemReset(reset_type: u32, reset_reason: u32) noreturn {
    const val: u32 = switch (reset_type) {
        0 => 0x5555,   // SHUTDOWN → QEMU PASS（退出了）
        1, 2 => 0x7777, // COLD/WARM REBOOT → QEMU RESET
        else => 0x3333, // unknown → FAIL
    };
    mmioWrite32(TEST_DEV_BASE, val);  // 写 SiFive test device
    while (true) { asm volatile ("wfi"); }  // 等重置生效
}
```

**3 种复位类型：** SHUTDOWN（关机）、COLD_REBOOT（冷重启，重新上电）、WARM_REBOOT（热重启，不掉电）。

### 5.7 DBCN — 调试串口（3 个函数，v2.0-rc1 引入）

**代码量：~25 行 Zig**

```zig
pub fn consolePutByte(byte: u8) void {
    while (mmioRead8(UART_LSR) & UART_LSR_TX_EMPTY == 0) {}  // 等 TX FIFO 有空位
    mmioWrite8(UART_THR, byte);  // 写发送寄存器
}
pub fn consoleGetByte() ?u8 {
    if (mmioRead8(UART_LSR) & UART_LSR_RX_READY == 0) return null;  // 没数据
    return mmioRead8(UART_RBR);  // 读接收寄存器
}
```

**3 个 DBCN 函数：**
| FID | 函数 | 说明 |
|-----|------|------|
| 0 | console_write | 写 n 个字节到串口 |
| 1 | console_read | 从串口读 n 个字节 |
| 2 | console_write_byte | 写单个字节（最常用） |

---

## 6. Legacy vs EID — 两代调用方式的对比

### Legacy（v0.x）—— 朴素时代

```
a7 = 0  → set_timer(stime_value)       // 设闹钟
a7 = 1  → console_putchar(ch)
a7 = 2  → console_getchar()            // 返回字符（-1 = 无数据）
a7 = 3  → clear_ipi()
a7 = 4  → send_ipi(hart_mask)
a7 = 5  → remote_fence_i(hart_mask)
a7 = 6  → remote_sfence_vma(hart_mask, start, size)
a7 = 7  → remote_sfence_vma_asid(hart_mask, start, size, asid)
a7 = 8  → shutdown()
```

**特点：** a7 直接是函数号。返回值只有 a0（错误码），没有 a1（值）。没有 probe 机制——OS 调用前不知道函数存不存在。

### EID 体系（v1.0+）—— 规范时代

```
a7 = eid = 0x10 → BASE
a7 = eid = 0x54494D45("TIME") → Timer
...

a6 = fid  // 扩展内的函数编号
a0~a5 = 参数
返回值：a0 = error, a1 = value
```

**改进：**
1. 有 EID 命名空间，不会冲突
2. 返回值有 error + value 两个字段
3. 有 probe_extension → OS 可以先探测再调用
4. EID 值是 ASCII 字符串（0x54494D45 = "TIME"，0x53525354 = "SRST"）——可读性好

---

## 7. SBI 版本号编码（wire format）

SBI 规范版本用 `(major << 24) | minor` 编码，不直接用语义版本号（如 "1.0"）：

| 规范版本 | Wire值 | 说明 |
|----------|--------|------|
| v0.1–v1.0-rc3 | < `1 << 24` | 旧编码，只有 1 个数字 |
| v1.0 | `1 << 24` (0x01000000) | major=1, minor=0 |
| v2.0 | `2 << 24` (0x02000000) | major=2, minor=0 |
| v3.0 | `3 << 24` (0x03000000) | major=3, minor=0 |

**Linux 检测逻辑：** `sbi_spec_version >= (1 << 24)` → 使用 EID 体系；否则走 Legacy 兼容路径。

---


```zig
// preset(.full, v3_0, platform) 编译期展开后等价于：
const MySbi = struct {
    const enable_time  = true;
    const enable_ipi   = true;
    const enable_hsm   = true;
    const enable_rfnc  = true;
    const enable_srst  = true;
    const enable_dbcn  = true;
    // enable_pmu/enable_susp/... = false （v0.1 未实现）

    fn dispatch(eid: u32, fid: u32, args: SbiArgs) SbiRet {
        return switch (eid) {
            // comptime 条件编译：enable_time=false 时，这个 case 被刪除
            0x54494D45 => if (enable_time) switch (fid) {
                0 => timeSetTimer(args.a0),
                else => .not_supported,
            } else .not_supported,
            // 同理 IPI/HSM/RFNC/SRST/DBCN...
            else => probe_extension 覆盖不到的 → .not_supported
        };
    }
};
// sizeof(MySbi) = 0（ZST，零字节运行时开销）
```

**设计要点：**
- `dispatch()` 是 `switch (eid)` → `switch (fid)` 的嵌套 **comptime 展开**
- 未启用的扩展在编译期就被删掉了——`.not_supported` 是编译期常量折叠
- 每个扩展的 `Platform` 方法（如 `setTimer()`）在被调用时才存在，`@hasDecl` 保证编译安全
- 新增扩展只需多加一个 `switch case`，不影响已有代码

---

## 9. 总结 — SBI 的边界

**SBI 管的事：**
- 定时器（设闹钟）
- 核间中断（通知其他核）
- TLB/缓存一致性（多核 MMU）
- Hart 启停（多核 bringup）
- 系统复位（关机/重启）
- 调试串口（打印/输入）
- 性能监控（PMU 计数器）
- 高级功能：电源管理、虚拟化加速、固件注入事件

**SBI 不管的事：**
- 中断控制器（PLIC/APLIC 由 OS 直接管，SBI 只负责一些委托）
- 内存管理（页表、虚拟内存是 OS 的事）
- 设备驱动（UART 之外的设备，如网卡、磁盘）
- 文件系统、网络协议栈
- 进程调度、IPC

**一句话：SBI = 硬件抽象层的第一层（也是最底层），负责屏蔽板级差异，给 OS 一个统一的接口。**
