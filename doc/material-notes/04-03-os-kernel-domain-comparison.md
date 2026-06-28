# 04-03 — OS 内核横向深对比：8 项目 × 9 维度全景表

> **核心问题：**
> 1. xv6 / tg-rcore / arceos / asterinas / DragonOS / StarryOS / seL4 / unikraft —— 8 个本地项目，**每个子系统**（boot / mm / sched / fs / net / driver / syscall / 安全 / 工程化）各自怎么做？
> 2. 同一个问题（如 "secondary hart 怎么启动"）每个项目的答案是什么？为什么不一样？
>
>
>
> 与 [03-05](03-05-boot-domain-comparison.md) boot 横向对比的对应关系：03-05 是 boot 6 项目对比，本笔记是 OS 8 项目对比。结构刻意对齐，便于跨章学习。

---

## 0. 8 项目 × 9 维度全景大表

> 看不懂表格里的术语 → 回 [04-01 § 7](04-01-os-kernel-overview.md) 词典。

### 8 个对比项目（按学习顺序）

| # | 项目 | 语言 | 大小 | 范式（结构 / 边界 / 部署）| 教学/工业 |
|---|------|------|------|--------------------------|----------|
| 1 | **xv6** | C | 2 K 行 | 宏 / 多 MMU / 通用 | 教学（必学起点）|
| 2 | **tg-rcore** | Rust | 2 万行 | 宏 / 多 MMU / 通用 | 教学（rCore 课程）|
| 4 | **StarryOS** | Rust | 10 万行 | 组件 / 多 MMU / 通用 | 教学（arceos 上的宏内核人格）|
| 5 | **asterinas** | Rust | 5 万行 | 组件 / 多 MMU / 通用 | 工业（framekernel 安全）|
| 6 | **DragonOS** | Rust+C | 50 万行 | 宏 / 多 MMU / 通用 | 工业（国产，Linux ABI）|
| 7 | **seL4** | C+Haskell | 9 K + 验证 | 微 / 多 MMU / 通用 | 工业（形式化）|
| 8 | **unikraft** | C | ~50 万行 | 库 / 单地址 / 单 app | 工业（Unikernel 框架）|

### 9 个对比维度

```
A. 启动 (boot to first task)
B. 内存管理 (mm)
C. 调度器 (sched)
D. syscall 机制
E. 文件系统 (vfs)
F. 网络栈 (net)
G. 驱动框架 (driver)
H. 安全 / 隔离模型
I. 工程化 (build / config / test)
```

### 全景大表（点选每行进入对应章节）

| 维度 | xv6 | tg-rcore | arceos | StarryOS | asterinas | DragonOS | seL4 | unikraft |
|------|-----|----------|--------|----------|-----------|----------|------|----------|
| A 启动 | sbi+main | sbi+rust_main | axhal+axruntime | 同 arceos | OSTD | bsp+main | elfloader | unikraft entry |
| B mm | 简单 buddy | sv39 + buddy | axalloc 多 alloc | 同 arceos | safer alloc | slab+buddy | capability | 简单 alloc |
| C sched | RR | RR / Stride | FIFO / RR / CFS | 同 arceos + 多人格 | preempt | CFS-like | priority | 协作 / RR |
| D syscall | trap-based | trap | 无（unikernel）/ 有（人格）| Linux 完整 | Linux | Linux 完整 | IPC | hostcall |
| E fs | inode | easyfs | axfs（FAT/ext4 可选）| ext4 / FAT | ext2/3/4 | ext4 / VFS | 用户态 | 9p / fat |
| F net | 无 | 无（教学）| smoltcp | smoltcp | smoltcp | smoltcp + lwip | netbsd-like | mTCP / lwip |
| G driver | 硬编码 | 硬编码 | axdriver + virtio | 同 + Linux DM | OSTD device | uclass / DT | 用户态 server | platform abstraction |
| H 安全 | 进程隔离 | 进程隔离 | 类型隔离 | 进程 + 类型 | framekernel | 多机制 | capability + 形式化 | 单地址（无）|
| I 工程化 | Makefile | cargo + qemu | cargo features + axconfig | 同 | cargo + IDL | cargo + 自构建 | bazel + Isabelle | KConfig + Make |


---

## 1. 维度 A — 启动（boot to first task）

### 1.1 各项目入口对比

