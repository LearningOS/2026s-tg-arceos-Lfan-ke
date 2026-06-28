# 14-01 X.Org server 深度精读（H1）

> **本文位置：** 14 大类（图形栈深度精读，H 系列首篇）
>
>
> **本地仓库：** `gui/xserver/`（2026-05-10 cloned from `https://gitlab.freedesktop.org/xorg/xserver.git`）
>
> **与上层笔记关系：**
> - 上层概览：[00-23 §2 X11 完整谱](00-23-graphics-ui-overview.md)（架构维度）
> - 演化背景：[00-22 显示窗口系统演化](00-22-display-evolution.md)（历史维度）
> - 配套：[00-13 § 驱动系统设计 + 兼容](00-13-driver-system-design-and-compat.md)（DRM/KMS 是驱动的具体使用案例）
>

---

## §0 总览：为什么读 X.Org server 源码

### §0.1 X.Org server 的工程意义

X.Org server（前身 XFree86）是 **40 年实战考验的图形服务器**。即使 Wayland 是 X 的官方接班人，X.Org 至今仍：
- 支撑 i3wm / xfce / lxde / mate 桌面
- 是大量商业 CAD / EDA / 远程桌面方案的基础
- Xwayland 复用其 90% 代码（让 X 应用跑 Wayland 桌面）

**核心问题：** X 协议看似简单（请求/响应/事件/错误 4 类消息）—— 但 X.Org server 实现含**150+ 万行 C** —— 复杂度从哪里来？答案：40 年累积的 22+ extensions + 多 hw backend + 数十种 input 设备 + DRM/DRI 协同 + 网络透明 + 安全沙箱。

### §0.2 仓库定位

```
gui/xserver/                          ← X.Org server 完整源码
├── dix/                              ⭐ Device-Independent X (协议核心)
│   ├── main.c (360 行)               — server main()
│   ├── dispatch.c (4135 行)          — protocol dispatch（协议主循环）
│   ├── events.c                      — 事件分发
│   ├── window.c / colormap.c / gc.c / cursor.c — 核心 X 对象
│   ├── extension.c                   — extension 注册
│   ├── devices.c / getevents.c       — 输入设备核心
│   └── ... 50+ 文件
├── os/                                — OS abstraction（IO/auth/log）
│   ├── connection.c                  — 客户端连接
│   ├── io.c                           — 协议 read/write
│   ├── WaitFor.c                     — main IO loop（select/poll/epoll）
│   ├── auth.c / mitauth.c            — X auth cookie
│   ├── log.c / backtrace.c           — 日志/异常
│   └── inputthread.c                 — 输入线程
├── hw/                                ⭐ Device-Dependent X (DDX)
│   ├── xfree86/                      — 经典 X.Org server (xorg)
│   │   ├── common/                   — DDX framework
│   │   ├── dixmods/ / loader/        — 模块加载
│   │   ├── modes/                    — modeset
│   │   ├── parser/                   — xorg.conf 解析
│   │   └── drivers/                  — DDX driver 头（具体 driver 在独立 packages）
│   ├── xwayland/                     — X server on Wayland（含上面 dix/os）
│   ├── kdrive/                       — embedded 微 X server
│   ├── vfb/                          — virtual fb（无显示器测试）
│   ├── xnest/                        — X server 嵌套（X-on-X）
│   ├── xquartz/                      — macOS Quartz 后端
│   └── xwin/                         — Windows 后端
├── Xext/                              — extensions（XSHM / XTest / SECURITY / SHM 等）
├── Xi/                                — XInput2 扩展（多触点 / pressure）
├── composite/ / damageext/ / dri3/   — 现代 compositor 必需扩展
├── glx/                               — GLX (OpenGL 通过 X)
├── dbe/ / fb/ / mi/ / miext/         — 后端实现 helpers
├── present/                           — Present extension（vsync）
├── glamor/                            — GPU 2D 加速（mesa-based）
├── exa/                               — 老 GPU 2D 加速（已 legacy）
├── render/                            — 矢量渲染
├── randr/                             — XRandR
├── record/                            — XRecord
├── xkb/                               — XKeyboard
├── doc/                               — 文档
└── meson.build                        — build system
```

**核心 3 层：** DIX（协议核心）+ os（系统抽象）+ hw（设备特定后端）。Xwayland 复用 dix + os，hw 用 wayland 后端。

