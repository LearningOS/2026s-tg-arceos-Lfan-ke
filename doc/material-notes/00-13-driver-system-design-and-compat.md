# 00-13 驱动系统设计 + 兼容策略全谱

> **本文位置：** 00 大类总览 / 设计维度
>
>
> **与其他笔记的关系：**
> - **互补**：[00-12 设备/驱动模型演化史](00-12-device-driver-evolution.md)（演化维度，写"过程"）；本文写"设计 + 兼容"（结构 + 工程决策）
> - **承上**：[00-11 中断演化](00-11-interrupt-evolution.md)（驱动里中断处理依赖此）+ [03-04 DTS/DTB/FDT 参考](03-04-dts-dtb-fdt-syntax-reference.md)（驱动里 DT 匹配依赖此）
> - **配套**：[00-23 图形界面 / 图形栈概览](00-23-graphics-ui-overview.md)（图形栈对驱动的具体使用案例）
> - **下游**：未来 G 系列深度精读笔记（G1 Linux 驱动模型源码精读 / G3 FreeBSD LinuxKPI 精读 / G4 ReactOS 精读 等）
>

---

## §0 总览：为什么需要这篇笔记


2. **兼容 x11 等等图形界面** —— 让 X11 协议 / 桌面环境能跑

这两个目标都涉及"如何对接已有生态"——即**兼容性工程**问题。

### §0.2 兼容性工程的核心问题

设计一个新 OS 时，"驱动如何来" 是必答题，三条路：

| 路径 | 含义 | 代表 |
|------|------|------|
| **b. 兼容已有生态** | 复用 Linux/Windows/BSD 等海量已有驱动 | FreeBSD LinuxKPI / ReactOS |
| **c. 混合策略** | 核心子系统自己写 + 长尾外设借生态 | DragonOS / asterinas |


### §0.3 全文结构（15 节）

| 节 | 主题 | 关键词 |
|----|------|--------|
| §1 | 类 Linux 驱动系统全谱 | kobject / sysfs / device / driver / bus / class |
| §2 | 驱动子系统拆解 | platform / I2C / SPI / PCI / USB / MMC / GPIO / clk / dma / iommu |
| §3 | DT 在驱动中的角色 | of_match_table / compatible / phandle / overlay |
| §4 | module 加载机制 | .ko ELF / EXPORT_SYMBOL / DKMS / 签名 |
| §5 | Rust for Linux 驱动 ABI | kernel crate / module! 宏 / driver trait |
| ⭐ §6 | `.sys` vs `.ko` 横向对比（17 维度）| Windows vs Linux 驱动文件根本差异 |
| §7 | 跨 OS 驱动框架横向（设计维度）| Linux / Windows / macOS / BSD / illumos / Fuchsia / Android |
| ⭐ §8 | 驱动兼容策略全谱（5 路径）| ABI / source-level / shim / API 重写 / VFIO |
| ⭐ §9 | 跨 OS 兼容案例 | ReactOS / DragonOS / FreeBSD LinuxKPI / illumos SPL / WSL1+2 / Haiku |
| ⭐ §10 | 兼容机制底层 | 符号桥接 / struct 布局 / 锁中断语义 / DMA buffer / KABI shim |
| §11 | 本地项目驱动模型现状对照 | arceos / DragonOS / asterinas / Theseus / TornadoOS / StarryOS |
| §12 | 设计模式归纳 | type-state / capability / 错误代数 / DMA-safe buffer / threaded irq |
| §14 | 词典 | 关键术语速查 |
| §15 | 练习题 | 4 题 |

---

## §1 类 Linux 驱动系统全谱

> **目的：** 把 Linux 驱动模型作为"类 Linux 驱动系统"的参照系完整讲清楚——任何后续兼容工作都要先理解 Linux 模型。

### §1.1 kobject + kset 基础

**kobject** 是 Linux 驱动模型最底层抽象，定义于 `include/linux/kobject.h`：

```c
struct kobject {
    const char        *name;       // sysfs 节点名
    struct list_head   entry;      // 链入 kset 的节点
    struct kobject    *parent;     // sysfs 父节点
    struct kset       *kset;       // 所属 kset
    const struct kobj_type *ktype; // 类型信息（含 release/sysfs_ops/default_attrs）
    struct kernfs_node *sd;        // sysfs 目录节点
    struct kref        kref;       // 引用计数
    unsigned int       state_initialized:1;
    unsigned int       state_in_sysfs:1;
    unsigned int       state_add_uevent_sent:1;
    unsigned int       state_remove_uevent_sent:1;
    unsigned int       uevent_suppress:1;
};
```

**核心职责：**
1. **引用计数**（`kref_get` / `kref_put`）—— 防止设备被多用户引用时提前释放
2. **sysfs 表示**（`/sys/...` 目录树）—— 把内核对象暴露给用户态
3. **uevent 通知**（向 udev / mdev 发设备热插拔事件）
4. **层次关系**（parent / kset 形成树）

**kset** 是 kobject 容器（一组同类 kobject）：

```c
struct kset {
    struct list_head   list;          // kobject 列表
    spinlock_t         list_lock;
    struct kobject     kobj;          // kset 自身也是个 kobject
    const struct kset_uevent_ops *uevent_ops;  // uevent 过滤
};
```

**典型 kset：**
- `bus_kset` —— `/sys/bus/`，所有总线
- `class_kset` —— `/sys/class/`，所有类
- `devices_kset` —— `/sys/devices/`，所有设备

### §1.2 sysfs 文件系统

`sysfs` 是 kobject 树在 VFS 中的具象化，挂载于 `/sys`，每个 kobject = 一个目录，每个属性 = 一个文件。

**典型布局：**
```
/sys/
├── bus/                    # 所有总线
│   ├── pci/
│   │   ├── devices/        # 此总线上所有设备的符号链接
│   │   ├── drivers/        # 此总线上注册的驱动
│   │   └── drivers_autoprobe
│   ├── usb/
│   ├── i2c/
│   └── platform/
├── class/                  # 所有类
│   ├── net/
│   │   ├── eth0 -> ../../devices/.../net/eth0
│   │   └── lo
│   ├── block/
│   ├── input/
│   └── tty/
├── devices/                # 实际设备树
│   ├── pci0000:00/
│   │   └── 0000:00:1f.6/
│   │       └── net/
│   │           └── eth0/
│   └── platform/
├── kernel/                 # 内核内部对象
├── firmware/
└── module/                 # 已加载模块
```

**两条遍历同一设备的路径：**
- 物理路径：`/sys/devices/pci.../net/eth0`
- 类路径：`/sys/class/net/eth0`（符号链接到上面）
- 总线路径：`/sys/bus/pci/devices/0000:00:1f.6/net/eth0`

**sysfs 属性文件：** 每个文件可读写一个内核值，如 `/sys/class/net/eth0/mtu` 是 MTU 整数。

### §1.3 device / driver 分离

Linux 驱动模型最核心的设计 —— **device 和 driver 是两个独立对象**：

```c
struct device {
    struct device       *parent;
    struct device_private *p;
    struct kobject       kobj;
    const char          *init_name;     // 初始名（probe 后变 dev_name(dev)）
    const struct device_type *type;
    struct bus_type     *bus;           // 关联的总线
    struct device_driver *driver;       // 当前 bound 的驱动（NULL 即未绑定）
    void                *platform_data; // 平台传过来的私有数据
    void                *driver_data;   // 驱动私有数据
    struct dev_pm_info   power;
    struct dev_pm_domain *pm_domain;
    struct dev_pin_info *pins;
    struct dma_map_ops  *dma_ops;
    u64                 *dma_mask;
    u64                  coherent_dma_mask;
    struct device_node  *of_node;       // DT 节点（DT 平台）
    struct fwnode_handle *fwnode;       // ACPI / DT 抽象
    dev_t                devt;          // major/minor
    u32                  id;
    spinlock_t           devres_lock;
    struct list_head     devres_head;
    void               (*release)(struct device *dev);
    /* ... 60+ 字段 ... */
};

struct device_driver {
    const char          *name;
    struct bus_type     *bus;
    struct module       *owner;
    const char          *mod_name;
    bool                 suppress_bind_attrs;
    enum probe_type      probe_type;
    const struct of_device_id  *of_match_table;
    const struct acpi_device_id *acpi_match_table;
    int                (*probe)   (struct device *dev);
    void               (*sync_state)(struct device *dev);
    int                (*remove)  (struct device *dev);
    void               (*shutdown)(struct device *dev);
    int                (*suspend) (struct device *dev, pm_message_t state);
    int                (*resume)  (struct device *dev);
    const struct attribute_group **groups;
    const struct attribute_group **dev_groups;
    const struct dev_pm_ops *pm;
    void (*coredump)(struct device *dev);
    struct driver_private *p;
};
```

**核心原则：**
1. **device** 表示"这里有一个硬件存在" —— 由总线扫描 / DT 解析 / 平台注册产生
2. **driver** 表示"我能驱动某种类型的硬件" —— 由模块加载注册
3. **bus** 负责把两者**配对（match）+ 绑定（bind）+ 分离（unbind）**

**生命周期：**
```
device_create  →  bus_match_each_driver（找匹配 driver）
               →  device_bind_driver（调 driver->probe）
               → ... 设备工作中 ...
               →  device_unbind_driver（调 driver->remove）
               →  device_destroy
```

### §1.4 bus 总线

**bus_type** 是 device 和 driver 的"红娘"：

```c
struct bus_type {
    const char          *name;
    const char          *dev_name;
    struct device       *dev_root;
    const struct attribute_group **bus_groups;
    const struct attribute_group **dev_groups;
    const struct attribute_group **drv_groups;

    int  (*match)   (struct device *dev, struct device_driver *drv);
    int  (*uevent)  (struct device *dev, struct kobj_uevent_env *env);
    int  (*probe)   (struct device *dev);
    void (*sync_state)(struct device *dev);
    void (*remove)  (struct device *dev);
    void (*shutdown)(struct device *dev);

    int  (*online)  (struct device *dev);
    int  (*offline) (struct device *dev);

    int  (*suspend) (struct device *dev, pm_message_t state);
    int  (*resume)  (struct device *dev);

    int  (*num_vf)  (struct device *dev);
    int  (*dma_configure)(struct device *dev);
    void (*dma_cleanup)  (struct device *dev);

    const struct dev_pm_ops *pm;
    const struct iommu_ops  *iommu_ops;

    struct subsys_private  *p;
    struct lock_class_key  lock_key;

    bool need_parent_lock;
};
```

**主流 bus 实例：**

| bus | 文件 | 设备数（典型 PC）|
|-----|------|------------------|
| `pci_bus_type` | `drivers/pci/pci-driver.c` | 数十 |
| `usb_bus_type` | `drivers/usb/core/driver.c` | 十几 |
| `i2c_bus_type` | `drivers/i2c/i2c-core-base.c` | 几十（嵌入式更多）|
| `spi_bus_type` | `drivers/spi/spi.c` | 几个 |
| `mmc_bus_type` | `drivers/mmc/core/bus.c` | 1-2 |
| `platform_bus_type` | `drivers/base/platform.c` | 几十（嵌入式 SoC）|
| `virtio_bus` | `drivers/virtio/virtio.c` | VM 中几个 |
| `xen_bus_type` | `drivers/xen/xenbus/xenbus_probe.c` | Xen guest 中 |

**bus->match 是关键：** 决定 device 和 driver 是否匹配。各 bus 有不同策略：

- **PCI**：比对 `vendor:device` ID（`pci_match_one_device`）
- **USB**：比对 `idVendor:idProduct` 或 class/subclass
- **I2C**：比对名字（早期）或 of_match_table（现代）
- **platform**：比对 of_match_table compatible 字符串

### §1.5 class 分类

**class** 把功能相同但物理总线不同的设备归为一类，提供 `/sys/class/` 视图：

```c
struct class {
    const char          *name;
    struct module       *owner;
    const struct attribute_group  **class_groups;
    const struct attribute_group  **dev_groups;
    struct kobject      *dev_kobj;

    int (*dev_uevent)    (struct device *dev, struct kobj_uevent_env *env);
    char *(*devnode)     (struct device *dev, umode_t *mode);

    void (*class_release)(struct class *cls);
    void (*dev_release)  (struct device *dev);

    int  (*shutdown_pre) (struct device *dev);
    const struct kobj_ns_type_operations *ns_type;
    const void *(*namespace)(struct device *dev);

    void (*get_ownership)(struct device *dev, kuid_t *uid, kgid_t *gid);
    const struct dev_pm_ops *pm;
    struct subsys_private *p;
};
```

**典型 class：**

| class | 含义 | 物理来源可能 |
|-------|------|-------------|
| `net` | 网络接口 | PCI / USB / virtio / platform |
| `block` | 块设备 | PCI（NVMe）/ USB / virtio / mmc |
| `tty` | 终端 | platform（UART）/ USB / virtual |
| `input` | 输入设备 | USB / I2C / platform / serio |
| `gpu` | GPU 设备 | PCI（独显）/ platform（SoC GPU）|
| `power_supply` | 电池/充电器 | I2C / platform / ACPI |
| `thermal` | 温度传感器 | I2C / platform / ACPI |
| `hwmon` | 硬件监控 | I2C / SMBus / platform |

**class 的设计意义：** 用户态程序（如 NetworkManager / udev rules）可以根据 class 找到所有同类设备，无需关心底层总线。

### §1.6 完整对象关系图

```
┌───────────────────────────────────────────────────────────┐
│                       kobject 层                          │
│  (引用计数 + sysfs 表示 + uevent + 父子关系)              │
└───────────────────────┬───────────────────────────────────┘
                        │
        ┌───────────────┼─────────────────┬──────────────┐
        ▼               ▼                 ▼              ▼
   ┌────────┐      ┌────────┐       ┌────────────┐  ┌──────┐
   │ device │      │ driver │       │ bus_type   │  │ class│
   └────┬───┘      └────┬───┘       └────┬───────┘  └───┬──┘
        │               │                │              │
        │ bus           │ bus            │              │
        ├───────────────┴────────────────┤              │
        │           bus->match()         │              │
        │           bus->probe() →       │              │
        │           driver->probe(dev)   │              │
        │                                │              │
        ├──── of_node ──→ device_node ───┤              │
        │     (DT 平台)                   │              │
        │                                │              │
        ├──── fwnode ──→ acpi_device ────┤              │
        │     (ACPI 平台)                 │              │
        │                                │              │
        ▼                                              ▼
   /sys/devices/                                  /sys/class/
   (物理拓扑)                                      (功能分类)
                                                       │
   /sys/bus/<bus>/devices/  ←──── 链接到 ─────────────┘
                                                       │
   /sys/bus/<bus>/drivers/  ←──── driver 注册 ─────────┘
```

### §1.7 probe / shutdown / suspend / resume 生命周期

**probe**（探测）—— 驱动认领设备时调：
```c
static int my_drv_probe(struct device *dev) {
    /* 1. 校验设备硬件版本 */
    /* 2. 申请资源（IRQ / MMIO / DMA buffer / clk）*/
    /* 3. 初始化硬件（reset / 写控制寄存器）*/
    /* 4. 注册到上层框架（netdev_register / cdev_add / class_create）*/
    /* 5. 启用中断 + 启动状态机 */
    return 0;  // 0 = 接管成功；非 0 = 拒绝（bus 会试下个 driver）
}
```

**remove**（移除）—— 设备热拔出 / 模块 unload 时调：
```c
static int my_drv_remove(struct device *dev) {
    /* 1. 停止设备 IO，让排队请求完成 */
    /* 2. 注销框架（netdev_unregister / cdev_del）*/
    /* 3. 释放 IRQ / MMIO / DMA / clk */
    /* 4. 释放驱动私有数据 */
    return 0;
}
```

**shutdown**（关机）—— 系统关机/重启时调，要求**快速**让硬件停在安全状态：
```c
static void my_drv_shutdown(struct device *dev) {
    /* 写控制寄存器让设备掉电进入安全态 */
    /* 无需释放资源（系统快关了）*/
}
```

