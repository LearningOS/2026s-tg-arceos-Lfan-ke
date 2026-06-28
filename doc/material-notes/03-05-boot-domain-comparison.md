# 03-05 — boot 领域横向深对比 + 补充项目大全 + 单项精读路线

> **核心问题：** 现有 03-02 已经讲过 6 个 boot 项目的"基础定位 + 三大流派"，03-06 是 U-Boot 详解。本笔记不重复以上内容，专做两件事：① **6 项目设计取舍 / 协议 / 可裁剪性的深度横向对比矩阵**（03-02 没有）② **补充本仓库以外的相关项目**（coreboot / LinuxBoot / TF-A / Slim Bootloader / sd-boot / Limine 等）—— 让你看清整个 boot 生态全貌，不只是本仓库 6 项。
>
> **一句话答案：** 6 项目按"运行时机 + 特权级"分 4 类（Bootloader / UEFI 实现 / Boot Manager / TEE），加上未在本仓库的 ~10 个相关项目，构成完整 boot 生态。学习顺序：u-boot（已有 03-06）→ edk2 → barebox → rboot/grub2/optee_os 平行。

按 [user_learning_style](../CLAUDE.md) 9 阶段递归大纲：本笔记是**阶段 2（领域深度横向对比）+ 阶段 3 引导**。其他阶段分布：
- **阶段 1 大纲** → [03-02-boot-overview](03-02-boot-overview.md)
- **阶段 3-9（u-boot 精读）** → [03-06-u-boot-overview](03-06-u-boot-overview.md)
- **阶段 3-9（barebox/edk2/rboot/grub2/optee_os 精读）** → 03-14 ~ 03-17（待写）

---

## 0. 与现有 boot 笔记的分工

避免重复内容。各笔记定位：

| 笔记 | 内容 | 不在本笔记的内容 |
|------|------|----------------|
| [03-03-fdt-dts-boot-flow](03-03-fdt-dts-boot-flow.md) | DTB / FDT 在启动链各阶段如何传递、谁修改、谁消费 | 设备树语法 |
| [03-02-boot-overview](03-02-boot-overview.md) | 五阶段全景 + 三大流派（BIOS/UEFI/SBI/TrustZone）+ 6 项目基础定位 + boot 名词词典 | 横向对比深表 / 项目内部细节 |
| [03-06-u-boot-overview](03-06-u-boot-overview.md) | U-Boot 单项 9 阶段精读（SPL / proper / DM / cmd / env / FIT / Bootflow） | 其他 5 项目 |
| [03-05-boot-domain-comparison](03-05-boot-domain-comparison.md)（**本笔记**）| 6 项目深度横向矩阵 + 补充非本仓库项目 + 单项精读路线规划 | 单项内部细节 |
| [00-02 § 6](00-02-fullstack-vertical.md) | 跨架构启动全貌（QEMU / 真机 / x86 / ARM / LoongArch）| boot 项目细节 |
| [00-19](00-19-image-and-bootflow-quickstart.md) | 镜像制作 + RustSBI 14 distros 实战矩阵 + U-Boot vs EDK2 路径 | boot 项目源码内部 |

---

## 1. 6 项目深度横向对比矩阵

### 1.1 设计哲学

| 维度 | u-boot | barebox | edk2 | rboot | grub2 | optee_os |
|------|--------|---------|------|-------|-------|----------|
| **起源年** | 2000（DENX，Wolfgang Denk）| 2007 fork from u-boot（Sascha Hauer）| 2004（Intel EFI 1.0 → 2008 TianoCore 开源）| 2018（rcore-os 项目）| 1995 GRUB 0.x → 2005 GRUB 2 重写 | 2014（Linaro / ARM）|
| **语言** | C（部分汇编）| C（更现代风格）| C + 大量 .dec/.dsc/.inf 元数据 | Rust | C | C |
| **构建系统** | Kconfig + Make | Kconfig + Make | EDK Build (Python + DSC/FDF) | Cargo | autoconf + Make | Make + Python |
| **核心抽象** | Driver Model (DM, 2014+) | DM 重写（POSIX 风）| Protocol / Handle 数据库 | UEFI Application | Module / loader | Pseudo-TA / Driver |
| **代码规模** | ~70 万行 | ~10 万行 | ~200 万行 | ~几百行 | ~30 万行 | ~5 万行 |
| **内存占用** | SPL 几十 KB / proper 数百 KB | 类似 u-boot 但稍大 | 数 MB（PFLASH 通常 32M）| 几百 KB | 几 MB | 几百 KB（secure RAM）|

