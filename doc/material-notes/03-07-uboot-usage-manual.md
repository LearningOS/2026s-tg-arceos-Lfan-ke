# 03-07 — U-Boot 使用手册精读：从命令行到 FIT 镜像到完整启动流程

> **核心问题：** 看完 03-06 / 03-10 / 03-11 / 03-09 知道 U-Boot 是什么、怎么编、源码长啥样，但**怎么"用"它**？怎么从串口里敲出第一行命令、怎么写 boot script、怎么持久化环境变量、怎么打 FIT 镜像让 SPL 加载、怎么从 USB / 网络 / NVMe 启动？本笔记全程精读 U-Boot 官方 `doc/usage/` 手册（157 文件），按主题串成完整使用流程。
>
> **一句话答案：** U-Boot 的"使用"是 **3 个层次**：① 串口 shell 里**敲命令**（150+ cmd，如 `tftp` / `bootm` / `mmc` / `usb start`）② 写 **环境变量脚本**（`bootcmd` / `bootargs`）让 U-Boot 自动启动 OS ③ 配 **FIT 镜像** 把 kernel + DTB + initramfs + signature 打成一个 `.itb` 文件让 SPL 直接加载。再加 SPL 的 30+ 种存储 backend（MMC/NVMe/SATA/USB/NAND/NOR/SPI/Ethernet/UART/RAM/XIP/Semihosting/...），覆盖 99% 的板子启动场景。
>
> **本笔记定位：** 03 大类 boot 第 17 篇 —— **使用视角**。配 [03-06 U-Boot 概览](03-06-u-boot-overview.md)（项目身份）+ [03-10 SPL 源码](03-10-u-boot-spl-source-walkthrough.md) / [03-11 proper 源码](03-11-u-boot-proper-source-walkthrough.md)（实现细节）+ [03-08 U-Boot 开发手册精读](03-08-uboot-develop-manual.md)（开发视角）。

---

## 0. 学完本笔记你能做什么

| 能力 | 例子 |
|------|------|
| 在串口看到 U-Boot 提示符 → 不慌 | `=>` / `U-Boot> ` 你认得 |
| 看任何 U-Boot script → 读懂 | `bootcmd=run distro_bootcmd; tftpboot $kernel_addr_r` 你会拆 |
| 改 boot 行为 → 不重编 | `setenv bootcmd 'run my_boot' && saveenv` |
| 从任何 backend 启动板子 | MMC / NVMe / USB / Ethernet / UART / NAND 全部会 |
| 打 FIT 镜像 | `mkimage -f kernel.its kernel.itb`，含签名 |
| 调试启动失败 | 知道 `bdinfo` / `printenv` / `md` / `mw` / `bootefi bootmgr` 怎么用 |
| 后续读 KuBoot checklist 时知道要做什么 | §12 完整列出 |

---

## 1. U-Boot 命令行基础

### 1.1 进入 U-Boot shell

板子上电后串口（默认 115200 8N1）打印：
```
U-Boot 2024.10 (Oct 15 2024 - 12:34:56 +0000)

CPU:   ARM/RISC-V/...
Model: <board name>
DRAM:  256 MiB
...
Hit any key to stop autoboot:  3   ← 倒计时 3 秒
```

按任意键打断 → 进入交互 shell：
```
=>
```

### 1.2 通用命令规则（doc/usage/cmdline.rst）

- **大小写不敏感**（`PRINTENV` = `printenv`）
- **缩写支持**（`pri` = `printenv`，前缀唯一即可）
- **历史导航**：上下方向键（如启用 readline）
- **Tab 补全**（如启用 `CONFIG_AUTO_COMPLETE`）
- **`?` / `help` 显示所有命令**；`help <cmd>` 显示某命令详情
- **`;` 命令分隔符**：`mmc rescan; mmc list`
- **`&&` / `||`** 条件执行（如启用 hush parser）
- **变量替换**：`${var}` 或 `$var`
- **算术**：`setexpr` 命令做加减乘除位移
- **退出**：板子重启 / `reset` / `bootm` 后不返回

### 1.3 命令分类（150+ 个，doc/usage/cmd/ 119 个详细文档）

