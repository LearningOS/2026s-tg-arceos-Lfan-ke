# 03-18 — LinuxBoot 精读：把 Linux 内核当 Bootloader（NERF / u-root / Heads / OCP）

> **核心问题：** 既然 Linux 已经有完整的驱动栈、文件系统、网络协议、密码学库，**为什么还要再写一套 UEFI / U-Boot / GRUB？** 能不能直接用 Linux 内核当 bootloader，省掉 200 万行 EDK2 / 150 万行 U-Boot 的重复造轮？
>
> **一句话答案：** **LinuxBoot = "Linux kernel + initramfs（u-root userspace） 替换 UEFI 的 DXE+BDS 阶段"**。Google / Facebook / 9elements / Equus 等 OCP 服务器厂商主推，思路是用 coreboot 或最小 SEC/PEI 完成裸硬件初始化（DDR / CPU init），然后**直接 jump 到 Linux 内核**而不是 UEFI DXE，再由 Linux + u-root 完成 OS 选择 / kexec 到真正业务 OS。**它不是新发明，是"既然 Linux 都成熟了，何必 boot 时再写一套"的工程务实主义**。
>
> **本笔记定位：** 03 大类 boot 第 16 篇 —— **同基因不同体量**视角，配合 [`03-14 barebox 精读`](03-14-barebox-walkthrough.md)（仿 Linux 风格的小 bootloader）+ [`03-12 EDK2`](03-12-edk2-walkthrough.md) / [`03-13 UEFI 演化`](03-13-uefi-evolution-case-study.md)（被替代对象）+ [`03-01 bootloader 全谱`](03-01-bootloader-responsibilities-survey.md)（6 级链中 LinuxBoot 占第 3-4 级）。

---

## 0. ⭐ 先把概念厘清：LinuxBoot / NERF / u-root / Heads 四个名字的关系

读本笔记前最关键的认知：**这四个名字不是同义词**，搞混会让讨论失焦。

### 0.1 四个名字本质对比

| 名字 | 是什么 | 维护方 | 出现时间 | 与其他三者关系 |
|------|--------|--------|---------|----------------|
| **LinuxBoot** | **联盟项目 / 概念 / 路线图**：用 Linux 内核当 bootloader 替换 UEFI 的 DXE+BDS 阶段 | Linux Foundation 项目（2018 成立） | 2018 | **总称**，包含 NERF、u-root、Heads 等子项目 |
| **NERF** | **Non-Extensible Reduced Firmware** —— Google 内部提出的固件简化方案，是 LinuxBoot 的前身 / 同义词 | Google 工程师 Ron Minnich | 2017 | LinuxBoot 的早期名字，2018 后基本被 LinuxBoot 替代 |
| **u-root** | **Go 写的 initramfs + userspace 工具集**，提供 LinuxBoot 启动后的 boot 决策能力 | u-root.org（社区，Google 主导） | 2015 | LinuxBoot 的"应用层"——LinuxBoot 决定用 Linux 当 bootloader，u-root 负责 Linux 起来后做啥 |
| **Heads** | **注重 anti-evil-maid 攻击防护的 LinuxBoot 发行版** | Trammell Hudson | 2017 | LinuxBoot 的安全特化分支，关注硬件级信任根（TPM / Coreboot measured boot） |

### 0.2 一句话区分

- **LinuxBoot = 路线（用 Linux 当 boot）**
- **NERF = 这条路线在 Google 内部的早期名字**（2017 → 2018 改名 LinuxBoot 后基本不用了）
- **u-root = 在 LinuxBoot 启动的 Linux 上跑的 userspace（Go 工具集）**
- **Heads = 用 LinuxBoot + 硬件信任根做出的安全特化方案**（用户面）

### 0.3 整体栈位置（与 UEFI 对照）