**suspend / resume**（休眠/唤醒）—— ACPI/PSCI/SBI HSM suspend 时调：
```c
static int my_drv_suspend(struct device *dev, pm_message_t state) {
    /* 保存设备寄存器到内存 */
    /* 让设备进入低功耗态 */
    return 0;
}
static int my_drv_resume(struct device *dev) {
    /* 恢复寄存器 */
    /* 重新启用中断 */
    return 0;
}
```

**runtime PM**（运行时电源管理，独立于系统级休眠）：
```c
static const struct dev_pm_ops my_pm_ops = {
    SET_SYSTEM_SLEEP_PM_OPS(my_suspend, my_resume)
    SET_RUNTIME_PM_OPS(my_runtime_suspend, my_runtime_resume, NULL)
};
```

**完整状态机：**
```
unbound ──probe──> active ──suspend──> suspended ──resume──> active
   │                  │                                          │
   └─bind─────────────┘                                          │
                      │                                          │
                      └──remove──> removed (or unbind)           │
                                                                 │
                      ┌──shutdown──> halted (final)──────────────┘
```

---

## §2 驱动子系统拆解

> **目的：** 列出 Linux 主流总线/子系统的 driver/device 框架，每个子系统的 `match` + `ops` + 注册流程。**总数 ~30 个总线，本节挑核心 10 个。**

### §2.1 platform bus（最特殊）

**platform_bus_type** 是 SoC 时代的核心 —— 没有自动发现协议（不像 PCI/USB），靠 DT/ACPI 静态注册：

```c
struct platform_device {
    const char         *name;
    int                 id;
    bool                id_auto;
    struct device       dev;
    u32                 num_resources;
    struct resource    *resource;     // MMIO + IRQ + DMA 等
    const struct platform_device_id *id_entry;
    struct mfd_cell    *mfd_cell;
    struct pdev_archdata archdata;
};

struct platform_driver {
    int  (*probe)   (struct platform_device *);
    int  (*remove)  (struct platform_device *);
    void (*shutdown)(struct platform_device *);
    int  (*suspend) (struct platform_device *, pm_message_t);
    int  (*resume)  (struct platform_device *);
    struct device_driver driver;
    const struct platform_device_id *id_table;
};
```

**注册：**
```c
static const struct of_device_id my_of_match[] = {
    { .compatible = "vendor,my-uart" },
    { .compatible = "vendor,my-uart-v2", .data = (void *)V2_FLAGS },
    {},
};
MODULE_DEVICE_TABLE(of, my_of_match);

static struct platform_driver my_drv = {
    .probe  = my_probe,
    .remove = my_remove,
    .driver = {
        .name = "my-uart",
        .of_match_table = my_of_match,
    },
};
module_platform_driver(my_drv);  // 一行宏展开成 init/exit
```

**资源获取：**
```c
struct resource *res = platform_get_resource(pdev, IORESOURCE_MEM, 0);
void __iomem *base = devm_ioremap_resource(&pdev->dev, res);
int irq = platform_get_irq(pdev, 0);
```

### §2.2 I2C / SMBus

```c
struct i2c_driver {
    unsigned int        class;
    int  (*probe)       (struct i2c_client *, const struct i2c_device_id *);
    int  (*probe_new)   (struct i2c_client *);  // 现代签名，不传 id_table
    int  (*remove)      (struct i2c_client *);
    void (*shutdown)    (struct i2c_client *);
    void (*alert)       (struct i2c_client *, enum i2c_alert_protocol, unsigned int);
    int  (*command)     (struct i2c_client *, unsigned int, void *);
    struct device_driver driver;
    const struct i2c_device_id *id_table;
    int  (*detect)      (struct i2c_client *, struct i2c_board_info *);
    const unsigned short *address_list;
    struct list_head    clients;
};

struct i2c_client {
    unsigned short      flags;
    unsigned short      addr;          // 7-bit I2C 地址
    char                name[I2C_NAME_SIZE];
    struct i2c_adapter *adapter;       // 所属 I2C 控制器
    struct device       dev;
    int                 init_irq;
    int                 irq;
    struct list_head    detected;
    i2c_slave_cb_t      slave_cb;
};
```

**核心操作：**
```c
i2c_master_send(client, buf, len);    // 主写
i2c_master_recv(client, buf, len);    // 主读
i2c_smbus_read_byte_data(client, reg);   // SMBus 读寄存器
i2c_smbus_write_byte_data(client, reg, val);  // SMBus 写寄存器
i2c_transfer(adapter, msgs, num);     // 多消息事务
```

### §2.3 SPI

```c
struct spi_driver {
    const struct spi_device_id *id_table;
    int  (*probe)   (struct spi_device *);
    int  (*remove)  (struct spi_device *);
    void (*shutdown)(struct spi_device *);
    struct device_driver driver;
};

struct spi_device {
    struct device       dev;
    struct spi_controller *controller;
    u32                 max_speed_hz;
    u8                  chip_select;     // CS 编号
    u8                  bits_per_word;
    bool                rt;
    u32                 mode;            // CPOL / CPHA / LSB_FIRST
    int                 irq;
    void               *controller_state;
    void               *controller_data;
    char                modalias[SPI_NAME_SIZE];
    int                 cs_gpio;         // 可用 GPIO 模拟 CS
    /* ... */
};

int spi_sync(struct spi_device *spi, struct spi_message *message);
int spi_write(struct spi_device *spi, const void *buf, size_t len);
int spi_read(struct spi_device *spi, void *buf, size_t len);
```

### §2.4 PCI / PCIe

PCI 是最复杂的总线之一 —— 有自动配置空间（256B + 4KB extended）+ Capability 链表 + MSI/MSI-X 中断。

```c
struct pci_driver {
    struct list_head    node;
    const char         *name;
    const struct pci_device_id *id_table;
    int  (*probe)       (struct pci_dev *, const struct pci_device_id *);
    void (*remove)      (struct pci_dev *);
    int  (*suspend)     (struct pci_dev *, pm_message_t);
    int  (*resume)      (struct pci_dev *);
    void (*shutdown)    (struct pci_dev *);
    int  (*sriov_configure)(struct pci_dev *, int num_vfs);
    int  (*sriov_set_msix_vec_count)(struct pci_dev *, int);
    u32  (*sriov_get_vf_total_msix)(struct pci_dev *);
    const struct pci_error_handlers *err_handler;
    const struct attribute_group **groups;
    const struct attribute_group **dev_groups;
    struct device_driver driver;
    struct pci_dynids   dynids;
    bool                driver_managed_dma;
};

struct pci_device_id {
    __u32 vendor, device;            // VID:PID
    __u32 subvendor, subdevice;
    __u32 class, class_mask;         // PCI class（如 NETWORK_ETHERNET）
    kernel_ulong_t driver_data;
};

#define PCI_DEVICE(vend, dev) \
    .vendor = (vend), .device = (dev), \
    .subvendor = PCI_ANY_ID, .subdevice = PCI_ANY_ID
```

**典型 probe：**
```c
static int my_pci_probe(struct pci_dev *pdev, const struct pci_device_id *id) {
    int err;
    err = pci_enable_device(pdev);              // 上电 + 启用 BAR
    err = pci_request_regions(pdev, "mydrv");   // 锁定 BAR 地址段
    void __iomem *base = pci_iomap(pdev, 0, 0); // 映射 BAR0
    pci_set_master(pdev);                       // 启用 bus mastering（DMA）
    err = pci_alloc_irq_vectors(pdev, 1, 4, PCI_IRQ_MSIX | PCI_IRQ_MSI | PCI_IRQ_LEGACY);
    err = request_irq(pci_irq_vector(pdev, 0), my_isr, IRQF_SHARED, "mydrv", priv);
    /* ... */
    return 0;
}
```

### §2.5 USB

```c
struct usb_driver {
    const char *name;
    int  (*probe)         (struct usb_interface *, const struct usb_device_id *);
    void (*disconnect)    (struct usb_interface *);
    int  (*unlocked_ioctl)(struct usb_interface *, unsigned int, void *);
    int  (*suspend)       (struct usb_interface *, pm_message_t);
    int  (*resume)        (struct usb_interface *);
    int  (*reset_resume)  (struct usb_interface *);
    int  (*pre_reset)     (struct usb_interface *);
    int  (*post_reset)    (struct usb_interface *);
    const struct usb_device_id *id_table;
    const struct attribute_group **dev_groups;
    struct usb_dynids dynids;
    struct usbdrv_wrap drvwrap;
    unsigned int no_dynamic_id:1;
    unsigned int supports_autosuspend:1;
    unsigned int disable_hub_initiated_lpm:1;
    unsigned int soft_unbind:1;
};

struct usb_device_id {
    __u16 match_flags;        // USB_DEVICE_ID_MATCH_*
    __u16 idVendor, idProduct;
    __u16 bcdDevice_lo, bcdDevice_hi;
    __u8  bDeviceClass, bDeviceSubClass, bDeviceProtocol;
    __u8  bInterfaceClass, bInterfaceSubClass, bInterfaceProtocol;
    __u8  bInterfaceNumber;
    kernel_ulong_t driver_data;
};
```

**USB 设备模型分层：**
```
struct usb_device              ← 一个 USB 设备（可能 composite）
├── usb_config_descriptor      ← 设备的配置描述符
│   └── usb_interface[]        ← 多接口（如 USB 网卡 + 串口 composite）
│       └── usb_endpoint[]     ← 每接口多端点（IN/OUT, ctrl/bulk/int/iso）
```

**驱动绑定到 interface（不是 device），所以同一 USB 设备可被多驱动认领。**

### §2.6 MMC / SD / eMMC

```c
struct mmc_driver {
    struct device_driver drv;
    int  (*probe)(struct mmc_card *);
    void (*remove)(struct mmc_card *);
    int  (*shutdown)(struct mmc_card *);
    int  (*suspend)(struct mmc_card *);
    int  (*resume)(struct mmc_card *);
};
```

MMC 子系统典型架构：
- **mmc_host** —— 控制器（一个 SoC 可能多个）
- **mmc_card** —— 卡片（每 host 通常 1 个 SD / eMMC，SDIO 可多 card）
- **block driver** —— `drivers/mmc/core/block.c` 把 mmc_card 暴露为 `/dev/mmcblk*`

### §2.7 GPIO / pinctrl

**GPIO subsystem：**
```c
struct gpio_chip {
    const char *label;
    struct gpio_device *gpiodev;
    struct device *parent;
    struct module *owner;
    int  (*request)(struct gpio_chip *gc, unsigned offset);
    void (*free)   (struct gpio_chip *gc, unsigned offset);
    int  (*get_direction)(struct gpio_chip *gc, unsigned offset);
    int  (*direction_input)(struct gpio_chip *gc, unsigned offset);
    int  (*direction_output)(struct gpio_chip *gc, unsigned offset, int value);
    int  (*get)    (struct gpio_chip *gc, unsigned offset);
    int  (*get_multiple)(struct gpio_chip *gc, unsigned long *mask, unsigned long *bits);
    void (*set)    (struct gpio_chip *gc, unsigned offset, int value);
    void (*set_multiple)(struct gpio_chip *gc, unsigned long *mask, unsigned long *bits);
    int  (*set_config)(struct gpio_chip *gc, unsigned offset, unsigned long config);
    int  (*to_irq) (struct gpio_chip *gc, unsigned offset);
    /* ... */
    int base, ngpio;
    const char *const *names;
};
```

**pinctrl** 配置 SoC 引脚的功能复用（pin mux）+ 电气特性（pull / drive strength）：
```c
struct pinctrl_ops {
    int (*get_groups_count)(struct pinctrl_dev *);
    const char *(*get_group_name)(struct pinctrl_dev *, unsigned);
    int (*get_group_pins)(struct pinctrl_dev *, unsigned, const unsigned **, unsigned *);
    void (*pin_dbg_show)(struct pinctrl_dev *, struct seq_file *, unsigned);
    int (*dt_node_to_map)(struct pinctrl_dev *, struct device_node *, struct pinctrl_map **, unsigned *);
    void (*dt_free_map)(struct pinctrl_dev *, struct pinctrl_map *, unsigned);
};
```

### §2.8 clk / regulator / phy

**clk subsystem** —— 时钟树管理：
```c
struct clk_hw_init_data {
    const char *name;
    const struct clk_ops *ops;
    const char * const *parent_names;
    u8 num_parents;
    unsigned long flags;
};

struct clk_ops {
    int  (*prepare)(struct clk_hw *hw);
    void (*unprepare)(struct clk_hw *hw);
    int  (*enable)(struct clk_hw *hw);
    void (*disable)(struct clk_hw *hw);
    int  (*is_enabled)(struct clk_hw *hw);
    unsigned long (*recalc_rate)(struct clk_hw *hw, unsigned long parent_rate);
    long (*round_rate)(struct clk_hw *hw, unsigned long, unsigned long *);
    int  (*set_rate)(struct clk_hw *hw, unsigned long, unsigned long parent_rate);
    int  (*set_parent)(struct clk_hw *hw, u8 index);
    u8   (*get_parent)(struct clk_hw *hw);
    /* ... */
};
```

**regulator subsystem** —— 电压调节器（电源管理）：
```c
struct regulator_ops {
    int (*list_voltage)(struct regulator_dev *, unsigned selector);
    int (*set_voltage)(struct regulator_dev *, int min_uV, int max_uV, unsigned *selector);
    int (*get_voltage)(struct regulator_dev *);
    int (*enable)(struct regulator_dev *);
    int (*disable)(struct regulator_dev *);
    int (*is_enabled)(struct regulator_dev *);
    int (*set_current_limit)(struct regulator_dev *, int min_uA, int max_uA);
    int (*set_mode)(struct regulator_dev *, unsigned int mode);
    /* ... */
};
```

**phy subsystem** —— 物理层接口（USB/PCIe/SATA/Ethernet 物理层 IP 核）：
```c
struct phy_ops {
    int (*init)(struct phy *);
    int (*exit)(struct phy *);
    int (*power_on)(struct phy *);
    int (*power_off)(struct phy *);
    int (*set_mode)(struct phy *, enum phy_mode mode, int submode);
    int (*reset)(struct phy *);
    /* ... */
};
```

### §2.9 dma engine

DMA 引擎驱动框架（`drivers/dma/`）：
```c
struct dma_device {
    unsigned int chancnt;
    struct list_head channels;
    dma_cap_mask_t cap_mask;
    enum dma_residue_granularity residue_granularity;
    struct dma_chan *(*device_alloc_chan_resources)(struct dma_chan *);
    void (*device_free_chan_resources)(struct dma_chan *);
    struct dma_async_tx_descriptor *(*device_prep_dma_memcpy)(...);
    struct dma_async_tx_descriptor *(*device_prep_dma_memset)(...);
    struct dma_async_tx_descriptor *(*device_prep_slave_sg)(...);
    struct dma_async_tx_descriptor *(*device_prep_dma_cyclic)(...);
    int (*device_config)(struct dma_chan *, struct dma_slave_config *);
    int (*device_pause)(struct dma_chan *);
    int (*device_resume)(struct dma_chan *);
    int (*device_terminate_all)(struct dma_chan *);
    enum dma_status (*device_tx_status)(struct dma_chan *, dma_cookie_t, struct dma_tx_state *);
    void (*device_issue_pending)(struct dma_chan *);
    /* ... */
};
```

### §2.10 iommu

