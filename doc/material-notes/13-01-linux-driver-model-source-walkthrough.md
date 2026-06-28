# 13-01 Linux 驱动模型源码精读（G1）

> **本文位置：** 13 大类（驱动深度精读，G 系列首篇）
>
>
> **本地仓库：** `fs/linux-fs/`（sparse-checkout，drivers/base + include/linux + Documentation/driver-api 等已扩展）
>
> **与上层笔记关系：**
> - 上层概览：[00-13 §1 类 Linux 驱动系统全谱](00-13-driver-system-design-and-compat.md)（设计维度）
> - 演化背景：[00-12 设备/驱动模型演化史](00-12-device-driver-evolution.md)（历史维度）
> - 配套：[03-04 DTS/DTB/FDT 语法格式 API 参考](03-04-dts-dtb-fdt-syntax-reference.md)（设备树驱动 probe 的 DT 端）
>

---

## §0 总览：为什么读 drivers/base/

### §0.1 drivers/base/ 是 Linux 驱动模型的"基础设施"

Linux 驱动模型的所有抽象（kobject / device / driver / bus / class）都实现在 `drivers/base/` 目录下 —— 这是 **Linux 驱动子系统的基石**，被 PCI / USB / I2C / SPI / platform / virtio / 等几十种 bus 共享调用。

### §0.2 仓库定位

```
fs/linux-fs/                    ← Linux 内核 sparse checkout
├── drivers/base/               ← ⭐ 本文重点（驱动模型核心）
│   ├── core.c                  — device 层核心实现
│   ├── bus.c                   — bus_type 实现
│   ├── driver.c                — driver 层
│   ├── class.c                 — class 层
│   ├── dd.c                    — driver/device matching + bind/unbind
│   ├── devres.c                — devm_* 资源管理
│   ├── attribute_container.c   — 通用 sysfs 属性容器
│   ├── module.c                — 模块依赖追踪
│   ├── platform.c              — platform_bus_type
│   ├── component.c             — component framework（多设备组合驱动）
│   ├── auxiliary.c / auxiliary_sysfs.c — auxiliary bus（功能切分）
│   ├── faux.c                  — fake bus（无总线设备）
│   ├── devtmpfs.c              — /dev 文件系统（自动创建设备节点）
│   ├── firmware.c / firmware_loader/ — 固件加载
│   ├── topology.c / cacheinfo.c / cpu.c / memory.c / node.c / arch_*.c — CPU/内存/NUMA 拓扑暴露
│   ├── isa.c                   — legacy ISA bus
│   ├── container.c             — VMware-style hypervisor container
│   ├── devcoredump.c           — 设备核心 dump（调试）
│   ├── map.c                   — IO/内存映射
│   ├── hypervisor.c            — virt 子系统
│   ├── init.c                  — 子系统初始化
│   └── base.h                  — 内部头文件
├── include/linux/
│   ├── kobject.h (222 lines)   — kobject + kset
│   ├── device.h (1346 lines)   — device + driver + bus + class 全部
│   ├── sysfs.h (824 lines)     — sysfs 属性
│   ├── device/{bus,class,driver}.h — 子头文件
│   └── platform_device.h       — platform 总线
└── Documentation/driver-api/   — 详尽的驱动 API 文档（数十文件）
```

### §0.3 阅读顺序建议

按"自顶向下 + 内容连贯"原则：
1. **kobject 基础**（§1）— 所有抽象的根
2. **sysfs**（§2）— kobject 在 VFS 的具象化
3. **device + driver 数据结构**（§3）— 主战场
4. **bus_type**（§4）— 设备/驱动配对
5. **class**（§5）— 功能分类
6. **完整生命周期**（§6）— device_register → bus_match → driver_probe → unbind → device_destroy
7. **devres（资源自动管理）**（§7）— 现代驱动必用
8. **platform bus 实例**（§8）— 嵌入式 SoC 主战场
9. **Rust for Linux binding**（§9）— 现代演进

---

## §1 kobject：所有抽象的根（include/linux/kobject.h, 222 行）

### §1.1 struct kobject 完整定义（精确字段）

```c
// include/linux/kobject.h
struct kobject {
    const char        *name;
    struct list_head   entry;
    struct kobject    *parent;
    struct kset       *kset;
    const struct kobj_type *ktype;
    struct kernfs_node *sd;            // sysfs 目录节点
    struct kref        kref;           // 引用计数（atomic）
#ifdef CONFIG_DEBUG_KOBJECT_RELEASE
    struct delayed_work release;
#endif
    unsigned int       state_initialized:1;
    unsigned int       state_in_sysfs:1;
    unsigned int       state_add_uevent_sent:1;
    unsigned int       state_remove_uevent_sent:1;
    unsigned int       uevent_suppress:1;
};
```

**字段含义（逐字段）：**

| 字段 | 类型 | 作用 |
|------|------|------|
| `name` | `const char *` | sysfs 目录名（如 "0000:00:1f.6"）|
| `entry` | `list_head` | 链入 kset->list（同 kset 兄弟链）|
| `parent` | `kobject*` | sysfs 父节点（决定 /sys 路径层次）|
| `kset` | `kset*` | 所属 kset（同类 kobject 容器）|
| `ktype` | `kobj_type*` | 类型信息（含 release/sysfs_ops/default_attrs）|
| `sd` | `kernfs_node*` | sysfs 目录的 kernfs 节点（VFS 端）|
| `kref` | `kref` | 原子引用计数 |
| `state_initialized` | bit | 已 kobject_init |
| `state_in_sysfs` | bit | 已 kobject_add（出现在 /sys）|
| `state_add_uevent_sent` | bit | KOBJ_ADD uevent 已发 |
| `state_remove_uevent_sent` | bit | KOBJ_REMOVE uevent 已发 |
| `uevent_suppress` | bit | 临时屏蔽 uevent |

### §1.2 struct kobj_type（类型描述）