### §0.3 阅读顺序

1. **server main()**（§1）— 入口
2. **协议主循环 dispatch.c**（§2）— ⭐ 核心
3. **事件 + 输入设备**（§3）— events.c / devices.c
4. **核心 X 对象**（§4）— window / GC / pixmap / colormap
5. **extension 框架**（§5）— extension.c
6. **DDX 架构**（§6）— xfree86 / xwayland / kdrive 横向
7. **GLAMOR + GLX + DRI3**（§7）— GPU 加速 + OpenGL 桥
8. **后续 H 系列规划**（§8）

---

## §1 server 入口（dix/main.c, 360 行）

### §1.1 main() 主流程

```c
// dix/main.c (节选)
int main(int argc, char *argv[], char *envp[]) {
    int i;
    HWEventQueueType alwaysCheckForInput[2];

    display = "0";

    InitRegions();
    CheckUserParameters(argc, argv, envp);
    CheckUserAuthorization();

    InitConnectionLimits();
    ProcessCommandLine(argc, argv);                 // -listen / -auth 等

    alwaysCheckForInput[0] = 0;
    alwaysCheckForInput[1] = 1;

    while (1) {                                      // ⭐ 主循环
        serverGeneration++;
        ScreenSaverTime = defaultScreenSaverTime;
        ScreenSaverInterval = defaultScreenSaverInterval;
        ScreenSaverBlanking = defaultScreenSaverBlanking;
        ScreenSaverAllowExposures = defaultScreenSaverAllowExposures;

        if (serverGeneration == 1) {
            CreateWellKnownSockets();                 // X11 socket
            for (i = 1; i < LimitClients; i++)
                clients[i] = NullClient;
            serverClient = malloc(sizeof(ClientRec));
            InitClient(serverClient, 0, (void *) NULL);
        } else {
            ResetWellKnownSockets();
        }

        clients[0] = serverClient;
        currentMaxClients = 1;

        /* 初始化各子系统 */
        InitAtoms();
        InitCallbackManager();
        InitOutput(&screenInfo, argc, argv);          // ⭐ 调 DDX 初始化（每个 backend）

        if (screenInfo.numScreens < 1) FatalError("no screens found");

        InitExtensions(argc, argv);                    // ⭐ 加载所有 extensions
        for (i = 0; i < screenInfo.numScreens; i++)
            InitRootWindow(screenInfo.screens[i]->root);

        InitCoreDevices();
        InitInput(argc, argv);                         // ⭐ 输入设备初始化
        InitAndStartDevices();
        ReserveClientIds(serverClient);

        dispatchException = 0;

        if (!SetDefaultFontPath(defaultFontPath)) ErrorF("failed to set default font path '%s'", defaultFontPath);

        if (!SetDefaultFont(defaultTextFont)) FatalError("could not open default font '%s'", defaultTextFont);

        if (!CreateGCperDepthPerScreen()) FatalError("failed to create scratch GCs");

        if (!CreateGCperDepthArray()) FatalError("failed to create scratch GCs");

        Dispatch();                                    // ⭐ 主协议派发（永不返回正常退出）

        /* 退出后清理 */
        UndisplayDevices();
        CloseDownExtensions();
        CloseDownDevices();
        CloseDownEvents();
        for (i = screenInfo.numScreens - 1; i >= 0; i--) {
            FreeAllResources(screenInfo.screens[i]->root, RT_NONE);
            screenInfo.screens[i]->CloseScreen(screenInfo.screens[i]);
            dixFreeScreenSpecificPrivates(screenInfo.screens[i]);
            free(screenInfo.screens[i]);
            screenInfo.numScreens = i;
        }

        /* clean up */
        FreeFonts();
        FreeAllAtoms();
        FreeAuditTimer();
        if (dispatchException & DE_TERMINATE) break;
    }
    return 0;
}
```

### §1.2 关键初始化函数

| 函数 | 文件 | 作用 |
|------|------|------|
| `InitOutput` | DDX backend（每 hw/ 各自实现）| 探测 GPU + 创建 ScreenRec[] |
| `InitExtensions` | Xext/ + dix/extension.c | 注册 GLX / XSHM / Composite / RANDR / ... 数十扩展 |
| `InitInput` | DDX backend | 探测键鼠 + 创建 DeviceIntRec[] |
| `Dispatch` | dix/dispatch.c | 主循环（read 客户端请求 + 派发）|