| 类别 | 代表命令 |
|------|---------|
| **环境** | `printenv` / `setenv` / `saveenv` / `env import` / `env export` / `editenv` |
| **内存** | `md` (memory display) / `mw` (memory write) / `mm` (memory modify) / `cp` (copy) / `cmp` |
| **存储 - 块** | `mmc` / `usb` / `scsi` / `nvme` / `sata` / `pci` |
| **存储 - 闪存** | `nand` / `nor` / `sf` (SPI flash) / `mtd` |
| **文件系统** | `ls` / `load` / `save` / `fatls` / `ext4ls` / `fsload` / `fstype` |
| **分区** | `part` (list/print/info/start/size) |
| **网络** | `dhcp` / `tftpboot` / `ping` / `nfs` / `wget` / `httpd` / `bootp` / `rarpboot` |
| **boot** | `bootm` / `boot` / `bootefi` / `bootz` (Linux ARM zImage) / `booti` (ARM64 Image) / `bootflow` |
| **设备树** | `fdt addr` / `fdt print` / `fdt set` / `fdt rm` / `fdt resize` / `fdt apply` |
| **info** | `bdinfo` / `coninfo` / `licenseinfo` / `version` / `time` |
| **控制流** | `if` / `then` / `else` / `fi` / `for` / `while` / `repeat` / `sleep` |
| **重启 / 关机** | `reset` / `poweroff` |
| **EFI** | `bootefi` / `efidebug` / `efivar` |
| **调试** | `gpio` / `i2c` / `spi` / `pmic` / `regulator` |

→ 详见 `doc/usage/cmd/` 目录下 119 个 .rst 文件，每命令一份。

---

## 2. 环境变量系统（doc/usage/environment.rst, 591 行）

### 2.1 为什么 U-Boot 要环境变量

环境变量 = U-Boot 的"配置文件 + 启动脚本一体化"。它解决：
- **跨重启持久**（写到 SPI/MMC 的环境分区）
- **运行时可改**（`setenv` 不需要重编固件）
- **启动逻辑脚本化**（`bootcmd` 写整个 boot 流程）
- **跨 OS 配置**（`bootargs` 传给 Linux kernel cmdline）

### 2.2 重要变量速查表（板子开起来不能不会的）

| 变量 | 含义 | 例 |
|------|------|---|
| **`bootcmd`** ⭐ | autoboot 时执行的命令（如果用户没按键打断） | `bootcmd=run distro_bootcmd` |
| **`bootargs`** ⭐ | 启动 OS 时传的内核命令行 | `bootargs=root=/dev/mmcblk0p2 rw console=ttyS0,115200` |
| **`bootdelay`** | autoboot 倒计时秒数（-1=禁止 autoboot，0=立即不可中断） | `bootdelay=3` |
| `baudrate` | 串口波特率 | `baudrate=115200` |
| `bootfile` | TFTP 默认下载文件名 | `bootfile=Image` |
| `serverip` | TFTP/NFS 服务器 IP | `serverip=192.168.1.10` |
| `ipaddr` | 板子 IP（也可 dhcp 动态） | `ipaddr=192.168.1.100` |
| `loadaddr` / `kernel_addr_r` | RAM 中加载内核的地址 | `loadaddr=0x40200000` |
| `fdt_addr_r` | RAM 中加载 DTB 的地址 | `fdt_addr_r=0x44000000` |
| `ramdisk_addr_r` | RAM 中加载 initramfs 的地址 | `ramdisk_addr_r=0x46000000` |
| `fdtfile` | 默认 DTB 文件名 | `fdtfile=qemu/qemu-virt.dtb` |
| `stdout` / `stdin` / `stderr` | 控制台设备 | `stdout=serial,vidconsole` |
| `autostart` | 自动 boot 加载的镜像 | `autostart=yes` |
| `verify` | 是否做镜像 CRC 验证 | `verify=no` |

### 2.3 环境变量操作命令

```bash
# 查看所有变量
=> printenv
# 或简写
=> pri

# 查看某变量
=> printenv bootcmd

# 设置变量（仅内存中）
=> setenv myvar "hello world"

# 删除变量
=> setenv myvar       (不带值就是删)

# 持久化到存储（写到 env partition / SPI 等）
=> saveenv

# 编辑现有变量（带原值进编辑器）
=> editenv bootcmd

# 导出 / 导入（用于备份）
=> env export -t -s 0x80000000 4096
=> env import -t 0x80000000 4096
```