### 1.2 适用架构覆盖度

| 架构 | u-boot | barebox | edk2 | rboot | grub2 | optee_os |
|------|--------|---------|------|-------|-------|----------|
| x86_64 | 部分（不主推）| ❌ | ✅ 主战场 | ✅ | ✅ 主战场 | ❌ |
| ARM (32) | ✅ | ✅ 主战场 | ✅ | ❌ | ✅ | ✅ |
| AArch64 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ 主战场 |
| RISC-V | ✅ 主流 | ✅ | ✅ (RiscVVirt 包) | ✅ (实验) | ✅ | ✅ (qemu_virt 实验) |
| LoongArch | 部分 | ❌ | ❌ | ❌ | ✅ | ❌ |
| MIPS | ✅ | ✅ | ❌ | ❌ | ✅ (老) | ❌ |
| PowerPC | ✅ | ✅ | ❌ | ❌ | ✅ (老 IBM) | ❌ |

### 1.3 启动协议 / OS 加载方式

| 协议 | u-boot | barebox | edk2 | rboot | grub2 |
|------|--------|---------|------|-------|-------|
| **booti (raw Image)** | ✅ | ✅ | — | — | ❌ |
| **bootm (legacy uImage)** | ✅ | ✅ | — | — | ❌ |
| **bootefi (EFI Stub)** | ✅ | ❌ | ✅ 原生 | ✅ 原生 | ✅ chainload |
| **multiboot / multiboot2** | ❌ | ❌ | ❌ | ❌ | ✅ 原生 |
| **FIT (.itb)** | ✅ 原生 | ✅ | ❌ | ❌ | ❌ |
| **PXE / netboot** | ✅ | ✅ | ✅ | ❌ | ✅ |
| **chainload Win/BSD** | ❌ | ❌ | UEFI 自动 | UEFI 自动 | ✅ |
| **Linux EFI Stub 直接** | bootefi | — | UEFI 自动 | ✅ | linux/initrd |
| **TFTP / NFS root** | ✅ | ✅ | 部分 | ❌ | ✅ |

### 1.4 移植新板的工作量

| 项目 | 新板需要做什么 | 难度 |
|------|--------------|------|
| **u-boot** | `board/<vendor>/<board>/`（C 文件）+ `arch/.../dts/<board>.dts` + `configs/<board>_defconfig` + Kconfig 选项 | ⭐⭐⭐ |
| **barebox** | `arch/<arch>/boards/<board>/` + dts + defconfig | ⭐⭐ |
| **edk2** | 新 Platform Pkg：DSC（描述）+ FDF（固件布局）+ INF（模块）+ DXE drivers + 平台库 | ⭐⭐⭐⭐⭐ |
| **rboot** | 改 `rboot.conf` + cargo build；底层 UEFI 由 OVMF/EDK2 提供 | ⭐ |
| **grub2** | 通常**不改 GRUB**，只配置菜单 + initrd（GRUB 跑在 EDK2/BIOS 之上）| ⭐ |
| **optee_os** | `core/arch/<arch>/plat-<plat>/`：板级时钟 / 中断 / RPC / 内存映射 | ⭐⭐⭐⭐ |

### 1.5 与 SBI / TF-A 的协作模式

```
RISC-V 路径（u-boot 主导）：
  ↑                              ↑                ↑
  ZSBL                        M-mode 固件      S-mode 接管

ARM 标准路径（TF-A + 多种 BL33）：
  BootROM → TF-A BL1 → TF-A BL2 → TF-A BL31 (EL3) → BL32 OP-TEE (secure-EL1, 可选) → BL33 (U-Boot/EDK2/Slim Bootloader) → kernel
                                  ↑                  ↑                                ↑
                              EL3 monitor       secure world OS                  normal world bootloader

x86 服务器路径（EDK2 主导）：
  CPU Reset → SEC → PEI → DXE (EDK2 核心) → BDS → GRUB → kernel
              ↑     ↑       ↑                ↑
            缓存→RAM RAM训练 通用驱动  Boot Device Selection

x86 桌面 PC（OEM UEFI + GRUB）：
  OEM UEFI 固件（基于 EDK2）→ shim (Microsoft 签名) → GRUB → Linux

ChromeBook（coreboot 路径）：
  coreboot → depthcharge / U-Boot payload → kernel
```