```c
struct kobj_type {
    void (*release)(struct kobject *kobj);
    const struct sysfs_ops *sysfs_ops;
    const struct attribute_group **default_groups;
    const struct kobj_ns_type_operations *(*child_ns_type)(const struct kobject *kobj);
    const void *(*namespace)(const struct kobject *kobj);
    void (*get_ownership)(const struct kobject *kobj, kuid_t *uid, kgid_t *gid);
};
```

- `release()` — kref 减到 0 时调（释放包含 kobject 的外层结构）
- `sysfs_ops` — sysfs 文件读写回调（show/store）
- `default_groups` — 默认创建的 sysfs 属性组

### §1.3 sysfs_ops（属性文件 read/write 桥接）

```c
struct sysfs_ops {
    ssize_t (*show)(struct kobject *, struct attribute *, char *);
    ssize_t (*store)(struct kobject *, struct attribute *, const char *, size_t);
};
```

**用法：** 用户态 `cat /sys/.../some_attr` → VFS → kernfs → kobj_type->sysfs_ops->show(kobject, attribute, buf)

### §1.4 关键 API（kobject_*）

#### §1.4.1 初始化 + 添加
```c
void kobject_init(struct kobject *kobj, const struct kobj_type *ktype);
int  kobject_add(struct kobject *kobj, struct kobject *parent, const char *fmt, ...);
int  kobject_init_and_add(struct kobject *kobj, const struct kobj_type *ktype,
                          struct kobject *parent, const char *fmt, ...);
```

`kobject_init_and_add` 是常用合并版 —— 一次完成 init + add（出现在 sysfs）。

#### §1.4.2 引用计数
```c
struct kobject *kobject_get(struct kobject *kobj);     // kref_inc
void            kobject_put(struct kobject *kobj);     // kref_dec → 0 时调 release
struct kobject *kobject_get_unless_zero(struct kobject *kobj);  // race-safe
```

**重要：** 持有 `kobject *` 引用必须 `kobject_get`。释放时 `kobject_put`。**最后一个 put 触发 release callback** —— 释放外层 struct（如 struct device）。

#### §1.4.3 移除
```c
void kobject_del(struct kobject *kobj);
```

从 sysfs 摘除（不释放内存，等 kobject_put 才释放）。

### §1.5 kset：kobject 容器

```c
struct kset {
    struct list_head           list;
    spinlock_t                 list_lock;
    struct kobject             kobj;             // ⭐ kset 本身也是 kobject
    const struct kset_uevent_ops *uevent_ops;
};

struct kset_uevent_ops {
    int  (*const filter)(const struct kobject *kobj);
    const char *(*const name)(const struct kobject *kobj);
    int  (*const uevent)(const struct kobject *kobj, struct kobj_uevent_env *env);
};
```

**典型 kset：**
- `bus_kset` —— `/sys/bus/`，所有总线 kset
- `class_kset` —— `/sys/class/`，所有 class kset
- `devices_kset` —— `/sys/devices/`，所有 device kset
- `module_kset` —— `/sys/module/`，所有模块 kset

**关键：kset 自身是 kobject** —— 这意味着 kset 也出现在 sysfs 中，可以有自己的属性。kset 可嵌套（`bus->bus_kset` 内含每个 bus 的 kset）。

---

## §2 sysfs：kobject 在 VFS 的具象化（include/linux/sysfs.h, 824 行）

### §2.1 sysfs 类型分类

```c
// include/linux/sysfs.h
struct attribute {
    const char    *name;
    umode_t        mode;       // 0444 / 0644 等
};

struct attribute_group {
    const char       *name;
    umode_t        (*is_visible)(struct kobject *, struct attribute *, int);
    struct attribute   **attrs;
    struct bin_attribute  **bin_attrs;
};

struct bin_attribute {
    struct attribute  attr;
    size_t            size;
    void             *private;
    ssize_t  (*read)(struct file *, struct kobject *, struct bin_attribute *,
                     char *, loff_t, size_t);
    ssize_t  (*write)(struct file *, struct kobject *, struct bin_attribute *,
                      char *, loff_t, size_t);
    int      (*mmap)(struct file *, struct kobject *, struct bin_attribute *,
                     struct vm_area_struct *);
};
```

**3 种 sysfs 文件：**
1. **普通属性**（`struct attribute`）— 文本，行数少，通过 sysfs_ops 转发
2. **二进制属性**（`struct bin_attribute`）— 大数据 / mmap 友好
3. **属性组**（`struct attribute_group`）— 多个 attr + 可见性过滤

### §2.2 DEVICE_ATTR / SHOW_STORE 宏

```c
// include/linux/sysfs.h（精简）
#define __ATTR(_name, _mode, _show, _store) {        \
    .attr = {.name = __stringify(_name), .mode = VERIFY_OCTAL_PERMISSIONS(_mode) }, \
    .show   = _show,                                 \
    .store  = _store,                                \
}

#define DEVICE_ATTR(_name, _mode, _show, _store) \
    struct device_attribute dev_attr_##_name = __ATTR(_name, _mode, _show, _store)

#define DEVICE_ATTR_RO(_name) \
    struct device_attribute dev_attr_##_name = __ATTR_RO(_name)
#define DEVICE_ATTR_WO(_name) \
    struct device_attribute dev_attr_##_name = __ATTR_WO(_name)
#define DEVICE_ATTR_RW(_name) \
    struct device_attribute dev_attr_##_name = __ATTR_RW(_name)
```

**标准用法：**
```c
static ssize_t my_attr_show(struct device *dev, struct device_attribute *attr, char *buf) {
    return sprintf(buf, "%d\n", some_value);
}
static DEVICE_ATTR_RO(my_attr);

static struct attribute *my_attrs[] = {
    &dev_attr_my_attr.attr,
    NULL,
};
ATTRIBUTE_GROUPS(my);  // 生成 my_groups[] 数组
```

### §2.3 sysfs 实际由 kernfs 实现

`drivers/base/` 不直接调 VFS —— 通过中间层 **kernfs**（`fs/kernfs/`）：
```
sysfs (drivers/base/) → kernfs (fs/kernfs/) → VFS
                          ↑
                同样被 cgroupfs / bpffs 等用
```