### 2.4 环境变量定义方式（编译期 vs 运行期）

U-Boot 环境变量有 3 种来源，**优先级从低到高**：

1. **C 默认环境**（`include/env_default.h`）—— 旧式，每板宏定义
2. **Text-based `.env` 文件**（推荐，doc/usage/environment.rst § Text-based）—— `board/<vendor>/<board>/<board>.env`
3. **持久化存储**（`saveenv` 写入的 SPI/MMC 区）—— **最高优先级**

#### 2.4.1 Text-based .env 例子（snapper9260 板）

```c
// board/bluewater/snapper9260.env
stdout=serial
#ifdef CONFIG_VIDEO
stdout+=,vidconsole
#endif
bootcmd=
    /* U-Boot script for booting */
    if [ -z ${tftpserverip} ]; then
        echo "Use 'setenv tftpserverip a.b.c.d' to set IP address."
    fi

    usb start; setenv autoload n; bootp;
    tftpboot ${tftpserverip}:
    bootm
failed=
    echo CONFIG_SYS_BOARD boot failed - please check your image
```

→ **支持 C 预处理器**（`#ifdef`、`#include <env/ti/mmc.env>`）+ **多行变量**（缩进规则） + **`+=` 追加**。

### 2.5 环境变量的 backend 存储

U-Boot 支持把环境写到多种媒介（Kconfig 选 `CONFIG_ENV_IS_IN_*`）：

| backend | Kconfig | 适用 |
|---------|---------|------|
| MMC | `CONFIG_ENV_IS_IN_MMC` | SD / eMMC |
| SPI Flash | `CONFIG_ENV_IS_IN_SPI_FLASH` | NOR Flash |
| NAND | `CONFIG_ENV_IS_IN_NAND` | NAND Flash |
| FAT | `CONFIG_ENV_IS_IN_FAT` | FAT 分区文件 |
| ext4 | `CONFIG_ENV_IS_IN_EXT4` | ext4 分区文件 |
| UBI | `CONFIG_ENV_IS_IN_UBI` | NAND 上 UBI 卷 |
| RAM | `CONFIG_ENV_IS_NOWHERE` | 不持久（每次默认值） |
| SCSI/SATA | `CONFIG_ENV_IS_IN_SCSI` | SATA 硬盘 |

**A/B 冗余**：Kconfig `CONFIG_SYS_REDUNDAND_ENVIRONMENT` 让 U-Boot 在两个位置存环境，写一份成功才覆盖另一份 → 防掉电损坏。

---

## 3. 启动流程脚本：bootcmd 详解

U-Boot 启动流程的核心就是 `bootcmd` 这一个变量。**理解 bootcmd = 理解 U-Boot 启动**。

### 3.1 简单启动场景

```bash
# 最简单：从固定地址 boot
bootcmd=bootm 0x80800000 - 0x82000000

# 含义：bootm <kernel_addr> <ramdisk_addr> <fdt_addr>
# - 表示无 ramdisk
```

### 3.2 嵌入式 SD 启动（典型）

```bash
bootcmd=
    setenv bootargs root=/dev/mmcblk0p2 rw rootwait console=ttyS0,115200;
    fatload mmc 0:1 ${kernel_addr_r} Image;
    fatload mmc 0:1 ${fdt_addr_r} qemu-virt.dtb;
    booti ${kernel_addr_r} - ${fdt_addr_r};
```

**逐行**：
1. `setenv bootargs ...` — 设 Linux kernel cmdline
2. `fatload mmc 0:1 ${kernel_addr_r} Image` — 从 mmc 设备 0 分区 1（FAT）加载 Image 文件到 RAM
3. `fatload ... fdt` — 加载 DTB
4. `booti ${kernel_addr_r} - ${fdt_addr_r}` — boot ARM64 Image 格式（- 表示无 initramfs）

### 3.3 distro_bootcmd（U-Boot 经典 distro 启动协议）