```
传统 UEFI 启动栈：
  ┌─────────────────────────┐
  │ 真实 OS (Linux/Win)     │
  ├─────────────────────────┤
  │ GRUB / systemd-boot     │  ← OS Loader (UEFI 应用)
  ├─────────────────────────┤
  │ UEFI BDS (Boot Device   │  ← 选启动设备 + 跑 boot manager
  │           Selection)    │
  ├─────────────────────────┤
  │ UEFI DXE (Driver        │  ← USB/Net/FS/GPU 驱动 (50+ 种)
  │           Execution)    │
  ├─────────────────────────┤
  │ UEFI PEI (Pre-EFI Init) │  ← 内存初始化 / CPU init
  ├─────────────────────────┤
  │ SEC / FSP (Intel)       │  ← 上电 → cache as RAM
  └─────────────────────────┘

LinuxBoot 启动栈：
  ┌─────────────────────────┐
  │ 真实 OS (Linux/Win)     │
  ├─────────────────────────┤
  │ kexec 跳转              │  ← Linux → real Linux 切换
  ├─────────────────────────┤
  │ u-root (Go initramfs)   │  ← OS 选择 / 网络 boot / TPM 验证
  ├─────────────────────────┤
  │ Linux kernel            │  ← 替换 UEFI DXE+BDS 全部！
  ├─────────────────────────┤
  │ coreboot ramstage       │  ← 等价 UEFI PEI 末段 + 把 Linux 当 payload
  ├─────────────────────────┤
  │ coreboot romstage       │  ← 等价 UEFI PEI（DDR / CPU）
  ├─────────────────────────┤
  │ coreboot bootblock      │  ← 等价 UEFI SEC
  └─────────────────────────┘

  ★ LinuxBoot 替换的是 UEFI 上半部（DXE + BDS + GRUB）三段，
    保留 coreboot 完成 SEC + PEI 等价的硬件初始化（200K 行 vs UEFI 的 200 万行）
```

---

## 1. 阶段 3 — 项目身份

| 项 | 值 |
|----|---|
| **正式名** | LinuxBoot |
| **官方** | https://www.linuxboot.org/ + https://github.com/linuxboot |
| **托管** | Linux Foundation 项目（2018 成立） |
| **倡议者** | Ron Minnich (Google), Trammell Hudson (Two Sigma), David Hendricks (Facebook), Andrea Barberio (Facebook) |
| **代码组成** | u-root (Go) + NERF / Heads / fiano (UEFI 镜像处理工具，Go) + 文档 + 集成脚本 |
| **协议** | 各子项目独立（u-root: BSD-3-Clause；Heads: GPLv2；fiano: BSD-3-Clause） |
| **本仓库路径** | **未克隆**（用户可后续 add 到 `boot/linuxboot/`） |
| **目标平台** | x86_64 服务器（主战场）+ ARM 服务器 + RISC-V 实验性 |
| **核心思想** | "如果 Linux 已经成熟，何必再写一套 boot 时的驱动栈" |

---

## 2. 核心理念：用 Linux 当 Bootloader 到底是什么意思

### 2.1 问题：UEFI 为啥那么大

UEFI 规范（2.10 + PI 1.8）+ 完整 EDK2 实现 ≈ **200 万行 C**。这 200 万行包含：

- **驱动**：USB / NVMe / SATA / NIC（Intel/Realtek/Broadcom 数十款）/ GPU framebuffer / TPM / SMBus / I2C / SPI / GPIO / ...
- **文件系统**：FAT12/16/32（必须）+ NTFS / ext2/3/4 / squashfs（部分实现）
- **网络**：完整 TCP/IP + HTTP + iSCSI + PXE + IPv6 + TLS + DNS + DHCP
- **密码学**：RSA / ECDSA / SHA / AES / TLS 1.2/1.3
- **GUI**：HII（Human Interface Infrastructure）+ 图形菜单
- **Variable**：NVRAM 持久化变量管理
- **SMM**：System Management Mode 黑魔法（独立特权级）
- **Setup utility**：BIOS 设置界面

**问题**：每一项 Linux 都已经实现得**更好、更稳、更新更快**。UEFI 在 boot 阶段重新写一套，意味着：
- 维护成本翻倍（同样的 NVMe 驱动 UEFI 要写一份、Linux 要写一份）
- bug 双倍（UEFI 网络栈 CVE 多得离谱，2014-2024 几十个）
- 攻击面巨大（UEFI runtime services 留在 OS 跑期间，TrustedComputingGroup 一直担忧）