kernfs 提供"内存中目录树 + on-demand 文件"的通用基础设施。

---

## §3 device + driver 数据结构（include/linux/device.h, 1346 行）

### §3.1 struct device 完整字段（精读）

`include/linux/device.h` 是核心头文件，1346 行，含 device + driver + bus + class + dev_pm_ops 等所有抽象。

```c
struct device {
    struct kobject         kobj;
    struct device         *parent;
    struct device_private *p;

    const char            *init_name;       // 初始名（probe 后由 dev_name(dev) 取代）
    const struct device_type *type;

    struct bus_type       *bus;             // 所属总线
    struct device_driver  *driver;          // 当前 bound driver（NULL 即未绑定）
    void                  *platform_data;   // 平台传过来的私有数据
    void                  *driver_data;     // 驱动私有数据（dev_set_drvdata）
    struct mutex           mutex;

    struct dev_links_info  links;
    struct dev_pm_info     power;
    struct dev_pm_domain  *pm_domain;
#ifdef CONFIG_ENERGY_MODEL
    struct em_perf_domain *em_pd;
#endif
#ifdef CONFIG_GENERIC_MSI_IRQ
    struct dev_msi_info    msi;
#endif
#ifdef CONFIG_DMA_OPS
    const struct dma_map_ops *dma_ops;
#endif
    u64                   *dma_mask;        // 最大 DMA 物理地址
    u64                    coherent_dma_mask;
    u64                    bus_dma_limit;
    const struct bus_dma_region *dma_range_map;
    struct device_dma_parameters *dma_parms;
    struct list_head       dma_pools;
#ifdef CONFIG_DMA_DECLARE_COHERENT
    struct dma_coherent_mem *dma_mem;
#endif
#ifdef CONFIG_DMA_CMA
    struct cma             *cma_area;
#endif
#ifdef CONFIG_SWIOTLB
    struct io_tlb_mem      *dma_io_tlb_mem;
#endif
    struct dev_archdata    archdata;

    struct device_node    *of_node;         // ⭐ DT 节点
    struct fwnode_handle  *fwnode;          // ACPI / DT 抽象
    int                    numa_node;       // NUMA 节点编号
    dev_t                  devt;            // major/minor
    u32                    id;
    spinlock_t             devres_lock;
    struct list_head       devres_head;
    struct class          *class;           // 所属 class（可空）
    const struct attribute_group **groups;
    void  (*release)(struct device *dev);   // kref→0 时调
    struct iommu_group    *iommu_group;
    struct dev_iommu      *iommu;
    struct device_physical_location *physical_location;
    enum device_removable  removable;
    bool                   offline_disabled:1;
    bool                   offline:1;
    bool                   of_node_reused:1;
    bool                   state_synced:1;
    bool                   can_match:1;
#if defined(CONFIG_ARCH_HAS_SYNC_DMA_FOR_DEVICE) || ...
    bool                   dma_coherent:1;
#endif
#ifdef CONFIG_DMA_OPS_BYPASS
    bool                   dma_ops_bypass:1;
#endif
};
```

**字段重点（按用途归类）：**

| 类别 | 字段 |
|------|------|
| **身份** | kobj.name / init_name / id / devt / type |
| **关系** | parent / bus / driver / class |
| **数据** | platform_data / driver_data / p (private) |
| **DT/ACPI** | of_node / fwnode |
| **DMA** | dma_ops / dma_mask / coherent_dma_mask / dma_range_map / dma_pools / dma_parms |
| **电源** | power / pm_domain / em_pd |
| **中断** | msi |
| **NUMA** | numa_node |
| **IOMMU** | iommu_group / iommu |
| **物理位置** | physical_location / removable |
| **资源管理** | devres_head / devres_lock（devm_* 自动释放）|
| **生命周期** | release callback |

### §3.2 struct device_driver

```c
struct device_driver {
    const char            *name;
    const struct bus_type *bus;
    struct module         *owner;
    const char            *mod_name;
    bool                   suppress_bind_attrs;
    enum probe_type        probe_type;
    const struct of_device_id *of_match_table;
    const struct acpi_device_id *acpi_match_table;
    int  (*probe)         (struct device *dev);
    void (*sync_state)    (struct device *dev);
    int  (*remove)        (struct device *dev);
    void (*shutdown)      (struct device *dev);
    int  (*suspend)       (struct device *dev, pm_message_t state);
    int  (*resume)        (struct device *dev);
    const struct attribute_group **groups;
    const struct attribute_group **dev_groups;
    const struct dev_pm_ops *pm;
    void (*coredump)      (struct device *dev);
    struct driver_private *p;
};
```

**核心 callback 5 个：**
- `probe(dev)` — 认领设备 + 初始化
- `remove(dev)` — 释放资源
- `shutdown(dev)` — 关机时快速安全态
- `suspend/resume(dev, state)` — 系统休眠/唤醒
- `sync_state(dev)` — 所有 consumer 都已 probe 完成时调（用于关闭 boot-loader 留下的资源）

### §3.3 of_device_id（DT 匹配表）

```c
struct of_device_id {
    char            name[32];
    char            type[32];
    char            compatible[128];
    const void     *data;       // 可携带 driver-specific 静态数据
};

#define MODULE_DEVICE_TABLE(type, name)        \
    extern typeof(name) __mod_##type##__##name##_device_table \
    __attribute__ ((unused, alias(__stringify(name))))
```

`MODULE_DEVICE_TABLE` 让 `depmod` 工具扫描 .ko 自动生成 `modules.alias` 文件 —— hotplug 触发自动 modprobe。

### §3.4 driver_private（内部 bookkeeping）

```c
// drivers/base/base.h
struct driver_private {
    struct kobject     kobj;          // /sys/bus/<bus>/drivers/<drv>/
    struct klist       klist_devices;  // 此 driver bound 到的所有设备
    struct klist_node  knode_bus;
    struct module_kobject *mkobj;
    struct device_driver *driver;
};

struct device_private {
    struct klist                klist_children;     // 子设备
    struct klist_node           knode_parent;
    struct klist_node           knode_driver;
    struct klist_node           knode_bus;
    struct klist_node           knode_class;
    struct list_head            deferred_probe;     // 等待依赖的设备
    const struct device_driver *async_driver;
    char                       *deferred_probe_reason;
    struct device              *device;
    u8                          dead:1;
};
```