|---------------|--------|---------|
| **KuBoot SPL** | u-boot SPL（DDR 训练 + FIT 加载）| edk2 PEI（太复杂） |
| **KuBoot proper** | barebox（更现代 C 风格）| u-boot proper（历史包袱） |
| **KuUEFI** | edk2 + rboot（Rust 简洁实现思路） | 完全重新设计（兼容生态太重） |

---

## 2. 补充项目大全（**非本仓库**，但同生态位）

> 本节弥补 6 项目的视野盲区。这些项目本仓库未 clone，但学习时必须知道。

### 2.1 BIOS / 固件类

| 项目 | 类别 | Git | 一句话 |
|------|------|-----|-------|
| **coreboot** | 开源 BIOS 替代 | https://review.coreboot.org/coreboot.git | Chromebook 标配；Google 服务器主推；EDK2 的 BIOS 派对手 |
| **Slim Bootloader (SBL)** | Intel 开源 SoC bootloader | https://github.com/slimbootloader/slimbootloader | EDK2 的轻量 Intel 替代 |
| **LinuxBoot** | Linux 内核取代 UEFI DXE | https://github.com/linuxboot/linuxboot | 服务器 boot 大幅简化；Facebook/Google 推 |
| **Heads** | 安全 boot 固件（基于 coreboot+Linux）| https://github.com/osresearch/heads | NitroPad / Librem 用 |
| **NERF (Non-Extensible Reduced Firmware)** | LinuxBoot 早期形态 | (LinuxBoot 前身) | 保留 PEI、删 DXE 用 Linux |

### 2.2 ARM / RISC-V Secure 固件类

| 项目 | 类别 | Git | 一句话 |
|------|------|-----|-------|
| **TF-A (Trusted Firmware-A)** | ARM EL3 标准固件 | https://git.trustedfirmware.org/TF-A/trusted-firmware-a.git | 几乎所有 ARM 服务器 / 手机用 |
| **TF-M (Trusted Firmware-M)** | ARM Cortex-M secure 固件 | https://git.trustedfirmware.org/TF-M/trusted-firmware-m.git | TrustZone-M MCU 安全 |
| **Hafnium** | ARM SPM Reference | https://git.trustedfirmware.org/hafnium/hafnium.git | 多 SP 隔离层 (FF-A) |

### 2.3 UEFI Boot Manager / Loader 类

| 项目 | 类别 | Git | 一句话 |
|------|------|-----|-------|
| **systemd-boot (sd-boot)** | 极简 UEFI loader | systemd 子项目 | GRUB 的轻量替代；Pop!_OS / Arch 偏好 |
| **rEFInd** | 美观 UEFI loader | https://github.com/rEFInd/rEFInd | 双系统首选 |
| **Clover** | UEFI bootloader (黑苹果用) | https://github.com/CloverHackyColor/CloverBootloader | macOS 上 PC 用 |
| **OpenCore** | 更现代黑苹果 loader | https://github.com/acidanthera/OpenCorePkg | Clover 接班 |
| **Limine** | 现代 multi-arch boot | https://github.com/limine-bootloader/limine | x86/aarch64/RISC-V/LoongArch 教学 OS 偏好 |
| **iPXE** | 网络启动增强 | https://github.com/ipxe/ipxe | PXE 强化 + HTTP/HTTPS/iSCSI |
| **GNU GRUB 1 (Legacy)** | 老 GRUB | (已废弃) | Multiboot 1 协议起源 |

### 2.4 Linux-as-Bootloader 类

| 项目 | 类别 | Git | 一句话 |
|------|------|-----|-------|
| **Petitboot** | POWER / OpenPOWER 上 Linux 引导 | https://github.com/open-power/petitboot | LinuxBoot 思想前身（IBM）|
| **u-boot bootefi linux** | u-boot 直接当 EFI loader | u-boot 部分 | 把内核当 EFI 应用 |
| **kexec** | Linux 内启动新 Linux | Linux 内核功能 | 跳过固件二次启动 |
| **kdump** | 崩溃时 kexec 到调试内核 | Linux 内核功能 | 服务器调试 |

### 2.5 嵌入式特殊类