### 2.2 解决：让 Linux 自己当 boot 期的 OS

**核心洞察**：boot 阶段需要的所有能力（识盘 / 选择 OS / 网络拉镜像 / 校验签名 / 显示菜单），Linux 都有。那就**用 Linux 自己当 boot 期 OS**：

1. 上电后由 coreboot（200K 行 C）完成 CPU/DDR 等"必须裸金属做"的初始化
2. coreboot 直接把 **Linux 内核 + initramfs（u-root）** 当 payload 加载
3. Linux 起来后跑 u-root 工具集：
   - 识别所有磁盘（用 Linux NVMe/SATA/USB driver）
   - 解析任意 fs（FAT/ext4/btrfs/zfs/squashfs，全 Linux 原生）
   - 拉网络镜像（dhcp / wget / git pull，用 Linux TCP/IP）
   - 验证签名（openssl，用 Linux 密码库）
   - 决定 boot 哪个真 OS
4. 用 **kexec** 系统调用把 boot 期 Linux 换成业务 Linux

### 2.3 这不是新点子（前史）

| 年份 | 项目 | 意义 |
|------|------|------|
| 1995 | Linux Loader (LILO) | 不算 LinuxBoot，但已经体现"专门为 Linux 写 boot"的思想 |
| 2000s | Klibc + initramfs | initramfs 出现，"Linux 起来后自己挂 rootfs"成为主流 |
| 2002 | **kexec syscall** ⭐ | Linux 加入"内核换内核"的能力 —— **LinuxBoot 的技术基础** |
| 2010 | Petitboot | IBM POWER / OpenPOWER 用 Linux + kexec 替换 firmware boot manager（**LinuxBoot 的概念前身**！） |
| 2015 | u-root | Google 启动，把 initramfs 全用 Go 重写 |
| 2017 | NERF | Google 内部（Ron Minnich）提出 Non-Extensible Reduced Firmware 概念 |
| 2018 | **LinuxBoot 联盟** | Linux Foundation 项目正式成立，整合 u-root + NERF + Heads |
| 2019 | Facebook OCP 部署 | Facebook 在 OCP 服务器（Tioga Pass / Yosemite）大规模上线 LinuxBoot |
| 2020 | Google ChromeOS | 早期 Chromebook 用 LinuxBoot 风格（coreboot + depthcharge，depthcharge 是 minimal Linux-like loader） |
| 2024 | RISC-V Tinaboot 等实验 | RISC-V 上的 LinuxBoot 探索（少量博客） |

---

## 3. LinuxBoot 替换 UEFI 的哪一段（精确划界）

UEFI 5 阶段：**SEC → PEI → DXE → BDS → RT**

| 阶段 | UEFI 做什么 | LinuxBoot 怎么处理 |
|------|------------|-------------------|
| **SEC** (Security) | 上电后第一段，cache-as-RAM 准备 | **保留**（由 coreboot bootblock 替代，几 KB 汇编） |
| **PEI** (Pre-EFI Init) | DDR 训练 / CPU init / 早期 chipset 配置 | **保留**（由 coreboot romstage 替代，依赖 Intel FSP 二进制 blob） |
| **DXE** (Driver Execution) | 加载几十种 UEFI 驱动 + 建 protocol 数据库 | **完全替换** ← Linux kernel 接管 |
| **BDS** (Boot Device Selection) | 选启动设备 + 跑 boot manager + 启动 OS Loader | **完全替换** ← u-root 接管 |
| **RT** (Runtime Services) | OS 跑起来后还能调的 UEFI 服务（Variable / Time / Reset） | **完全去除** ← 直接由 Linux 自己处理（kernel 已有完整等价实现） |

### 3.1 RT 去除带来的安全收益

