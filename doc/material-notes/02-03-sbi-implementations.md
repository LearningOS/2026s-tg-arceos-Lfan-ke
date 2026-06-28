# SBI 实现横向对比：BBL / OpenSBI / RustSBI / tg-rcore

> 数据来源：本地源码直接扫描（`sbi/riscv-pk`、`sbi/opensbi`、`sbi/rustsbi`、`core/tg-rcore`）

---

## 0. 速览对比表

| 维度 | BBL (riscv-pk) | tg-rcore SBI | RustSBI Prototyper | OpenSBI v1.8 |
|------|---------------|-------------|-------------------|-------------|
| **语言** | C | Rust | Rust | C |
| **SBI 版本** | pre-spec (Legacy only) | v1.0（声称）| v2.0 | v3.0 |
| **扩展数量** | 1 (Legacy, 9函数) | 4个EID（~11函数）| 9个（library层12个）| 15个 + Legacy |
| **总 FID 数** | 9 | ~11 | 43 (library) / 33 (prototyper) | 70+ |
| **代码体量** | ~300行 | ~150行 | ~3000行 | ~60000行 |
| **适用场景** | Spike模拟器遗留 | 教学（rCore教程）| 研究/原型 | 生产固件 |
| **关键文件** | `machine/mcall.h` `machine/mtrap.c` | `msbi.rs` `m_entry.asm` `lib.rs` | `library/src/*.rs` `prototyper/src/sbi/*.rs` | `lib/sbi/sbi_ecall_*.c` |

---

## 1. BBL（Berkeley Boot Loader）— pre-spec 原型

### 历史地位

BBL 是 SBI 规范的**前身**，SBI v0.1 的 Legacy 函数就是从 BBL 的行为中抽象出来的。
代码极简，是理解"为什么 SBI 要这样设计"的最佳一手资料。

### 调用约定（pre-spec，无 EID/FID 概念）

```
a7 = SBI magic number（=调用号，只有一层，没有 EID/FID 之分）
a0-a5 = 参数
a0 = 返回值（仅 a0，无 a1 错误码）
```

### 9 个 Legacy 调用清单

| Magic (a7) | 常量名 | 处理函数 | 核心实现（2-3行）|
|-----------|--------|---------|----------------|
| `0x0` | `SBI_SET_TIMER` | `mcall_set_timer()` | 写 `timecmp` CSR；清 STIP；置 MTIE，让 M-mode 定时器中断代理 S-mode |
| `0x1` | `SBI_CONSOLE_PUTCHAR` | `mcall_console_putchar()` | 派发到 UART 驱动（uart/uart16550/uart_litex）或 HTIF；字符在 a0 |
| `0x2` | `SBI_CONSOLE_GETCHAR` | `mcall_console_getchar()` | 从 UART/HTIF 读字符；无字符时返回 -1（非阻塞）|
| `0x3` | `SBI_CLEAR_IPI` | `mcall_clear_ipi()` | 清 `mip.SSIP`；返回旧值 |
| `0x4` | `SBI_SEND_IPI` | `send_ipi_many(IPI_SOFT)` | 按 a0 指向的 hart bitmask，原子置各 hart `mipi_pending`；发软件中断 |
| `0x5` | `SBI_REMOTE_FENCE_I` | `send_ipi_many(IPI_FENCE_I)` | 向目标 hart 发 IPI；目标 hart IPI 处理器中执行 `fence.i` |
| `0x6` | `SBI_REMOTE_SFENCE_VMA` | `send_ipi_many(IPI_SFENCE_VMA)` | 类似 fence.i，目标 hart 执行 `sfence.vma`（无 ASID）|
| `0x7` | `SBI_REMOTE_SFENCE_VMA_ASID` | `send_ipi_many(IPI_SFENCE_VMA)` | **与 0x6 实现相同**，ASID 未区分（历史债）|
| `0x8` | `SBI_SHUTDOWN` | `mcall_shutdown()` | 调用 `poweroff(0)` → `finisher_exit()` 或 `htif_poweroff()` |

### IPI 子类型（mentry.S 中处理）

```
IPI_SOFT      (0x1) → 置 mip.SSIP（软件中断透传给 S-mode）
IPI_FENCE_I   (0x2) → 执行 fence.i
IPI_SFENCE_VMA(0x4) → 执行 sfence.vma
IPI_HALT      (0x8) → WFI 死循环（poweroff 时用）
```

### 关键设计：Hart-Local Storage (HLS)

每个 hart 在 M-mode 栈顶维护一个 HLS 结构：
```c
// machine/mtrap.h
typedef struct {
  volatile int mipi_pending;    // 待处理 IPI 类型
  volatile void *ipi;           // IPI doorbell 寄存器地址
  // ...
} hls_t;
```
`send_ipi_many` 通过 `OTHER_HLS(hartid)->mipi_pending` 跨核通信，自旋等待目标核响应。

---

## 2. tg-rcore SBI — 教学最简实现

### 设计哲学

"够用即可"——给 rCore 教学内核用，覆盖三大核心需求：
**控制台输出 + 定时器 + 关机**。代码约 150 行，M-mode 和 S-mode 调用侧均在同一仓库。

### 文件结构

```
tg-rcore-tutorial-sbi/src/
├── msbi.rs       ← M-mode 处理函数（SBI 固件侧）
├── m_entry.asm   ← M-mode 启动 + 陷阱向量（汇编）
└── lib.rs        ← S-mode ecall 封装（内核调用侧）
```

### 支持的 EID/FID

| EID | EID 值 | FID | 函数 | 功能 |
|-----|--------|-----|------|------|
| Legacy PUTCHAR | `0x01` | — | `console_putchar(c)` | 向 UART 输出单字节（阻塞）|
| Legacy GETCHAR | `0x02` | — | `console_getchar()` | 从 UART 读单字节（阻塞）|
| Legacy SHUTDOWN | `0x08` | — | `shutdown(false)` | 关机 |
| BASE | `0x10` | 0 | `GET_SBI_VERSION` | 返回 `0x01000000`（v1.0）|
| BASE | `0x10` | 1 | `GET_IMPL_ID` | 返回 `0xFFFF`（自定义）|
| BASE | `0x10` | 2 | `GET_IMPL_VERSION` | 返回 `1` |
| BASE | `0x10` | 3 | `PROBE_EXTENSION` | 简化实现，统一返回 `1` |
| BASE | `0x10` | 4-6 | `GET_M*ID` | 均返回 `0` |
| TIME | `0x54494D45` | 0 | `set_timer(time)` | 写 CLINT mtimecmp；置 MTIE；清 STIP |
| SRST | `0x53525354` | 0 | `shutdown` | 写 `0x5555` 到 QEMU test 设备 |
| SRST | `0x53525354` | 1 | `cold_reboot` | 写 `0x3333` 到 QEMU test 设备 |
| SRST | `0x53525354` | 2 | `warm_reboot` | 写 `0x3333` 到 QEMU test 设备 |