**重要：** `device_private` 和 `driver_private` 是内部数据，外部代码不直接访问。`drivers/base/base.h` 里定义。

---

## §4 bus_type（设备/驱动配对核心）（drivers/base/bus.c）

### §4.1 struct bus_type 完整定义

```c
struct bus_type {
    const char        *name;
    const char        *dev_name;
    const struct attribute_group **bus_groups;
    const struct attribute_group **dev_groups;
    const struct attribute_group **drv_groups;

    int  (*match)     (struct device *dev, const struct device_driver *drv);
    int  (*uevent)    (const struct device *dev, struct kobj_uevent_env *env);
    int  (*probe)     (struct device *dev);
    void (*sync_state)(struct device *dev);
    void (*remove)    (struct device *dev);
    void (*shutdown)  (struct device *dev);

    int  (*online)    (struct device *dev);
    int  (*offline)   (struct device *dev);

    int  (*suspend)   (struct device *dev, pm_message_t state);
    int  (*resume)    (struct device *dev);

    int  (*num_vf)    (struct device *dev);

    int  (*dma_configure)(struct device *dev);
    void (*dma_cleanup)  (struct device *dev);

    const struct dev_pm_ops *pm;
    const struct iommu_ops  *iommu_ops;
    bool                     need_parent_lock;
};
```

### §4.2 bus_register（drivers/base/bus.c）核心流程

```c
// drivers/base/bus.c (精简)
int bus_register(const struct bus_type *bus) {
    int retval;
    struct subsys_private *priv;
    struct kobject *bus_kobj;

    priv = kzalloc(sizeof(*priv), GFP_KERNEL);
    if (!priv) return -ENOMEM;

    priv->bus = bus;
    bus->p = priv;

    BLOCKING_INIT_NOTIFIER_HEAD(&priv->bus_notifier);

    retval = kobject_set_name(&priv->subsys.kobj, "%s", bus->name);
    if (retval) goto out;

    priv->subsys.kobj.kset = bus_kset;
    priv->subsys.kobj.ktype = &bus_ktype;
    priv->drivers_autoprobe = 1;

    retval = kset_register(&priv->subsys);
    if (retval) goto out;

    retval = bus_create_file(bus, &bus_attr_uevent);

    /* 创建 /sys/bus/<bus>/devices/ */
    priv->devices_kset = kset_create_and_add("devices", NULL, bus_kobj);
    /* 创建 /sys/bus/<bus>/drivers/ */
    priv->drivers_kset = kset_create_and_add("drivers", NULL, bus_kobj);

    klist_init(&priv->klist_devices, klist_devices_get, klist_devices_put);
    klist_init(&priv->klist_drivers, NULL, NULL);

    retval = add_probe_files(bus);

    retval = sysfs_create_groups(bus_kobj, bus->bus_groups);

    pr_debug("bus: '%s': registered\n", bus->name);
    return 0;
out:
    kfree(priv);
    return retval;
}
EXPORT_SYMBOL_GPL(bus_register);
```

**步骤：**
1. 分配 `subsys_private`（per-bus 内部数据）
2. 设置 kobject name = bus name
3. 注册到 `bus_kset`（`/sys/bus/`）
4. 创建 `/sys/bus/<bus>/devices/` 和 `/drivers/` kset
5. 初始化 `klist_devices` 和 `klist_drivers`（设备 + 驱动列表）
6. 创建 sysfs 默认属性

### §4.3 bus_for_each_drv / bus_for_each_dev

```c
// drivers/base/bus.c
int bus_for_each_drv(const struct bus_type *bus, struct device_driver *start,
                     void *data, int (*fn)(struct device_driver *, void *)) {
    struct klist_iter i;
    struct device_driver *drv;
    int error = 0;

    if (!bus) return -EINVAL;

    klist_iter_init_node(&bus->p->klist_drivers, &i,
                          start ? &start->p->knode_bus : NULL);
    while ((drv = next_driver(&i)) && !error)
        error = fn(drv, data);
    klist_iter_exit(&i);
    return error;
}
```

**用途：** 当一个 device 注册到 bus 时，遍历该 bus 下所有 driver 找匹配。

---

## §5 driver_register / device_register / 完整生命周期（drivers/base/dd.c）

### §5.1 device_register

```c
// drivers/base/core.c
int device_register(struct device *dev) {
    device_initialize(dev);
    return device_add(dev);
}
EXPORT_SYMBOL_GPL(device_register);

int device_add(struct device *dev) {
    struct device *parent;
    struct kobject *kobj;
    struct class_interface *class_intf;
    int error = -EINVAL;
    bool is_fwnode_dev = false;

    dev = get_device(dev);                    // kref_inc
    if (!dev) return -ENOMEM;

    if (!dev->p) {
        error = device_private_init(dev);     // 分配 device_private
        if (error) goto done;
    }

    /* 设置 kobj name */
    if (dev->init_name) {
        dev_set_name(dev, "%s", dev->init_name);
        dev->init_name = NULL;
    }

    parent = get_device(dev->parent);
    kobj = get_device_parent(dev, parent);   // sysfs 父节点
    if (kobj) dev->kobj.parent = kobj;

    /* 注册到 sysfs */
    error = kobject_add(&dev->kobj, dev->kobj.parent, NULL);
    if (error) goto Error;

    /* uevent */
    error = device_create_file(dev, &dev_attr_uevent);

    /* 创建 sysfs symbolic links（subsystem / device）*/
    error = device_add_class_symlinks(dev);
    error = device_add_attrs(dev);

    /* 加入 bus 的 klist_devices */
    error = bus_add_device(dev);

    /* 把 dev 加入 parent 的 klist_children */
    error = device_pm_add(dev);
    error = device_add_attrs(dev);

    /* ⭐ 核心：触发 driver 匹配 */
    bus_probe_device(dev);

    /* 通知 uevent + 加入 devices_kset */
    if (parent) klist_add_tail(&dev->p->knode_parent, &parent->p->klist_children);

    if (dev->class) {
        klist_add_tail(&dev->p->knode_class,
                        &dev->class->p->klist_devices);
    }

    kobject_uevent(&dev->kobj, KOBJ_ADD);    // 通知 udev
done:
    put_device(dev);
    return error;
Error:
    /* ... 错误处理 ... */
}
```