UEFI Runtime Services 是 **OS 跑期间还有 UEFI 代码运行**——这是巨大的攻击面：
- LogoFAIL (2023): UEFI logo 解析漏洞，因 RT 仍持续运行可被利用
- BlackLotus (2023): UEFI bootkit，利用 RT 持久化
- LoJax (2018): 第一个 UEFI rootkit

**LinuxBoot 完全 kexec 后，旧的 boot 期 Linux 内存被 OS 接管，没有任何"firmware code"再驻留** → 攻击面从 200 万行 UEFI 缩到 0。

---

## 4. u-root 详解（LinuxBoot 的应用层）

### 4.1 u-root 是什么

**u-root** = Go 编写的 initramfs 工具集。把传统 BusyBox + GRUB 的功能用 Go 全部重写，目的：

- **小**：Go 静态编译 + 共享 runtime → 一个二进制内嵌所有命令（类 BusyBox 单 binary 思路），整个 initramfs 几 MB
- **可读 / 易改**：Go 代码 vs C 代码（U-Boot/UEFI），新人 1 天能看懂
- **跨平台**：x86_64 / ARM64 / RISC-V64 一份代码三平台

### 4.2 u-root 模式（uroot 的 5 种 build mode）

| 模式 | 名字 | 用途 |
|------|------|------|
| 1 | `bb` | BusyBox 风：所有命令链接成单 binary，符号链接分发 |
| 2 | `binary` | 每命令单 binary（传统 Unix 风） |
| 3 | `source` | 源码 + on-the-fly 编译（开发期） |
| 4 | `bzImage` | 内嵌进 Linux bzImage（最终交付） |
| 5 | `tamago` | 跑在裸金属上无内核（u-root 的实验分支） |

### 4.3 u-root 内置命令（部分清单）

`init`, `ip`, `dhclient`, `wget`, `httpd`, `ls`, `cat`, `mount`, `umount`, `cpio`, `tar`, `gzip`, `kexec`, `boot`, `pxeboot`, `localboot`, `fdisk`, `losetup`, `dd`, `chroot`, `mkfs`, `mke2fs`, `bootflash`, `tcz`, `rush` (Go 写的 shell), ... **共 ~150 命令**。

### 4.4 u-root 启动流程（典型 boot decision flow）

```
Linux kernel 启动 → 加载 initramfs (u-root) → 跑 /init (u-root 的 init)
   ↓
扫描所有磁盘 (用 Linux block driver)
   ↓
查找 boot config:
   - GRUB grub.cfg？解析
   - syslinux.cfg？解析
   - BLS (Boot Loader Spec)？解析
   - Linux 内核 + initrd 直接发现？
   ↓
（可选）网络 boot：dhclient → wget kernel from HTTP
   ↓
（可选）TPM 验证 / 用户输入密码 / SSH 远程解锁
   ↓
kexec -l <kernel> --initrd=<initrd> --command-line=<cmdline>
   ↓
kexec -e   ← 切换到目标内核，u-root Linux 被替换
```

---

## 5. NERF 详解（LinuxBoot 的早期身份）

**NERF = Non-Extensible Reduced Firmware**（不可扩展的精简固件）

Ron Minnich 2017 在 USENIX 提出的概念：

> **观察**：UEFI "extensible" 是它的卖点（任何 OEM / IHV 都能加 driver），但也是它的攻击面来源（3rd 方 driver 失控）。
> **方案**：构造 "Non-Extensible" 固件——只跑可信路径，没有第三方扩展点 = 没有攻击面。

### 5.1 NERF 4 步去 UEFI 化

NERF 的实施步骤（针对 Intel 服务器）：

1. **保留**：Intel 必须的 FSP（Firmware Support Package）二进制 blob（DDR/CPU 微码相关，Intel 不开源不能去）
2. **删除**：UEFI DXE 阶段所有 driver（用 Linux 自己的 driver 替代）
3. **删除**：UEFI BDS（用 u-root + kexec 替代）
4. **替换**：UEFI shell → Linux shell

### 5.2 NERF vs LinuxBoot 名字演化