---

## §2 协议主循环（dix/dispatch.c, 4135 行）

### §2.1 Dispatch() 核心循环

```c
// dix/dispatch.c (节选)
void Dispatch(void) {
    int result;
    ClientPtr client;
    int nready, *clientReady;

    nextFreeClientID = 1;
    nClients = 0;

    clientReady = calloc(sizeof(int), MaxClients);
    if (!clientReady) return;

    while (!dispatchException) {
        if (ScreenSaverTime > 0) UpdateCurrentTimeIf();

        nready = WaitForSomething(clientReady);     // ⭐ select/poll/epoll 等待 IO

        if (nready && !SmartScheduleSignalEnable) {
            clientsDoingWork++;
            for (...) { /* schedule */ }
        }

        while (!dispatchException && (--nready >= 0)) {
            client = clients[clientReady[nready]];
            if (!client) continue;

            isItTimeToYield = FALSE;
            requestingClient = client;

            start_tick = SmartScheduleTime;
            while (!isItTimeToYield) {
                if (*server_client_changed) {
                    /* ... */
                }
                /* 读 X 协议 request */
                if (!XdmcpUseDestExpr || ...)
                    result = ReadRequestFromClient(client);
                if (result <= 0) {
                    if (result < 0) CloseDownClient(client);
                    break;
                }

                client->sequence++;

                /* ⭐ 调 protocol dispatch table */
                client->requestVector[MAJOROP](client);
                                ↑
                   MAJOROP = client->requestBuffer 第一字节
                   requestVector[] 是函数指针表，256 项

                if (result != Success) {
                    if (client->noClientException != Success)
                        CloseDownClient(client);
                    else SendErrorToClient(client, MAJOROP, MinorOpcodeOfRequest(client),
                                           client->errorValue, result);
                    break;
                }
            }

            FlushAllOutput();
            client = clients[clientReady[nready]];
            if (client) client->smart_stop_tick = SmartScheduleTime;
        }
        dispatchException &= ~DE_PRIORITYCHANGE;
    }
    free(clientReady);
}
```

### §2.2 协议 dispatch 表（256 项）

```c
// dix/dispatch.c
int (*InitialVector[3]) (ClientPtr) = {
    0, ProcInitialConnection, 0,
};

int (*ProcVector[256]) (ClientPtr) = {
    ProcBadRequest,
    ProcCreateWindow,           /* 1 */
    ProcChangeWindowAttributes, /* 2 */
    ProcGetWindowAttributes,    /* 3 */
    ProcDestroyWindow,          /* 4 */
    ProcDestroySubwindows,      /* 5 */
    ProcChangeSaveSet,
    ProcReparentWindow,         /* 7 */
    ProcMapWindow,              /* 8 */
    ProcMapSubwindows,
    ProcUnmapWindow,            /* 10 */
    ProcUnmapSubwindows,
    ProcConfigureWindow,
    ProcCirculateWindow,
    ProcGetGeometry,
    ProcQueryTree,
    /* ... 一直到 256 */
    ProcNoOperation             /* 127 */
    /* 128-255 是 extension opcodes */
};
```

### §2.3 ProcCreateWindow 示例

```c
// dix/window.c (精简)
int ProcCreateWindow(ClientPtr client) {
    WindowPtr pParent, pWin;
    REQUEST(xCreateWindowReq);
    int result;
    int len;
    Mask mask = stuff->mask;

    REQUEST_AT_LEAST_SIZE(xCreateWindowReq);

    LEGAL_NEW_RESOURCE(stuff->wid, client);

    if (!(pParent = (WindowPtr) SecurityLookupIDByType(client, stuff->parent,
                                                        RT_WINDOW, DixGetAttrAccess)))
        return BadWindow;

    len = client->req_len - bytes_to_int32(sizeof(xCreateWindowReq));
    if (Ones(mask) != len) return BadLength;

    if (!stuff->width || !stuff->height) {
        client->errorValue = 0;
        return BadValue;
    }

    pWin = CreateWindow(stuff->wid, pParent, stuff->x, stuff->y,
                        stuff->width, stuff->height, stuff->borderWidth,
                        stuff->class, stuff->mask, (XID *)&stuff[1],
                        (int) stuff->depth, client, stuff->visual, &result);
    if (pWin) {
        Mask access_mode = DixCreateAccess;
        result = XaceHookResourceAccess(client, stuff->wid, RT_WINDOW, pWin,
                                         RT_WINDOW, pParent, access_mode);
        if (result != Success) {
            FreeResource(stuff->wid, RT_NONE);
            return result;
        }
        if (!AddResource(stuff->wid, RT_WINDOW, (void *)pWin)) return BadAlloc;
    }

    return result;
}
```