**关键：`bus_probe_device(dev)`** 是触发 driver 匹配的主入口。

### §5.2 bus_probe_device → driver_probe_device

```c
// drivers/base/bus.c
void bus_probe_device(struct device *dev) {
    const struct bus_type *bus = dev->bus;
    struct subsys_interface *sif;

    if (!bus) return;

    if (bus->p->drivers_autoprobe)
        device_initial_probe(dev);

    mutex_lock(&bus->p->mutex);
    list_for_each_entry(sif, &bus->p->interfaces, node)
        if (sif->add_dev) sif->add_dev(dev, sif);
    mutex_unlock(&bus->p->mutex);
}

void device_initial_probe(struct device *dev) {
    __device_attach(dev, true);
}
```

`__device_attach` 走遍 bus 的 driver list 找匹配：
```c
// drivers/base/dd.c
static int __device_attach(struct device *dev, bool allow_async) {
    int ret = 0;
    bool async = false;

    device_lock(dev);
    if (dev->p->dead) { ... }
    if (dev->driver) {                       // 已 bound
        ret = device_bind_driver(dev);
        goto out_unlock;
    }

    if (dev->bus) {
        struct device_attach_data data = {
            .dev = dev,
            .check_async = allow_async,
            .want_async = false,
        };

        if (dev->parent) pm_runtime_get_sync(dev->parent);

        ret = bus_for_each_drv(dev->bus, NULL, &data, __device_attach_driver);

        if (!ret && allow_async && data.have_async) {
            ret = bus_for_each_drv(dev->bus, NULL, &data, __device_attach_async_helper);
            async = true;
        }

        if (dev->parent) pm_runtime_put(dev->parent);
    }
out_unlock:
    device_unlock(dev);
    return ret;
}
```

每个 driver 调 `__device_attach_driver`：
```c
static int __device_attach_driver(struct device_driver *drv, void *_data) {
    struct device_attach_data *data = _data;
    struct device *dev = data->dev;
    int ret;

    ret = driver_match_device(drv, dev);     // 调 bus->match()
    if (ret == 0) return 0;                  // 不匹配
    if (ret < 0) return ret;                 // 错误（推迟 probe）

    if (driver_allows_async_probing(drv)) {
        if (data->check_async) {
            data->have_async = true;
            return 0;
        }
    }

    return driver_probe_device(drv, dev);    // ⭐ 真正调 driver->probe
}
```

### §5.3 driver_probe_device 核心

```c
// drivers/base/dd.c
int driver_probe_device(const struct device_driver *drv, struct device *dev) {
    int ret = 0;

    if (!device_is_registered(dev)) return -ENODEV;

    dev_dbg(dev, "matched driver %s\n", drv->name);

    pm_runtime_get_sync(dev->parent);
    pm_runtime_barrier(dev);
    if (initcall_debug) ret = really_probe_debug(dev, drv);
    else ret = really_probe(dev, drv);
    pm_request_idle(dev);
    pm_runtime_put(dev->parent);

    return ret;
}

static int really_probe(struct device *dev, const struct device_driver *drv) {
    int ret = -EPROBE_DEFER;
    int local_trigger_count = atomic_read(&deferred_trigger_count);
    bool test_remove = IS_ENABLED(CONFIG_DEBUG_TEST_DRIVER_REMOVE) && !drv->suppress_bind_attrs;

    if (defer_all_probes) return ret;

    ret = device_links_check_suppliers(dev);
    if (ret == -EPROBE_DEFER) driver_deferred_probe_add_trigger(dev, local_trigger_count);
    if (ret) return ret;

    atomic_inc(&probe_count);
    pr_debug("bus: '%s': %s: probing driver %s with device %s\n",
             drv->bus->name, __func__, drv->name, dev_name(dev));

    if (!list_empty(&dev->devres_head)) {
        dev_crit(dev, "Resources present before probing\n");
        ret = -EBUSY;
        goto done;
    }

re_probe:
    dev->driver = (struct device_driver *)drv;          // 标记 bound
    ret = device_links_check_suppliers(dev);
    if (ret == -EPROBE_DEFER) goto pinctrl_bind_failed;
    if (ret) goto pinctrl_bind_failed;

    ret = dma_configure(dev);
    if (ret) goto probe_failed;

    if (driver_sysfs_add(dev)) goto probe_failed;

    if (dev->pm_domain && dev->pm_domain->activate) { ... }

    if (drv->probe) {                                   // ⭐ 调 driver->probe
        ret = drv->probe(dev);
        if (ret) goto probe_failed;
    } else if (dev->bus->probe) {
        ret = dev->bus->probe(dev);
        if (ret) goto probe_failed;
    }

    if (test_remove) { ... }                            // CONFIG_DEBUG_TEST_DRIVER_REMOVE 反复 probe/remove 测

    pinctrl_init_done(dev);

    if (dev->pm_domain && dev->pm_domain->sync) dev->pm_domain->sync(dev);

    driver_bound(dev);                                   // 加入 driver->klist_devices

    pr_debug("bus: '%s': %s: bound device %s to driver %s\n",
             drv->bus->name, __func__, dev_name(dev), drv->name);
    goto done;

pinctrl_bind_failed:
    /* ... */
probe_failed:
    /* unwind */
done:
    atomic_dec(&probe_count);
    wake_up_all(&probe_waitqueue);
    return ret;
}
```