`distro_bootcmd` 是 U-Boot 内置的"通用 distro 启动脚本"，按顺序探测 mmc / usb / pxe / scsi 等设备，找到 `extlinux/extlinux.conf` 或 `boot.scr` 就启动。

```bash
bootcmd=run distro_bootcmd

distro_bootcmd=
    for target in ${boot_targets};
    do
        run bootcmd_${target};
    done

boot_targets=mmc0 mmc1 usb0 pxe dhcp

bootcmd_mmc0=
    setenv devnum 0;
    run mmc_boot

mmc_boot=
    if mmc dev ${devnum}; then
        setenv devtype mmc;
        run scan_dev_for_boot_part;
    fi
```

→ **distro_bootcmd 支持任意 Linux distro 的 zerorisc-like 启动**（Fedora / Debian / Ubuntu / OpenSUSE 都用）。**已被新的 bootstd 取代**（详见 [03-08 § X bootstd](03-08-uboot-develop-manual.md)），但当前主线仍兼容。

### 3.4 bootflow / bootstd（新一代启动协议，2022+）

```bash
=> bootflow scan -lb
=> bootflow list
=> bootflow boot
```

**bootstd** 是 U-Boot 主线 2022 引入的"distro 启动统一框架"，逐步替代 `distro_bootcmd`。**KuBoot 设计时直接用 bootstd**（详见 [03-08](03-08-uboot-develop-manual.md)）。

---

## 4. FIT 镜像（doc/usage/fit/，10 个 .rst 文件）

### 4.1 为什么有 FIT

传统 U-Boot 启动 Linux 需要传 3 个独立文件：
- kernel image (`Image` / `zImage`)
- device tree blob (`*.dtb`)
- initramfs (`initrd.img`)

→ 容易传错版本、不能签名、不便存储。

**FIT (Flattened Image Tree)** 把这 3 个（+ 多种 DTB / 多种 kernel / 多种 OS）打进**一个文件**（`.itb`），DTS 描述结构，可签名，可多 config。

### 4.2 FIT 文件结构（DTS 描述）

```dts
/dts-v1/;

/ {
    description = "Configuration to load Linux on QEMU virt";
    #address-cells = <1>;

    images {
        kernel-1 {
            description = "Linux 6.18.7";
            data = /incbin/("./Image");
            type = "kernel";
            arch = "riscv";
            os = "linux";
            compression = "none";
            load = <0x80200000>;
            entry = <0x80200000>;
            hash-1 {
                algo = "sha256";
            };
        };

        fdt-1 {
            description = "QEMU virt DTB";
            data = /incbin/("./qemu-virt.dtb");
            type = "flat_dt";
            arch = "riscv";
            compression = "none";
            hash-1 {
                algo = "sha256";
            };
        };

        ramdisk-1 {
            description = "Buildroot rootfs";
            data = /incbin/("./rootfs.cpio.gz");
            type = "ramdisk";
            arch = "riscv";
            os = "linux";
            compression = "gzip";
            hash-1 {
                algo = "sha256";
            };
        };
    };

    configurations {
        default = "conf-1";
        conf-1 {
            description = "QEMU virt boot config";
            kernel = "kernel-1";
            fdt = "fdt-1";
            ramdisk = "ramdisk-1";
            signature-1 {
                algo = "sha256,rsa2048";
                key-name-hint = "dev";
                sign-images = "kernel", "fdt", "ramdisk";
            };
        };
    };
};
```

### 4.3 FIT 操作命令

```bash
# 1. 把上面 DTS 编成 itb
$ mkimage -f kernel.its -k keys/ -K u-boot.dtb -r kernel.itb

# 2. U-Boot 里 boot
=> tftpboot ${loadaddr} kernel.itb
=> bootm ${loadaddr}#conf-1
```

`#conf-1` 选 configurations 中的某个 config。**多 DTB 场景**（一份 kernel + 多板 DTB）特别有用：

```dts
configurations {
    conf-rpi4 { kernel = "kernel-1"; fdt = "fdt-rpi4"; };
    conf-bpi-r3 { kernel = "kernel-1"; fdt = "fdt-bpi-r3"; };
    conf-vf2 { kernel = "kernel-1"; fdt = "fdt-vf2"; };
};
```