**关键模式：**
- `REQUEST(xCreateWindowReq)` — 把请求 buffer cast 为协议 struct
- `LEGAL_NEW_RESOURCE` — 检查 client 持有的资源 ID 范围
- `SecurityLookupIDByType` — 安全检查 + ID → object 转换
- `CreateWindow` — 实际创建（dix/window.c）
- `AddResource` — 注册到资源 ID 哈希表

---

## §3 事件 + 输入设备（dix/events.c + devices.c）

### §3.1 输入事件流程

```
键鼠物理事件
  ↓ Linux evdev (/dev/input/eventN)
  ↓ udev hotplug 通知 X
hw/xfree86/common/xf86Xinput.c (xfree86 backend)
  ↓ 通过 libinput / evdev driver 把事件读出
  ↓ 调 mieqEnqueue (mi/mieq.c) — Mini-Input 事件队列
  ↓ 由 dispatch loop 内的 ProcessInputEvents 出队列
dix/events.c
  ↓ ProcessKeyboardEvent / ProcessPointerEvent
  ↓ DeliverEventsToWindow (找到目标 window)
  ↓ TryClientEvents (检查 client 是否选中此 event 类型)
  ↓ WriteEventsToClient (写到客户端 socket)
```

### §3.2 DeviceIntRec（输入设备核心 struct）

```c
// include/inputstr.h
typedef struct _DeviceIntRec {
    DeviceIntPtr next;
    Bool         startup;            // 启动时存在
    DeviceProc   deviceProc;         // 控制 callback
    Bool         inited;
    Bool         enabled;
    Bool         coreEvents;
    GrabInfoRec  deviceGrab;
    int          type;
    Atom         xinput_type;        // XInput device class
    char        *name;
    int          id;
    KeyClassPtr  key;                // 键盘特性
    ValuatorClassPtr valuator;       // 坐标轴
    TouchClassPtr touch;             // 触屏
    GestureClassPtr gesture;         // 手势
    ButtonClassPtr button;           // 按钮
    FocusClassPtr focus;
    ProximityClassPtr proximity;
    KbdFeedbackPtr kbdfeed;
    PtrFeedbackPtr ptrfeed;
    IntegerFeedbackPtr intfeed;
    StringFeedbackPtr stringfeed;
    BellFeedbackPtr bell;
    LedFeedbackPtr leds;
    struct _XkbInterest *xkb_interest;
    char        *config_info;        // 配置串
    PrivateRec  *devPrivates;
    DeviceUnwrapProc unwrapProc;
    SpriteInfoPtr spriteInfo;
    DeviceIntPtr  master;            // 主设备（XInput2 MD/SD 区分）
    DeviceIntPtr  lastSlave;
    Bool         isMaster;
    /* ... */
} DeviceIntRec;
```

XInput2 引入 **MD（Master Device）+ SD（Slave Device）** 两层 —— Master 是逻辑设备（虚拟键盘 + 虚拟鼠标 各 1 对），Slave 是物理设备（多个键盘 / 多个鼠标），物理事件从 SD 流向 MD 再到 client。

### §3.3 事件传递（DeliverEventsToWindow）

```c
// dix/events.c (精简)
int DeliverEventsToWindow(DeviceIntPtr pDev, WindowPtr pWin, xEvent *pEvents,
                          int count, Mask filter, GrabPtr grab) {
    int deliveries = 0, nondeliveries = 0;
    int attempt;
    InputClients *other;
    ClientPtr client = NullClient;
    Mask deliveryMask = 0;
    const int type = pEvents->u.u.type;

    /* 1. 看 window 自己是否选中 */
    if ((attempt = TryClientEvents(wClient(pWin), pDev, pEvents, count,
                                     pWin->eventMask, filter, grab)) > 0) {
        if (attempt > 0) {
            deliveries++;
            client = wClient(pWin);
            deliveryMask = pWin->eventMask;
        }
    }

    /* 2. 看其它 client 是否选中此 window 的此 event */
    if (filterNonDeliverable(type, filter)) {
        for (other = wOtherClients(pWin); other; other = other->next) {
            if ((attempt = TryClientEvents(rClient(other), pDev, pEvents, count,
                                             other->mask, filter, grab)) > 0) {
                /* ... */
            }
        }
    }

    /* 3. XInput2 event */
    /* ... */

    return (deliveries ? deliveries : nondeliveries);
}
```