### MMIO 地址（QEMU virt 机器）

```
0x1000_0000  UART NS16550A  (THR@+0, LSR@+5)
0x200_4000   CLINT mtimecmp (hart0)
0x10_0000    QEMU test 设备  (关机/重启控制)
```

### M-mode 启动序列（m_entry.asm）

```asm
_m_start:
  1. 设置 M-mode 栈，mscratch = 栈顶
  2. mstatus.MPP = 01（S-mode），MPIE = 1
  3. mepc = _start（S-mode 内核入口）
  4. mtvec = m_trap_vector
  5. mideleg = 0xffff（所有中断委托给 S-mode，定时器中断除外）
  6. medeleg = 0xffff & ~(1<<9)（所有异常委托，除 ecall from S-mode）
  7. PMP: 全地址 RWX 开放给 S-mode
  8. mcounteren = -1（允许 S-mode 读 cycle/time/instret）
  9. mret → 跳入 S-mode
```

### 定时器中断代理模式（关键设计）

```
S-mode 请求 set_timer(T)
  → M-mode ecall handler
  → 写 mtimecmp = T
  → 开 MTIE（mie |= 1<<7）
  → 清 STIP（mip &= ~(1<<5)）

之后 mtime >= T：
  → M-mode 定时器中断触发（M_TRAP_VECTOR）
  → m_handle_mtimer:
      关 MTIE（防中断风暴）
      置 STIP（mip |= 1<<5）
      mret 回 S-mode
  → S-mode 看到 STIP，处理定时器中断
```

### S-mode 调用侧（lib.rs）

```rust
// nobios 模式（内置 M-mode）：返回 SbiRet{error, value}
pub fn set_timer(timer: u64) { sbi_call(0x54494D45, 0, timer as usize, 0, 0); }
pub fn console_putchar(c: u8) { sbi_call(0x01, 0, c as usize, 0, 0); }
pub fn console_getchar() -> usize { sbi_call(0x02, 0, 0, 0, 0) }
pub fn shutdown(failure: bool) -> ! { sbi_call(0x53525354, failure as usize, 0, 0, 0); loop {} }
```

---

## 3. RustSBI Prototyper — 研究级分层实现

### 架构分层

```
library/rustsbi/src/     ← trait 接口层（只定义规范，不实现）
prototyper/src/sbi/      ← 具体实现层（注册到 SBI struct）
```

### Library 层（trait 定义，SBI v2.0 接口规范）

| 文件 | Trait | EID |
|------|-------|-----|
| `timer.rs` | `Timer` | `0x54494D45` |
| `ipi.rs` | `Ipi` | `0x735049` |
| `rfence.rs` | `Rfence` | `0x52464E43` |
| `hsm.rs` | `Hsm` | `0x48534D` |
| `reset.rs` | `Reset` | `0x53525354` |
| `pmu.rs` | `Pmu` | `0x504D55` |
| `console.rs` | `Console` | `0x4442434E` |
| `susp.rs` | `Susp` | `0x53555350` |
| `cppc.rs` | `Cppc` | `0x43505043` |
| `nacl.rs` | `Nacl` | `0x4E41434C` |
| `sta.rs` | `Sta` | `0x5354` |

### Prototyper 层（实际注册的 SBI struct）

```rust
// prototyper/src/sbi/mod.rs
#[derive(RustSBI, Default)]
#[rustsbi(dynamic)]
pub struct SBI {
    #[rustsbi(console)]          pub console: Option<SbiConsole>,   // DBCN
    #[rustsbi(ipi, timer)]       pub ipi:     Option<SbiIpi>,       // IPI + TIME（共用 CLINT）
    #[rustsbi(hsm)]              pub hsm:     Option<SbiHsm>,       // HSM
    #[rustsbi(reset)]            pub reset:   Option<SbiReset>,     // SRST
    #[rustsbi(fence)]            pub rfence:  Option<SbiRFence>,    // RFNC
    #[rustsbi(pmu)]              pub pmu:     Option<SbiPmu>,       // PMU
    #[rustsbi(susp)]             pub susp:    Option<SbiSuspend>,   // SUSP
    // CPPC / NACL / STA：library 有 trait，但 prototyper 未注册
}
```

### 完整 FID 清单

**BASE (0x10) — 自动生成，7 个 FID**

| FID | 函数 |
|-----|------|
| 0 | `get_sbi_spec_version` → v2.0 |
| 1 | `get_sbi_impl_id` → 4（RustSBI）|
| 2 | `get_sbi_impl_version` |
| 3 | `probe_extension` |
| 4-6 | `get_mvendorid/marchid/mimpid` |

**TIME (0x54494D45) — 1 个 FID**

| FID | 函数 |
|-----|------|
| 0 | `set_timer(stime_value: u64)` |

> `SbiIpi` 同时实现 `Timer` + `Ipi` trait，共享 CLINT 驱动

**IPI (0x735049) — 1 个 FID**

| FID | 函数 |
|-----|------|
| 0 | `send_ipi(hart_mask: HartMask)` |

**RFNC (0x52464E43) — 7 个 FID，后 4 个 H-extension 相关**

| FID | 函数 | 状态 |
|-----|------|------|
| 0 | `remote_fence_i(hart_mask)` | ✅ 实现 |
| 1 | `remote_sfence_vma(hart_mask, start, size)` | ✅ 实现 |
| 2 | `remote_sfence_vma_asid(hart_mask, start, size, asid)` | ✅ 实现 |
| 3 | `remote_hfence_gvma_vmid(...)` | ⚠️ 返回 not_supported |
| 4 | `remote_hfence_gvma(...)` | ⚠️ 返回 not_supported |
| 5 | `remote_hfence_vvma_asid(...)` | ⚠️ 返回 not_supported |
| 6 | `remote_hfence_vvma(...)` | ⚠️ 返回 not_supported |

**HSM (0x48534D) — 4 个 FID**

| FID | 函数 |
|-----|------|
| 0 | `hart_start(hartid, start_addr, opaque)` |
| 1 | `hart_stop()` |
| 2 | `hart_get_status(hartid)` |
| 3 | `hart_suspend(type, resume_addr, opaque)` |

**SRST (0x53525354) — 1 个 FID**

| FID | 函数 | 支持类型 |
|-----|------|---------|
| 0 | `system_reset(reset_type, reset_reason)` | SHUTDOWN(0) / COLD_REBOOT(1) / WARM_REBOOT(2) |

**PMU (0x504D55) — 8 个 FID**