→ 一份 kernel.itb 跨多板复用。

### 4.4 FIT 验证启动（Verified Boot）

`mkimage -k keys/` 用 RSA 私钥签名；U-Boot 用 `u-boot.dtb` 内嵌的公钥验证。验证失败拒绝启动 → 防固件被篡改。详见 doc/usage/fit/beaglebone_vboot.rst。

---

## 5. SPL Boot 全谱（doc/usage/spl_boot.rst, 321 行）

### 5.1 SPL/TPL/VPL 阶段划分

```
TPL  → SPL  → U-Boot proper
(可选) (可选) (主程序)

或：

ROM → SPL → BL31 (TF-A) → U-Boot proper (BL33)
ROM → SPL → EDK2
ROM → SPL → Linux (Falcon mode)
ROM → SPL → OpenSBI → U-Boot (RISC-V 主流)
```

| 阶段 | 全称 | 大小 | 职责 |
|------|------|------|------|
| **TPL** | Tertiary Program Loader | 极小（几 KB） | 早期 init，只够加载 SPL/VPL |
| **VPL** | Verifying Program Loader | 较小 | A/B 验证启动选哪个 SPL（WIP）|
| **SPL** | Secondary Program Loader | 中（~30-100 KB） | 设 DDR、加载 U-Boot proper / 其他 firmware |
| **U-Boot proper** | — | 大（~500 KB-1 MB） | 命令、env、bootcmd、加载 OS |

### 5.2 SPL 可加载的目标

- **raw binary**（仅 SPL 支持，不是 TPL）
- **FIT 镜像**（推荐）
- **Legacy U-Boot 镜像**（有 magic + CRC）

### 5.3 SPL 30+ 种存储 backend（按设备分类）

#### 5.3.1 块设备（11 种）

| backend | Kconfig | 关键路径 |
|---------|---------|---------|
| MMC1 / MMC2 | `CONFIG_SPL_MMC=y` | SD card / eMMC |
| NVMe | `CONFIG_SPL_NVME=y` + PCI 一堆 | PCIe NVMe SSD |
| SATA | `CONFIG_SPL_SATA=y` | SATA HDD/SSD |
| USB | `CONFIG_SPL_USB_HOST=y + USB_STORAGE` | USB 块设备 |
| SCSI | `CONFIG_SPL_SCSI=y` | SCSI 控制器 |

文件系统加载（要 `CONFIG_SPL_FS_FAT=y` 或 `_EXT=y`）：
```
CONFIG_SPL_FS_LOAD_PAYLOAD_NAME="u-boot.itb"
```

#### 5.3.2 闪存（7 种）

| backend | Kconfig | 适用 |
|---------|---------|------|
| NAND raw | `CONFIG_SPL_NAND_SUPPORT=y` | 老式 NAND |
| NAND UBI | `CONFIG_SPL_UBI=y` | 现代 NAND（带磨损均衡） |
| NOR | `CONFIG_SPL_NOR_SUPPORT=y` | NOR Flash |
| OneNAND | `CONFIG_SPL_ONENAND_SUPPORT=y` | OneNAND |
| SPI NOR | `CONFIG_SPL_SPI_FLASH + CONFIG_SPI_LOAD` | 最常用 SoC 启动介质 |
| Sunxi SPI | `CONFIG_SPL_SPI_SUNXI` | Allwinner 专用 |

#### 5.3.3 网络与协议（5 种）

| backend | Kconfig | 适用 |
|---------|---------|------|
| Ethernet | `CONFIG_SPL_NET=y` + `SPL_ETH_DEVICE` | 通过 BOOTP/TFTP 拉镜像 |
| UART (Y-Modem) | `CONFIG_SPL_YMODEM_SUPPORT=y` | 串口接收（开发期） |
| USB SDP | `CONFIG_SPL_USB_SDP_SUPPORT=y` | i.MX 系列 ROM 协议 |
| DFU | `CONFIG_DFU=y + SPL_RAM_SUPPORT` | USB DFU 协议 |
| Semihosting | `CONFIG_SPL_SEMIHOSTING=y` | QEMU/JTAG 半模拟读宿主 fs |