---

## §4 核心 X 对象（window / GC / pixmap）

### §4.1 WindowRec

```c
// include/windowstr.h
typedef struct _Window {
    DrawableRec drawable;           // common base
    PrivateRec *devPrivates;
    WindowPtr parent;
    WindowPtr nextSib;
    WindowPtr prevSib;
    WindowPtr firstChild;
    WindowPtr lastChild;
    ScreenPtr drawable_screen;
    RegionRec clipList;             // visible 区域
    RegionRec borderClip;
    RegionRec winSize;
    RegionRec borderSize;
    DDXPointRec origin;
    Mask eventMask;
    Mask deliverableEvents;
    Mask dontPropagateMask;
    OtherClients *otherClients;
    GrabPtr passiveGrabs;
    PropertyPtr userProps;
    CARD32 backingBitPlanes;
    CARD32 backingPixel;
    RegionPtr backStorage;
    WindowOptPtr optional;
    /* ... 30+ 字段 */
    unsigned mapped:1;
    unsigned realized:1;
    unsigned viewable:1;
    unsigned visibility:2;
    unsigned overrideRedirect:1;
    unsigned saveUnder:1;
    unsigned bitGravity:4;
    unsigned winGravity:4;
    unsigned redirectDraw:2;
    unsigned forcedBG:1;
    /* ... */
} WindowRec;
```

### §4.2 GC（图形上下文）

```c
typedef struct _GC {
    DrawablePtr  pScreen;
    unsigned char depth;
    unsigned char alu;
    unsigned short lineWidth;
    unsigned short dashOffset;
    unsigned short numInDashList;
    unsigned char *dash;
    unsigned int  lineStyle:2;
    unsigned int  capStyle:2;
    unsigned int  joinStyle:2;
    unsigned int  fillStyle:2;
    unsigned int  fillRule:1;
    unsigned int  arcMode:1;
    unsigned int  subWindowMode:1;
    unsigned int  graphicsExposures:1;
    unsigned int  clientClipType:2;
    unsigned int  miTranslate:1;
    unsigned int  fExpose:1;
    unsigned int  freeCompClip:1;
    unsigned int  scratch_inuse:1;
    unsigned int  serialNumber;
    unsigned long planemask;
    unsigned long fgPixel;
    unsigned long bgPixel;
    PixmapPtr     tile;
    PixmapPtr     stipple;
    DDXPointRec   patOrg;
    struct _Font *font;
    DDXPointRec   clipOrg;
    void         *clientClip;
    unsigned long stateChanges;
    unsigned long serial;
    GCFuncs      *funcs;            // 一组 callback
    GCOps        *ops;              // 另一组 callback
    PrivateRec   *devPrivates;
} GC;
```

GC 是绘图状态封装 —— 颜色、线宽、字体、剪切区域等。每个 client 创建多个 GC，复用绘图设置。

### §4.3 PixmapRec

```c
typedef struct _Pixmap {
    DrawableRec drawable;
    PrivateRec *devPrivates;
    int         refcnt;
    int         devKind;            // bytes per row
    DevUnion   *devPrivate;         // 后端实现私有
    /* ... */
} PixmapRec;
```

Pixmap = 离屏图像缓冲，client 可在 server 侧创建 + 绘制 + 拷贝到 window。

---

## §5 extension 框架（dix/extension.c）

### §5.1 ExtensionEntry

```c
// dix/extension.c
typedef struct _ExtensionEntry {
    int          index;              // 扩展号
    Bool         (*CloseDown)(ExtensionEntry *);
    char         *name;
    int          base;               // 第一个 extension opcode
    int          eventBase;          // 第一个 extension event
    int          errorBase;          // 第一个 extension error
    int          num_aliases;
    char        **aliases;
    void         *extPrivate;
    /* ... */
} ExtensionEntry;
```