**核心步骤：**
1. 检查 supplier 设备已 probe（device link）
2. dma_configure（设置 DMA mask / IOMMU）
3. driver_sysfs_add（创建 /sys/bus/<bus>/drivers/<drv>/ symlink）
4. **调 `drv->probe(dev)`** ⭐
5. driver_bound（加入 driver->klist_devices）

**EPROBE_DEFER 机制：** 如果 probe 返回 -EPROBE_DEFER，dev 加入 `deferred_probe_pending_list`，等其它设备 probe 完后再试 —— 解决"驱动 A 依赖驱动 B 还没 probe"问题。

### §5.4 driver_register

```c
// drivers/base/driver.c
int driver_register(struct device_driver *drv) {
    int ret;
    struct device_driver *other;

    if (!drv->bus->p) {
        pr_err("Driver '%s' was unable to register with bus_type '%s' because the bus was not initialized.\n",
               drv->name, drv->bus->name);
        return -EINVAL;
    }

    if ((drv->bus->probe && drv->probe) || (drv->bus->remove && drv->remove) ||
        (drv->bus->shutdown && drv->shutdown))
        pr_warn("Driver '%s' needs updating - please use bus_type methods\n", drv->name);

    other = driver_find(drv->name, drv->bus);
    if (other) {
        pr_err("Error: Driver '%s' is already registered, aborting...\n", drv->name);
        return -EBUSY;
    }

    ret = bus_add_driver(drv);                          // 加入 bus->klist_drivers
    if (ret) return ret;

    ret = driver_add_groups(drv, drv->groups);
    if (ret) {
        bus_remove_driver(drv);
        return ret;
    }

    kobject_uevent(&drv->p->kobj, KOBJ_ADD);            // 通知 udev
    deferred_probe_extend_timeout();

    return ret;
}
```

`bus_add_driver` 进入 `bus->p->klist_drivers` —— 让后续 device_register 能找到这个 driver；同时**对所有已注册设备遍历检查匹配**：
```c
// drivers/base/bus.c
int bus_add_driver(struct device_driver *drv) {
    /* ... */
    if (drv->bus->p->drivers_autoprobe) {
        error = driver_attach(drv);                     // 遍历 bus->klist_devices
        /* ... */
    }
    /* ... */
}

int driver_attach(struct device_driver *drv) {
    return bus_for_each_dev(drv->bus, NULL, drv, __driver_attach);
}

static int __driver_attach(struct device *dev, void *data) {
    struct device_driver *drv = data;
    int ret;

    ret = driver_match_device(drv, dev);
    if (ret == 0) return 0;
    if (ret < 0) return ret;

    device_lock(dev);
    if (!dev->driver && !dev->p->dead) driver_probe_device(drv, dev);
    device_unlock(dev);

    return 0;
}
```

**总结：** device 和 driver 注册是**对称的** —— 都遍历对方列表找匹配。

### §5.5 完整生命周期图

```
┌─────────────────────────────────────────────────────────────┐
│                    device 加入流程                          │
├─────────────────────────────────────────────────────────────┤
│ device_register(dev)                                        │
│   ↓ device_initialize(dev) — kobject_init                   │
│   ↓ device_add(dev)                                         │
│       ↓ device_private_init                                 │
│       ↓ kobject_add — 进 sysfs                              │
│       ↓ device_create_file(uevent)                          │
│       ↓ device_add_class_symlinks                           │
│       ↓ bus_add_device — 进 bus->klist_devices              │
│       ↓ device_pm_add — 进 power 子系统                     │
│       ↓ ⭐ bus_probe_device — 触发 driver match              │
│           ↓ device_initial_probe                            │
│           ↓ __device_attach                                 │
│           ↓ bus_for_each_drv(__device_attach_driver)        │
│           ↓ driver_match_device(drv, dev) — 调 bus->match() │
│           ↓ driver_probe_device                             │
│           ↓ really_probe                                    │
│           ↓ ⭐ drv->probe(dev) — 真正驱动 probe              │
│           ↓ driver_bound — 加入 driver->klist_devices       │
│       ↓ kobject_uevent(KOBJ_ADD) — 通知 udev               │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                  driver 加入流程（对称）                    │
├─────────────────────────────────────────────────────────────┤
│ driver_register(drv)                                        │
│   ↓ bus_add_driver                                          │
│       ↓ 进 bus->klist_drivers                               │
│       ↓ driver_attach                                       │
│       ↓ bus_for_each_dev(__driver_attach)                   │
│       ↓ driver_match_device(drv, dev) — 调 bus->match()     │
│       ↓ driver_probe_device                                 │
│       ↓ really_probe                                        │
│       ↓ ⭐ drv->probe(dev)                                   │
└─────────────────────────────────────────────────────────────┘
```

### §5.6 unbind / device_release

```c
// drivers/base/dd.c
static void __device_release_driver(struct device *dev, struct device *parent) {
    const struct device_driver *drv;

    drv = dev->driver;
    if (drv) {
        pm_runtime_get_sync(dev);
        while (device_links_busy(dev)) {
            __device_driver_unlock(dev, parent);
            device_links_unbind_consumers(dev);
            __device_driver_lock(dev, parent);
        }

        device_links_driver_cleanup(dev);

        if (drv->remove) drv->remove(dev);              // ⭐ driver->remove
        else if (dev->bus->remove) dev->bus->remove(dev);

        device_remove(dev);
        if (dev->bus) blocking_notifier_call_chain(&dev->bus->p->bus_notifier,
                                                    BUS_NOTIFY_UNBOUND_DRIVER, dev);
        kobject_uevent(&dev->kobj, KOBJ_UNBIND);
        klist_remove(&dev->p->knode_driver);
        device_pm_check_callbacks(dev);

        dev->driver = NULL;                              // 标记未 bound
        dev_set_drvdata(dev, NULL);
        if (dev->pm_domain && dev->pm_domain->dismiss)
            dev->pm_domain->dismiss(dev);
        pm_runtime_reinit(dev);
        dev_pm_set_driver_flags(dev, 0);

        klist_remove(&dev->p->knode_driver);
    }
}
```

**关键：** unbind 时调 `drv->remove()`，driver 释放资源 + dev->driver = NULL 标记未 bound。