IOMMU 子系统（`drivers/iommu/`）—— 设备 DMA 地址转换 + 隔离：
```c
struct iommu_ops {
    bool (*capable)(enum iommu_cap);
    struct iommu_domain *(*domain_alloc)(unsigned iommu_domain_type);
    void (*domain_free)(struct iommu_domain *);
    int  (*attach_dev)(struct iommu_domain *, struct device *);
    void (*detach_dev)(struct iommu_domain *, struct device *);
    int  (*map)  (struct iommu_domain *, unsigned long, phys_addr_t, size_t, int);
    size_t (*unmap)(struct iommu_domain *, unsigned long, size_t, struct iommu_iotlb_gather *);
    void (*flush_iotlb_all)(struct iommu_domain *);
    void (*iotlb_sync)(struct iommu_domain *, struct iommu_iotlb_gather *);
    phys_addr_t (*iova_to_phys)(struct iommu_domain *, dma_addr_t);
    struct iommu_device *(*probe_device)(struct device *);
    void (*release_device)(struct device *);
    /* SVA / SVM / DMA API integration ... */
};
```

主要 IOMMU 实现：Intel VT-d / AMD-Vi / ARM SMMU / RISC-V IOMMU（Andes/SiFive）/ Apple DART。

### §2.11 子系统横向对比

| 子系统 | 设备发现 | 匹配方式 | 资源数 | 中断模式 | 嵌入式典型 |
|--------|---------|---------|--------|---------|-----------|
| platform | DT/ACPI 静态 | of_match_table compatible | 多（MMIO+IRQ+DMA+...）| 共享 IRQ / 平台特定 | UART / 串口控制器 / SoC 外设 |
| PCI | 配置空间扫描自动 | VID:PID | 6 BAR | INTx / MSI / MSI-X | NVMe / GPU / 网卡 |
| USB | hub 枚举自动 | VID:PID + class | endpoints | 端点 transfer | U 盘 / 鼠标 / WiFi 模块 |
| I2C | DT 静态 | of_match_table | 1 (slave addr) | 主控中断 | EEPROM / RTC / 传感器 |
| SPI | DT 静态 | of_match_table | 1 (CS) | 主控中断 | Flash / TFT 屏 |
| MMC | host 检测自动 | 卡 CID | 1 (host) | host 中断 | SD 卡 / eMMC |
| MDIO | 主控扫描 | PHY ID | 1 (PHY addr) | 共享中断 | Ethernet PHY |
| virtio | bus 扫描 | virtio device id | virtqueue | 配置中断 | VM guest 设备 |

---

## §3 DT 在驱动中的角色

> **配套：** [03-04 DTS/DTB/FDT/FIT 语法格式 API 参考手册](03-04-dts-dtb-fdt-syntax-reference.md) 已详细讲 DT 语法 + libfdt API；本节聚焦"驱动如何用 DT 数据"。

### §3.1 of_match_table / compatible

DT 节点 → driver 匹配的核心 = `compatible` 字符串：

```dts
// arch/arm64/boot/dts/rockchip/rk3588.dtsi
uart0: serial@feb50000 {
    compatible = "rockchip,rk3588-uart", "snps,dw-apb-uart";
    reg = <0x0 0xfeb50000 0x0 0x100>;
    interrupts = <GIC_SPI 331 IRQ_TYPE_LEVEL_HIGH>;
    clocks = <&cru SCLK_UART0>, <&cru PCLK_UART0>;
    clock-names = "baudclk", "apb_pclk";
    reg-shift = <2>;
    reg-io-width = <4>;
    status = "disabled";
};
```

驱动声明能匹配的 compatible 列表：
```c
static const struct of_device_id dw_uart_match[] = {
    { .compatible = "snps,dw-apb-uart" },     // 通用兼容
    { .compatible = "rockchip,rk3588-uart" }, // RK3588 特化
    {}
};
MODULE_DEVICE_TABLE(of, dw_uart_match);
```

**匹配规则：** DT 节点 compatible 列表**从前往后**找第一个能 match 的 driver。所以 `"rockchip,rk3588-uart", "snps,dw-apb-uart"` 这种"特化在前，通用在后"是惯例。

### §3.2 of_device_get_match_data

driver 要根据具体 compatible 取不同行为时：
```c
static const struct dw_uart_data v1_data = { .has_dma = false };
static const struct dw_uart_data v2_data = { .has_dma = true  };

static const struct of_device_id dw_uart_match[] = {
    { .compatible = "vendor,uart-v1", .data = &v1_data },
    { .compatible = "vendor,uart-v2", .data = &v2_data },
    {}
};

static int probe(struct platform_device *pdev) {
    const struct dw_uart_data *data = of_device_get_match_data(&pdev->dev);
    if (data->has_dma) { /* enable DMA */ }
    /* ... */
}
```

### §3.3 of_xlate（资源解析回调）

某些资源（如 IRQ / GPIO / clk）有跨节点引用语义：
```dts
gpio0: gpio@feb20000 {
    #gpio-cells = <2>;            // 引用时需 2 个 cell
    /* ... */
};

led {
    gpios = <&gpio0 5 GPIO_ACTIVE_HIGH>;  // 引用 gpio0 第 5 引脚，高有效
};
```

`of_xlate` 是 GPIO 控制器把 `<5 GPIO_ACTIVE_HIGH>` 这两个 cell 翻译为驱动内部 GPIO descriptor 的回调。

### §3.4 livetree（运行时 DT）

Linux 5.x 后引入 livetree —— 把 DTB 解析为运行时可修改的内存树：
```c
struct device_node {
    const char *name;
    const char *type;
    phandle phandle;
    const char *full_name;
    struct fwnode_handle fwnode;
    struct property *properties;
    struct property *deadprops;
    struct device_node *parent, *child, *sibling;
    struct kobject kobj;
    unsigned long _flags;
    void *data;
};
```

**livetree 用途：** overlay 加载 / 运行时改属性（如 cpu freq table）/ 热插拔 FPGA 子树。

### §3.5 phandle 引用

phandle = 32-bit 唯一 ID，跨节点引用：
```dts
crtc: display-controller@feb00000 {
    /* ... */
    ports {
        port@0 {
            reg = <0>;
            crtc_out: endpoint {
                remote-endpoint = <&hdmi_in>;  // phandle 引用
            };
        };
    };
};

hdmi: hdmi@feb40000 {
    /* ... */
    ports {
        port@0 {
            reg = <0>;
            hdmi_in: endpoint {
                remote-endpoint = <&crtc_out>;  // 反向引用
            };
        };
    };
};
```

DRM 子系统通过 OF graph 这种 phandle 双向引用构建 display pipeline。

### §3.6 overlay

DT overlay = 运行时打补丁的 DT：
```dts
/dts-v1/;
/plugin/;

&i2c1 {
    #address-cells = <1>;
    #size-cells = <0>;
    eeprom@50 {
        compatible = "atmel,24c02";
        reg = <0x50>;
        pagesize = <16>;
    };
};
```

编译为 `.dtbo`，运行时 `mkdir /sys/kernel/config/device-tree/overlays/foo` + `cat foo.dtbo > .../foo/dtbo` 加载。

**用途：**
- 树莓派 HAT 子卡热插拔
- FPGA bitstream 加载后启用对应外设
- Beaglebone capemgr

---

## §4 module 加载机制

### §4.1 .ko ELF 重定位文件结构

`.ko` 是 ELF 格式的 **relocatable object file**（不是可执行）—— 内核加载时做重定位。

`readelf -h drivers/net/e1000/e1000.ko` 典型输出：
```
ELF Header:
  Magic:   7f 45 4c 46 02 01 01 00 ...
  Class:                             ELF64
  Type:                              REL (Relocatable file)
  Machine:                           Advanced Micro Devices X86-64
  Version:                           0x1
  Entry point address:               0x0
  Start of program headers:          0
  Number of program headers:         0
  Start of section headers:          ...
  Section header string table index: 1
```

**关键 sections：**
| section | 用途 |
|---------|------|
| `.text` / `.rodata` / `.data` / `.bss` | 标准代码/数据 |
| `.modinfo` | 模块元信息（license / author / depends / version）|
| `.init.text` / `.init.data` | init 阶段代码（加载后释放）|
| `.exit.text` / `.exit.data` | unload 阶段代码 |
| `__versions` | 符号版本（modversions / vermagic）|
| `.gnu.linkonce.this_module` | 模块自身的 `struct module` 对象 |
| `__ksymtab*` / `__kcrctab*` / `__kstrtab*` | 此模块导出的符号表 |
| `.rel.text` / `.rel.data` | 重定位条目 |

### §4.2 EXPORT_SYMBOL / EXPORT_SYMBOL_GPL / unused

模块要用其它模块/内核的函数 → 后者必须 `EXPORT_SYMBOL`：

```c
// drivers/usb/core/usb.c
struct usb_device *usb_get_dev(struct usb_device *dev) { ... }
EXPORT_SYMBOL_GPL(usb_get_dev);    // 仅 GPL 兼容模块可用
```

**4 种导出：**
| 宏 | 含义 | License 限制 |
|----|------|-------------|
| `EXPORT_SYMBOL(sym)` | 任何模块可用 | 不限 |
| `EXPORT_SYMBOL_GPL(sym)` | 仅 GPL 兼容模块 | 模块需声明 GPL/GPL v2 等 |
| `EXPORT_SYMBOL_NS(sym, ns)` | 特定命名空间模块 | 模块需 `MODULE_IMPORT_NS(ns)` |
| `EXPORT_SYMBOL_GPL_FOR_MODULES(sym, mods)` | 白名单内特定模块 | 实验性，6.x |

**License 检查发生在加载时**：内核读模块的 `MODULE_LICENSE("GPL v2")` 与导出符号的限制匹配。

### §4.3 insmod / modprobe / rmmod 工作原理

**insmod**（`kmod` 工具）：
```
insmod mymod.ko arg1=v1 arg2=v2
   ↓ syscall init_module(map, len, args)
   ↓
内核：
  1. copy_from_user 模块 ELF + 参数串
  2. ELF 解析 + section 复制到内核空间
  3. 重定位（处理 .rel.* 条目，把符号解析为内核地址）
  4. 解析 .modinfo + 检查 license
  5. 注册到 modules 链表
  6. 调 init_module（即 mod->init 函数指针）
  7. 释放 .init.* sections
```

**modprobe** 比 insmod 多一步：递归处理 `depends:` 依赖（modinfo 中 `depends` 字段），自动加载依赖模块。

**rmmod**：
```
rmmod mymod
   ↓ syscall delete_module(name, flags)
   ↓
内核：
  1. 检查 ref count（其它模块在用 / 设备在用 → -EBUSY）
  2. 调 mod->exit
  3. 注销符号表
  4. 释放 module 内存
```

### §4.4 init_module / finit_module syscall

```c
// kernel/module/main.c (linux 6.x)
SYSCALL_DEFINE3(init_module, void __user *, umod, unsigned long, len, const char __user *, uargs)
SYSCALL_DEFINE3(finit_module, int, fd, const char __user *, uargs, int, flags)
```

`finit_module` 比 `init_module` 多一个 fd 参数（直接读取已 open 的 .ko fd），效率更高 + 方便 fanotify 跟踪。

### §4.5 DKMS 动态内核模块系统

**DKMS（Dynamic Kernel Module Support）** 是 RHEL/Debian/Ubuntu 用户态工具，自动重编 out-of-tree 模块当内核升级时：

```
/usr/src/<modname>-<version>/    # 模块源码 + dkms.conf
/var/lib/dkms/<modname>/<version>/<kernel>/<arch>/   # 编译产物
```

`dkms.conf`：
```
PACKAGE_NAME="nvidia"
PACKAGE_VERSION="535.183.01"
BUILT_MODULE_NAME[0]="nvidia"
DEST_MODULE_LOCATION[0]="/kernel/drivers/video"
AUTOINSTALL="yes"
```

**用户视角：** 装 NVIDIA / VirtualBox / ZFS 等 out-of-tree 驱动后，apt upgrade 自动新内核重编。

### §4.6 CONFIG_MODULE_SIG 签名

```
CONFIG_MODULE_SIG=y
CONFIG_MODULE_SIG_FORCE=y       # 必需（开则拒载未签名模块）
CONFIG_MODULE_SIG_ALL=y         # 编译时自动签名
CONFIG_MODULE_SIG_KEY="certs/signing_key.pem"
CONFIG_MODULE_SIG_HASH="sha256"
```

**签名工具：** `scripts/sign-file SHA256 priv.pem pub.crt mod.ko`

模块末尾追加 PKCS#7 签名 + 魔数 `~Module signature appended~\n`。

**Secure Boot + MOK：** 自定义模块要被 Secure Boot 接受，需用户态 mokutil 注册公钥。

---

## §5 Rust for Linux 驱动 ABI

### §5.1 kernel crate

Rust for Linux 在 `rust/kernel/` 提供安全 wrapper：

```rust
use kernel::prelude::*;
use kernel::{c_str, file::File, miscdev, module};

module! {
    type: HelloModule,
    name: "hello_world",
    author: "Rustacean",
    description: "A simple Rust kernel module",
    license: "GPL",
}

struct HelloModule {
    _miscdev: miscdev::Registration<HelloModule>,
}

impl kernel::Module for HelloModule {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("Hello from Rust!\n");
        let miscdev = miscdev::Registration::new_pinned(c_str!("hello"), ())?;
        Ok(Self { _miscdev: miscdev })
    }
}

impl Drop for HelloModule {
    fn drop(&mut self) {
        pr_info!("Bye from Rust!\n");
    }
}
```

### §5.2 module! 宏

`module!` 宏展开成 C 端可识别的 `__this_module` + init/exit 入口 + .modinfo 字段。本质是把 Rust 的"创建 trait 对象 + Drop"机制桥接到 Linux 模块生命周期。

### §5.3 driver trait

每个总线在 `rust/kernel/<bus>/` 提供 driver trait：

```rust
// rust/kernel/platform.rs
pub trait Driver {
    type IdInfo: 'static = ();
    const ID_TABLE: IdTable<Self::IdInfo>;
    fn probe(pdev: &mut Device, id_info: Option<&Self::IdInfo>) -> Result<Pin<Box<Self>>>;
}
```

驱动实现 trait + 用 `module_platform_driver!` 宏注册。

### §5.4 Drop = remove

Rust 驱动的"remove" 自动通过 `Drop` 实现 —— 设备 unbind 时 Box drop，回调 Drop trait 释放资源。

**对比 C：** C 必须手写 `remove` 函数；Rust 用 RAII 把"释放资源"内嵌进类型系统，编译期保证不漏释放。

### §5.5 进度（2026-05）

- 主线已合并的 Rust 驱动模块：少量（NVMe 部分 / DRM 部分实验 / Asahi M1 GPU 用户态 driver / ehci-rust 等）
- 主线已加 Rust 子系统 binding：platform / pci / clk / gpio / file / chrdev / miscdev / sync 等

---

## §6 `.sys` vs `.ko` 横向对比（17 维度）

>
> **结论先行：** 17 维度差异的根因 = **ABI 稳定性的设计取舍**。Windows 用 KMDF/UMDF 提供稳定 ABI 抽象层，Linux 故意不稳定 KABI 以快速迭代。

### §6.1 文件格式

| | `.sys`（Windows）| `.ko`（Linux）|
|--|------------------|---------------|
| ELF/PE | **PE/COFF**（Portable Executable / Common Object File Format）—— 同 .exe / .dll 同根 | **ELF**（Executable and Linkable Format）—— 同 .so / .o 同根 |
| 文件类型 | "executable image"（已重定位，加载时 base address relocation 即可）| "relocatable object"（未完全重定位，加载时需做 R_X86_64_PC32 等重定位条目）|
| 入口约定 | 通过 PE header `AddressOfEntryPoint` 字段 | 通过 ELF symbol `init_module` |
| 元数据位置 | PE 资源段 + INF 配套文件（外部）| ELF section `.modinfo`（内嵌）|
| 工具链 | MSVC link.exe / lld-link | GNU ld / lld（通过 kbuild Makefile）|
| 验证 | `dumpbin.exe /HEADERS x.sys` | `readelf -h x.ko` / `modinfo x.ko` |

**设计影响：** PE 把元数据放外部（.inf）让安装/更新更灵活；ELF 内嵌 .modinfo 让单文件即可携带全部信息。

### §6.2 加载机制