### §5.2 AddExtension

```c
ExtensionEntry *AddExtension(const char *name, int NumEvents, int NumErrors,
                              int (*MainProc)(ClientPtr c1),
                              int (*SwappedMainProc)(ClientPtr c2),
                              void (*CloseDownProc)(ExtensionEntry *e),
                              unsigned short (*MinorOpcodeProc)(ClientPtr c)) {
    int i;
    ExtensionEntry *ext, **newexts;

    if (!MainProc || !SwappedMainProc || !MinorOpcodeProc) return NULL;
    if ((lastEvent + NumEvents > MAXEVENTS) ||
        (unsigned)(lastError + NumErrors > LAST_ERROR)) return NULL;

    ext = malloc(sizeof(ExtensionEntry));
    if (!ext) return NULL;
    /* ... 初始化 ... */

    ext->name = strdup(name);
    ext->num_aliases = 0;
    ext->aliases = NULL;
    if (!ext->name) { free(ext); return NULL; }

    i = NumExtensions;
    newexts = reallocarray(extensions, i + 1, sizeof(ExtensionEntry *));
    if (!newexts) { free(ext->name); free(ext); return NULL; }

    NumExtensions++;
    extensions = newexts;
    extensions[i] = ext;
    ext->index = i;
    ext->base = i + EXTENSION_BASE;

    ProcVector[ext->base] = MainProc;
    SwappedProcVector[ext->base] = SwappedMainProc;

    if (NumEvents) {
        EventSwapVector[lastEvent] = (EventSwapPtr) DefaultSwap;
        ext->eventBase = lastEvent;
        lastEvent += NumEvents;
    } else ext->eventBase = 0;

    if (NumErrors) {
        ext->errorBase = lastError;
        lastError += NumErrors;
    } else ext->errorBase = 0;

    return ext;
}
```

### §5.3 主流 extension 初始化（典型）

```c
// Xext/glxext.c (示例)
void GlxExtensionInit(void) {
    ExtensionEntry *extEntry;

    extEntry = AddExtension(GLX_EXTENSION_NAME, __GLX_NUMBER_EVENTS,
                              __GLX_NUMBER_ERRORS, __glXDispatch,
                              __glXSwapDispatch, ResetExtension,
                              StandardMinorOpcode);
    /* ... */
}
```

22+ extension 每个都有自己的 init 函数，由 dix/extinit.c 的 `InitExtensions` 统一调用。

---

## §6 DDX 架构横向（hw/ 子目录）

### §6.1 4 种 DDX backend

| backend | 路径 | 用途 |
|---------|------|------|
| **xorg**（xfree86）| hw/xfree86/ | 经典 X.Org server，跑物理硬件 |
| **xwayland** | hw/xwayland/ | X server 输出到 Wayland surface |
| **xnest** | hw/xnest/ | 嵌套 X server（X-on-X 测试）|
| **vfb** | hw/vfb/ | 虚拟 framebuffer（无显示器测试 / CI）|
| **kdrive** | hw/kdrive/ | 嵌入式微 X server |
| **xquartz** | hw/xquartz/ | macOS 原生（Quartz 后端，已半 legacy）|
| **xwin** | hw/xwin/ | Windows 原生（Cygwin/X 用）|

### §6.2 xfree86 框架 main

```c
// hw/xfree86/common/xf86Init.c (节选)
void InitOutput(ScreenInfo *pScreenInfo, int argc, char **argv) {
    int i;
    Bool was_loaded = FALSE;

    xf86Initialising = TRUE;

    /* 解析 xorg.conf */
    config_init();
    config_parse_options(argc, argv);

    /* 加载 module（drivers / extensions） */
    if (!was_loaded) {
        was_loaded = TRUE;
        xf86OpenConsole();
        if (!xf86LoaderCheckSymbol("xf86Msg"))
            FatalError("X server core symbols not exported\n");
    }

    /* PCI 探测 */
    xf86BusProbe();
    xf86PostProbe();

    /* 调用 driver -> ScreenInit */
    for (i = 0; i < xf86NumScreens; i++) {
        ScreenPtr pScreen;
        ScrnInfoPtr pScrn = xf86Screens[i];
        AddScreen(pScrn->ScreenInit, argc, argv);
        pScreen = screenInfo.screens[pScrn->scrnIndex];
        /* ... */
    }
}
```