---

## §6 class（功能分类，drivers/base/class.c）

```c
// include/linux/device/class.h
struct class {
    const char        *name;
    const struct attribute_group **class_groups;
    const struct attribute_group **dev_groups;
    int  (*dev_uevent)    (const struct device *dev, struct kobj_uevent_env *env);
    char *(*devnode)      (const struct device *dev, umode_t *mode);
    void (*class_release) (const struct class *class);
    void (*dev_release)   (struct device *dev);
    int  (*shutdown_pre)  (struct device *dev);
    const struct kobj_ns_type_operations *ns_type;
    const void *(*namespace)(const struct device *dev);
    void (*get_ownership) (const struct device *dev, kuid_t *uid, kgid_t *gid);
    const struct dev_pm_ops *pm;
};

int class_register(const struct class *cls);
void class_unregister(const struct class *cls);
```

### §6.1 class_register（drivers/base/class.c 节选）

```c
int class_register(const struct class *cls) {
    struct subsys_private *cp;
    struct lock_class_key *key;
    int error;

    pr_debug("device class '%s': registering\n", cls->name);

    cp = kzalloc(sizeof(*cp), GFP_KERNEL);
    if (!cp) return -ENOMEM;

    klist_init(&cp->klist_devices, klist_class_dev_get, klist_class_dev_put);
    INIT_LIST_HEAD(&cp->interfaces);
    kset_init(&cp->glue_dirs);
    __mutex_init(&cp->mutex, "subsys mutex", key);
    error = kobject_set_name(&cp->subsys.kobj, "%s", cls->name);
    if (error) goto out;

    cp->subsys.kobj.kset = class_kset;                  // 挂在 class_kset 下
    cp->subsys.kobj.ktype = &class_ktype;
    cp->class = cls;

    error = kset_register(&cp->subsys);
    if (error) goto out;

    error = sysfs_create_groups(&cp->subsys.kobj, cls->class_groups);

    cls->p = cp;
    return 0;
out:
    kfree(cp);
    return error;
}
EXPORT_SYMBOL_GPL(class_register);
```

**典型 class：**
- `class_register(&net_class)` → `/sys/class/net/`
- `class_register(&tty_class)` → `/sys/class/tty/`
- `class_register(&input_class)` → `/sys/class/input/`
- `class_register(&block_class)` → `/sys/class/block/`

---

## §7 devres（设备资源自动释放，drivers/base/devres.c）

### §7.1 devres 概念

每个设备有 `devres_head`（链表）—— 注册的资源（IRQ / IO 映射 / 内存 / GPIO）跟着设备生命周期，**driver remove 时自动释放**，避免资源泄漏。

```c
// drivers/base/devres.c
struct devres_node {
    struct list_head     entry;
    dr_release_t         release;
#ifdef CONFIG_DEBUG_DEVRES
    const char          *name;
    size_t               size;
#endif
};

struct devres {
    struct devres_node   node;
    union {
        unsigned long long data[];
        struct {
            const struct device *dev;
            const void *data;
        };
    };
};
```

### §7.2 典型 devm_* API

```c
void *devm_kmalloc(struct device *dev, size_t size, gfp_t gfp);
void *devm_kzalloc(struct device *dev, size_t size, gfp_t gfp);
char *devm_kstrdup(struct device *dev, const char *s, gfp_t gfp);
int   devm_request_irq(struct device *dev, unsigned int irq, irq_handler_t handler,
                       unsigned long irqflags, const char *devname, void *dev_id);
void __iomem *devm_ioremap_resource(struct device *dev, const struct resource *res);
int   devm_gpio_request(struct device *dev, unsigned gpio, const char *label);
struct clk *devm_clk_get(struct device *dev, const char *id);
struct regulator *devm_regulator_get(struct device *dev, const char *id);
```

**驱动写法：**
```c
static int my_probe(struct device *dev) {
    void __iomem *base;
    int irq;

    base = devm_ioremap_resource(dev, res);   // 自动 unmap on remove
    if (IS_ERR(base)) return PTR_ERR(base);

    irq = platform_get_irq(pdev, 0);
    err = devm_request_irq(dev, irq, my_isr, 0, "my-drv", priv);  // 自动 free_irq

    return 0;  // 没有显式释放，devm 自动管理
}
```

**好处：** 错误处理路径大大简化，无需手动 unwind。

---

## §8 platform bus（嵌入式 SoC 主战场，drivers/base/platform.c）

### §8.1 platform_bus_type

```c
// drivers/base/platform.c
const struct bus_type platform_bus_type = {
    .name        = "platform",
    .dev_groups  = platform_dev_groups,
    .match       = platform_match,
    .uevent      = platform_uevent,
    .probe       = platform_probe,
    .remove      = platform_remove,
    .shutdown    = platform_shutdown,
    .dma_configure = platform_dma_configure,
    .dma_cleanup = platform_dma_cleanup,
    .pm          = &platform_dev_pm_ops,
};
EXPORT_SYMBOL_GPL(platform_bus_type);
```

### §8.2 platform_match（5 路 fallback）

```c
static int platform_match(struct device *dev, const struct device_driver *drv) {
    struct platform_device *pdev = to_platform_device(dev);
    struct platform_driver *pdrv = to_platform_driver(drv);

    /* 1. driver_override 高优先级（用户态强制）*/
    if (!strcmp(pdev->driver_override, drv->name)) return 1;

    /* 2. of_match_table（DT 平台）*/
    if (of_driver_match_device(dev, drv)) return 1;

    /* 3. acpi_match_table（ACPI 平台）*/
    if (acpi_driver_match_device(dev, drv)) return 1;

    /* 4. id_table（platform_device_id）*/
    if (pdrv->id_table) return platform_match_id(pdrv->id_table, pdev) != NULL;

    /* 5. name 匹配（最 fallback）*/
    return (strcmp(pdev->name, drv->name) == 0);
}
```

### §8.3 platform_get_resource / platform_get_irq