- 2017: Ron Minnich 内部 talk + paper 称 NERF
- 2018: Linux Foundation 整合 NERF + u-root + Heads → 起新名 **LinuxBoot**
- 2018+: 业界用 LinuxBoot 这个名字，NERF 慢慢只在历史文献出现

---

## 6. Heads 详解（LinuxBoot 的安全特化）

**Heads** = Trammell Hudson（前 NSA / Two Sigma 安全研究员）2017 启动的 LinuxBoot 安全特化方案。

### 6.1 Heads 解决的问题：Anti-Evil-Maid 攻击

"Evil Maid" 攻击场景：你出差住酒店，笔记本放房间，清洁阿姨被买通 → 物理接触你电脑 → 改 BIOS / 装 bootkit / 替换 GRUB → 你回来开机一切正常但密码已被记录。

### 6.2 Heads 的 4 层防护

| 层 | 技术 | 做什么 |
|------|------|--------|
| 1 | **TPM measured boot** | coreboot + Heads 把每段代码 hash 存进 TPM PCR；任何修改都会让 PCR 值变化 |
| 2 | **TOTP attestation** | TPM PCR 值 + secret → TOTP（同手机 Authenticator 一样的算法）→ 用户开机时手机/Yubikey 显示 6 位数，对得上才 boot |
| 3 | **GPG-signed boot config** | /boot 下 grub.cfg / kernel / initrd 都用 GPG 签名，Heads 用用户 key 验证 |
| 4 | **Smart card auth** | YubiKey / Nitrokey 物理插入才能解 LUKS 加密分区 |

### 6.3 Heads 支持的硬件

主要老 ThinkPad（X230 / T430 / X220）+ Purism Librem + System76 部分机型 + Talos II（POWER9 工作站）。**RISC-V 上还没有 Heads**。

---

## 7. coreboot + LinuxBoot 全栈（端到端拆解）

### 7.1 完整启动链（Intel x86_64 服务器为例）

```
1. 上电 → CPU reset vector → coreboot bootblock (~16 KB 汇编)
   ↓
2. coreboot bootblock 跳转 coreboot romstage
   ↓ 用 Intel FSP-T (TempRAMInit)
3. coreboot romstage：DDR 训练（调 Intel FSP-M），CPU 拓扑发现
   ↓ 用 Intel FSP-M (MemoryInit)
4. coreboot ramstage：跑在 RAM 中，初始化 chipset
   ↓ 用 Intel FSP-S (SiliconInit)
5. coreboot 把 payload 加载到 RAM
   ↓ payload 可以是 SeaBIOS / UEFI (TianoCorePayloadPkg) / GRUB / 或【LinuxBoot 的 Linux kernel + u-root initramfs】
6. payload (LinuxBoot Linux kernel) 起来
   ↓
7. Linux init = u-root /init
   ↓
8. u-root 决策启动哪个 OS：
   - 如果本地有可启动 disk → 解析 grub.cfg / BLS → kexec
   - 如果没有 → DHCP + wget kernel from HTTP → kexec
   - 如果有 TPM 不匹配 → 拒绝启动 + 告警
   ↓
9. kexec → 切换到业务 Linux 内核 → u-root Linux 内存被回收
```

### 7.2 与传统 UEFI 链对比

| 阶段 | UEFI 链 | coreboot+LinuxBoot 链 |
|------|---------|----------------------|
| 1 | UEFI SEC（Intel 闭源 / OEM 闭源） | coreboot bootblock（开源）+ Intel FSP-T blob（闭源，必须） |
| 2 | UEFI PEI（OEM 二改） | coreboot romstage（开源）+ Intel FSP-M blob（闭源，必须） |
| 3 | UEFI DXE（200 万行 C，扩展点失控） | **替换** ← Linux kernel 接管驱动栈 |
| 4 | UEFI BDS（OEM 自定义） | **替换** ← u-root 接管 boot 决策 |
| 5 | GRUB（200K 行 C，UEFI 应用） | **替换** ← u-root 直接 kexec |
| 6 | UEFI RT（OS 跑期间持续驻留） | **去除** ← kexec 后无 firmware code 残留 |