#### 5.3.4 其他（5 种）

| backend | 适用 |
|---------|------|
| **BOOTROM** | 让 ROM 加载（Allwinner FEL / 特殊 SoC） |
| **RAM** | 镜像已被前置 loader 加载到 RAM |
| **XIP** | NOR Flash 直接 execute-in-place |
| **VBE Simple** | VPL 阶段用 |
| **Sandbox** | x86 host 上 sim 测试 |

### 5.4 SPL 启动顺序（多 backend）

板子代码实现 `board_boot_order()` 函数返回 `BOOT_DEVICE_*` 枚举的数组（最多 5 个）：

```c
// arch/<arch>/cpu/<soc>/boot.c
void board_boot_order(u32 *spl_boot_list)
{
    spl_boot_list[0] = BOOT_DEVICE_MMC1;     // 优先 SD
    spl_boot_list[1] = BOOT_DEVICE_MMC2;     // 然后 eMMC
    spl_boot_list[2] = BOOT_DEVICE_USB;      // 然后 USB
    spl_boot_list[3] = BOOT_DEVICE_UART;     // 最后 UART recovery
    spl_boot_list[4] = BOOT_DEVICE_NONE;
}
```

SPL 主流程依次尝试，直到一个成功为止。

---

## 6. 文件系统命令（doc/usage/filesystems/）

### 6.1 支持的 fs

| fs | Kconfig | 可读写 |
|----|---------|--------|
| FAT12/16/32 | `CONFIG_FAT` | RW |
| ext2/3/4 | `CONFIG_EXT4` | RW |
| btrfs | `CONFIG_BTRFS` | RO |
| ZFS | `CONFIG_ZFS` | RO |
| squashfs | `CONFIG_SQUASHFS` | RO |
| EROFS | `CONFIG_EROFS` | RO |
| JFFS2 | `CONFIG_JFFS2` | RO |
| UBIFS | `CONFIG_UBIFS` | RO |
| Sandbox host fs | `CONFIG_SANDBOX` | RW（仅 sandbox） |

### 6.2 fs 命令

```bash
# 通用 fs 命令（自动检测格式）
=> fstype mmc 0:1
=> ls mmc 0:1 /boot
=> load mmc 0:1 ${loadaddr} /boot/Image

# fs 特定命令
=> fatls mmc 0:1
=> fatload mmc 0:1 ${loadaddr} Image
=> ext4ls mmc 0:2 /boot
=> ext4load mmc 0:2 ${loadaddr} /boot/vmlinuz

# 写入（仅 RW fs）
=> ext4write mmc 0:2 ${loadaddr} /boot/saved.bin 0x100000
=> fatwrite mmc 0:1 ${loadaddr} myfile.bin 0x100000
```

---

## 7. 分区操作（doc/usage/partitions.rst）

### 7.1 支持的分区表

| 类型 | Kconfig | 适用 |
|------|---------|------|
| MBR | `CONFIG_MAC_PARTITION` 风 | 老式 PC（最大 2TB / 4 主分区） |
| GPT | `CONFIG_EFI_PARTITION` | UEFI 现代（128 分区，PB 级） |
| Mac (APM) | `CONFIG_MAC_PARTITION` | 老 PowerPC Mac |
| AMIGA | `CONFIG_AMIGA_PARTITION` | 老 Amiga |
| Sun | `CONFIG_ISO_PARTITION` | Sun 工作站 |
| ISO | `CONFIG_ISO_PARTITION` | CD/DVD ISO9660 |

### 7.2 分区命令

```bash
# 列分区
=> part list mmc 0
Partition Map for MMC device 0  --   Partition Type: EFI

Part   Start LBA  End LBA   Name
       Attributes  Type GUID  Partition GUID
1      0x00000800 0x00081fff  "boot"
       attrs: 0x0000000000000000  type: c12a7328-...
2      0x00082000 0x003fffff  "rootfs"
       attrs: 0x0000000000000000  type: 0fc63daf-...

# 拿单个分区起始 LBA
=> part start mmc 0 1 part_start
=> printenv part_start

# 写新 GPT
=> gpt write mmc 0 "name=boot,size=64MiB,type=esp;name=root,size=0,type=linux"
```