### §6.3 xorg.conf 解析器（hw/xfree86/parser/）

xorg.conf 格式：
```
Section "Device"
    Identifier  "MyCard"
    Driver      "modesetting"
    BusID       "PCI:0:2:0"
EndSection

Section "Screen"
    Identifier  "Screen0"
    Device      "MyCard"
    Monitor     "Monitor0"
    DefaultDepth 24
EndSection

Section "ServerLayout"
    Identifier  "default"
    Screen      "Screen0"
EndSection
```

xorg.conf 已基本被自动配置取代（modeset driver + KMS + udev hotplug），仅在专用场景（CAD / multi-GPU）需手写。

---

## §7 GLAMOR + GLX + DRI3（GPU 加速）

### §7.1 GLAMOR

GLAMOR = 把 X 2D 绘图操作（XRender / XComposite）转换为 OpenGL 命令，借 mesa 跑到 GPU。

```
X client →(XRender/XComposite/...)→ X.Org server
                                       ↓ GLAMOR
                                       ↓ OpenGL（通过 mesa）
                                       ↓ DRM/KMS
                                       ↓ GPU
```

**核心文件：**
- `glamor/glamor.c` — 入口 + 全局
- `glamor/glamor_render.c` — 渲染加速
- `glamor/glamor_picture.c` — XRender Picture 加速
- `glamor/glamor_compose.c` — XComposite 加速

### §7.2 GLX（OpenGL on X）

GLX 是把 OpenGL 命令通过 X 协议传递的扩展。客户端调 OpenGL → libGL.so → libGLX.so → X 协议 → X server 解析 → mesa 执行。

**现代加速：** DRI3 + Present 扩展让客户端**直接画到 dma-buf**，X server 仅 page-flip。X server 不再涉及 GL 命令拷贝，性能接近原生。

### §7.3 DRI3 扩展

```c
// dri3/dri3.c (精简)
static int proc_dri3_open(ClientPtr client) {
    REQUEST(xDRI3OpenReq);
    int fd;

    REQUEST_SIZE_MATCH(xDRI3OpenReq);
    /* 找 GPU device fd 给 client */
    fd = dri3_open(client, stuff->drawable, stuff->provider);
    if (fd < 0) return BadValue;

    /* 通过 X 协议把 fd 传给 client（SCM_RIGHTS UNIX socket）*/
    return WriteFdToClient(client, fd, TRUE);
}

static int proc_dri3_pixmap_from_buffer(ClientPtr client) {
    PixmapPtr pixmap;
    REQUEST(xDRI3PixmapFromBufferReq);
    int fd;

    fd = ReadFdFromClient(client);                  // 收 dma-buf fd
    pixmap = dri3_pixmap_from_fd(screen, fd, ...);  // 包装为 X Pixmap
    /* ... */
}
```

DRI3 关键创新：**通过 X 协议传 dma-buf fd**（用 UNIX socket SCM_RIGHTS），让 GPU client 和 X server 共享 GPU 缓冲区零拷贝。

---

## §8 Xwayland（X-on-Wayland，hw/xwayland/）

### §8.1 Xwayland 是 hw/ 的特殊 backend

```c
// hw/xwayland/xwayland.c (节选)
void InitOutput(ScreenInfoPtr screenInfo, int argc, char **argv) {
    int i;

    xwl_screen = xwl_screen_create();
    if (!xwl_screen) FatalError("Failed to create xwl_screen");

    /* 连 Wayland compositor */
    xwl_screen->display = wl_display_connect(NULL);
    if (!xwl_screen->display) FatalError("Failed to connect to Wayland");

    /* 注册 wl_registry listener 找 compositor / shm / seat / output */
    xwl_screen->registry = wl_display_get_registry(xwl_screen->display);
    wl_registry_add_listener(xwl_screen->registry, &registry_listener, xwl_screen);

    wl_display_dispatch(xwl_screen->display);

    /* 创建 ScreenRec 等 */
    AddScreen(xwl_screen_init, argc, argv);
}
```

每个 X 窗口对应一个 Wayland surface；输入由 Wayland compositor 派发到 Xwayland，再派发到 X client。

### §8.2 xwayland_present 集成