**代码量对比**：
- 传统 UEFI 链：200 万行 EDK2 + 200K GRUB = 220 万行
- coreboot+LinuxBoot：200K coreboot + Linux kernel（你本来就有）+ 50K u-root = **代码量 1/10 + 漏洞面 1/100**

---

## 8. LinuxBoot vs barebox 对比深度（同基因不同体量）

### 8.1 共同基因（哲学）

| 共同点 | 体现 |
|--------|------|
| 拒绝重写 boot 期 OS | "boot 阶段需要的能力 Linux 都有，何必再写" |
| 仿 / 用 Linux API | barebox 仿（DM/DT/cdev/initcall）；LinuxBoot 直接用 |
| 拒绝 UEFI 复杂性 | 都不实现 UEFI Boot/Runtime Services |
| 强调可读性 | barebox 用 Linux kernel coding style；u-root 用 Go（比 C 更易读） |
| 工业实践驱动 | barebox 由 Pengutronix 工业 Linux 公司推动；LinuxBoot 由 Google/Facebook 数据中心推动 |

### 8.2 不同点（实施）

| 维度 | barebox | LinuxBoot |
|------|---------|-----------|
| **体量** | ~1-2 MB（独立 bootloader） | ~10-100 MB（含 Linux 内核） |
| **是不是 Linux** | **不是**——仿 Linux 风格的独立 C 代码 | **就是 Linux**——直接编 Linux 内核当 bootloader |
| **driver 来源** | barebox 自己写（少数从 Linux port） | 直接用 Linux 全部 driver |
| **fs 支持** | 自己写（FAT/ext2/UBIFS/JFFS2，少） | 直接用 Linux（FAT/ext4/btrfs/zfs/squashfs，全） |
| **网络** | 简单 IP4 + TFTP / DHCP | 完整 Linux TCP/IP + IPv6 + TLS |
| **目标场景** | 嵌入式（工业控制 / 汽车 ECU / Phytec / TQ） | 数据中心服务器（Facebook OCP / Google） |
| **DDR 训练等裸金属初始化** | barebox 自己做 | **不做** ← 由 coreboot 或前级做 |
| **userspace 工具** | 无（barebox 自己是 bootloader，无 userspace 概念） | u-root（Go 写 150+ 命令） |
| **替换的链段** | 替换 SPL+U-Boot proper 全段 | 替换 UEFI DXE+BDS（保留 SEC+PEI） |

### 8.3 选哪个？

| 场景 | 选 barebox | 选 LinuxBoot |
|------|-----------|-------------|
| 嵌入式 SoC（256 MB 内存以下） | ✅ | ❌（Linux 内核就 4-8 MB，emb 嵌入式吃不消） |
| 工业控制 / 汽车 / 飞控 | ✅ | ⚠️（Linux 启动 1-2 秒，barebox 100 ms） |
| 数据中心服务器 | ⚠️（barebox 不做 PCIe 服务器特性） | ✅ |
| 安全性优先 | ⚠️ | ✅（Heads 方案 + RT 完全去除） |
| 开发体验 | ✅（C，与 Linux 风格一致） | ✅（Go，更易读） |

---

## 9. RISC-V 上的 LinuxBoot 探索

### 9.1 现状（2026-05）

RISC-V 上的 LinuxBoot **尚未成熟**，有几个实验性项目：

| 项目 | 说明 | 状态 |
|------|------|------|
| **Tinaboot** | 全志（Allwinner）D1 上的 LinuxBoot 探索（小型 Linux + busybox 当 bootloader） | 实验性 |
| **OpenSBI + Linux as payload** | OpenSBI 直接 jump 到 LinuxBoot Linux 而非 U-Boot | 概念验证 |
| **u-root RISC-V port** | u-root 已支持 riscv64 编译 | ✅ 可用，少量测试 |
| **coreboot RISC-V** | coreboot 有 RISC-V 实验代码（HiFive Unmatched 等） | 实验性，活跃度低 |

### 9.2 为什么 RISC-V 上慢