```
项目         | 入口 fn                    | 入口前提（boot 给的）           | 跳到 first task
─────────────────────────────────────────────────────────────────────────
xv6          | _entry → start → main      | OpenSBI 跳到 0x80200000        | scheduler() 在 main 末尾
tg-rcore     | _start → rust_main         | sbi 给 a0=hartid a1=dtb        | run_first_task()
arceos       | _start → rust_entry        | axhal 解 dtb / 调 axruntime     | axruntime::rt_main()
asterinas    | OSTD entry → main          | bootloader (multiboot2)        | task::spawn_init()
DragonOS     | bsp entry → kernel_main    | grub multiboot2                 | sched::start()
seL4         | elfloader → init thread    | uefi / firmware                | rootserver
unikraft     | unikraft entry             | kvm/firecracker hyper-v        | uk_main
```

### 1.2 secondary hart 启动方式（多核必看）

| 项目 | 方式 | 备注 |
|------|------|------|
| xv6 | wfi + IPI | OpenSBI HSM `hart_start` |
| tg-rcore | 同 xv6 | 教学清晰 |
| arceos | axhal smp module | 抽象 SBI HSM |
| StarryOS | 同 arceos | 复用 |
| asterinas | OSTD smp module | 类似 |
| DragonOS | 平台 ASM + sched 接管 | x86 / RISC-V 各异 |
| seL4 | rootserver IPC 启动 | 微内核风 |
| unikraft | 单 vCPU 默认 | 多 vCPU 选项 |


---

## 2. 维度 B — 内存管理（mm）

### 2.1 物理内存分配器

| 项目 | 分配器 | 特点 |
|------|--------|------|
| xv6 | kalloc 链表 | 极简，4K 页粒度 |
| tg-rcore | buddy | 教学经典 |
| arceos | **axalloc 多算法**（buddy / slab / TLSF）| **可换** —— cargo features 选 |
| StarryOS | 同 arceos | 复用 |
| asterinas | safer alloc + alloc_zeroed | 全程安全 Rust |
| DragonOS | slab + buddy + percpu | 工业三层 |
| seL4 | **untyped capability** | 内核不分配，应用 retype |
| unikraft | 简单 alloc（uk_alloc）| 几种插件 |

### 2.2 页表实现

| 项目 | 架构 | 页表 |
|------|------|------|
| xv6 | RISC-V | Sv39 三级 |
| tg-rcore | RISC-V | Sv39 |
| arceos | x86 / ARM / RISC-V | axhal::paging 抽象 |
| asterinas | x86_64 | x86_64 page_table_x86 |
| DragonOS | x86 / RISC-V | 多架构 |
| seL4 | 全架构 | capability-based |
| unikraft | 全架构 | uk_paging |

**axalloc 风可换分配器** + **axhal::paging 多架构抽象**——直接借鉴 arceos。

---

## 3. 维度 C — 调度器（sched）

### 3.1 调度算法

| 项目 | 算法 | 抢占 | 多核 |
|------|------|------|------|
| xv6 | RR | 否 | 是 |
| tg-rcore | RR + Stride | 否 | 教学单核 |
| arceos | **多算法**（FIFO / RR / CFS）| 可选 | 是 |
| StarryOS | 同 arceos | 是 | 是 |
| asterinas | preempt + 优先级 | 是 | 是 |
| DragonOS | CFS-like | 是 | 是 |
| seL4 | priority + budgets | 是 | 是 |
| unikraft | 协作 / RR / EDF | 协作默认 | 单核默认 |

### 3.2 任务（task）数据结构

| 项目 | 结构 | 备注 |
|------|------|------|
| xv6 | struct proc | 简单 PCB |
| tg-rcore | struct TaskControlBlock | 教学清晰 |
| arceos | struct AxTask | 字段裁剪（cargo features 选）|
| asterinas | struct Task | safer Rust 包装 |
| DragonOS | struct ProcessControlBlock | Linux 风 |
| seL4 | TCB capability | 不可直接访问 |

**axtask + 多算法可换** + **抢占可选 cargo feature**——直接借鉴 arceos。具体多人格调度器选择见 StarryOS。

---

## 4. 维度 D — syscall 机制

### 4.1 syscall 表实现

| 项目 | 表实现 | 数量 |
|------|--------|------|
| xv6 | sysnum.h + syscall.c switch | ~22 |
| tg-rcore | enum + match | ~30 |
| arceos | **无 syscall**（unikernel）/ axsyscall（人格）| 0 / Linux 兼容 |
| StarryOS | Linux 完整 | 200+ |
| asterinas | Linux 完整 | 200+ |
| DragonOS | Linux 完整 | 350+ |
| seL4 | IPC 形式（非传统 syscall）| 9 个原始 op |
| unikraft | hostcall（非传统 syscall）| 几十 |