```c
struct resource *platform_get_resource(struct platform_device *dev,
                                        unsigned int type, unsigned int num) {
    u32 i;
    for (i = 0; i < dev->num_resources; i++) {
        struct resource *r = &dev->resource[i];
        if (type == resource_type(r) && num-- == 0) return r;
    }
    return NULL;
}
EXPORT_SYMBOL_GPL(platform_get_resource);

int platform_get_irq(struct platform_device *dev, unsigned int num) {
    int ret = platform_get_irq_optional(dev, num);
    if (ret < 0) return dev_err_probe(&dev->dev, ret, "IRQ index %u not found\n", num);
    return ret;
}
EXPORT_SYMBOL_GPL(platform_get_irq);
```

---

## §9 Rust for Linux binding（rust/kernel/）

### §9.1 kernel crate 主要模块

```
rust/kernel/
├── lib.rs                — crate 入口
├── module.rs             — module! 宏 + ThisModule
├── prelude.rs            — 常用 import
├── error.rs              — Result + Error 代数
├── alloc.rs / box_ext.rs — Rust alloc 包装
├── platform.rs           — platform_driver binding
├── pci.rs                — pci_driver binding
├── clk.rs / regulator.rs — clk / regulator binding
├── gpio/
├── chrdev.rs / miscdev.rs — char device / misc device
├── file.rs               — file_operations 包装
├── sync/                 — Mutex / SpinLock / Arc / Lock 等
├── workqueue.rs          — workqueue
├── irq.rs                — request_irq 包装
└── ...
```

### §9.2 module! 宏

```rust
// rust/kernel/macros/src/module.rs
#[proc_macro]
pub fn module(ts: TokenStream) -> TokenStream {
    let info = ModuleInfo::parse(&mut ts.into_iter().peekable());
    /* 展开为：
       __this_module 全局变量 +
       init_module / cleanup_module 函数 +
       MODULE_INFO 元数据
    */
}
```

**用户写法（重温）：**
```rust
use kernel::prelude::*;
use kernel::miscdev;

module! {
    type: HelloModule,
    name: "hello",
    author: "Rustacean",
    license: "GPL",
}

struct HelloModule { _miscdev: miscdev::Registration<HelloModule> }

impl kernel::Module for HelloModule {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("Hello from Rust!\n");
        let m = miscdev::Registration::new_pinned(c_str!("hello"), ())?;
        Ok(Self { _miscdev: m })
    }
}
```

### §9.3 platform driver 在 Rust 端

```rust
// rust/kernel/platform.rs (精简)
pub trait Driver {
    type IdInfo: 'static = ();
    const ID_TABLE: IdTable<Self::IdInfo>;
    fn probe(pdev: &mut Device, id_info: Option<&Self::IdInfo>) -> Result<Pin<Box<Self>>>;
}

pub struct Adapter<T: Driver> { /* ... */ }

impl<T: Driver> Adapter<T> {
    extern "C" fn probe_callback(pdev: *mut bindings::platform_device) -> i32 { /* 桥到 T::probe */ }
    extern "C" fn remove_callback(pdev: *mut bindings::platform_device) { /* Drop T */ }
}

#[macro_export]
macro_rules! module_platform_driver {
    ($($f:tt)*) => {
        kernel::module_driver!(<T>, kernel::platform::Adapter<T>, { $($f)* });
    };
}
```

**关键：** Rust driver 用 `Pin<Box<Self>>` 表示生命周期；remove 通过 Drop 自动调用。

---



|--------|-----------|---------------|
| 引用计数 | `kref` 原子 + release callback | 用 Rust Arc / 自实现 atomic kref |
| sysfs 暴露 | kobject + kernfs | 是否复刻 sysfs（可能太重，嵌入式形态可省）|
| device/driver 分离 | 两 struct + bus match | 同 / 合并 / trait 化 |
| bus_type | C struct 含 callback | Rust trait |
| match 5 路 fallback | DT > ACPI > id_table > name | 选哪些匹配方式 |
| EPROBE_DEFER | 链表 + 重试 | 是否需要（依赖关系处理）|
| devres 自动释放 | devm_* | Rust RAII 自动（更优雅）|
| platform bus | `platform_bus_type` 全局变量 | 第一个 bus 类型 |
| Rust 端 binding | kernel crate + Adapter pattern | 直接借鉴 |

---

## §11 后续 G 系列规划

| 笔记 | 主题 | 主要源码 |
|------|------|---------|
| G2 = 13-02 | platform / I2C / SPI / PCI / USB 子系统精读 | drivers/{i2c,spi,pci,usb}/ |
| G3 = 13-03 | FreeBSD LinuxKPI 精读（source-level 兼容工业最成功）| core/freebsd/sys/compat/linuxkpi/ |
| G4 = 13-04 | ReactOS 精读（ABI 级 Windows .sys 兼容）| core/reactos/ |
| G5 = 13-05 | illumos SPL 精读（反向 source-level）| core/illumos/ |
| G6 = 13-06 | Rust for Linux 深度（kernel crate 全谱）| core/rust-for-linux/rust/ |
| G7 = 13-07 | Fuchsia DFv2 精读（用户态驱动 + FIDL ABI）| core/fuchsia/ |
| G8 = 13-08 | Windows WDF 精读（KMDF + UMDF 文档对照）| Microsoft 文档 + ReactOS 对照 |

---

## §12 词典

| 术语 | 含义 |
|------|------|
| **kobject** | 内核对象基类 |
| **kset** | kobject 容器 |
| **kref** | 原子引用计数 |
| **kernfs** | sysfs / cgroupfs / bpffs 共用的内存 FS 后端 |
| **uevent** | kobject 状态变化通知 udev 用户态 |
| **klist** | 锁保护的 list（kobject 系统专用）|
| **devres** | 设备资源自动释放框架（devm_*）|
| **EPROBE_DEFER** | probe 推迟（等 supplier）|
| **device link** | supplier ↔ consumer 显式依赖 |
| **subsys_private** | bus / class 内部 bookkeeping |
| **driver_private** | driver 内部 bookkeeping |
| **device_private** | device 内部 bookkeeping |

---