- RISC-V 主流 boot 链是 **OpenSBI + U-Boot SPL/proper** 而非 coreboot
- OpenSBI 已经是"M-mode 极简固件"，比 UEFI 干净，**LinuxBoot 在 RISC-V 上的"减负"价值不如 x86 大**
- RISC-V 服务器市场尚小（2026 才刚起步），数据中心驱动不强烈
- 但 RISC-V Profile 标准化后（RVA22/23/24），UEFI on RISC-V 在服务器上会增加，**届时 LinuxBoot 在 RISC-V 服务器有空间**

### 9.3 RISC-V LinuxBoot 假想链

```
2. KuBoot SPL (DDR 训练 + 加载 Linux + u-root)
3. Linux kernel 起来（boot 期 Linux）
4. u-root 决策（识 disk / 拉网络 / TPM 验证）
5. kexec → 真正业务 Linux
```

**关键**：与 KuEFI 路径的区别在于不需要 KuEFI / KuGrub，全 Linux + u-root 取代之。

---

## 10. 工业应用案例

### 10.1 Facebook OCP（最大规模部署）

- **机型**：Tioga Pass（双路 Skylake/Cooper Lake）+ Yosemite（多节点 Microserver）
- **部署规模**：数十万台 OCP 服务器（2019-至今）
- **替换前**：AMI MegaRAC UEFI BIOS
- **替换后**：coreboot + LinuxBoot
- **收益**：
  - boot 时间从 90 秒 → 30 秒
  - 漏洞响应：新 CVE 出来当天能 patch（自家 Linux），UEFI 时代要等 OEM 几个月
  - 可观测性：boot 期就有完整 Linux 工具链
- **公开演讲**：OCP Summit 2018-2024 多场

### 10.2 Google ChromeOS

- **机型**：Chromebook（多家 ODM 制造，Google 主导固件）
- **路线**：coreboot + depthcharge（depthcharge 不是完整 LinuxBoot，是 minimal Linux-like loader，但同哲学）
- **演化**：2019+ 部分新机型用更接近 LinuxBoot 的方案（coreboot + flashrom verified boot + Chromium OS firmware）

### 10.3 9elements（咨询公司，欧洲 LinuxBoot 主力推手）

- 提供 LinuxBoot 商业咨询和移植服务
- 客户：欧洲数据中心 + 安全敏感行业（金融 / 政府）
- 移植目标：x86 服务器、ARM 服务器

### 10.4 Equus Compute Solutions

- 美国服务器 ODM
- 提供 LinuxBoot 预装的 OEM 服务器

### 10.5 Two Sigma（金融）

- Trammell Hudson 的雇主，Heads 项目的工业用户
- 内部所有员工笔记本用 Heads 解决 evil maid

---




```
   或
```

### 11.2 LinuxBoot 路径假设（如果未来要做）

```
```

**前置条件**：
- 需要 u-root RISC-V binary（已有，可借用）

**潜在收益**：
- 不重写 fs / network / driver（直接用 Linux）
- boot 期可观测性极强（完整 Linux 工具链）
- 安全性高（kexec 后无 firmware 残留）

**潜在代价**：
- boot 时间长（Linux 启动 1-2 秒，比 KuBoot bootcmd 100 ms 慢）
- 镜像大（Linux 内核 8 MB + initramfs 5-10 MB）
- 嵌入式不适用（小 SoC 内存吃不消）



---

## 12. 9 阶段递归大纲（学习路径）

按 [00-01 学习材料索引](00-01-material-index.md) + [00-02 全栈纵向](00-02-fullstack-vertical.md) 的 9 阶段法：

1. **阶段 1**（项目身份）✅ §1
2. **阶段 2**（运行机制）✅ §2-3
3. **阶段 3**（细化）✅ §4-7
4. **阶段 4**（横向对比）✅ §8（vs barebox）
5. **阶段 5**（QuickStart）—— 见 §13.1
6. **阶段 6**（社区 / 生态）✅ §10
7. **阶段 7**（精通深读）—— 进 u-root 源码 / Heads 配置 / fiano 工具
8. **阶段 8**（子功能 / 子模块）—— u-root 的 150 命令逐个 / coreboot payload spec