| FID | 函数 |
|-----|------|
| 0 | `num_counters()` |
| 1 | `counter_get_info(idx)` |
| 2 | `counter_config_matching(base, mask, flags, event_idx, event_data)` |
| 3 | `counter_start(base, mask, flags, initial)` |
| 4 | `counter_stop(base, mask, flags)` |
| 5 | `counter_fw_read(idx)` |
| 6 | `counter_fw_read_hi(idx)` |
| 7 | `snapshot_set_shmem(shmem, flags)` |

**DBCN (0x4442434E) — 3 个 FID**

| FID | 函数 | 阻塞性 |
|-----|------|--------|
| 0 | `write(bytes: Physical<&[u8]>)` | 非阻塞 |
| 1 | `read(bytes: Physical<&mut [u8]>)` | 非阻塞 |
| 2 | `write_byte(byte: u8)` | 阻塞 |

**SUSP (0x53555350) — 1 个 FID**

| FID | 函数 | 支持类型 |
|-----|------|---------|
| 0 | `system_suspend(sleep_type, resume_addr, opaque)` | SUSPEND_TO_RAM(0) |

**未注册（library 有 trait，prototyper 跳过）：**
- CPPC (0x43505043)：4 个 FID，probe/read/read_hi/write
- NACL (0x4E41434C)：5 个 FID，probe_feature/set_shmem/sync_csr/sync_hfence/sync_sret
- STA (0x5354)：1 个 FID，set_shmem

---

### 3.X RustSBI 2026-05 大更新（commits `9d65606..d346d52`）


#### A. AIA (Advanced Interrupt Architecture) 全栈支持 ⭐

**新增模块：** `prototyper/prototyper/src/platform/aia.rs`（243 行）

实现内容：
- **APLIC delegation** —— `init_qemu_m_aplic_delegation` 把 M-level APLIC 中断委派给 S-level（QEMU virt 0x0c00_0000）
- **IMSIC IPI backend** —— 替代 CLINT MSIP，用 IMSIC 的 `seteipnum` 写消息触发 IPI（更现代）
- **`is_aia_active()` / `set_aia_active()`** 全局开关
- **MSI 配置寄存器** 写入（MMSICFGADDR / SMSICFGADDR + LHXW hart-index-bits）

**配套修改：**
- `prototyper/src/sbi/ipi.rs` —— IpiDevice trait 加 IMSIC backend 路径
- `prototyper/src/riscv/csr.rs`（+37 行）—— 新 AIA CSR：`stopei` / `siselect` / `sireg` / `mtopei` / `miselect` / `mireg` 等
- `prototyper/src/sbi/trap/handler.rs`（+125 行）—— mext_handler 加 AIA 外部中断 claim 路径
- `prototyper/src/platform/mod.rs`（+396 行）—— 隔离 M-level 中断控制器（commit "feat(prototyper): isolate M-level interrupt controllers"）

**openEuler AIA CI（新）：**
- `.github/scripts/prototyper-openeuler-aia.sh`（157 行）
- `.github/workflows/openeuler-aia.yml`（89 行）—— 自动测试用 AIA 启动 openEuler RISC-V


#### B. CoVE (Confidential VM Extension) 实现 ⭐

**新增模块：**
- `library/riscv-cove/src/host.rs`（+41 行）—— covh trait/EID 定义（EID `0x434F5648` = "COVH"）
- `library/riscv-cove-rt/src/host.rs`（+257 行）—— 实际 sbi_covh_* 调用包装
- 提交：`feat(cove-rt): add covh sbi calls for fid #0 to #2` + `feat(covg): add TVM zero/shared pages and vCPU management (FID #12-#15)` + `feat(riscv-cove): add COVE host extension functions`

**covh 全 20 个 FID（CoVE Spec Chapter 10）：**
| FID | 函数 |
|-----|------|
| 0 | `get_tsm_info` —— 读 TSM 状态 |
| 1 | `convert_pages` —— 普通内存转机密内存 |
| 2 | `reclaim_pages` —— 回收机密内存 |
| 3 | `global_fence` |
| 4 | `local_fence` |
| 5 | `create_tvm` —— 创建 TEE Virtual Machine |
| 6 | `finalize_tvm` |
| 7 | `destroy_tvm` |
| 8 | `add_tvm_memory_region` |
| 9 | `promote_to_tvm` |
| 10 | `add_tvm_page_table_pages` |
| 11 | `add_tvm_measured_pages` |
| 12 | `add_tvm_zero_pages` |
| 13 | `add_tvm_shared_pages` |
| 14 | `create_tvm_vcpu` |
| 15 | `run_tvm_vcpu` |
| 16 | `tvm_fence` |
| 17 | `tvm_invalidate_pages` |
| 18 | `tvm_validate_pages` |
| 19 | `tvm_remove_pages` |

**CoVE 不是 SBI 3.0 spec 一部分**（独立 RVI spec）—— 但 RustSBI 已实现，OpenSBI 尚未。

#### C. Firmware 模块重构 + 测试 + 修复

- `prototyper/src/firmware/mod.rs`（+355 行）—— payload / jump / dynamic 三种模式 cfg_if 切分；`is_work_hart` 重写；`patch_device_tree` / `set_pmp` / `log_pmp_cfg` 新 export
- `prototyper/src/main.rs`（+30 行）—— 启动主流程整合 AIA + 新 firmware
- `prototyper/src/sbi/trap/boot.rs` / `trap/handler.rs` / `trap/mod.rs` —— trap 处理增强（HSM stopped hart 拒绝、accept pflash firmware next address）
- `prototyper/src/platform/clint.rs` / `console.rs` —— 后端隔离（CLINT 与 AIA 分离）
- `prototyper/src/sbi/pmu.rs` —— PMU event-to-counter typo 修复
- `prototyper/src/sbi/logger.rs` —— firmware log 输出序列化
- `prototyper/test-kernel/src/main.rs`（+102 行）—— 测试 PMU/RFENCE topology-aware
- `library/sbi-rt/src/lib.rs`（+54 行）+ `library/sbi-testing/src/{hsm.rs,log_test.rs}` —— 测试覆盖
- 配置：`prototyper/prototyper/config/default.toml` 加 4 行 AIA 默认值

#### 16 个 commits 速览（按主题）