### 4.2 trap entry 实现

| 项目 | 入口 | 寄存器保存 |
|------|------|-----------|
| xv6 | uservec / kernelvec | 全 31 通用 |
| tg-rcore | __alltraps in trap.S | 全保 |
| arceos | axhal::arch::trap | 抽象 |
| asterinas | OSTD trap entry | 安全 Rust |
| DragonOS | platform-specific .S | 各架构独立 |
| seL4 | priv.S + cap-protected | 形式化验证过 |

**axsyscall 风**：unikernel 默认无 syscall；**personality-monolithic 启用 Linux ABI 兼容层**（200+ syscall，参照 [04-13](04-13-syscall-linux-list.md)）。

---

## 5. 维度 E — 文件系统（vfs / fs）

### 5.1 VFS 抽象层

| 项目 | VFS | 后端 fs |
|------|-----|---------|
| xv6 | inode + struct file | 自定义（无 VFS）|
| tg-rcore | 简单 trait | easyfs |
| arceos | **axfs trait** | **可选**：lwext4_rust / ext4_rs / fatfs |
| StarryOS | 同 arceos | 同 |
| asterinas | aster-fs | ext2 / 9p / proc |
| DragonOS | VFS（Linux 风）| ext4 / FAT / FUSE |
| seL4 | 无（用户态 fs server）| 用户实现 |
| unikraft | 多 fs（vfscore）| ext4 / fat / 9p |

**axfs trait + 多后端可选**——cargo features 选择 ext4_rs / lwext4_rust / fatfs。

详见后续 06-XX 章节（FS 大类专题）。

---

## 6. 维度 F — 网络栈（net）

### 6.1 网络栈实现

| 项目 | 栈 | 备注 |
|------|------|------|
| xv6 | **无** | 无网络 |
| tg-rcore | **无** | 教学不含 |
| arceos | **smoltcp**（默认）| Rust 无 alloc 栈 |
| StarryOS | smoltcp | 同 |
| asterinas | smoltcp | 同 |
| DragonOS | smoltcp + lwip | 双栈 |
| seL4 | 无（用户态 server）| 用户实现 |
| unikraft | mTCP / lwip / smoltcp | 多选 |

**smoltcp 默认**——arceos 风。详见后续 07-XX 章节（Net 大类专题）。

---

## 7. 维度 G — 驱动框架（driver）

### 7.1 驱动模型

| 项目 | 模型 | 风格 |
|------|------|------|
| xv6 | **硬编码** | 平台特定（uart16550 直接 inb）|
| tg-rcore | **硬编码** | 同 xv6 |
| arceos | **axdriver trait + virtio** | 组件化 + virtio |
| StarryOS | 同 + Linux DM 风 | 多人格 |
| asterinas | OSTD device | safer Rust |
| DragonOS | **uclass + DT** | Linux 风 |
| seL4 | **用户态 server** | 微内核风 |
| unikraft | platform abstraction | 库风 |


- 同一份源码，不同消费者各取所需


---

## 8. 维度 H — 安全 / 隔离模型

### 8.1 隔离机制

| 项目 | 隔离方式 | 强度 |
|------|---------|------|
| xv6 | 进程 MMU | 弱（C 内核）|
| tg-rcore | 进程 MMU | 中（Rust 内核）|
| arceos | **类型隔离**（默认 unikernel）| 中 |
| StarryOS | 进程 + 类型 | 强 |
| asterinas | **framekernel** | **极强**（unsafe ~5%）|
| DragonOS | 进程 + 类型 | 中-强 |
| seL4 | **capability + 形式化** | **最强**（形式化证明）|
| unikraft | **无**（单地址）| 无 |

### 8.2 形式化验证

只有 seL4 通过 —— ~9K 行 C + ~30 万行 Isabelle 证明。

asterinas 用 Rust 类型系统达到"接近形式化的强度"——所有 unsafe 包在 framekernel base，base 之外的内核代码 100% safe Rust。

**framekernel 思想**（asterinas 风）+ **safe Rust 优先** —— 关键路径 unsafe 限制在 base 模块，业务代码 safe Rust。形式化验证不做（成本 > 收益）。