---

## 13. QuickStart：5 分钟跑通 LinuxBoot（QEMU）

### 13.1 最小实验路径（不需要真硬件）

```bash
# 1. 拉 u-root
go install github.com/u-root/u-root@latest

# 2. 编 Linux kernel（开 CONFIG_KEXEC + CONFIG_INITRAMFS_SOURCE=initramfs.cpio）

# 3. 用 u-root 生成 initramfs
u-root -o /tmp/initramfs.cpio core boot

# 4. QEMU 跑（boot 期 Linux）
qemu-system-x86_64 \
  -kernel bzImage \
  -initrd /tmp/initramfs.cpio \
  -append "console=ttyS0" \
  -nographic

# 5. 进入 u-root shell 后用 kexec 切到真 OS
# /# kexec -l /boot/vmlinuz --initrd=/boot/initrd.img --command-line="..."
# /# kexec -e
```

### 13.2 RISC-V 上跑（实验性）

```bash
# u-root 编 riscv64
GOARCH=riscv64 u-root -o /tmp/initramfs-rv64.cpio core boot

# QEMU virt + OpenSBI + boot 期 Linux + u-root
qemu-system-riscv64 -M virt -bios opensbi.bin \
  -kernel Image-riscv64 \
  -initrd /tmp/initramfs-rv64.cpio \
  -nographic
```

---

## 14. 词典

| 术语 | 全称 / 含义 |
|------|------------|
| **LinuxBoot** | 用 Linux 内核当 bootloader 替换 UEFI DXE+BDS 的路线 |
| **NERF** | Non-Extensible Reduced Firmware，LinuxBoot 早期名字 |
| **u-root** | Go 写的 initramfs + 工具集，LinuxBoot 的应用层 |
| **Heads** | LinuxBoot 安全特化分支，注重 anti-evil-maid |
| **kexec** | Linux syscall，"内核换内核"，LinuxBoot 的技术基础 |
| **coreboot** | 开源 firmware，做 SEC+PEI 等价的硬件初始化 |
| **FSP** | Firmware Support Package，Intel 闭源二进制 blob，做 DDR/CPU init |
| **Petitboot** | IBM POWER 上 Linux+kexec 替换 firmware 的早期实践 |
| **TOTP** | Time-based One-Time Password，Heads 用作 attestation |
| **Tioga Pass** | Facebook OCP 服务器机型，LinuxBoot 主力部署机型 |
| **fiano** | LinuxBoot 项目下 Go 写的 UEFI 镜像处理工具 |
| **measured boot** | 每段代码 hash 入 TPM PCR，可验证完整性 |

---

## 15. 进一步阅读

- **官方**：https://www.linuxboot.org/
- **u-root**：https://github.com/u-root/u-root
- **NERF 起源 paper**：Ron Minnich, "NERF: A Method for Reduced Firmware" (USENIX 2017)
- **Heads**：https://osresearch.net/
- **Trammell Hudson 博客**：https://trmm.net/Heads
- **本仓库相关**：[03-14 barebox 精读](03-14-barebox-walkthrough.md)（同基因不同体量对照）+ [03-12 EDK2](03-12-edk2-walkthrough.md)（被替代对象）+ [03-13 UEFI 演化](03-13-uefi-evolution-case-study.md)（UEFI 标准 vs 实现）+ [03-01 bootloader 全谱](03-01-bootloader-responsibilities-survey.md)（6 级链中位置）

---

## 16. 4 道练习题

1. **概念辨析**：LinuxBoot / NERF / u-root / Heads 四个词的关系是什么？分别在哪个层次解决什么问题？
2. **替换边界**：UEFI 5 阶段（SEC/PEI/DXE/BDS/RT）中，LinuxBoot 替换了哪几个？保留了哪几个？为什么不替换 SEC+PEI？
3. **vs barebox**：barebox 也是"仿 Linux 风格的 bootloader"，与 LinuxBoot 相比，两者哲学上的相同与不同点各是什么？为什么嵌入式选 barebox 而服务器选 LinuxBoot？