**Windows 加载链路：**
```
用户：sc create my-drv binPath= "C:\drivers\my.sys" type= kernel
      ↓
Service Control Manager (services.exe)
      ↓ 写注册表 HKLM\SYSTEM\CurrentControlSet\Services\my-drv
I/O Manager (ntoskrnl.exe → IoLoadDriver)
      ↓ 加载 .sys 到内核空间（NX/RO 段属性）
      ↓ 调 DriverEntry(DriverObject, RegistryPath)
DriverEntry 注册 IRP MajorFunction[] → I/O Manager 索引
      ↓ PnP Manager 触发 IRP_MN_START_DEVICE
DriverObject->MajorFunction[IRP_MJ_PNP](DeviceObject, Irp)
```

**Linux 加载链路：**
```
用户：modprobe my-drv arg=v
      ↓
modprobe（用户态）
      ↓ 解析 modules.dep（depmod 生成）+ 递归加载依赖
      ↓ open(my-drv.ko) + finit_module(fd, args, flags)
finit_module syscall (kernel/module/main.c)
      ↓ ELF 解析 + section 复制到 vmalloc 空间
      ↓ 重定位（处理 .rela.text / .rela.data 等）
      ↓ 解析 __versions 段（CRC 校验符号兼容）
      ↓ 注册到 modules 全局链表
      ↓ 调 init_module (即 mod->init = struct module 函数指针)
init_module 通过 *_register 注册到 bus / class / netdev / chrdev 等
```

**核心差异：**
- Windows 加载是 **service-oriented**（SCM + 注册表驱动启动）
- Linux 加载是 **module-oriented**（modprobe + .modinfo depends 递归）
- Windows 的设备/驱动**绑定**通过 INF 文件预声明匹配规则；Linux 的绑定通过 driver 内嵌 of_match_table / pci_device_id 等

### §6.3 驱动模型层级（WDM/KMDF/UMDF vs Linux DM）

**Windows 驱动模型 4 代演化：**
```
NT 4.0 (1996)     →  Plug-and-Play DDK              ← legacy
Win 2000          →  WDM (Windows Driver Model)     ← 老 PnP 模型，写驱动直接面对 IRP
Win XP/Vista      →  KMDF (Kernel-Mode Driver Framework) ← 在 WDM 上加抽象层
                  →  UMDF (User-Mode Driver Framework)  ← 用户态驱动
Win 8+            →  WDF (Windows Driver Framework) = KMDF + UMDF 统一
                  →  WDF 进一步成熟 + 强化 PnP/电源/IO 队列抽象
Win 10/11         →  WDF v1.x（稳定生产）
```

**Linux DM（单一模型，内嵌内核）：**
- 没有"framework on top of framework"分层 —— 驱动直接用 `struct platform_driver` / `struct pci_driver` 等
- 抽象更薄但暴露更直接 → driver 必须懂总线细节、IRP 排队、PnP 协议（Linux 不叫 IRP，但等价）

**KMDF 例子（Windows）：**
```c
NTSTATUS DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath) {
    WDF_DRIVER_CONFIG config;
    WDF_DRIVER_CONFIG_INIT(&config, EvtDeviceAdd);
    return WdfDriverCreate(DriverObject, RegistryPath, WDF_NO_OBJECT_ATTRIBUTES, &config, WDF_NO_HANDLE);
}

NTSTATUS EvtDeviceAdd(WDFDRIVER Driver, PWDFDEVICE_INIT DeviceInit) {
    /* KMDF 自动处理 PnP IRP / PowerIRP / WMI */
    /* 驱动只写设备特定逻辑 */
}
```

**Linux 等价：**
```c
static struct platform_driver my_drv = {
    .probe  = my_probe,
    .remove = my_remove,
    .driver = { .name = "my-drv", .of_match_table = my_match },
};
module_platform_driver(my_drv);
```

KMDF 多一层 framework，**驱动开发者写得少**；Linux DM 没框架但**机制更直白**。哪个好？取决于哲学 —— Microsoft 用稳定 framework 抽象屏蔽内核演化（用户驱动一份代码跑 20 年）；Linux 期望驱动跟随 mainline 演化（每个 LTS 都可能要改）。

### §6.4 入口点

| | Windows `.sys` | Linux `.ko` |
|--|---------------|-------------|
| 入口函数 | `NTSTATUS DriverEntry(PDRIVER_OBJECT, PUNICODE_STRING)` | `int init_module(void)` 或 `module_init(fn)` 注册 |
| 出口函数 | `DriverObject->DriverUnload = MyUnload;`（在 DriverEntry 设置）| `void cleanup_module(void)` 或 `module_exit(fn)` 注册 |
| 多个驱动一文件 | 一个 .sys 一个 DriverEntry | 一个 .ko 一个 init_module（但可注册多个 driver / device）|
| 退出原因传递 | DriverObject 字段 | `__exit` 标记区分 unload 和静态编译 |

### §6.5 设备对象

**Windows DEVICE_OBJECT + DRIVER_OBJECT：**
```c
typedef struct _DEVICE_OBJECT {
    CSHORT Type;
    USHORT Size;
    LONG ReferenceCount;
    struct _DRIVER_OBJECT *DriverObject;     // 创建此设备的驱动
    struct _DEVICE_OBJECT *NextDevice;
    struct _DEVICE_OBJECT *AttachedDevice;   // 上层 filter device（驱动栈）
    PIRP CurrentIrp;
    PIO_TIMER Timer;
    ULONG Flags, Characteristics;
    struct _VPB *Vpb;                        // Volume Parameter Block
    PVOID DeviceExtension;                   // 驱动私有数据
    DEVICE_TYPE DeviceType;
    /* 60+ 字段 ... */
} DEVICE_OBJECT;
```

**Linux struct device：**（已在 §1.3 列）轻得多

**关键差异：**
- Windows 引入 **driver stack**（一个物理设备多个 DEVICE_OBJECT 串联）—— filter driver / bus driver / function driver 三层
- Linux 没有 stack，一个设备绑一个 driver，filter 通过其他机制（如 netfilter / DM-mapper）实现

### §6.6 请求模型（IRP vs file_operations）

**Windows IRP（I/O Request Packet）：**
```c
typedef struct _IRP {
    CSHORT Type;
    USHORT Size;
    PMDL MdlAddress;                  // 缓冲区描述（MDL = Memory Descriptor List）
    ULONG Flags;
    union { ... } AssociatedIrp;
    LIST_ENTRY ThreadListEntry;
    IO_STATUS_BLOCK IoStatus;         // 完成状态
    KPROCESSOR_MODE RequestorMode;    // 来自 user 还是 kernel
    BOOLEAN PendingReturned;
    CHAR StackCount;
    CHAR CurrentLocation;
    UCHAR ApcEnvironment;
    UCHAR AllocationFlags;
    PIO_STATUS_BLOCK UserIosb;
    PKEVENT UserEvent;
    union { ... } Overlay;
    union { volatile LONG_PTR CancelRoutine; ... } Tail;
} IRP;
```

驱动看 IRP 里的 **MajorFunction**（IRP_MJ_CREATE / READ / WRITE / DEVICE_CONTROL / PNP / POWER / CLOSE / CLEANUP / ... 共 28 种），分发到对应 handler。

**Linux file_operations：**
```c
struct file_operations {
    struct module *owner;
    loff_t (*llseek) (struct file *, loff_t, int);
    ssize_t (*read) (struct file *, char __user *, size_t, loff_t *);
    ssize_t (*write) (struct file *, const char __user *, size_t, loff_t *);
    ssize_t (*read_iter) (struct kiocb *, struct iov_iter *);
    ssize_t (*write_iter) (struct kiocb *, struct iov_iter *);
    int (*iopoll)(struct kiocb *kiocb, struct io_comp_batch *, unsigned int);
    int (*iterate) (struct file *, struct dir_context *);
    __poll_t (*poll) (struct file *, struct poll_table_struct *);
    long (*unlocked_ioctl) (struct file *, unsigned int, unsigned long);
    long (*compat_ioctl) (struct file *, unsigned int, unsigned long);
    int (*mmap) (struct file *, struct vm_area_struct *);
    int (*open) (struct inode *, struct file *);
    int (*flush) (struct file *, fl_owner_t id);
    int (*release) (struct inode *, struct file *);
    int (*fsync) (struct file *, loff_t, loff_t, int datasync);
    /* 30+ ops */
};
```

**核心差异：**
- IRP 是 **packet 模型**（请求是一个对象，串联在驱动栈中流转，每层加自己的处理）
- Linux file_operations 是 **callback 模型**（VFS 直接调驱动函数指针，无 packet 概念）
- IRP 支持**异步 + 取消**（PendingReturned + Cancel callback）；Linux 早期同步，io_uring 后才有 packet 风模型

### §6.7 PnP / 电源

| | Windows | Linux |
|--|---------|-------|
| PnP 触发源 | PnP Manager 发 IRP_MJ_PNP | bus->probe / bus->remove |
| StartDevice | IRP_MN_START_DEVICE | driver->probe |
| RemoveDevice | IRP_MN_REMOVE_DEVICE | driver->remove |
| QueryRemove（拒绝拔出）| IRP_MN_QUERY_REMOVE_DEVICE → 驱动可返回 STATUS_UNSUCCESSFUL | 没有等价机制（Linux 不支持驱动拒绝热拔出）|
| 资源协商 | IRP_MN_QUERY_RESOURCE_REQUIREMENTS / FILTER_RESOURCE_REQUIREMENTS | platform_get_resource / pci_resource_start |
| 电源状态 | 7 个 D-state（D0/D1/D2/D3hot/D3cold + 设备特定）| 4 个：D0/D1/D2/D3 + runtime PM |
| 电源策略权 | Windows 默认 OS 决策 | Linux 默认 driver 决策（需 driver 实现 runtime PM ops）|

**核心差异：** Windows PnP 是 OS 主导（驱动响应请求）；Linux PnP 是 driver 主导（driver 注册 ops，OS 触发回调）。

### §6.8 中断模型

**Windows IRQL（Interrupt Request Level）：**
```
IRQL = 31     HIGH_LEVEL (machine check)
IRQL = 30     POWER_LEVEL
IRQL = 29     IPI_LEVEL (inter-processor interrupt)
IRQL = 28     CLOCK_LEVEL (timer tick)
IRQL = 4-27   DIRQL (device IRQs)
IRQL = 2      DISPATCH_LEVEL (DPC, scheduler)
IRQL = 1      APC_LEVEL (asynchronous procedure calls)
IRQL = 0      PASSIVE_LEVEL (普通线程上下文)
```

驱动代码声明运行的 IRQL：
- **PASSIVE_LEVEL**（IRQL 0）—— 可以 sleep / paged memory / I/O
- **APC_LEVEL**（IRQL 1）—— 不能 sleep 但可以接收 APC
- **DISPATCH_LEVEL**（IRQL 2）—— 不能 sleep / 不能用 paged memory / 不能 I/O，但可调度 DPC
- **DIRQL**（IRQL 3-27）—— ISR（中断服务例程）运行级别，禁中断（同级及以下）

ISR 必须很短，把工作扔给 DPC（Deferred Procedure Call）在 DISPATCH_LEVEL 处理，再扔给 work item 在 PASSIVE_LEVEL 处理。

**Linux preempt_count + irq context：**
```
preempt_count > 0   = 不能 sleep
in_atomic()         = 在原子上下文（spinlock / hardirq / softirq）
in_interrupt()      = 在 hardirq 或 softirq 上下文
in_hardirq()        = 硬中断上下文（ISR 直接运行）
in_softirq()        = 软中断上下文（softirq / tasklet）
in_nmi()            = NMI 上下文（不可屏蔽中断）
```

驱动 ISR 通过 `request_irq()` 注册 → 在 hardirq 上下文运行，必须很短，复杂工作扔给 softirq / tasklet / threaded irq / workqueue。

**核心差异：**
- Windows IRQL 是**显式的层级**，驱动每行代码有明确 IRQL 要求
- Linux 是**隐式的上下文**，靠 `might_sleep()` / `BUG_ON(in_atomic())` 等运行时断言检查
- 概念等价：`PASSIVE_LEVEL ≈ process context`，`DISPATCH_LEVEL ≈ atomic/softirq`，`DIRQL ≈ hardirq`

### §6.9 内存

| | Windows | Linux |
|--|---------|-------|
| 非分页池 | `ExAllocatePoolWithTag(NonPagedPool, ...)` | `kmalloc(GFP_ATOMIC)` 或 `vmalloc()` |
| 分页池 | `ExAllocatePoolWithTag(PagedPool, ...)`（IRQL ≤ APC）| 不区分（Linux 内核栈/堆都不会被换出）|
| GFP flags | 没有等价（IRQL 已隐含）| GFP_KERNEL / GFP_ATOMIC / GFP_NOIO / GFP_NOFS |
| 页面分配 | `MmAllocatePagesForMdlEx` | `alloc_pages` / `__get_free_pages` |
| 物理连续 | NonPagedPoolNx 不保证连续 | `dma_alloc_coherent` 保证连续 |
| 用户态内存 | `ProbeForRead/Write` + try/except | `access_ok` + `copy_from_user` |

**关键差异：** Windows 显式声明"分页/非分页"；Linux 内核内存全部锁住，只用 GFP flags 区分**调用上下文**（atomic 不能阻塞，IO/FS 流程禁递归）。

### §6.10 锁

| 用途 | Windows | Linux |
|------|---------|-------|
| 自旋锁 | `KSPIN_LOCK` + `KeAcquireSpinLock` | `spinlock_t` + `spin_lock_irqsave` |
| 互斥锁 | `KMUTEX` + `KeWaitForSingleObject` | `struct mutex` + `mutex_lock` |
| 快速互斥 | `FAST_MUTEX` + `ExAcquireFastMutex` | （没有完全等价；接近 `mutex_trylock`）|
| 读写锁 | `ERESOURCE` + `ExAcquireResourceExclusiveLite` | `rwlock_t` / `struct rw_semaphore` |
| 信号量 | `KSEMAPHORE` | `struct semaphore` |
| RCU | 无原生（需 SDK 提供）| `rcu_read_lock` / `synchronize_rcu`（Linux 独有的高效读优先锁）|
| seqlock | 无 | `seqlock_t`（Linux 独有，多读单写，不阻塞）|

**关键差异：** Linux 有 **RCU + seqlock** 等高级锁，专为多核扩展性设计；Windows 锁体系较简单，但 KMDF 提供更高层 IO Queue 抽象屏蔽了锁。

### §6.11 同步原语

| 用途 | Windows | Linux |
|------|---------|-------|
| 事件通知 | `KEVENT` + `KeWaitForSingleObject` | `wait_queue_head_t` + `wait_event` |
| 一次完成通知 | （用 KEVENT 模拟）| `struct completion` + `complete()` |
| 多个等待 | `KeWaitForMultipleObjects` | （Linux 没有原生；用 wait_event 表达式）|
| 计时器 | `KTIMER` + `KeSetTimerEx` | `struct hrtimer` / `struct timer_list` |
| 工作延迟 | `WORK_QUEUE_ITEM` / `IO_WORKITEM` | `struct work_struct` + workqueue |
| 等待对象类型 | KEVENT / KMUTEX / KSEMAPHORE / KTIMER 都可被 KeWait... 等 | 各类型独立 API（比 Windows 碎片化）|

### §6.12 DMA

**Windows：**
```c
WdfDmaEnablerCreate(device, &profile, &attribs, &enabler);
WdfDmaTransactionCreate(enabler, &attribs, &transaction);
WdfDmaTransactionInitializeUsingRequest(transaction, request, EvtProgramDma, ...);
WdfDmaTransactionExecute(transaction, NULL);
```
KMDF 把 DMA 抽象为 Enabler / Transaction 两层，自动处理 DMA buffer 锁页、SG list 拆分、IOMMU bypass / 不 bypass。