| 项目 | 类别 | Git | 一句话 |
|------|------|-----|-------|
| **lk (Little Kernel)** | Android boot loader | https://github.com/littlekernel/lk | 高通 Android 设备 |
| **abootimg** | Android boot image 工具 | (utility) | 解析/打包 boot.img |
| **MX-Boot** | NXP MCU 引导 | NXP 官方 | i.MX 系列 |
| **STM32CubeProgrammer / DFU** | ST 烧写器 | ST 官方 | STM32 USB DFU |
| **ESP-IDF Bootloader** | ESP32 引导 | https://github.com/espressif/esp-idf | RISC-V/Xtensa MCU |
| **MaskROM (Allwinner/Rockchip)** | SoC 内置 boot | (闭源 ROM) | 嵌入式 SoC 首阶段 |
| **HSS (Microchip PolarFire)** | RISC-V Hart Software Services | https://github.com/polarfire-soc/hart-software-services | PolarFire SoC 上的 SPL/SBI 前置 |

### 2.6 Hypervisor 引导类

| 项目 | 类别 | Git | 一句话 |
|------|------|-----|-------|
| **Cloud Hypervisor Firmware** | Rust UEFI 子集 | https://github.com/cloud-hypervisor/rust-hypervisor-firmware | Cloud-Hypervisor 用，加载 Linux |
| **Bao Boot** | Bao hypervisor 配套 | (Bao 项目) | 静态分区 hypervisor 启动 |
| **Xen** | Type-1 hypervisor 自带 boot | https://xenbits.xen.org/git-http/xen.git | OS 层启动协议 |

### 2.7 国产 / 商业固件

| 项目 | 国 / 厂 | 备注 |
|------|--------|------|
| **LoongBoot** | 龙芯 | EDK2 fork，LoongArch 专用 UEFI 实现；https://github.com/loongson/edk2 |
| **kboot (LoongArch)** | 龙芯 | LoongArch 平台 GRUB 替代品；与 GRUB 同生态位但更简洁；https://github.com/loongson/grub 中含 LoongArch 支持，部分国产 distro 用 kboot 名 |
| **Bianbu boot** | 平头哥 | RISC-V SoC 配套 |
| **昇腾 BMC + UEFI** | 华为 | 鲲鹏 920 服务器 |
| **InsydeH2O / AMI Aptio / Phoenix SecureCore** | 商业 UEFI | OEM PC/服务器主流 |
| **MegaRAC SP-X** | AMI BMC | 服务器管理（与本节互补，BMC 不是 boot 但常并提）|

> **LoongArch 启动链速记：** `LoongBoot (EDK2 fork) → GRUB or kboot → Linux PLV0`
> 与 RISC-V 链对比：`U-Boot SPL → OpenSBI → U-Boot proper → Linux S-mode`。LoongArch 多了 EDK2/UEFI 抽象层（更接近 x86 PC），但少了"runtime SBI 监视器"概念（电源管理通过 ACPI 表 + hvcl）。详见 [00-02 § Layer 5](00-02-fullstack-vertical.md) LoongArch 对比。

---

## 3. 单项精读路线（阶段 3-9）

按 9 阶段递归大纲，6 个项目分两批展开：

### 3.1 已完成（u-boot）
- [03-06-u-boot-overview.md](03-06-u-boot-overview.md) — 覆盖 SPL / proper / DM / cmd / env / dts / FIT / Bootflow / BSP / 用户自定义

### 3.2 待写（按优先级）

| 优先级 | 笔记 | 重点（阶段 3-5） |
|--------|------|------------------|
| ⭐⭐⭐⭐⭐ | **03-12-edk2-walkthrough** | UEFI 阶段（SEC/PEI/DXE/BDS）+ DSC/FDF/INF + Protocol/Handle 数据库 + RiscVVirt 实战 |
| ⭐⭐⭐⭐ | **03-17-optee-walkthrough** | TA/REE/TEE 接口 + RPC + ARM TrustZone + RISC-V port |
| ⭐⭐⭐ | **03-15-grub2-walkthrough** | grub.cfg 语法 + multiboot2 + 模块系统 + 内嵌 Lua/JS |
| ⭐⭐ | **03-14-barebox-walkthrough** | 与 u-boot 异同 + 现代 C 风格 + console.io |
| ⭐⭐ | **03-16-rboot-walkthrough** | Rust UEFI 应用结构 + uefi-rs 库 + 加载内核流程（短小可全读）|

### 3.3 后续阶段 6-9 全图

每篇精读笔记后续可补：
- **阶段 6 全 API / 功能 / 选项 / 场景大纲**（项目 cmd / config / 全部驱动 / 全部协议）
- **阶段 7 精通**：边角 case + 隐藏选项 + 性能 trick + 安全 trick
- **阶段 8 子功能细化**：每个 cmd / 每个驱动 / 每个 board / 每个协议单独拆讲
- **阶段 9 设计与实现**：源码读路径 + 关键数据结构 + 关键算法 + 设计权衡