| Commit | 主题 |
|--------|------|
| 1f58b1b | feat: add AIA IMSIC IPI backend |
| 2ed15a9 | feat: isolate M-level interrupt controllers |
| 5ce4040 | ci: boot openEuler with AIA |
| fbd4295 | fix: guard AIA external interrupt claim |
| 485fd15 | fix: gate QEMU AIA MMIO setup |
| 73205c5 | fix: gate legacy timer probe on backend |
| 361b970 | feat(riscv-cove): add COVE host extension functions (#209) |
| 831e081 | feat(cove-rt): add covh sbi calls for fid #0 to #2 (#203) |
| 12c282c | feat(covg): add TVM zero/shared pages and vCPU management (FID #12-#15) (#205) |
| f7dae46 | fix: reject stopped harts in IPI/RFENCE paths (#208) |
| d044fa3 | fix: accept pflash firmware next address |
| a66c707 | fix: serialize firmware log output |
| 5aca2d9 | fix: correct pmu firmware event typo (#202) |
| 55a0906 | fix: correct typo in PMU event-to-counter mapping helper name (#206) |
| 742a426 | fix(test-kernel): make PMU and RFENCE tests topology-aware |
| 9b13a14 | test: cover RFENCE on started harts |

---

## 4. OpenSBI v1.8 — 生产级全量实现

### 版本声明

```c
// include/sbi/sbi_version.h
#define OPENSBI_SPEC_MAJOR 3
#define OPENSBI_SPEC_MINOR 0
// → 报告 SBI spec v3.0，当前最新
```

### 所有扩展完整清单（15 个命名扩展 + Legacy）

**Legacy (EID 0x0-0x8) — 兼容旧调用**

| EID | 函数 |
|-----|------|
| 0x0 | SET_TIMER |
| 0x1 | CONSOLE_PUTCHAR |
| 0x2 | CONSOLE_GETCHAR |
| 0x3 | CLEAR_IPI |
| 0x4 | SEND_IPI |
| 0x5 | REMOTE_FENCE_I |
| 0x6 | REMOTE_SFENCE_VMA |
| 0x7 | REMOTE_SFENCE_VMA_ASID |
| 0x8 | SHUTDOWN |

**BASE (0x10) — 7 个 FID**（与上表一致，略）

**TIME (0x54494D45) — 1 个 FID**：SET_TIMER

**IPI (0x735049) — 1 个 FID**：SEND_IPI

**RFNC (0x52464E43) — 7 个 FID**（全部实现，含 H-extension 4个）

| FID | 函数 |
|-----|------|
| 0-2 | FENCE_I / SFENCE_VMA / SFENCE_VMA_ASID |
| 3 | REMOTE_HFENCE_GVMA_VMID |
| 4 | REMOTE_HFENCE_GVMA |
| 5 | REMOTE_HFENCE_VVMA_ASID |
| 6 | REMOTE_HFENCE_VVMA |

**HSM (0x48534D) — 4 个 FID**：HART_START / HART_STOP / HART_GET_STATUS / HART_SUSPEND

**SRST (0x53525354) — 1 个 FID**：RESET

**PMU (0x504D55) — 9 个 FID（含 v3.0 新增）**

| FID | 函数 |
|-----|------|
| 0-7 | 同 RustSBI（见上）|
| 8 | EVENT_GET_INFO（v3.0 新增）|

**DBCN (0x4442434E) — 3 个 FID**：WRITE / READ / WRITE_BYTE

**SUSP (0x53555350) — 1 个 FID**：SUSPEND

**CPPC (0x43505043) — 4 个 FID**：PROBE / READ / READ_HI / WRITE

---

**v3.0 新增扩展（OpenSBI 独有，RustSBI 尚未实现）：**

**FWFT (0x46574654) — 2 个 FID**

| FID | 函数 | 功能 |
|-----|------|------|
| 0 | SET | 设置固件特性（如 MISALIGNED_EXC_DELEG）|
| 1 | GET | 读取固件特性当前值 |

**DBTR (0x44425452) — 8 个 FID**

| FID | 函数 |
|-----|------|
| 0 | NUM_TRIGGERS |
| 1 | SETUP_SHMEM |
| 2 | TRIGGER_READ |
| 3 | TRIGGER_INSTALL |
| 4 | TRIGGER_UPDATE |
| 5 | TRIGGER_UNINSTALL |
| 6 | TRIGGER_ENABLE |
| 7 | TRIGGER_DISABLE |

**SSE (0x535345) — 10 个 FID**

| FID | 函数 |
|-----|------|
| 0 | READ_ATTR |
| 1 | WRITE_ATTR |
| 2 | REGISTER |
| 3 | UNREGISTER |
| 4 | ENABLE |
| 5 | DISABLE |
| 6 | COMPLETE |
| 7 | INJECT |
| 8 | HART_UNMASK |
| 9 | HART_MASK |

**MPXY (0x4D505859) — 8 个 FID**

| FID | 函数 |
|-----|------|
| 0 | GET_SHMEM_SIZE |
| 1 | SET_SHMEM |
| 2 | GET_CHANNEL_IDS |
| 3 | READ_ATTRS |
| 4 | WRITE_ATTRS |
| 5 | SEND_MSG_WITH_RESP |
| 6 | SEND_MSG_WITHOUT_RESP |
| 7 | GET_NOTIFICATION_EVENTS |

**Vendor Extension** (0x09000000–0x09FFFFFF)：由平台实现，FID 任意

---

### 4.X OpenSBI 2026-05 大更新（commits `2257e995..f34cf053`）


#### A. Smrnmi (Resumable NMI) 扩展支持 ⭐

RISC-V **Smrnmi 扩展**：Resumable Non-Maskable Interrupt —— 比传统 NMI 强，**handler 可以 mret 返回继续执行**（不像传统 NMI 只能 reset）。

新增内容：
- `firmware/fw_base.S`（156 行修改）—— `_trap_rnmi_handler` 汇编入口，保存 MN* CSR（MNSCRATCH/MNEPC/MNSTATUS/MNCAUSE），mnret 返回
- `lib/sbi/sbi_trap.c`（+39 行）—— `sbi_trap_rnmi_handler()` 派发到平台 ops->rnmi_handler
- `include/sbi/sbi_trap.h`（+2 行）—— RNMI handler 接口
- `include/sbi/sbi_platform.h`（+8 行）—— `struct sbi_platform_operations.rnmi_handler` 回调
- `include/sbi/sbi_scratch.h`（+11 行）—— **新增 tmp1 scratch space 给 RNMI 用**（tmp0 留给普通 trap）
- `include/sbi/riscv_encoding.h`（+10 行）—— 新增 Smrnmi 寄存器位定义
- `lib/sbi/sbi_hart.c`（+37 行）—— `Detect and enable Smrnmi before trap-based feature detection`（提交 2d211fe6）

**为什么早期检测：** 没有 Smrnmi 时，trap 跳到未定义位置可能 reboot；有 Smrnmi 时，`mnret` 提供"安全失败"路径。所以 Smrnmi 检测必须在 trap-based feature probing **之前**。

**Reviewer：** Anup Patel（OpenSBI 维护者）；提交者：Evgeny Voevodin（Tenstorrent）

#### B. K210 平台移除（kendryte/k210）

提交 `f34cf053` 删除：
- `platform/kendryte/k210/Kconfig`（10 行）
- `platform/kendryte/k210/configs/defconfig`
- `platform/kendryte/k210/k210.dts`（70 行）
- `platform/kendryte/k210/objects.mk`（25 行）
- `platform/kendryte/k210/platform.c`（176 行）
- `platform/kendryte/k210/platform.h`（50 行）
- `README.md` -1 行
- `docs/platform/platform.md` -3 行

K210 是 Kendryte 早期 RISC-V SoC（双 RV64IMAFC，2018 嘉楠科技），曾用于 Sipeed Maix 系列开发板。**OpenSBI 主线宣告 K210 deprecated**，社区维护移到 OpenSBI fork 或专用 fork。理由：硬件过老（无 H 扩展、无 AIA、bug 多），主线维护成本高。


#### C. Zkr 熵初始化重构

提交 `0cfd6c0b`：把 `__stack_chk_guard` 的 Zkr 熵初始化从 `fw_base.S` 移到 `lib/sbi/sbi_init.c::init_coldboot()`。

**理由：**
- 旧位置（fw_base.S）需要 trap-based 检测 Zkr 是否存在
- 在 Smrnmi 检测之前用 trap-based 检测 = **崩溃风险**
- 新位置（init_coldboot）在 device tree parse 之后，可以查 DT 知道 Zkr 是否实现，**无需 trap**
- 进一步：init_coldboot 不返回，所以 stack canary 检查时不会用错误的旧值

#### D. 7 个 commits 速览

| Commit | 主题 |
|--------|------|
| f34cf053 | platform: Remove kendryte/k210 platform |
| 2d211fe6 | Detect and enable Smrnmi before trap-based feature detection |
| 0cfd6c0b | Move Zkr entropy initialization from fw_base.S to init_coldboot |
| 882b8b08 | Move device tree features detection before trap-based checks |
| 00fec20b | firmware: Add RNMI handler infrastructure |
| b63606f9 | Add Smrnmi extension macros for registers and bits |
| 5d248a01 | sbi_scratch: Add tmp1 scratch space for RNMI context saving |


---

## 5. 三层对照矩阵（"BBL原形 → 规范抽象 → 工程实现"）

| SBI 功能 | BBL 实现 | SBI 规范 | tg-rcore | RustSBI | OpenSBI |
|---------|---------|---------|---------|---------|---------|
| **定时器** | `mcall_set_timer()` → 写 timecmp，代理 STIP | TIME 0x54494D45 FID0 | ✅ set_timer | ✅ SbiIpi.set_timer | ✅ |
| **控制台写** | `mcall_console_putchar()` → UART/HTIF | DBCN 0x4442434E FID0/2 | ✅ putchar | ✅ SbiConsole.write | ✅ |
| **控制台读** | `mcall_console_getchar()` → 非阻塞 -1 | DBCN FID1 | ✅ getchar | ✅ SbiConsole.read | ✅ |
| **软 IPI** | `send_ipi_many(IPI_SOFT)` | IPI 0x735049 FID0 | ❌ | ✅ SbiIpi.send_ipi | ✅ |
| **远程 fence.i** | `send_ipi_many(IPI_FENCE_I)` | RFNC FID0 | ❌ | ✅ | ✅ |
| **远程 sfence.vma** | `send_ipi_many(IPI_SFENCE_VMA)` | RFNC FID1-2 | ❌ | ✅ | ✅ |
| **hart 管理** | ❌（无 SMP 支持）| HSM 0x48534D FID0-3 | ❌ | ✅ SbiHsm | ✅ |
| **系统复位** | `mcall_shutdown()` → htif/finisher | SRST 0x53525354 FID0 | ✅ shutdown | ✅ SbiReset | ✅ |
| **PMU 计数器** | ❌ | PMU 0x504D55 FID0-8 | ❌ | ✅ SbiPmu | ✅ |
| **系统挂起** | ❌ | SUSP 0x53555350 FID0 | ❌ | ✅ SbiSuspend | ✅ |
| **CPPC 频率** | ❌ | CPPC 0x43505043 FID0-3 | ❌ | ❌（trait有）| ✅ |
| **SSE 事件** | ❌ | SSE 0x535345 FID0-9 | ❌ | ❌ | ✅ |
| **MPXY 消息** | ❌ | MPXY 0x4D505859 FID0-7 | ❌ | ❌ | ✅ |
| **FWFT 特性** | ❌ | FWFT 0x46574654 FID0-1 | ❌ | ❌ | ✅ |
| **DBTR 调试触发** | ❌ | DBTR 0x44425452 FID0-7 | ❌ | ❌ | ✅ |

---

## 6. 数量统计

```
实现       | 扩展数  | FID总数 | 规范版本 | AIA | CoVE | Smrnmi | 代码量
-----------|--------|--------|---------|-----|------|--------|-------
BBL        |  1*    |   9    | pre-spec| ❌  | ❌   | ❌     | ~300行
tg-rcore   |  4     |  ~11   | v1.0声称| ❌  | ❌   | ❌     | ~150行
RustSBI    | 10+3†  |  60+   | v2.0+部分v3.0 | ✅(2026-05) | ✅(2026-05) | ❌ | ~5000行
OpenSBI    | 17+1*  | ~80+   | v3.0    | ✅  | ❌   | ✅(2026-05) | ~60000行

† RustSBI library 还有 CPPC/NACL/STA 3 trait 未在 prototyper 注册
```

- **Legacy 完整支持**（仅 console_putchar 已做）
- PMU / CPPC / NACL / STA（v1.0-rc3 ~ v2.0-rc5 引入）
- SSE / MPXY / DBTR（v3.0 新增）
- **+ 非 SBI 但生产必需：** AIA / CoVE / Smrnmi / HS-mode（H 扩展）

---

## 7. 学习路线建议

按三层对照读每个扩展：

```
1. BBL (mcall.h + mtrap.c)
   → 最简实现，一眼看穿"这个功能是干什么的"

2. SBI 规范 (specs/riscv-sbi-v3.0.pdf)
   → 了解抽象：为什么这样定义参数？为什么分 EID/FID？

3. tg-rcore (msbi.rs)
   → 最小规范实现：看怎么从 BBL 行为映射到规范接口

4. RustSBI (library/src/*.rs + prototyper/src/sbi/*.rs)
   → 工程化：trait 隔离、平台注入、错误返回

5. OpenSBI (lib/sbi/sbi_ecall_*.c)
   → 生产级：完整错误处理、多平台、v3.0 全量
```

---


### 8.1 当前状态（v0.1.1-dev）

✅ 已实现 9 扩展：BASE / TIME / IPI / RFNC / HSM / SRST / DBCN / SUSP / FWFT
✅ Kconfig + plats/ submodule 双轨配置
✅ Boot hart 原子抢占（amoswap.w / compile-time BOOT_HART_ID）
✅ CI（GitHub Actions 4×4 矩阵）
✅ Release 默认 FDT Generic 平台通用二进制

### 8.2 必做剩余清单（"超越 OpenSBI / RustSBI" 路线）


#### a. SBI 3.0 全 17 扩展剩余 8 项

| # | 扩展 | EID | 规范版本 | 优先级建议 | OpenSBI 参考 | RustSBI 参考 |
|---|------|-----|---------|-----------|-------------|-------------|
| 1 | **Legacy 完整 9 函数** | 0x00-0x08 | v0.x | **高**（兼容老 OS）| `lib/sbi/sbi_ecall_legacy.c` | `library/rustsbi/src/legacy_stdio.rs` |
| 2 | **PMU** | 0x504D55 | v1.0-rc3 | **高**（perf 工具基础）| `lib/sbi/sbi_pmu.c` + `sbi_ecall_pmu.c` | `prototyper/src/sbi/pmu.rs` |
| 3 | **CPPC** | 0x43505043 | v2.0-rc2 | 中（DVFS 电源）| `lib/sbi/sbi_ecall_cppc.c` | library trait 已定义 |
| 4 | **NACL** | 0x4E41434C | v2.0-rc3 | **高**（虚拟化加速 + H 扩展配套）| `lib/sbi/sbi_ecall_nacl.c` | library trait |
| 5 | **STA** | 0x535441 | v2.0-rc5 | 中（云 VM）| `lib/sbi/sbi_ecall_sta.c` | library trait |
| 6 | **SSE** | 0x535345 | v3.0-rc2 | 高（v3.0 新特性）| `lib/sbi/sbi_ecall_sse.c` | ❌ 未实现 |
| 7 | **MPXY** | 0x4D505859 | v3.0-rc3 | 中（v3.0 新特性）| `lib/sbi/sbi_ecall_mpxy.c` | ❌ 未实现 |
| 8 | **DBTR** | 0x44425452 | v3.0-rc4 | 中（调试用）| `lib/sbi/sbi_ecall_dbtr.c` | ❌ 未实现 |

#### b. "超越"必需的非 SBI 项

| # | 项目 | 含义 | 优先级 | OpenSBI 参考 | RustSBI 参考 |
|---|------|------|-------|-------------|-------------|
| 9 | **AIA (APLIC + IMSIC)** | 现代中断架构 | **高**（SiFive Pro / 现代 SoC 必需）| `lib/utils/irqchip/{aplic,imsic}.c` | `prototyper/src/platform/aia.rs`（2026-05 新增）|
| 10 | **CoVE (covh + covg)** | 机密计算扩展（独立 RVI spec） | 中-高（差异化）| ❌ 未实现 | `library/riscv-cove/` + `cove-rt/`（2026-05 新增）|
| 11 | **Smrnmi (Resumable NMI)** | 安全可恢复 NMI 处理 | 高（OpenSBI 2026-05 已加） | `firmware/fw_base.S` + `lib/sbi/sbi_trap.c` (2026-05 新) | ❌ 未实现 |
| 12 | **HS-mode (H 扩展)** | RISC-V Hypervisor 扩展 | **高**（虚拟化核心）| `lib/sbi/sbi_hext.c` | 部分 |
| 13 | **多平台 BSP** | 已有 plats/ 框架，需补 SiFive HiFive Pro / SpacemiT K1 / VisionFive2 / Allwinner D1 等 | 持续 | `platform/` 几十家 SoC | `prototyper/` 主线只支持 generic |

#### c. "超越" 工程维度

|------|---------|---------|-----------|
| **SBI spec 版本选择** ⭐ | ❌ 固定最新版（编译时硬编码 v3.0）| ❌ 固定最新版 | ✅ **已实现 21 个版本变体**（Kconfig 选 v0.1 / v0.2 / v0.3-rc1 / v0.3 / v1.0-rc1/rc2/rc3 / v1.0 / v2.0-rc1..rc8 / v2.0 / v3.0-rc1rc2 / v3.0-rc2..rc8 / v3.0）|
| **扩展按 spec 版本自动启用** ⭐ | ❌（全开）| ❌（全开）| ✅ **已实现** —— `src/spec/ext_versions.zig` 每扩展声明引入版本 + `cfg.version.ge(EV.X)` 编译期自动 dispatch |
| 测试矩阵 | 14 distros (RustSBI Prototyper docs) | sbi-testing crate | **目标：完整 OpenSBI Test Suite 通过 + 自家测试** |
| 文档完整性 | 高（英文为主）| 中 | **目标：超越（中文 + 英文 + 设计文档 + 学习材料）** |
| 编译期裁剪 | Kconfig | cargo features | ✅ 已实现 Kconfig + 编译期 ZST + 版本驱动启用 |
| 形式化（差异化）| ❌ | ❌ | **目标：可选 TLA+ 模型**（远期，差异化优势）|
| 性能（IPI 延迟 / fence）| 微秒级 | 微秒级 | **目标：≤ 等于或更优** |
| 工具链 / CI | 标准 | 多 distro | **已有 CI 4×4 矩阵；可加 SBI Test Suite** |



**Kconfig 21 个可选版本（已实现）：**

```kconfig
choice
    prompt "SBI specification version"
    default KUSBI_SPEC_V3_0

    config KUSBI_SPEC_V0_1            bool "v0.1  (Legacy 9 函数)"
    config KUSBI_SPEC_V0_2            bool "v0.2"
    config KUSBI_SPEC_V0_3_RC1        bool "v0.3-rc1  (Legacy)"
    config KUSBI_SPEC_V0_3            bool "v0.3"
    config KUSBI_SPEC_V1_0_RC1        bool "v1.0-rc1  (BASE / TIME / IPI / RFNC / HSM / SRST introduced)"
    config KUSBI_SPEC_V1_0_RC2        bool "v1.0-rc2"
    config KUSBI_SPEC_V1_0_RC3        bool "v1.0-rc3  (PMU introduced)"
    config KUSBI_SPEC_V1_0            bool "v1.0"
    config KUSBI_SPEC_V2_0_RC1        bool "v2.0-rc1  (DBCN / SUSP introduced)"
    config KUSBI_SPEC_V2_0_RC2        bool "v2.0-rc2  (CPPC introduced)"
    config KUSBI_SPEC_V2_0_RC3        bool "v2.0-rc3  (NACL introduced)"
    config KUSBI_SPEC_V2_0_RC4        bool "v2.0-rc4"
    config KUSBI_SPEC_V2_0_RC5        bool "v2.0-rc5  (STA introduced)"
    config KUSBI_SPEC_V2_0_RC6        bool "v2.0-rc6"
    config KUSBI_SPEC_V2_0_RC7        bool "v2.0-rc7"
    config KUSBI_SPEC_V2_0_RC8        bool "v2.0-rc8"
    config KUSBI_SPEC_V2_0            bool "v2.0"
    config KUSBI_SPEC_V3_0_RC2        bool "v3.0-rc2  (SSE introduced; rc1rc2 combined in PDF)"
    config KUSBI_SPEC_V3_0_RC3        bool "v3.0-rc3  (MPXY introduced)"
    config KUSBI_SPEC_V3_0_RC4        bool "v3.0-rc4  (DBTR introduced)"
    config KUSBI_SPEC_V3_0_RC5        bool "v3.0-rc5  (FWFT introduced)"
    config KUSBI_SPEC_V3_0_RC6        bool "v3.0-rc6"
    config KUSBI_SPEC_V3_0_RC7        bool "v3.0-rc7"
    config KUSBI_SPEC_V3_0_RC8        bool "v3.0-rc8"
    config KUSBI_SPEC_V3_0            bool "v3.0"
endchoice
```

**编译期自动扩展启用（`src/spec/ext_versions.zig`）：**

```zig
pub const BASE: SbiVersion = SbiVersion.v1_0_rc1;
pub const TIME: SbiVersion = SbiVersion.v1_0_rc1;
pub const IPI:  SbiVersion = SbiVersion.v1_0_rc1;
pub const RFNC: SbiVersion = SbiVersion.v1_0_rc1;
pub const HSM:  SbiVersion = SbiVersion.v1_0_rc1;
pub const SRST: SbiVersion = SbiVersion.v1_0_rc1;
pub const PMU:  SbiVersion = SbiVersion.v1_0_rc3;
pub const DBCN: SbiVersion = SbiVersion.v2_0_rc1;
pub const SUSP: SbiVersion = SbiVersion.v2_0_rc1;
pub const CPPC: SbiVersion = SbiVersion.v2_0_rc2;
pub const NACL: SbiVersion = SbiVersion.v2_0_rc3;
pub const STA:  SbiVersion = SbiVersion.v2_0_rc5;
pub const SSE:  SbiVersion = SbiVersion.v3_0_rc2;
pub const MPXY: SbiVersion = SbiVersion.v3_0_rc3;
pub const DBTR: SbiVersion = SbiVersion.v3_0_rc4;
pub const FWFT: SbiVersion = SbiVersion.v3_0_rc5;
```

→ `if (cfg.version.ge(EV.DBCN))` 在 comptime 决定 DBCN 模块是否编译进去。

**这种设计的独占价值：**

1. **历史模拟器** —— 配置成 v1.0-rc1，可以测试老 OS（仅 6 主力扩展，无 PMU）能否跑
3. **教学利器** —— 配 v0.1 看 Legacy 时代代码；配 v1.0-rc1 看 EID/FID 体系初版；配 v3.0 看完整体系
4. **学术研究** —— SBI spec 演化研究、扩展引入时序分析的实证工具
5. **嵌入式裁剪极致** —— 老 SoC 只需 v0.1 时代功能 → 编出极小固件（仅 9 个 Legacy 函数，可能 < 5 KB）

→ **OpenSBI 和 RustSBI 都做不到这点**——它们只有"我现在是 v3.0"一种姿态。

**配套：** 
- `src/spec/version.zig`（版本编码 + 比较 + RC 排序）
- `src/spec/ext_versions.zig`（每扩展引入版本声明）
- `Kconfig`（21 个版本选项）
- `cfg.version.ge()` / `eq()` / `lt()`（comptime 比较 API）
- `BASE.GetSpecVersion` 自动返回当前编译选定版本的 wire format


---

## 9. 最小实现门槛：实现了什么，能跑什么

> 数据来源：本地源码扫描（core/tg-rcore、core/DragonOS、core/xv6、core/arceos、rtos/FreeRTOS、rtos/rt-thread）

### 9.1 各系统实际 SBI 依赖（源码扫描结果）

| 系统 | 实际调用的 EID | 关键文件 | 备注 |
|------|-------------|---------|------|
| **tg-rcore ch1-ch2** | Legacy 0x01 (putchar)、0x08/SRST (shutdown) | `lib.rs` | 无定时器，打印 + 关机 |
| **tg-rcore ch3+** | + TIME 0x54494D45 FID0 (set_timer) | `lib.rs` | 时间片调度从 ch3 开始 |
| **xv6-riscv** | 无 SBI 调用 | — | 直接运行在 M-mode 或 QEMU 内置固件，不走 SBI |
| **DragonOS (riscv64)** | Legacy 0x01 (putchar)、TIME FID0、RFNC FID1 (sfence_vma)、SRST FID0 | `driver/sbi.rs` `timer_riscv.rs` `mm/mod.rs` | 启动时 probe 12 个扩展，实际用 4 个 |
| **arceos / StarryOS** | 通过 axhal 抽象，不直接 ecall | `modules/axhal/` | SBI 调用封装在 axhal 平台层 |
| **FreeRTOS (RISC-V)** | 无 SBI 调用 | — | 直接在 M-mode 运行，无需 SBI |
| **rt-thread (RISC-V)** | 无 SBI 调用 | — | 裸机 RTOS，M-mode，无需 SBI |
| **Linux 内核** | BASE + TIME + IPI + RFNC + HSM（标准要求）| `arch/riscv/kernel/sbi.c` | 本地无 Linux 源码，根据规范和已知行为填写 |

---

### 9.2 SBI 实现等级定义

把"实现了哪些扩展"分为 5 个等级，从零往上叠加：

```
Level 0 — 裸启动
  BASE (0x10)
  ↓ 能做什么：SBI 规范可探测，其他什么都不能做

Level 1 — 最小可见
  BASE + TIME + Legacy PUTCHAR（或 DBCN write_byte）
  ↓ 能做什么：有输出、有时钟，单核教学内核能跑

Level 2 — 单核完整
  Level 1 + SRST
  ↓ 能做什么：可以优雅关机/重启，完整单核内核

Level 3 — 多核可用
  Level 2 + IPI + RFNC + HSM
  ↓ 能做什么：多核 Linux / DragonOS 可启动，TLB 一致性有保障

Level 4 — 生产就绪
  Level 3 + PMU + SUSP + FWFT + CPPC
  ↓ 能做什么：性能监控、ACPI 休眠、固件特性、CPU 频率调节

Level 5 — v3.0 全量
  Level 4 + DBTR + SSE + MPXY + NACL + STA
  ↓ 能做什么：调试触发器、安全事件、固件消息通道、虚拟化加速
```

---

### 9.3 实现等级 × 能跑的系统（完整矩阵）

| 能跑的系统 | 最低需要 | 说明 |
|-----------|---------|------|
| **FreeRTOS / rt-thread** | 无需 SBI | 自己跑 M-mode，SBI 对它们无意义 |
| **xv6-riscv** | 无需 SBI（或 QEMU 内置）| 可以直接 `-bios none -kernel xv6.elf` |
| **tg-rcore ch1-ch2** | **Level 1**（BASE + TIME + 控制台）| 只需打印 + 时钟 + 关机 |
| **tg-rcore ch3-ch8** | **Level 2** | ch3 加入时间片，需要 set_timer |
| **DragonOS (单核)** | **Level 2** | 实际用：putchar + set_timer + system_reset |
| **DragonOS (多核 SMP)** | **Level 3** | 多核需要 send_ipi + remote_sfence_vma + hart_start |
| **arceos (单核 unikernel)** | **Level 2** | axhal 封装 SBI，最少用 TIME + 控制台 |
| **arceos (SMP)** | **Level 3** | axhal IPI + RFNC |
| **Linux (单核, -nosmp)** | **Level 2** | BASE + TIME + console；SRST 用于 reboot |
| **Linux (SMP, 多核)** | **Level 3** | 必须有 HSM hart_start 才能唤醒从核 |
| **Linux (完整功能)** | **Level 4** | perf 需要 PMU；ACPI S3 需要 SUSP |
| **Hypervisor 客户机** | **Level 3+** | 宿主 SBI 还需支持 NACL/STA (H-extension) |

---

### 9.4 扩展必要性逐一分析

#### BASE（0x10）— **无条件必须**
SBI 规范唯一"强制"扩展，无需 probe 即可调用。Linux、DragonOS 等所有遵循规范的内核在**启动最早期**就会调 `probe_extension` 探测其他扩展是否存在。没有 BASE，内核无法判断 SBI 实现能力，可能直接 panic。

#### TIME（0x54494D45）— **几乎必须**
所有需要**时间片调度**的系统都依赖它。tg-rcore ch3、DragonOS、Linux 调度器全部依赖 set_timer 触发 STIP。

> 替代方案：用 Legacy 0x0（SBI_SET_TIMER）可以替代 TIME，但 Legacy 是废弃接口，新内核优先探测 TIME，失败才 fallback 到 Legacy。

#### 控制台（Legacy 0x01 / DBCN 0x4442434E）— **调试必须，理论可无**
没有控制台，内核启动报错完全不可见，调试无从下手。实际上所有教学内核、Linux early printk 都强依赖。
- Legacy putchar：最简单，1 个 ecall 出 1 个字节
- DBCN write/write_byte：新规范，支持批量写，效率更高

#### SRST（0x53525354）— **实用必须**
没有 SRST，系统无法优雅关机/重启，只能靠断电或 QEMU `Ctrl+A X`。DragonOS 的 reboot 路径直接调 system_reset。

#### IPI（0x735049）— **多核必须**
S-mode 发跨核软中断必须通过 IPI（M-mode 负责写 CLINT MSIP）。Linux SMP scheduler、TLB shootdown 流程全部依赖。

#### RFNC（0x52464E43）— **多核 + 虚存必须**
修改页表后需要让其他核刷 TLB（sfence.vma 只刷本核）。DragonOS 的 `remote_sfence_vma()` 直接来自此扩展。没有 RFNC，多核系统的 TLB 会不一致，导致随机内存错误。
- FID 0（fence_i）：修改代码段（JIT/动态链接）后必须
- FID 1-2（sfence_vma）：页表修改后必须

#### HSM（0x48534D）— **多核启动必须**
Linux 用 `sbi_hsm_hart_start()` 启动从核。没有 HSM，`-smp 4` 的 Linux 只有核 0 跑起来，其他核永远不会醒。

#### PMU（0x504D55）— **性能分析需要**
Linux `perf` 工具底层调用 PMU。普通内核运行不需要，但 `perf stat`、`perf record` 等需要。

#### SUSP（0x53555350）— **低功耗需要**
ACPI S3（suspend-to-RAM）依赖此扩展。嵌入式场景省电必须。教学内核通常不需要。

#### CPPC（0x43505043）— **CPU 频率调节需要**
操作系统动态调频（cpufreq governor）依赖。DragonOS 的 `probe_extensions` 会探测它，但目前未实际调用。

#### FWFT（0x46574654）— **固件特性协商需要**
Linux 6.9+ 用它启用 `MISALIGNED_EXC_DELEG`（非对齐访问异常委托给 S-mode）。不实现则内核只能靠 M-mode 软件模拟非对齐访问，性能损失约 10~100 倍。

#### NACL / STA / SSE / MPXY / DBTR — **虚拟化 / 高级调试**
普通 OS 不需要。Hypervisor 嵌套场景、JTAG 调试、安全事件驱动等专用场景才用。

---


> 注：原"阶段一/二/三"目标已基本完成。当前 v0.1.1-dev 处于阶段三末端 + 阶段四（"超越"）启动期。

```
阶段一 — Level 2（单核完整） ✅ 完成
  已实现：BASE + TIME + DBCN(write_byte) + SRST

阶段二 — Level 3（多核可用） ✅ 完成
  已实现：IPI + RFNC(fence_i/sfence_vma) + HSM(hart_start/stop/status)

阶段三 — Level 4（生产就绪） ✅ 已实现 SUSP + FWFT，⚠️ 缺 PMU + CPPC
  已实现：SUSP + FWFT
  待补：PMU + CPPC

阶段四 — Level 5（v3.0 全量）⚠️ 待启动
  待实现：SSE + MPXY + DBTR + NACL + STA + Legacy 完整 9 函数

阶段五 — "超越 OpenSBI / RustSBI" ⚠️ 远期
  待实现：AIA + CoVE + Smrnmi + HS-mode + 多平台 BSP
  详见 § 8.2 三类清单
```

**Level 2 已验证**（QEMU virt）：
2. ✅ Linux buildroot 启动到登录提示符
3. ✅ tg-rcore 全部章节启动
4. ✅ `sbi_system_reset(SHUTDOWN, 0)` 退出 QEMU 正常

- 路径 A：先补完 PMU + Legacy（生态兼容性优先）
- 路径 B：先做 AIA（追上 RustSBI 2026-05 + 现代 SoC 必需）
- 路径 C：先做 HS-mode（虚拟化场景）
- 路径 D：按 SBI 3.0 spec 顺序补完 NACL → SSE → MPXY → DBTR → STA → CPPC


---

### 9.6 实现顺序建议（依赖关系图）

```
BASE (无依赖，必须第一个)
  ↓
DBCN write_byte（依赖 UART 初始化）
  ↓
TIME（依赖 CLINT 访问）
  ↓
SRST（依赖 QEMU test 设备 / 平台关机接口）
  ↑↑
  以上四个 = 阶段一完成，可跑单核内核
  ↓
IPI（依赖 CLINT MSIP 寄存器）
  ↓
RFNC fence_i（依赖 IPI 基础设施）
  ↓
RFNC sfence_vma（与 fence_i 共享 IPI 路径）
  ↓
HSM hart_start（依赖 IPI + 原子操作 hart 状态机）
  ↑↑
  以上四个 = 阶段二完成，可跑多核内核
  ↓
PMU → SUSP → FWFT（各自独立，不互相依赖）
  ↑↑
  阶段三，可选功能
```