**Linux DMA API：**
```c
dma_addr_t dma_addr;
void *cpu_addr = dma_alloc_coherent(dev, size, &dma_addr, GFP_KERNEL);  // 一致性内存（CPU/DMA 双视图）
/* 或 */
dma_addr = dma_map_single(dev, cpu_buf, size, DMA_TO_DEVICE);  // streaming DMA
/* ... DMA 完成 ... */
dma_unmap_single(dev, dma_addr, size, DMA_TO_DEVICE);
```

更细粒度（cpu_to_dma / sync_for_cpu 等），但驱动开发者要懂得手动 sync。

**关键差异：** KMDF DMA 框架更**全自动**；Linux DMA API 暴露更多细节（一致性 vs streaming / direction / SG list 显式构建）。

### §6.13 签名

**Windows：**
- KMCS（Kernel-Mode Code Signing）—— Win64 强制要求
- WHQL（Windows Hardware Quality Lab）—— 微软认证签名
- EV Code Signing Certificate（必需）
- 工具链：SignTool.exe + WDK 签名脚本

**Linux：**
- CONFIG_MODULE_SIG（默认 N，distros 多数 Y）
- 自签：`scripts/sign-file SHA256 priv.pem pub.crt mod.ko`
- 公钥嵌内核 `.builtin_trusted_keys` 段
- Secure Boot 链：MOK（Machine Owner Key）让用户态注册受信公钥

**关键差异：**
- Windows 签名是**强制 + 集中信任**（微软是根 CA）
- Linux 签名是**可选 + 用户控制信任锚**（distro/用户自己的 CA）

### §6.14 ABI 稳定性 ⭐

| | Windows .sys | Linux .ko |
|--|--------------|-----------|
| **ABI 政策** | 稳定（KMDF v1.x 25 年兼容）| 故意不稳定（KABI 主动改）|
| **跨大版本** | XP 驱动多数能在 Win10 跑 | 5.10 驱动几乎不能在 6.6 跑 |
| **跨小版本** | 通常稳定 | 多数稳定但偶发崩 |
| **modversions / CRC** | 否（PE 没有等价机制）| `Module.symvers` + `__versions` 段 + CRC32 校验 |
| **Out-of-tree 驱动可行性** | 高（NVIDIA 一份 .sys 跑 10 年）| 中（NVIDIA 维护数百个 KABI 兼容补丁）|
| **mainline-out-of-tree 鸿沟** | 浅（KMDF 屏蔽内核）| 深（必须读 mainline 改驱动）|
| **Stable KABI 努力** | KMDF v1.x 是稳定 framework | GKI（Android Common Kernel）是妥协，主线无 |
| **API 文档** | DDK / WDK 文档 + Hardware Cert Kit | Documentation/ 子目录 + LWN 文章 + 邮件列表 |

**根本设计哲学：**
- Windows：**稳定 ABI = 第三方厂商生态友好** —— 适合商业闭源驱动繁荣
- Linux：**不稳定 ABI = 鼓励驱动进 mainline** —— 适合开源演化但对 out-of-tree 不友好

### §6.15 跨架构

| | Windows | Linux |
|--|---------|-------|
| 目标架构 | x86_64 / ARM64 | 30+（x86 / arm / arm64 / riscv / loongarch / mips / powerpc / sparc / s390 / ...）|
| 驱动写法 | 基本架构无关，少数地方 #ifdef _AMD64_ | 大量 #ifdef CONFIG_X86 等 |
| 编译产物 | 一架构一份 .sys | 一架构一份 .ko |
| Universal driver | Win10 引入"一份代码多 SKU" | （Linux 一直如此但 ABI 不稳定）|

### §6.16 生态规模

| | Windows | Linux |
|--|---------|-------|
| 驱动数量（粗估）| 数十万（含 OEM 私有）| 数千 in-tree + 数百 out-of-tree |
| 主要供应方 | OEM/IHV 商业为主 | mainline 社区为主 + NVIDIA / VMware / VirtualBox / ZFS 出 tree |
| 商业模式 | 闭源驱动主流 | 开源主流 |

### §6.17 加载兼容案例

| 项目 | 加载哪种驱动文件 | 路径 |
|------|------------------|------|
| **ReactOS** | ✅ **直接加载 Windows .sys**（NT 5/6 时代驱动）| ABI 级（路径 ①）|
| **ndiswrapper** | ✅ Windows NDIS .sys 网卡驱动 in Linux/FreeBSD | shim 桥接（路径 ③）|
| **Cygwin** | ❌ 不加载驱动，只跑 Windows API on POSIX 用户态 | 不算 |
| **Wine** | ❌ 用户态 Win32 模拟，不加载内核驱动 | 不算 |
| **WSL1** | ❌ Linux syscall 翻译为 NT primitives，无驱动 | 不算 |
| **WSL2** | ❌ 真 Linux VM，加载 .ko | 路径 ⑤ |
| **DragonOS** | 部分加载 .ko？无；用 source-level 重编 Linux 驱动源 | 路径 ② |
| **FreeBSD LinuxKPI** | ❌ 不加载 .ko；编译 Linux 驱动源到 FreeBSD .ko | 路径 ② |
| **几乎无 OS 直接加载 Linux .ko** | KABI 不稳代价过高 | |

---

## §7 跨 OS 驱动框架横向（设计维度）

>
> - § 7.8 **跨层级驱动复用**（SBI / U-Boot DM / EDK2 / GRUB / coreboot / Embassy / arceos / TamaGo）
> - § 7.10 **跨 OS 形态参考**（裸机 / RTOS / 宏 / 微 / 外 / LibOS / Unikernel / 组件化）

### §7.1 Linux DM 总结

（已在 §1-§5 详写，此处归纳设计哲学）

**3 大设计原则：**
1. **device 与 driver 分离** —— 由 bus 配对（match）
2. **kobject 统一对象抽象** —— 引用计数 + sysfs 表示 + uevent + 父子关系
3. **subsystem 平等独立** —— 没有 framework 层级（KMDF/UMDF 那样）

**优点：** 抽象薄、概念少、多核 RCU 等高级机制原生
**缺点：** 驱动直接面对内核演化、KABI 不稳

### §7.2 Windows WDM / KMDF / UMDF / WDF

**4 代演化：**

| 时期 | 模型 | 抽象层 | 驱动复杂度 |
|------|------|--------|-----------|
| Win 2000 | WDM | 直接 IRP/MJ_FUNCTION | 高（写每个 IRP handler）|
| Win XP+ | KMDF | IO Queue + Object Model | 中（框架处理 PnP/电源/IO）|
| Vista+ | UMDF | 用户态 + COM-style | 低（语言无关 + 沙箱）|
| Win 8+ | WDF（unified）| KMDF + UMDF 统一 API | 低（WDF v1.x 长期稳定）|

**WDF 对象模型：**
```
WDFDRIVER
└── WDFDEVICE
    ├── WDFQUEUE (default I/O queue)
    │   └── WDFREQUEST (一次 I/O)
    ├── WDFINTERRUPT
    ├── WDFTIMER
    ├── WDFDPC
    ├── WDFFILEOBJECT
    ├── WDFCHILDLIST (PnP 子设备)
    ├── WDFDMATRANSACTION
    └── ...
```

WDF 的关键创新：**对象生命周期管理**（自动销毁子对象）+ **IO Queue 自动处理 cancel/timeout/idle**。

### §7.3 macOS IOKit

**核心：** 用 **C++（受限子集 Embedded C++）** 写驱动，对象继承自 `IOService` 基类。

```cpp
class com_example_driver_MyDriver : public IOService {
    OSDeclareDefaultStructors(com_example_driver_MyDriver)
public:
    virtual bool start(IOService *provider) override;
    virtual void stop(IOService *provider) override;
};
```

**关键概念：**
- **IORegistry**：树状对象注册表（对应 sysfs）
- **IOService**：所有设备/驱动基类（match → start → stop 生命周期）
- **IOWorkLoop + IOInterruptEventSource**：单线程事件循环简化中断
- **IOCommandGate**：跨线程串行化访问

**与 Linux 对比：**
- C++ 而非 C，自然有析构 = remove
- 单一基类 IOService，所有驱动都继承 → 比 Linux 各 bus 独立 driver struct 更统一
- 用户态 IOKit 框架 + driverkit（macOS 11+ 用户态驱动）= UMDF 类似

### §7.4 BSD devfs / NetBSD rump kernel

**FreeBSD：** 用 `device_t` + `driver_t` + `devclass_t`，类似 Linux 但 API 风格不同（`bus_alloc_resource_any` / `bus_setup_intr` 等）。

**NetBSD rump kernel** —— ⭐独特创新：
- 把内核子系统（fs / network stack / driver）打包成**用户态 lib**
- 应用 link 这些 lib 后，可以在用户态运行 NetBSD 内核代码（FUSE 文件系统 / 用户态网卡）
- 思想接近 LibOS（unikraft）但是 BSD 自有

### §7.5 illumos / Solaris driver framework + DDI/DKI

**DDI（Device Driver Interface）** + **DKI（Driver Kernel Interface）** 是 Solaris 早期定义的驱动 API，关键设计：

```c
static struct cb_ops my_cb_ops = {
    .cb_open = my_open,
    .cb_close = my_close,
    .cb_read = my_read,
    .cb_write = my_write,
    .cb_ioctl = my_ioctl,
    .cb_devmap = my_devmap,
    .cb_mmap = nodev,
    .cb_segmap = my_segmap,
    .cb_chpoll = my_chpoll,
    .cb_prop_op = ddi_prop_op,
    .cb_str = NULL,
    .cb_flag = D_NEW | D_MP,
    .cb_rev = CB_REV,
    .cb_aread = my_aread,
    .cb_awrite = my_awrite,
};

static struct dev_ops my_ops = {
    .devo_rev = DEVO_REV,
    .devo_refcnt = 0,
    .devo_getinfo = my_getinfo,
    .devo_identify = nulldev,
    .devo_probe = nulldev,
    .devo_attach = my_attach,
    .devo_detach = my_detach,
    .devo_reset = nodev,
    .devo_cb_ops = &my_cb_ops,
    .devo_bus_ops = NULL,
    .devo_power = my_power,
    .devo_quiesce = my_quiesce,
};
```

**SPL（Solaris Porting Layer）** —— illumos 项目提供的 Linux→Solaris 兼容 shim，让 ZFS 这样在 Linux/Solaris 双向移植代码：
```
Linux ZFS source code (foo.c)
    ↓ 包含 <sys/spl/spl.h>（提供 Linux 风格 API）
    ↓ SPL 把 spin_lock(&l) → mutex_enter(&l) 等映射到 Solaris 等价
    ↓ 编进 illumos kernel
```

### §7.6 Fuchsia DFv2

**Fuchsia DFv2（Driver Framework v2）** 关键设计：

```rust
// driver_v2/lib/banjo or rust binding
use fdf::*;

#[derive(Driver)]
struct MyDriver { /* ... */ }

impl DriverImpl for MyDriver {
    async fn start(&mut self, ctx: DriverContext) -> Result<()> {
        let dev = ctx.create_device(...)?;
        Ok(())
    }
}
```

**核心特征：**
- **驱动跑在用户态（component model）** —— 微内核哲学
- **FIDL（Fuchsia Interface Definition Language）** —— 跨进程稳定 ABI
- **Banjo** —— 老的同进程驱动通信（向 FIDL 迁移）
- **devhost / driver_manager** —— 隔离每个驱动到独立进程

**与 Linux 对比：**
- Linux 驱动跑内核态；Fuchsia 跑用户态 → 失败隔离 + 升级不重启
- FIDL 是稳定 ABI 的极端表达（IPC 必须稳定）

### §7.7 Android HAL

**Android HAL（Hardware Abstraction Layer）** 抽象层 —— 不是真正的驱动，是**用户态 shim** 把 Linux 内核 driver 暴露给 Java framework：

```
Android App (Java/Kotlin)
    ↓ Binder IPC
HAL Service (C++ / 用户态)
    ↓ /dev/x ioctl 或 sysfs
Linux Kernel Driver (内核态)
    ↓
Hardware
```

**HIDL → AIDL 演化：**
- HIDL（HAL Interface Definition Language，老）—— Treble 项目（Android 8+）引入，强制 HAL 与内核解耦
- AIDL（新）—— 取代 HIDL，统一用 Binder



>
> **本节列出每一层的驱动机制 + 跨层复用尝试。**

#### §7.8.1 SBI 层驱动（M-mode 固件）

OpenSBI 把 console / IPI / irqchip / timer 等核心服务做成"driver" 概念：

```c
// lib/utils/serial/uart8250.c
static struct uart8250_priv uart8250 = {
    .reg_io_base = 0,
    .reg_shift = 0,
    .reg_io_width = 0,
    .baudrate = 0,
    .clock = 0,
};

static const struct sbi_console_device uart8250_console = {
    .name = "uart8250",
    .console_putc = uart8250_putc,
    .console_getc = uart8250_getc,
};

int uart8250_init(unsigned long base, u32 in_freq, u32 baudrate, u32 reg_shift,
          u32 reg_width, u32 reg_offset) {
    uart8250.reg_io_base = base;
    /* ... */
    sbi_console_set_device(&uart8250_console);
    return 0;
}
```

**OpenSBI driver 现状：**
- `lib/utils/serial/`：uart8250 / sifive_uart / shakti_uart / cadence_uart / etc
- `lib/utils/ipi/`：aclint_mswi / andes_plicsw
- `lib/utils/irqchip/`：plic / aplic / imsic
- `lib/utils/timer/`：aclint_mtimer / andes_plmt / sstc

**特点：**
- 极薄抽象 —— 没有 device/driver 分离，没有 bus，靠平台 init 函数显式调
- M-mode 单线程，无中断复用 → 无锁
- **资源模型：** 直接 MMIO 物理地址 + IRQ 编号，无虚拟化


#### §7.8.2 U-Boot Driver Model（DM）

U-Boot DM 是 bootloader 层最成熟的统一驱动框架（详见 [03-11](03-11-u-boot-proper-source-walkthrough.md) / [03-08](03-08-uboot-develop-manual.md)）：

```c
// drivers/serial/serial_dw.c
static const struct dm_serial_ops dw_serial_ops = {
    .putc = dw_serial_putc,
    .pending = dw_serial_pending,
    .getc = dw_serial_getc,
    .setbrg = dw_serial_setbrg,
};

static const struct udevice_id dw_serial_ids[] = {
    { .compatible = "snps,dw-apb-uart" },
    { .compatible = "rockchip,rk3288-uart" },
    { .compatible = "rockchip,rk3328-uart" },
    { .compatible = "rockchip,rk3368-uart" },
    { .compatible = "rockchip,rk3399-uart" },
    {}
};

U_BOOT_DRIVER(serial_dw) = {
    .name = "serial_dw",
    .id = UCLASS_SERIAL,                    // ⭐ uclass = "类似 Linux class"
    .of_match = dw_serial_ids,
    .ofdata_to_platdata = dw_serial_ofdata_to_platdata,
    .probe = dw_serial_probe,
    .remove = dw_serial_remove,
    .priv_auto_alloc_size = sizeof(struct dw_serial),
    .platdata_auto_alloc_size = sizeof(struct dw_serial_platdata),
    .ops = &dw_serial_ops,
    .flags = DM_FLAG_PRE_RELOC,
};
```

**核心设计：**
- **uclass**（U-Boot Class）= Linux class 的对应 —— 同种功能设备归一类（UCLASS_SERIAL / UCLASS_BLK / UCLASS_NET / UCLASS_GPIO / ...）
- **udevice** = Linux device 对应
- **driver** = Linux driver 对应
- **DT-driven probe** —— bootloader 阶段也用 DT compatible 字符串匹配
- **ops** = 函数指针表（per-uclass 定义）

**与 Linux DM 的相似度极高 —— 这是 U-Boot 2010 后主动学 Linux 的设计。**


#### §7.8.3 EDK2 Driver Binding Protocol