---

## 4. 名词词典补充（与 03-02 § 9 互补）

| 术语 | 含义 | 主要出现 |
|------|------|--------|
| **PBL (Pre-Bootloader)** | barebox 第一阶段（等价 u-boot SPL）| barebox |
| **DSC / FDF / INF / DEC** | EDK2 平台描述 / 固件布局 / 模块声明 / 包声明 | edk2 |
| **MdePkg / MdeModulePkg / OvmfPkg** | EDK2 核心模块包 / 通用驱动包 / OVMF 虚拟化 | edk2 |
| **Protocol / Handle / GUID** | UEFI 服务发现机制 | edk2 / rboot |
| **PEI / DXE / BDS / TSL / RT** | UEFI 阶段（Pre-EFI / Driver Execution / Boot Device Selection / Transient System Load / Runtime）| edk2 |
| **multiboot / multiboot2** | GRUB 加载协议 | grub2 / Limine |
| **chainload** | 把控制权交给另一 bootloader | grub2 / shim |
| **shim** | Microsoft 签名的小 EFI（用于 Secure Boot 链）| 桌面 Linux |
| **TA (Trusted Application)** | OP-TEE 中的安全应用 | optee_os |
| **REE / TEE** | Rich/Trusted Execution Environment | optee_os |
| **RPC (in OP-TEE)** | secure → normal world 调用 | optee_os |
| **Pseudo-TA** | 内置在 OP-TEE core 的 TA | optee_os |
| **fastboot** | Android bootloader 协议 | u-boot 实现 / lk |
| **bootflow** | U-Boot 现代统一启动框架（2022+）| u-boot |
| **HSS (Hart Software Services)** | Microchip PolarFire SoC 第一阶段 | hss |
| **MaskROM** | SoC 内不可改 boot 代码 | Allwinner / Rockchip |
| **PFLASH** | UEFI 用持久 Flash（QEMU 中是 .fd 文件）| edk2 / OVMF |
| **OVMF** | Open Virtual Machine Firmware（EDK2 的 QEMU 子项目）| edk2 |
| **CSM** | UEFI 兼容 BIOS 模块（已淘汰）| edk2 |
| **SPM (Secure Partition Manager)** | ARM FF-A 多 SP 隔离 | Hafnium |
| **CCSDS** | 空间数据系统协议（航天 boot/通信）| 航天领域 |

---

## 5. 进一步阅读

### 5.1 本仓库笔记串联

- **横向相关**：[03-03](03-03-fdt-dts-boot-flow.md) DTB 传递 / [03-02](03-02-boot-overview.md) 全景 + 名词 / [03-06](03-06-u-boot-overview.md) U-Boot 详解
- **跨领域**：[00-02 § 6](00-02-fullstack-vertical.md) 跨架构启动 / [00-19](00-19-image-and-bootflow-quickstart.md) 镜像+启动实战 / [02-* SBI 系列](02-01-boot-chain-and-sbi.md) RISC-V 监视器
- **学习材料地图**：[00-01 § boot/](00-01-material-index.md) 本仓库 boot 项目所有 git 地址

### 5.2 上游官方（本仓库未 clone 的项目）

- coreboot: https://review.coreboot.org/coreboot.git
- TF-A: https://git.trustedfirmware.org/TF-A/trusted-firmware-a.git
- LinuxBoot: https://github.com/linuxboot/linuxboot
- systemd-boot: https://github.com/systemd/systemd
- Limine: https://github.com/limine-bootloader/limine
- Slim Bootloader: https://github.com/slimbootloader/slimbootloader

### 5.3 学习路径建议

```
1. ✅ 阶段 1-2（领域大纲 + 横向对比）→ 已完成（本笔记 + 03-02）
2. ✅ u-boot 阶段 3-9 → 已完成（03-06）
3. 🚧 edk2 阶段 3-5 → 03-12 待写（重点）
4. 🚧 optee_os 阶段 3-5 → 03-17 待写
5. 🚧 grub2 阶段 3-5 → 03-15 待写
6. 🚧 barebox/rboot 阶段 3-5 → 03-14/07 待写
7. 📋 阶段 6-9 → 后续单项深耕
8. 📋 KuBoot 设计 → 等所有精读完成
```