---

## 8. 网络启动（doc/usage/pxe.rst, environment.rst § networking）

### 8.1 网络命令

```bash
# 配 IP（动态）
=> dhcp                 # DHCP 拿 IP + tftpboot bootfile

# 配 IP（静态）
=> setenv ipaddr 192.168.1.100
=> setenv serverip 192.168.1.10
=> setenv netmask 255.255.255.0

# 拉文件
=> tftpboot ${loadaddr} Image
=> nfs ${loadaddr} 192.168.1.10:/srv/nfs/Image
=> wget ${loadaddr} http://192.168.1.10/Image  # CONFIG_CMD_WGET

# ping 测试
=> ping 192.168.1.10
```

### 8.2 PXE 启动（doc/usage/pxe.rst）

PXE = Preboot eXecution Environment（Intel 1998），用 DHCP + TFTP 启动：

```bash
=> pxe get             # DHCP + TFTP get pxelinux.cfg/<MAC>
=> pxe boot            # 解析 cfg + 加载 kernel + boot
```

`pxelinux.cfg` 文件格式与 syslinux 兼容：

```
default linux
label linux
    kernel /Image
    initrd /initrd.img
    append root=/dev/nfs nfsroot=192.168.1.10:/srv/nfs
```

→ **PXE = 服务器机房标准批量装机方式**（IPMI 远程开机 → PXE → kickstart 装 Linux）。

---

## 9. UEFI 模式（doc/usage/measured_boot.rst, doc/develop/uefi/）

U-Boot **2017+ 内置 UEFI 实现**，可作为 UEFI 固件运行 UEFI 应用（grub.efi / shim.efi / Linux EFI stub）：

```bash
# 启动 grub.efi
=> setenv loadaddr 0x40400000
=> fatload mmc 0:1 ${loadaddr} EFI/BOOT/bootriscv64.efi
=> bootefi ${loadaddr}

# 用 EFI Boot Manager（看 BootOrder 启动）
=> bootefi bootmgr

# 列出 EFI 启动条目
=> efidebug boot dump

# 加 EFI 启动条目
=> efidebug boot add 0001 "Debian" mmc 0:1 EFI/debian/grubriscv64.efi
=> efidebug boot order 0001
```

### 9.1 measured boot

`CONFIG_MEASURED_BOOT=y` 让 U-Boot 把启动每段代码 hash 入 TPM PCR，类似 Heads（详见 [03-18 LinuxBoot](03-18-linuxboot-walkthrough.md)）。

---

## 10. 高级使用场景

### 10.1 DFU - Device Firmware Upgrade（doc/usage/dfu.rst）

USB DFU 协议把 U-Boot 当 DFU 设备，让 PC `dfu-util` 灌固件：

```bash
=> dfu 0 mmc 0
# PC 端：dfu-util -d 0525:a4a5 -a 0 -D u-boot.itb
```

### 10.2 netconsole（doc/usage/netconsole.rst）

把 U-Boot 串口重定向到 UDP，远程调试：

```bash
=> setenv stdout nc
=> setenv stdin nc
=> setenv ncip 192.168.1.10
# PC 端：socat - UDP-LISTEN:6666
```

### 10.3 fdt overlays（doc/usage/fdt_overlays.rst）

运行时合并 DTBO（device-tree overlay）改 base DT：

```bash
=> fdt addr ${fdt_addr_r}
=> fdt apply ${fdtoverlay_addr_r}    # 合并 overlay
```

→ **同一 base DTB + 多 overlay** 适配多硬件配置（如 hat / cape 扩展板）。

### 10.4 blkmap（doc/usage/blkmap.rst, U-Boot 2022+）

虚拟块设备：把 RAM / 文件 / 多个块设备 mapping 成单一虚拟块设备，用于复杂启动场景：

```bash
=> blkmap create vdisk
=> blkmap map vdisk 0 0x100 mem 0x80100000
=> blkmap get vdisk dev devnum
=> ls blkmap ${devnum}:1
```

### 10.5 semihosting（doc/usage/semihosting.rst）

ARM/RISC-V 半模拟（QEMU + JTAG）让 U-Boot 直接读宿主 fs：