EDK2 的驱动模型基于 **Protocol + Driver Binding**（详见 [03-12](03-12-edk2-walkthrough.md)）：

```c
// MdeModulePkg/Bus/Pci/PciBusDxe/PciBus.c
EFI_DRIVER_BINDING_PROTOCOL gPciBusDriverBinding = {
    PciBusDriverBindingSupported,
    PciBusDriverBindingStart,
    PciBusDriverBindingStop,
    0xa,
    NULL,
    NULL
};

EFI_STATUS EFIAPI PciBusDriverBindingSupported(
    IN EFI_DRIVER_BINDING_PROTOCOL *This,
    IN EFI_HANDLE                  ControllerHandle,
    IN EFI_DEVICE_PATH_PROTOCOL    *RemainingDevicePath OPTIONAL
);
```

**核心机制：**
- **EFI_DRIVER_BINDING_PROTOCOL** —— 每个驱动注册一个，提供 Supported/Start/Stop 三函数
- **Supported(ControllerHandle)** —— 驱动看 controller 是否能驱动（返回 EFI_SUCCESS 或 EFI_UNSUPPORTED）
- **Start(ControllerHandle)** —— driver 接管设备（相当于 probe）
- **Stop(ControllerHandle)** —— driver 释放设备（相当于 remove）

**Protocol 是核心抽象：** 所有功能都是 GUID 唯一标识的 Protocol（EFI_BLOCK_IO_PROTOCOL / EFI_NETWORK_INTERFACE_IDENTIFIER / EFI_GRAPHICS_OUTPUT_PROTOCOL 等）。

**与 Linux DM 对比：**
- EDK2 的 Driver Binding ≈ Linux driver-bus matching
- EDK2 的 Protocol ≈ Linux 的 ops + class
- EDK2 用 GUID 显式标识接口；Linux 隐式通过函数签名

#### §7.8.4 GRUB driver

GRUB 2 的驱动模型（详见 [03-15](03-15-grub2-walkthrough.md)）：

```c
// grub-core/disk/ata.c
static struct grub_disk_dev grub_atadisk_dev = {
    .name = "ATA",
    .id = GRUB_DISK_DEVICE_ATA_ID,
    .iterate = grub_atadisk_iterate,
    .open = grub_atadisk_open,
    .close = grub_atadisk_close,
    .read = grub_atadisk_read,
    .write = grub_atadisk_write,
    .next = 0
};

GRUB_MOD_INIT(ata) {
    grub_disk_dev_register(&grub_atadisk_dev);
}
```

**3 大 device 类：**
- **grub_disk_dev** —— 块设备（ATA / SCSI / USB / NVMe / network / virtual）
- **grub_term_input/output** —— 终端
- **grub_video** —— 显卡（vbe / efi / linux fb / coreboot）

**特点：**
- 极简（无 bus / 无 PnP）
- 单线程（GRUB 是 cooperative）
- 模块加载机制（`.mod` 文件，用 grub-install 装到 ESP）

#### §7.8.5 coreboot driver

coreboot 的驱动模型基于 **chip ops + device tree**：

```
device path 形式（mainboard.cb）:
chip soc/intel/skylake
    device cpu_cluster 0 on
        device lapic 0 on end
    end
    device domain 0 on
        device pci 00.0 on end                # host bridge
        device pci 02.0 on end                # graphics
        device pci 14.0 on
            chip drivers/usb/acpi
                register "type" = "UPC_TYPE_USB3_A"
                device usb 0.0 on end
            end
        end
    end
end
```

**chip 是 coreboot 的核心抽象** —— 每个 chip 提供 enable / init / disable callback。**device tree 在编译期固化**（不是 Linux DT 那种运行时解析）。

**特点：**
- 编译期 device tree（无运行时灵活性）
- chip-driven 而非 device-driven
- 与 Linux DM 哲学不同 —— coreboot 假设硬件配置在编译时已知

#### §7.8.6 Rust embedded-hal

`rust-embedded/embedded-hal` 是 Rust 嵌入式生态最广泛采用的硬件抽象 trait 集合：

```rust
// embedded-hal v1.0
pub trait OutputPin {
    type Error;
    fn set_low(&mut self) -> Result<(), Self::Error>;
    fn set_high(&mut self) -> Result<(), Self::Error>;
}

pub trait InputPin {
    type Error;
    fn is_high(&mut self) -> Result<bool, Self::Error>;
    fn is_low(&mut self) -> Result<bool, Self::Error>;
}

pub trait SerialWrite<Word = u8> {
    type Error;
    fn write(&mut self, word: Word) -> nb::Result<(), Self::Error>;
    fn flush(&mut self) -> nb::Result<(), Self::Error>;
}

pub trait I2c {
    type Error;
    fn read(&mut self, addr: SevenBitAddress, buf: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, addr: SevenBitAddress, buf: &[u8]) -> Result<(), Self::Error>;
    fn write_read(&mut self, addr: SevenBitAddress, write: &[u8], read: &mut [u8]) -> Result<(), Self::Error>;
    fn transaction(&mut self, addr: SevenBitAddress, ops: &mut [Operation<'_>]) -> Result<(), Self::Error>;
}
```

**特点：**
- **trait-based 抽象** —— 每个外围（GPIO/I2C/SPI/UART/PWM/ADC/...）一个 trait
- **零成本抽象** —— 编译期 monomorphization，无运行时开销
- **跨 RTOS / 裸机通用** —— FreeRTOS / Embassy / RTIC / 裸机 main 都能用同一份 driver crate
- **embedded-hal v1.0 已稳定**（2024）—— 真正稳定 ABI 嵌入式 Rust


#### §7.8.7 arceos device crate

arceos 提供 `axdevice` crate，在 unikernel / monolithic / hypervisor 三模式间共享：

```rust
// modules/axdriver/src/lib.rs
pub trait BaseDriverOps: Send + Sync {
    fn device_name(&self) -> &str;
    fn device_type(&self) -> DeviceType;
}

pub trait NetDriverOps: BaseDriverOps {
    fn mac_address(&self) -> EthernetAddress;
    fn can_transmit(&self) -> bool;
    fn can_receive(&self) -> bool;
    fn rx_queue_size(&self) -> usize;
    fn tx_queue_size(&self) -> usize;
    fn recycle_rx_buffer(&mut self, rx_buf: NetBufPtr) -> DevResult;
    fn recycle_tx_buffers(&mut self) -> DevResult;
    fn transmit(&mut self, tx_buf: NetBufPtr) -> DevResult;
    fn receive(&mut self) -> DevResult<NetBufPtr>;
    fn alloc_tx_buffer(&mut self, size: usize) -> DevResult<NetBufPtr>;
}

pub trait BlockDriverOps: BaseDriverOps {
    fn num_blocks(&self) -> u64;
    fn block_size(&self) -> usize;
    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> DevResult;
    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> DevResult;
    fn flush(&mut self) -> DevResult;
}
```

**特点：**
- trait 风格类似 embedded-hal 但更面向通用 OS 场景
- cargo features 切换：`feature = "virtio"` / `feature = "ixgbe"` / etc
- 同一份 driver 编译为 unikernel（直接跑）/ monolithic（StarryOS 用）/ hypervisor（axvisor 用）


#### §7.8.8 TamaGo（Go on bare metal）

TamaGo 让 Go 直接跑在 ARM / RISC-V 裸机：

```go
// tamago/board/usbarmory/mark-two/usbarmory.go
package usbarmory

import (
    "github.com/usbarmory/tamago/soc/nxp/imx6ul"
)

func init() {
    imx6ul.Init()
    imx6ul.UART2.Init()
}
```

**特点：**
- 标准 Go runtime + 用 Go 写 driver
- goroutine 直接跑在中断上下文
- USB armory mkII / RISC-V FU540 等板子


### §7.9 横向对比大表

| 框架 | 语言 | 抽象层 | bus 概念 | DT/ACPI | 用户/内核态 | 跨层级 | 跨 OS 形态 |
|------|------|--------|----------|---------|------------|--------|-----------|
| Linux DM | C / Rust（部分）| device + driver + bus + class | ✅ | DT + ACPI | 内核态 | ❌ | ❌ |
| Windows WDM | C / C++ | DEVICE_OBJECT + DRIVER_OBJECT | ❌（用 driver stack）| ACPI + INF | 内核态 | ❌ | ❌ |
| Windows KMDF | C | WDFDRIVER + WDFDEVICE + WDFQUEUE | ❌ | ACPI + INF | 内核态（KMDF）+ 用户态（UMDF）| ❌ | ❌ |
| macOS IOKit | Embedded C++ | IOService 单一基类 | ❌（IORegistry 树）| ACPI + IOKit registry | 内核态 + driverkit 用户态 | ❌ | ❌ |
| FreeBSD newbus | C | device_t + driver_t + devclass_t | ✅ | DT + ACPI | 内核态 | ❌ | ❌ |
| FreeBSD LinuxKPI | C | Linux 头文件兼容层 | ✅（复用 Linux）| 复用 Linux | 内核态 | ❌ | ❌ |
| NetBSD rump | C | rump 模块 | ✅（裁剪）| DT + ACPI | **用户态（lib）** | ❌ | ⭐ 部分（bare metal app + kernel） |
| illumos DDI/DKI | C | dev_ops + cb_ops | ✅ | ACPI + DT | 内核态 | ❌ | ❌ |
| Fuchsia DFv2 | C / C++ / Rust | FIDL Protocol | ❌ | DT + ACPI | **用户态** | ❌ | ❌ |
| Android HAL | C++ → Java | HIDL/AIDL Protocol | ❌（基于 Linux）| Linux DT/ACPI | 用户态 + 内核 driver | ❌ | ❌ |
| **OpenSBI driver** | C | SBI device callback | ❌ | DT | M-mode 固件 | ⭐ SBI 层 | ❌ |
| **U-Boot DM** | C | uclass + udevice + driver | ✅ | DT | bootloader | ⭐ Bootloader 层 | ❌ |
| **EDK2** | C | Driver Binding + Protocol | ❌ | ACPI | UEFI (DXE) | ⭐ UEFI 层 | ❌ |
| **GRUB** | C | grub_*_dev | ❌ | 自有 | OS Loader | ⭐ Loader 层 | ❌ |
| **coreboot chip ops** | C | chip + device tree | ✅（编译期）| 自有 | Firmware | ⭐ Firmware 层 | ❌ |
| **embedded-hal** | Rust | trait（GPIO/I2C/SPI/UART/PWM）| ❌ | 无 | RTOS / 裸机 | ❌ | ⭐ 裸机 + RTOS |
| **arceos device** | Rust | trait（Net/Block/Display/Char）| ❌ | DT + 编译期 | unikernel + 宏 + hypervisor | ❌ | ⭐ 3 模式复用 |
| **TamaGo** | Go | Go std + soc 包 | ❌ | 编译期 | bare metal Go runtime | ❌ | ⭐ 裸机 |


>
> **本节列出每种 OS 形态的驱动机制特征 + 跨形态复用难度。**

#### §7.10.1 裸机（FreeRTOS 之前的状态）

直接寄存器操作，**无总线抽象**：
```c
#define UART0_BASE 0x40000000
#define UART_DR    (*(volatile uint32_t *)(UART0_BASE + 0x000))
#define UART_FR    (*(volatile uint32_t *)(UART0_BASE + 0x018))

void uart_putc(char c) {
    while (UART_FR & UART_FR_TXFF);  // 等待 FIFO 不满
    UART_DR = c;
}
```

**特征：** 极简、无 driver 抽象、单文件

**跨形态复用难度：** ⭐⭐⭐⭐⭐ 极难 —— 几乎无法直接复用到上层 OS

#### §7.10.2 RTOS：FreeRTOS / rt-thread / uC-OS

**FreeRTOS：** 基本无 driver 框架，每个 BSP 自己写 hooks（vApplicationStackOverflowHook / portYIELD_FROM_ISR 等）。

**rt-thread：** 有 device 框架（`rt_device_register` + `rt_device_open/read/write`）：
```c
struct rt_device {
    struct rt_object parent;
    enum rt_device_class_type type;
    rt_uint16_t flag, open_flag;
    rt_uint8_t ref_count;
    rt_uint8_t device_id;
    /* 函数指针表 */
    rt_err_t (*init)(rt_device_t dev);
    rt_err_t (*open)(rt_device_t dev, rt_uint16_t oflag);
    rt_err_t (*close)(rt_device_t dev);
    rt_size_t (*read)(rt_device_t dev, rt_off_t pos, void *buffer, rt_size_t size);
    rt_size_t (*write)(rt_device_t dev, rt_off_t pos, const void *buffer, rt_size_t size);
    rt_err_t (*control)(rt_device_t dev, int cmd, void *args);
    /* 私有数据 */
    void *user_data;
};
```

**uC-OS：** 没有 driver 框架，应用直接面向硬件。

**跨形态复用难度：** ⭐⭐⭐ 中 —— 部分 RTOS 有 device 抽象但不通用；rt-thread 比较接近 Linux 风格

#### §7.10.3 宏内核：Linux

完整 Linux DM（已详写 §1）

#### §7.10.4 微内核：seL4 / Fuchsia

**驱动跑在用户态进程中**：
- seL4：driver 是普通 user-space 进程，用 capability + IPC 与硬件 IRQ / MMIO 资源对话
- Fuchsia DFv2：driver 是 component，用 FIDL 通信

**特征：** 失败隔离 + 升级不重启 + 但 IPC 开销

**跨形态复用难度：** ⭐⭐⭐⭐ 较难 —— 用户态 driver 与内核态 driver API 完全不同

#### §7.10.5 外核：jos / Exokernel

**应用自己写 driver**（外核不提供）—— 把硬件资源 raw 暴露给应用，应用拿到后用 LibOS 风格自己抽象。

**特征：** 极致灵活但应用复杂度高

**跨形态复用难度：** ⭐⭐⭐⭐⭐ 极难 —— 因为没有公共 driver 概念

#### §7.10.6 LibOS / Unikernel：unikraft / HermitOS / OSv / MirageOS

**Unikernel = 单进程 + 单地址空间 + 编译进 driver**：
```
Application code ─┐
LibOS runtime    ─┼─→ 单一二进制（一个内核 image = 一个应用）
Driver code      ─┘
```

**特征：** 极小 image / 极快启动 / 但只跑一个应用

**跨形态复用难度：** ⭐⭐ 较低（如果 driver 写成可移植的 lib 形式）—— unikraft 主动设计为 microlib 集合

#### §7.10.7 组件化内核：arceos / Theseus

**arceos：** cargo features 切换（K-component / K-builtin / U-server 等）；driver 同 crate 可编进 unikernel / monolithic / hypervisor

**Theseus：** intralingual cell（运行时加载 cell 替换内核功能）

**特征：** 模块边界明确 + 编译期/运行时切换灵活

**跨形态复用难度：** ⭐ 最低 —— 这就是组件化内核设计目标

### §7.11 跨形态复用难度大表

| OS 形态 | driver 抽象层级 | 跨形态复用难度 | 代表项目 |
|---------|----------------|----------------|---------|
| 裸机 | 无（直接 MMIO）| ⭐⭐⭐⭐⭐ | C 单文件 |
| RTOS（无 driver 框架）| 无 / hooks | ⭐⭐⭐⭐ | FreeRTOS / uC-OS |
| RTOS（rt-thread）| device + open/read/write | ⭐⭐⭐ | rt-thread |
| 宏内核 | device + driver + bus + class | ⭐⭐⭐ | Linux DM |
| 微内核（用户态）| FIDL / Capability + IPC | ⭐⭐⭐⭐ | seL4 / Fuchsia |
| 外核 | 无（应用自己写）| ⭐⭐⭐⭐⭐ | jos / ExOS |
| LibOS / Unikernel | microlib（lib 风）| ⭐⭐ | unikraft |
| 组件化（cargo features）| trait + features | ⭐ | arceos / Theseus |
| 嵌入式 Rust（embedded-hal）| trait（无 OS 假设）| ⭐ | embedded-hal / Embassy |