---

## 9. 维度 I — 工程化（build / config / test）

### 9.1 构建系统

| 项目 | 构建 | 配置 |
|------|------|------|
| xv6 | Makefile | Makefile 变量 |
| tg-rcore | Makefile + cargo | 简单 |
| arceos | **cargo + axconfig** | **cargo features + TOML** |
| StarryOS | 同 arceos | 同 |
| asterinas | cargo + IDL | TOML |
| DragonOS | cargo + 自定义构建 | TOML |
| seL4 | bazel + Isabelle | bazel + Kbuild |
| unikraft | **KConfig + Make**（Buildroot 风）| Kconfig |

### 9.2 测试

| 项目 | 测试方式 |
|------|--------|
| xv6 | 几个测试程序 |
| tg-rcore | usertests + Rust unit test |
| arceos | apps/ 目录 + cargo test |
| asterinas | 完整 testsuite + Linux test |
| DragonOS | LTP + 自己的 |
| seL4 | seL4test（Isabelle 验证）|


---



| # | 决策 | 主参考 | 次参考 | 备注 |
|---|------|-------|-------|------|
| A | 启动 | arceos (axhal + axruntime) | tg-rcore | 多核 SMP |
| B | mm | arceos (axalloc 多算法) | asterinas safer alloc | + paging 抽象 |
| C | sched | arceos (多算法可换) | StarryOS 多人格 | + asterinas 抢占 |
| D | syscall | DragonOS / asterinas (Linux 完整) | arceos 抽象 | 200+ syscall (04-13) |
| E | fs | arceos (axfs trait) | DragonOS VFS | 后续 06-XX |
| F | net | arceos (smoltcp) | DragonOS 双栈 | 后续 07-XX |
| H | 安全 | asterinas (framekernel) | seL4 思想（启示）| safe Rust 优先 |


---

## 11. 学习顺序按"决策点"排序


```
Stage 1 — 启动 + mm 决策（基础）
  xv6 → tg-rcore → arceos axhal/axalloc

Stage 2 — sched + syscall 决策
  arceos axtask → StarryOS（多人格）→ DragonOS / asterinas（Linux 完整）

Stage 3 — fs + net 决策
  arceos axfs/smoltcp → DragonOS VFS → 跨 06/07 章节

Stage 4 — driver + 安全决策
  arceos axdriver → DragonOS uclass → asterinas framekernel → seL4 capability（启示）

Stage 5 — 工程化决策
```

---

## 12. 进一步阅读

### 本仓库内
- [04-01 OS 大类全局视图](04-01-os-kernel-overview.md)
- [04-02 OS 范式归纳](04-02-os-kernel-paradigms.md)
- [04-12..03 syscall 三件套](04-12-syscall-arch-abi.md)
- [03-05 boot 横向对比](03-05-boot-domain-comparison.md) —— 结构对应

### 项目本地
- `core/{xv6,tg-rcore,arceos,tg-arceos,StarryOS,asterinas,DragonOS,seL4,unikraft}/`

### 经典论文
- [arceos paper](https://github.com/arceos-org/arceos/wiki) —— 组件化设计
- [Theseus paper](https://www.usenix.org/conference/osdi20/presentation/boos) (OSDI'20)
- [Asterinas paper](https://asterinas.github.io/) —— framekernel
- [seL4 paper](https://sel4.systems/About/seL4-whitepaper.pdf) —— 形式化

---

## FAQ

### Q1: 为什么没把 Linux 放进对比？

### Q2: 为什么 arceos 在 9 个维度中 6 次成主参考？


### Q4: seL4 学到什么程度算"够"？

### Q5: tg-arceos 在哪个维度对比？
tg-arceos = arceos 教学**示例集合**，不是独立项目，不参与 9 维度对比。学 arceos 必看 tg-arceos 示例（[04-01 § 2.2](04-01-os-kernel-overview.md) Stage 2）。

### Q6: 横向对比表里的"工业级"评分依据？
- ★★★★★：生产环境广泛部署（Linux / FreeRTOS / xen）
- ★★★★：成熟工业项目（DragonOS / unikraft / xen）
- ★★★：可工业部署但 niche（arceos / asterinas / seL4 在汽车）
- ★★：研究项目接近工业（StarryOS / Theseus / TornadoOS）
- ★：纯教学（xv6 / tg-rcore / jos）
- -：太新 / 太特殊（BareMetal SASOS）

---