```bash
=> smhload boot.itb ${loadaddr}      # 读宿主 boot.itb
```

→ 开发期实验用，避免每次重做镜像。

---

## 11. OS 启动

### 11.1 启动 Linux

```bash
# Linux ARM zImage
=> bootz ${kernel_addr_r} - ${fdt_addr_r}

# Linux ARM64 / RISC-V Image
=> booti ${kernel_addr_r} - ${fdt_addr_r}

# Linux 通过 FIT
=> bootm ${itb_addr}#conf-1

# Linux 通过 EFI stub
=> bootefi ${kernel_addr_r}
```

### 11.2 启动其他 OS

| OS | 命令 | 笔记 |
|----|------|------|
| Plan 9 | `bootp9` | doc/usage/os/plan9.rst |
| VxWorks | `bootvx` / `bootmvx` | doc/usage/os/vxworks.rst |
| FreeBSD | `bootefi` (UEFI loader) | 走 EFI stub |
| Windows IoT | `bootefi` | 走 UEFI |

---

## 12. KuBoot 借鉴 checklist（U-Boot usage 视角）


| 必做 | 含义 |
|------|------|
| ⭐ Text-based env | 抛弃 C 宏环境，全 .env 文件（U-Boot 推荐） |
| ⭐ FIT 镜像加载 | KuBoot 必须支持 FIT（kernel + DTB + initrd + signature） |
| ⭐ bootcmd / bootargs 等价 | 可脚本化的启动协议 |
| ⭐ bootstd 风格 | 而非旧 distro_bootcmd |
| ⭐ env 持久化 | A/B 冗余 + 多 backend |
| ⭐ DM (Driver Model) | 通过 DT 自动 probe driver |
| ⭐ 主流 fs | FAT + ext4 至少；squashfs/btrfs 可选 |
| ⭐ 主流分区 | GPT 必备，MBR 可选 |
| 重要 | 网络启动（DHCP + TFTP + HTTP wget） |
| 重要 | UEFI 模式（让 grub.efi / Linux EFI stub 能跑） |
| 重要 | 多 backend SPL 框架 |
| 可选 | DFU / netconsole / semihosting（开发期工具） |
| 可选 | measured boot |
| 可选 | fdt overlay 运行时合并 |
| 可选 | blkmap 虚拟块设备 |

---

## 13. 进一步阅读

- **官方手册**：https://u-boot.readthedocs.io/en/latest/usage/
- **本地源码**：`/home/heke/tgln/stage2/material/boot/u-boot/doc/usage/`
- **本仓库相关**：[03-06 U-Boot 概览](03-06-u-boot-overview.md)（项目身份）+ [03-10 SPL 源码](03-10-u-boot-spl-source-walkthrough.md)（SPL 实现）+ [03-11 proper 源码](03-11-u-boot-proper-source-walkthrough.md)（proper 实现）+ [03-09 U-Boot 演化](03-09-uboot-evolution-case-study.md)（24 年演化）+ **[03-08 U-Boot 开发手册](03-08-uboot-develop-manual.md)（继续学：怎么扩展 / 贡献）**

---

## 14. 4 道练习题

1. **bootcmd 拆解**：给一个 `bootcmd=run distro_bootcmd; tftpboot $kernel_addr_r`，分别说明这两个命令做什么、`distro_bootcmd` 内部是怎样的、若 `distro_bootcmd` 失败如何 fallback 到 tftpboot？
2. **FIT vs 三独立文件**：传统 U-Boot 启动 Linux 需 kernel + DTB + initrd 三个独立文件，FIT 把它们合一。FIT 解决了哪 4 个具体问题？为什么不用 tar/zip 直接打包？
3. **SPL backend 选择**：你现在在做一个 RISC-V SBC，主存储是 SD 卡，但开发期需要从串口 (Y-Modem) 灌镜像 + 量产期从 SPI Flash 启动。SPL 的 `board_boot_order()` 应该怎么写？需要哪些 `CONFIG_SPL_*=y`？
4. **env 持久化**：板子上电断电会丢失环境变量？答：会丢；要怎么办？答案有 2 种 backend，分别说明优缺点。