Xwayland 用 Wayland 的 `wp_presentation` 协议实现 X Present 扩展：让 X client 通过 DRI3 + Present 直接画到 dma-buf，Xwayland 把 dma-buf attach 到 Wayland surface 再 commit。

---

## §9 输入：os/inputthread.c

```c
// os/inputthread.c (节选)
static void *InputThreadDoWork(void *arg) {
    inputThreadInfo->running = TRUE;

    while (inputThreadInfo->running) {
        int sigio_fd = -1;

        if (epoll_fd >= 0)
            num_fds = epoll_wait(epoll_fd, events, MAXEVENTS, -1);

        for (i = 0; i < num_fds; i++) {
            input_handler_t *handler = events[i].data.ptr;
            if (handler && handler->fd >= 0)
                handler->callback(handler->fd, handler->data);
        }

        /* 通知 main thread 处理输入事件 */
        if (events_received) NotifyMainThread();
    }
    return NULL;
}
```

**双线程架构：**
- main thread：协议派发 + 渲染
- input thread：epoll 监听 evdev / libinput fd，事件入队后通知 main thread

---


|--------|-----------|---------------|
| 协议 dispatch | 256 项函数指针表 | trait 化 / Rust enum match |
| 256 资源类型 | RT_WINDOW / RT_PIXMAP / etc | TypeId / PhantomData |
| 客户端连接 | Unix socket + TCP（可选）| 同 |
| 网络透明 | 一等公民 | 是否需要（可选）|
| extension 框架 | 256 opcode + 加载 .so module | 编译期 features / dlopen |
| DDX 后端 | 6+ 种（xfree86/xwayland/...）| 选哪些（fbdev / DRM / wayland / xwayland）|
| 事件队列 | mieq + input thread | 异步 channel |
| GLAMOR | 把 2D 加速委托 OpenGL | 直接用 mesa / 软件 |
| auth | xauth cookie / SSH X11 forwarding | 重做或借鉴 |
| 配置 | xorg.conf + 自动 (modesetting + KMS + udev) | 编译期 / DT |

---

## §11 后续 H 系列规划

| 笔记 | 主题 | 主要源码 |
|------|------|---------|
| H2 = 14-02 | Wayland + wlroots / Smithay 精读（compositor 实现）| gui/wayland + gui/wlroots |
| H3 = 14-03 | DRM/KMS 内核子系统精读（atomic / GEM / TTM / fences）| fs/linux-fs/drivers/gpu/drm/ |
| H4 = 14-04 | mesa Gallium / NIR 精读（userspace OpenGL/Vulkan driver）| gui/mesa |
| H5 = 14-05 | 嵌入式 GUI 横向（LVGL / Slint / SDL2 / Iced / Tauri）| gui/lvgl + gui/slint + gui/sdl |
| H6 = 14-06 | DRI/Present/dma-buf 跨子系统协同 | gui/xserver/dri3 + linux drivers/gpu/drm |
| H7 = 14-07 | Vulkan ICD 横向精读（anv/radv/turnip/nvk/panvk/v3dv/lavapipe）| gui/mesa/src/vulkan/ |

---

## §12 词典

| 术语 | 含义 |
|------|------|
| **DIX** | Device-Independent X（协议核心）|
| **DDX** | Device-Dependent X（设备特定后端）|
| **GLAMOR** | GL Accelerated MOdule for Render（X 2D 加速基于 OpenGL）|
| **mi** | Mini-Implementation（DDX 共享通用实现）|
| **fb** | framebuffer（mi 软件 framebuffer 后端）|
| **EXA** | Embedded X Acceleration（已 legacy，被 GLAMOR 取代）|
| **DRI3** | Direct Rendering Infrastructure v3（dma-buf based）|
| **Present** | Present extension（vsync 翻页）|
| **Composite** | XComposite（窗口 offscreen 渲染）|
| **GLX** | OpenGL X Extension |
| **XInput2** | XI2，多触点 / pressure / 高级输入 |
| **XKB** | X Keyboard Extension |
| **xkbcommon** | XKB 协议 client lib（Wayland 也用）|
| **xkeyboard-config** | 键盘布局数据（多语言）|
| **MD/SD** | Master/Slave Device（XInput2 输入设备分层）|
| **xauth cookie** | X auth 凭证文件 |

---