- 想跨"裸机 + RTOS + 宏内核"复用 → 学 **embedded-hal / arceos device 风格的 trait** 是最低门槛
- 想跨"裸机 + RTOS + 宏内核 + 微内核 + Hypervisor"复用 → 需在 trait 之上加**用户态/内核态切换层**（FIDL 风 RPC）
- 想跨"SBI + Bootloader + GRUB + OS"复用 → 需 trait 设计**显式声明运行环境约束**（无 alloc / 无 sleep / 单线程 / 中断模式 / 等等）

---

## §8 驱动兼容策略全谱（5 路径）


### §8.1 路径 ① ABI 级兼容（直接加载二进制）

**含义：** 拿编译好的 .ko / .sys 二进制，运行时由内核加载 + 重定位 + 符号绑定。

**前提：**
1. 完整复刻源 OS 的内核 ABI（数千个 EXPORT_SYMBOL + 全部 struct 二进制布局对齐）
2. 复刻等价内核服务（kmalloc/锁/中断/DMA/sysfs/dentry/inode 全套）
3. ABI 版本锁定（同一 .ko 通常只能给同一 minor version 用）
4. 模块加载器（ELF/PE 解析 + 重定位 + 签名验证）
5. 许可证捆绑（GPL .ko 加载 = 宿主受 GPL 影响）

**代价：** 极高 —— 数千 EXPORT_SYMBOL + 所有 struct 字节对齐 + 跟版本维护

**典型实施：** ReactOS 加载 Windows .sys（NT 5/6 时代）—— 是**唯一活的工程项目**

**适合场景：** 想跑闭源驱动二进制（NVIDIA .ko / WHQL 签 .sys）；商业意义有限因为 KABI 不稳

### §8.2 路径 ② source-level 兼容（重新编译）

**含义：** 拿源 OS 的驱动 `.c / .h` 源码，在自己 OS 上**重新编译**。

**前提：**
1. 提供源 OS 风格的兼容头文件层（`<linux/spinlock.h>` 等）
2. 提供源 OS 内核 API 的实现（`kmalloc / spin_lock / wait_event`）
3. 宏定义对齐（`MODULE_LICENSE / EXPORT_SYMBOL / module_init`）
4. 编译工具链（GCC/Clang + 内核风格 flags）

**代价：** 中 —— 数百头文件 + 数千 API 实现，可分批做

**优势：**
- 不锁源 OS 版本（自己控制头文件升级节奏）
- struct 无需二进制对齐（重编时按自己头文件分配）
- 错误编译时暴露（不像 ABI 级是运行时崩）

**典型实施：** **FreeBSD LinuxKPI**（`sys/compat/linuxkpi/`）—— 工业最成功，让 FreeBSD 跑 Intel/AMD/NVIDIA 显卡驱动 + iwlwifi 等

**其它实施：** DragonOS / illumos SPL（ZFS-on-Linux）/ NetBSD（部分）

### §8.3 路径 ③ shim 桥接（KABI compat layer）

**含义：** 在源 OS 和宿主 OS 之间放一层"翻译层"，把源 OS 的 API 调用映射到宿主 OS 的等价 API。

**与 source-level 的区别：**
- source-level = **源码重编**（编译期映射）
- shim = **运行时函数调用映射**（动态翻译）

**典型实施：**
- **ndiswrapper**：Linux/FreeBSD 加载 Windows NDIS .sys 网卡驱动
  - 提供 NDIS API 的运行时实现
  - 把 .sys 当 PE 文件加载（不重编）
  - 调用映射到 Linux/FreeBSD 等价
- **WSL1**：NT 内核翻译 Linux syscall（不是驱动，但思想相同）

**代价：** 高 —— 既要复刻 ABI 又要做运行时翻译

**适合场景：** 闭源驱动 + 不愿做完整 ABI 复刻 —— 但工程投入大且容易出 bug

### §8.4 路径 ④ API 形态借鉴重写

**含义：** 看源 OS 驱动接口的"形状"，自己用 Rust/C 完全重写实现 —— **不直接复用任何源 OS 代码**。

**前提：**
1. 学习源 OS 驱动 API 的设计哲学
2. 设计自己的 driver model（可借鉴但不一致）
3. 自己写驱动（数量按需）

**代价：** 中低 —— 不需要兼容头文件层；缺点是没法直接用 Linux 现成驱动

**典型实施：**
- **asterinas** —— framework + Rust trait，自己写 driver
- **Fuchsia DFv2** —— 完全独立 + 稳定 ABI
- **Theseus** —— intralingual cell + 自己写
- **arceos** —— Rust trait + cargo features + 自己写驱动（少量）
- **HermitOS** —— Rust unikernel + 自己写 driver

**适合场景：** 不愿被 Linux KABI 绑架 + 愿意接受"驱动数量少"代价 —— 适合微内核 / 学术研究 / 小众商业 OS

### §8.5 路径 ⑤ VFIO / 直通（虚拟化备选）


**前提：**
2. 硬件支持 IOMMU / SR-IOV
3. VFIO（Linux 内核框架）暴露设备配置空间 + IRQ + DMA 给用户态

**代价：** 低（不写驱动）+ 但功能受限（设备只给 VM 用，host 不能用）

**典型实施：**
- **GPU passthrough**：物理 GPU 直通给 Windows VM 玩游戏
- **SR-IOV**：网卡的 VF 直通给多个 VM
- **NVMe passthrough**：物理 NVMe 直通给 VM


### §8.6 5 路径横向对比表

| 维度 | ① ABI 级 | ② source-level | ③ shim 桥接 | ④ API 重写 | ⑤ VFIO |
|------|---------|---------------|------------|-----------|--------|
| 兼容硬件数 | 全（理论）| 全（重编后）| 全（API 内）| 少（自己写多少）| 全（VFIO 范围）|
| 工程量 | 极高 | 中 | 高 | 中低 | 低 |
| KABI 不稳影响 | 致命 | 小（自控）| 大 | 无 | 无 |
| 许可证捆绑 | 强（GPL）| 中（按 GPL 义务）| 强 | 无 | 无 |
| 错误暴露时机 | 运行时 | 编译时 | 运行时 | 编译时 | 运行时（VFIO 配置错）|
| 驱动数量 | 海量 | 较多 | 少 | 自己写 | 全（但跑 VM 内）|
| 工业成功案例 | 几乎无 | **FreeBSD LinuxKPI ⭐** | ndiswrapper（窄）| asterinas / Fuchsia | KVM + libvirt 普及 |

---

## §9 跨 OS 兼容案例

> **目的：** 列出工程实践中各 OS 选了哪条兼容路径 + 当时的工程取舍。

### §9.1 ReactOS（路径 ① 加载 Windows .sys）

**目标：** 二进制兼容 Windows NT 5（XP）/ NT 6（Vista/7）应用 + 驱动

**实施：**
- NTOSKRNL 复刻：从 NT 4 开始重写，一比一对齐 Microsoft DDK / WDK API
- 加载真正的 Windows .sys 驱动：可加载多种网卡 / 存储 / 显卡 .sys
- 但不能加载 Vista+ 的 KMDF 驱动（因为 KMDF.sys 也是要加载的，依赖链复杂）

**经验：**
- 需要 30+ 年持续逆向 Microsoft 私有 ABI（Windows DDK 不公开内部 struct）
- 主线慢慢推进，已有可用的 Windows XP 驱动子集
- 法律风险：Microsoft 持续盯着但因 ReactOS 用 clean room 实现规避

### §9.2 DragonOS（路径 ② source-level）

**目标：** 跑 Linux musl/glibc 二进制 + 部分 Linux 驱动

**实施：**
- `kernel/src/driver/` Rust 实现 Linux 风格 driver API
- 某些块设备 / TTY / 网卡用 source-level 兼容
- 同时也写自己的 Rust 驱动

**经验：** 国产 OS 项目在 Rust + Linux 兼容路径上的探索

### §9.3 FreeBSD LinuxKPI（路径 ② 工业最成功）

**目标：** 让 FreeBSD 跑 Intel / AMD / NVIDIA 显卡 + Intel iwlwifi 网卡 + Mellanox mlx4/5 高端网卡

**实施：**
```
sys/compat/linuxkpi/
├── common/include/linux/    ← 提供 <linux/spinlock.h> 等头文件
│   ├── spinlock.h
│   ├── kernel.h
│   ├── pci.h
│   ├── device.h
│   ├── workqueue.h
│   └── ... 200+ 头文件
├── common/src/              ← 提供 Linux API 实现
│   ├── linux_compat.c
│   ├── linux_idr.c
│   ├── linux_pci.c
│   ├── linux_workqueue.c
│   └── ... 数百实现文件
└── dummy/                   ← 桩函数
```

**结果：** drm-kmod / iwlwifi-kmod 等 ports 能用接近 mainline Linux 的 driver 源码


### §9.4 illumos SPL（路径 ② Solaris Porting Layer）

**目标：** 让 ZFS-on-Linux 风格代码同时编进 Linux（ZoL）和 illumos

**实施：**
- `usr/src/uts/common/sys/spl/` 提供 Linux 风格 API 头
- `usr/src/uts/common/spl/` 提供实现
- 一份 ZFS 源码同时编 Linux + illumos

**关键决策：** SPL 不是 Linux 写进 illumos，是 ZoL（Linux ZFS port）的兼容层进 illumos —— 反向 source-level

### §9.5 NetBSD rump kernel + LinuxKPI 部分

**目标：** 把 NetBSD 内核子系统（fs / driver / network）打包成用户态 lib

**实施：**
- `rump.lib` —— NetBSD 内核的用户态 lib 形式
- 应用 link 后可在用户态跑 NetBSD 文件系统 / 网络栈
- 部分 LinuxKPI 让 Linux 驱动也能在 rump 跑


### §9.6 Haiku（BeOS R5 兼容）

**目标：** 二进制兼容 BeOS R5 应用 + 部分驱动

**实施：**
- BeOS R5 ABI 固定，复刻可行
- 自己写新驱动 + 兼容 BeOS R5 旧驱动
- 已发 R1（2019）/ R1/Beta5（2024）

**经验：** **小生态 ABI 兼容**可行（因为 BeOS 早 EOL，ABI 不再变）

### §9.7 Project Latte（Win32 on Linux 用户态）

**目标：** Microsoft 内部探索 —— 让 Win32 PE 应用直接在 Linux 跑（用户态，**不是驱动**）

**实施：** 接近 Wine 但 Microsoft 主导 —— 后续被 WSL2 思路替代

**与驱动的关联：** Win32 on Linux 是**用户态对应**的 ABI 兼容，思路与 ReactOS 内核态 ABI 兼容一致

### §9.8 ndiswrapper（路径 ③ shim 桥接）

**目标：** Linux / FreeBSD 加载 Windows NDIS .sys 网卡驱动

**实施：**
- 把 .sys 当 PE 文件用 user-mode loader 加载
- 实现 NDIS API 的 Linux 等价
- 调用从 .sys 进入 ndiswrapper.ko 进入 Linux 等价 API

**结局：** 主流 Linux 网卡驱动逐步原生支持，ndiswrapper 成历史遗物

### §9.9 WSL1 vs WSL2（不同思路）

| | WSL1 | WSL2 |
|--|------|------|
| 思路 | NT 内核翻译 Linux syscall | 真 Linux VM（Hyper-V）|
| 驱动 | 共享 NT 驱动 | 独立 Linux 驱动（VM 内）|
| 性能 | 文件 IO 慢 | 接近原生 |
| 兼容性 | 部分 syscall 不支持（io_uring / FUSE 早期）| 全支持 |

### §9.10 asterinas / Fuchsia / Theseus（路径 ④ 完全独立）

**asterinas：** Rust framekernel + 自己写 driver + 借鉴 Linux API 形态

**Fuchsia DFv2：** C++/Rust + 用户态驱动 + FIDL 稳定 ABI + 完全独立

**Theseus：** Rust intralingual cell + 内核功能 cell 化 + 完全独立

**经验：** 路径 ④ 的代价是"自己写驱动数量有限"——通常先支持核心（USB / NVMe / virtio / 主要网卡），其余慢慢补

---

## §10 兼容机制底层

> **目的：** 拆解兼容工作背后的具体技术机制 —— 选任何兼容路径都绕不开这些底层问题。

### §10.1 符号桥接（kallsyms / symbol resolution）


**机制：**
1. **静态符号表**：内核启动时记录所有 EXPORT_SYMBOL 的（名字 → 地址）映射 → `/proc/kallsyms`
2. **加载时绑定**：模块加载器扫描 .ko 的 `R_X86_64_*` 重定位条目 + 查 kallsyms → 把符号引用补上具体地址
3. **CRC 校验**：modversions 模式下，每个符号有 CRC32（基于函数签名）—— 模块的 `__versions` 段记录"我用的 kmalloc CRC = 0xdeadbeef"，加载时与内核记录对比；不一致拒载


### §10.2 struct 内存布局对齐


**机制：**
- ABI 级兼容：必须**字节对齐复刻**（同样的字段、同样的顺序、同样的 padding）

**实例：**
```c
// Linux 6.6: include/linux/device.h
struct device {
    struct kobject kobj;          // offset 0
    struct device *parent;        // offset 80（kobj 大小决定）
    struct device_private *p;     // offset 88
    /* ... */
};
```
不同 Linux 版本 `kobject` 大小可能变 → 偏移变 → ABI 级兼容崩。

### §10.3 锁 / 中断 / sleep 语义映射

**问题：** Linux `spin_lock_irqsave` / Windows `KeAcquireSpinLock` / Solaris `mutex_enter` 语义类似但不完全等价。

**关键差异点：**
- 是否禁中断？（Linux spin_lock_irqsave 禁；Linux spin_lock 不禁）
- 是否禁抢占？（Linux spin_lock 禁抢占）
- 是否检查死锁？（lockdep / Solaris dtrace lockstat）
- 释放后是否唤醒等待者？（mutex 唤醒一个 vs 全部）


### §10.4 DMA buffer 跨内存模型

**问题：** DMA 访问需要"物理连续 + cache 一致 + IOMMU 映射"三件事；不同 OS 的 DMA API 抽象不同。

**Linux dma_alloc_coherent：**
```c
void *cpu = dma_alloc_coherent(dev, size, &dma_addr, GFP_KERNEL);
```
保证返回的 `cpu` 虚拟地址 + `dma_addr` 物理地址 + 物理连续 + cache 一致。

**Windows MmAllocateContiguousMemorySpecifyCacheNode：**
```c
PVOID cpu = MmAllocateContiguousMemorySpecifyCacheNode(size, ...);
PHYSICAL_ADDRESS phys = MmGetPhysicalAddress(cpu);
```
两步（alloc + 查物理地址）。


### §10.5 KABI 版本差异 shim

**问题：** Linux 5.10 vs 6.6 的某些 EXPORT_SYMBOL 函数签名变了 —— 一份 source-level 兼容头文件如何同时支持？

**机制（FreeBSD LinuxKPI 做法）：**
```c
// linux/pci.h compat
#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 0, 0)
int pci_alloc_irq_vectors(struct pci_dev *dev, unsigned int min_vecs,
                           unsigned int max_vecs, unsigned int flags);
#elif LINUX_VERSION_CODE >= KERNEL_VERSION(5, 4, 0)
int pci_alloc_irq_vectors_affinity(...);  /* 老签名 */
#endif
```


### §10.6 错误码翻译

**问题：** Linux errno（POSIX 1-200）vs Windows NTSTATUS（HRESULT 复杂结构）vs FreeBSD errno（部分不同）—— 跨 OS 时如何翻译？

**Linux errno 例子：**
- `EINVAL` = 22
- `ENOMEM` = 12
- `EIO` = 5

**Windows NTSTATUS 对应：**
- `STATUS_INVALID_PARAMETER` = 0xC000000D
- `STATUS_INSUFFICIENT_RESOURCES` = 0xC000009A
- `STATUS_DEVICE_DATA_ERROR` = 0xC000009C

**实施：** source-level 兼容 / shim 都要提供 errno → 等价错误码的映射表（数百条）

---

## §11 本地项目驱动模型现状对照


### §11.1 arceos device

**位置：** `core/arceos/modules/axdriver/`

**核心：**
- trait `BaseDriverOps` + 各类 trait（`NetDriverOps` / `BlockDriverOps` / `DisplayDriverOps`）
- cargo features 选实现：`virtio` / `ixgbe` / `bcm2835-sdhci` 等
- 同 driver crate 编进 unikernel / monolithic / hypervisor 三模式


### §11.2 DragonOS driver

**位置：** `core/DragonOS/kernel/src/driver/`

**核心：**
- Rust 实现 Linux 风格 driver framework
- 部分 driver source-level 兼容 Linux
- 块设备 / TTY / 网卡 / 文件系统等


### §11.3 asterinas

**位置：** `core/asterinas/`

**核心：**
- framekernel（Rust 安全内核框架）
- 完全自己写 driver（path ④）
- 借鉴 Linux API 形态但不强求兼容


### §11.4 Theseus

**位置：** `core/Theseus/`

**核心：**
- intralingual cell（每个内核功能 = 一个 Rust crate cell）
- 运行时加载 / 替换 / 升级 cell
- 用 Rust trait 抽象硬件


### §11.5 TornadoOS

**位置：** `core/TornadoOS/`

**核心：**
- 全异步内核 + driver async-fn-syscall 流派
- 共享调度器 RingFifoScheduler
- 4 个 FFI 入口连接异步驱动栈


### §11.6 StarryOS

**位置：** `core/StarryOS/`

**核心：**
- 基于 arceos monolithic 人格
- 复用 arceos device crate
- 添加 Linux syscall 兼容层


### §11.7 横向对比

| 项目 | 语言 | driver 模型 | Linux 兼容路径 | 跨 OS 形态 |
|------|------|-------------|---------------|-----------|
| arceos | Rust | trait + features | ❌ | ⭐ unikernel + 宏 + hyper |
| DragonOS | Rust + C | Linux 风 + Rust | ⭐ source-level（部分）| ❌ |
| asterinas | Rust | framework + 自己写 | ❌（API 借鉴）| ❌ |
| Theseus | Rust | cell + trait | ❌ | ❌（cell 替换内核功能）|
| TornadoOS | Rust | async + FFI | ❌ | ❌ |
| StarryOS | Rust | arceos + Linux syscall | ⭐（用户态 syscall）| 复用 arceos 模式 |

---

## §12 设计模式归纳


### §12.1 type-state（设备生命周期编码进类型）

**问题：** 设备状态（initialized / configured / running / suspended）在 C 里靠 enum + 运行时检查；Rust 用 type-state 编进类型，编译期保证。

**模式：**
```rust
struct Uart<State> { _state: PhantomData<State>, /* ... */ }

struct Uninit;
struct Configured;
struct Running;

impl Uart<Uninit> {
    fn new(base: usize) -> Self { /* ... */ }
    fn configure(self, baud: u32) -> Uart<Configured> { /* ... */ }
}

impl Uart<Configured> {
    fn start(self) -> Uart<Running> { /* ... */ }
}

impl Uart<Running> {
    fn write(&mut self, byte: u8) { /* ... */ }
    fn stop(self) -> Uart<Configured> { /* ... */ }
}

// 用户：let uart = Uart::new(0x4000_0000).configure(115200).start();
// 编译器保证：uart.write() 仅在 Running 状态可调
```

**好处：** 编译期消除"设备未配置就用"等错误

### §12.2 capability-based（资源借出）

**问题：** 多个驱动竞争同一硬件资源（GPIO 引脚 / 中断号）—— 如何编译期保证不冲突？

**模式：**
```rust
struct Peripherals { /* contains all hardware */ }

impl Peripherals {
    fn take() -> Option<Self>;  // 只能成功一次（singleton）
}

let p = Peripherals::take().unwrap();
let uart0 = p.UART0;        // 借出，p 失去 UART0
let gpio = p.GPIOA.split(); // 引脚级粒度借出
let pin0 = gpio.pin0;       // 单引脚
```

**典型库：** `cortex-m-rt::Peripherals` / `embedded-hal` Capability pattern

### §12.3 错误代数（Errno + 自定义）

**模式：**
```rust
#[derive(Debug, thiserror::Error)]
enum DriverError {
    #[error("hardware not responding")]
    Timeout,
    #[error("invalid configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("DMA error")]
    Dma(#[from] DmaError),
    #[error("I/O error")]
    Io(#[from] io::Error),
}
```

**好处：** 编译期穷举处理所有错误情况；`?` 操作符自动传播。

### §12.4 DMA-safe buffer（DmaBuffer trait）

**问题：** DMA 缓冲区必须满足 cache 一致性 / 物理连续 / 对齐等约束 —— Rust 类型系统编码这些约束。

**模式：**
```rust
trait DmaBuffer {
    fn phys_addr(&self) -> u64;
    fn size(&self) -> usize;
    fn flush_for_device(&mut self);    // CPU 写完后让 DMA 看到
    fn invalidate_for_cpu(&mut self);  // DMA 写完后让 CPU 看到
}

struct CoherentBuffer { /* 自动 cache 一致 */ }
struct StreamingBuffer { /* 需要显式 flush/invalidate */ }

impl DmaBuffer for CoherentBuffer { /* ... */ }
impl DmaBuffer for StreamingBuffer { /* ... */ }
```

**好处：** 编译期区分 coherent vs streaming，调用 DMA API 时编译器检查参数类型。

### §12.5 threaded irq

**模式（Linux 风格）：**
```c
request_threaded_irq(irq, isr_quick, isr_deferred, IRQF_ONESHOT, "mydrv", priv);
```
- `isr_quick` 在 hardirq 上下文（必须快）—— 通常仅 acknowledge 中断
- `isr_deferred` 在内核线程上下文 —— 可 sleep / 复杂处理

**Rust 等价：**
```rust
register_irq(irq, |frame| {
    // hardirq 上下文 - 仅 ack
    ack_pending();
    Action::WakeThread
});

spawn_kthread(|| {
    // 线程上下文 - 处理工作
    loop { wait_signal(); process(); }
});
```


---




2. > "我需要参考这些项目的驱动系统来设计一款自己的驱动系统，同时兼容 SBI BOOTLOADER GRUB OS"
3. > "OS 指的是：裸机-RTOS/宏内核/微内核..."
4. > "驱动系统的设计 也是，类 Linux 驱动系统，以及驱动系统的设计 也是"
5. > "驱动系统的兼容也是"


1. **设计一款自己的驱动系统**（不直接抄 Linux DM）
2. **参考多 OS 项目**（Linux / Windows / macOS / BSD / illumos / Fuchsia 等）
3. **跨层级兼容**：SBI（M-mode 固件）+ BOOTLOADER（U-Boot/EDK2）+ GRUB（OS Loader）+ OS（多种）
4. **跨 OS 形态兼容**：裸机 + RTOS + 宏内核 + 微内核 + ...
5. **特别兼容**：Linux 驱动 + x11 图形界面

### §13.3 5 路径选型清单（仅事实，不替决定）

| 路径 | 工程量 | KABI 影响 | 工业案例 | 适合场景 |
|------|--------|-----------|---------|---------|
| ① ABI 加载 .ko | 极高 | 致命 | 几乎无 | 商业意义有限 |
| ② source-level | 中 | 小 | **FreeBSD LinuxKPI ⭐** | 跑 Linux 主流硬件 |
| ③ shim 桥接 | 高 | 大 | ndiswrapper（窄）| 闭源 + 不重编 |
| ④ API 重写 | 中低 | 无 | asterinas / Fuchsia / Theseus | 长期独立设计 |
| ⑤ VFIO 透传 | 低 | 无 | KVM / libvirt 普及 | 性能 + 不写驱动 |

### §13.4 跨层级（SBI/Bootloader/GRUB/OS）参考

| 层级 | 参考项目 | 驱动机制 | 资源模型 |
|------|---------|---------|----------|
| Bootloader | U-Boot DM | uclass + udevice + driver | DT + 编译期选项 |
| UEFI / EDK2 | EDK2 | Driver Binding + Protocol + GUID | ACPI + handle |
| OS Loader | GRUB | grub_*_dev callback | 自有 |
| Firmware | coreboot | chip ops + 编译期 device tree | 编译期 |
| RTOS | embedded-hal / Embassy | trait | 编译期 |
| 宏内核 | Linux DM | device + driver + bus + class | DT + ACPI |
| 组件化 | arceos | trait + cargo features | trait 类型 |

### §13.5 跨 OS 形态参考

| 形态 | 复用难度 | 参考 |
|------|---------|------|
| 裸机 | ⭐⭐⭐⭐⭐ | C 直接寄存器 |
| RTOS（无框架）| ⭐⭐⭐⭐ | FreeRTOS hooks |
| RTOS（有框架）| ⭐⭐⭐ | rt-thread device |
| 宏内核 | ⭐⭐⭐ | Linux DM |
| 微内核 | ⭐⭐⭐⭐ | seL4 user-space + IPC |
| 外核 | ⭐⭐⭐⭐⭐ | jos 应用驱动 |
| Unikernel | ⭐⭐ | unikraft microlib |
| 组件化 | ⭐ | arceos features |


- 选定主路径（① / ② / ③ / ④ / ⑤ 之一或组合）
- 选定参考样板（FreeBSD LinuxKPI / Fuchsia DFv2 / arceos / embedded-hal）

---

## §14 词典（关键术语速查）

| 术语 | 全称 / 含义 | 出现章节 |
|------|------------|---------|
| **kobject** | 内核对象基类 (Linux) | §1.1 |
| **sysfs** | kobject 的 VFS 表示 (`/sys`) | §1.2 |
| **DM** | Driver Model（Linux）/ Driver Model（U-Boot）| §1, §7.8.2 |
| **DM v2** | DFv2 = Driver Framework v2（Fuchsia）| §7.6 |
| **DDI/DKI** | Device Driver Interface / Driver Kernel Interface (Solaris) | §7.5 |
| **WDM** | Windows Driver Model | §6.3 |
| **KMDF** | Kernel-Mode Driver Framework (Windows) | §6.3 |
| **UMDF** | User-Mode Driver Framework (Windows) | §6.3 |
| **WDF** | Windows Driver Framework = KMDF + UMDF | §6.3 |
| **IRP** | I/O Request Packet (Windows) | §6.6 |
| **IRQL** | Interrupt Request Level (Windows) | §6.8 |
| **KSPIN_LOCK** | Windows 自旋锁 | §6.10 |
| **MDL** | Memory Descriptor List (Windows DMA) | §6.6 |
| **DPC** | Deferred Procedure Call (Windows IRQL 2) | §6.8 |
| **SCM** | Service Control Manager (Windows) | §6.2 |
| **WHQL** | Windows Hardware Quality Lab（签名认证）| §6.13 |
| **KMCS** | Kernel-Mode Code Signing (Windows) | §6.13 |
| **EXPORT_SYMBOL** | Linux 导出符号宏（含 _GPL 变种）| §4.2 |
| **modversions** | Linux 模块符号 CRC 校验 | §10.1 |
| **DKMS** | Dynamic Kernel Module Support | §4.5 |
| **of_match_table** | Linux 驱动 DT compatible 列表 | §3.1 |
| **uclass** | U-Boot DM 中的 device class | §7.8.2 |
| **udevice** | U-Boot DM 中的 device | §7.8.2 |
| **chip ops** | coreboot 编译期 device tree 抽象 | §7.8.5 |
| **DDX** | Device Dependent X driver（X.Org）| §7.10（图形栈见 00-23）|
| **FIDL** | Fuchsia Interface Definition Language | §7.6 |
| **HIDL/AIDL** | Android Interface Definition Language | §7.7 |
| **NDIS** | Network Driver Interface Specification (Windows 网卡)| §9.8 |
| **LinuxKPI** | FreeBSD Linux Kernel Programming Interface 兼容层 | §9.3 |
| **SPL** | Solaris Porting Layer | §9.4 |
| **rump kernel** | NetBSD 内核子系统的用户态 lib | §9.5 |
| **VFIO** | Virtual Function I/O (Linux 设备直通) | §8.5 |
| **KABI** | Kernel ABI（Linux ABI 政策稳定性）| §6.14 |
| **GKI** | Generic Kernel Image（Android KABI 稳定）| §6.14 |
| **embedded-hal** | rust-embedded 跨硬件 trait 集合 | §7.8.6 |

---

## §15 练习题

### 题 1：source-level vs ABI 级判断

下列场景，应该选哪条兼容路径？

| 场景 | 路径 | 理由 |
|------|------|------|

**参考答案：** ② source-level（FreeBSD LinuxKPI 路径）/ ① ABI（但代价极高，多数选择 ⑤ VFIO）/ ⑤ VFIO / ④ API 重写（Fuchsia 风）/ ③ shim 桥接（ndiswrapper 思路）

### 题 2：跨层级驱动复用


**参考答案：**
- U-Boot DM 证明了"Linux 风 driver 模型可裁剪到 KB 级 bootloader"（详见 §7.8.2）
- embedded-hal + Embassy 证明了"trait + 编译期 monomorphization 可跨 RTOS/裸机"（详见 §7.8.6）
- arceos device crate 证明了"同 driver 编进 unikernel/monolithic/hypervisor 三模式"（详见 §7.8.7）

### 题 3：`.sys` vs `.ko` 关键差异

为什么"几乎没有非 Linux OS 直接加载 .ko 二进制"，但"ReactOS 能直接加载 Windows .sys"？

**参考答案：**
- `.ko` ABI 不稳定（Linux KABI 主动改）→ 加载方需要持续跟版本维护，代价过高
- `.sys` ABI 稳定（KMDF v1.x 25 年兼容 + Microsoft 保稳定）→ ReactOS 复刻一次后长期可用
- 设计哲学差异：Linux 鼓励驱动进 mainline / Windows 鼓励第三方稳定生态

### 题 4：跨 OS 形态复用难度

按"复用难度从低到高"排序：裸机 / 宏内核 / 微内核 / 外核 / 组件化（arceos） / Unikernel

**参考答案：** 组件化 < Unikernel < 宏内核 ≈ RTOS（有框架）< 微内核 < RTOS（无框架）< 裸机 < 外核

理由（详见 §7.11）：
- 组件化（arceos）天生设计跨形态
- Unikernel（unikraft）microlib 风易拆分
- 宏内核 / RTOS 有 framework 但耦合内核
- 微内核改用户态 + IPC 通信，driver 形式变
- 裸机 / 外核 没有公共 driver 概念

---



| 笔记 | 主题 | 本地仓库 |
|------|------|---------|
| G1 | Linux 驱动模型源码精读（kobject/sysfs/device/driver/bus/class）| fs/linux-fs（sparse drivers/base ✅ 已扩展）|
| G2 | 内核驱动子系统精读（platform/I2C/SPI/PCI/USB/MMC 各 bus framework）| fs/linux-fs |
| G3 | FreeBSD LinuxKPI 精读（source-level 工业最成功案例）| core/freebsd（✅ 已 clone）|
| G4 | ReactOS 精读（ABI 级 Windows .sys 兼容唯一活案例）| core/reactos（✅ 已 clone）|
| G5 | illumos SPL 精读（ZFS-on-Linux 反向 source-level）| core/illumos（✅ 已 clone）|
| G6 | Rust for Linux 深度（kernel crate / module! 宏 / driver trait）| core/rust-for-linux（✅ 已 clone）|
| G7 | Fuchsia DFv2 精读（用户态驱动 + FIDL 稳定 ABI）| core/fuchsia（✅ 已 clone）|
| G8 | Windows WDF 精读（KMDF + UMDF 统一框架）| 无本地源码，靠 Microsoft 文档 + ReactOS 对照 |


---

