# exercise-sysmap · 为静态 musl 程序模拟 `mmap(2)` —— 实现笔记

## 目标
内核创建用户地址空间，从根文件系统加载 `/sbin/mapfile`（一个**静态链接 musl** Linux 程序），
进入用户态后在陷入循环里把系统调用分发到 `src/syscall.rs::handle_syscall`。
要让 payload 的 **文件映射 `mmap`** 能跑通——即实现 `SYS_MMAP`。

## 判分逻辑（scripts/test.sh）
QEMU 串口输出需含两行：
```
Read back content: hello, arceos!
MapFile ok!
```
payload（`payload/mapfile_c/mapfile.c`）先创建小文件写入 `hello, arceos!`，
再用**真实 fd** `mmap` 把它映射回来读出比对。没有可用的 `SYS_MMAP` 路径就无法完成。

## `sys_mmap` 实现（src/syscall.rs，约 362 行）
签名 `(addr, length, prot, flags, fd, offset)`，返回映射起始虚址或负 errno：
1. `length == 0` → `EINVAL`。
2. 解码位域：`MmapProt::from_bits_truncate(prot)`、`MmapFlags::from_bits_truncate(flags)`；
   `MappingFlags::from(prot)`（其 `From` 实现已自动置 `USER` 位）。
3. `length` 向上对齐到 4K 整页。
4. 取出 `main.rs` 建好的 `USER_ASPACE`（`Mutex<Option<Arc<Mutex<AddrSpace>>>>`），空则 `EFAULT`。
5. 选目标虚拟区间：
   - 含 `MAP_FIXED` → 严格用 `addr.align_down_4k()`；
   - 否则 `addr` 当 hint（为 0 时用默认 `0x1_0000_0000`），`aspace.find_free_area(hint, length, limit)`
     找空闲页对齐区，找不到 `ENOMEM`。
6. `aspace.map_alloc(start, length, map_flags, /*populate=*/true)` 分配并映射清零页
   （populate 让物理帧先就位，便于随后写入文件内容）。
7. **文件映射**（`!MAP_ANONYMOUS && fd >= 0`）：`with_file_fd(fd, |f| f.read_at(offset, &mut buf))`
   读 `length` 字节，再 `aspace.write(start, &buf)` 拷入刚映射的页；任一步出错则 `unmap` 回滚后返回 errno。
   匿名映射则保持清零。
8. 返回 `start`。

dispatch：`handle_syscall` 中 `SYS_MMAP => sys_mmap(args[0] as *mut c_void, args[1], args[2] as i32, args[3] as i32, args[4] as i32, args[5] as isize)`。
`SYS_MMAP` 号按架构区分（riscv64/aarch64=222，x86_64=9）。

## 跨 crate / 构建链
- `axmm`/`AddrSpace`：`base/end/find_free_area/map_alloc/write/unmap`；`memory_addr::{VirtAddr,VirtAddrRange}`；
  `page_table_multiarch::MappingFlags`；`axerrno::LinuxError`。
- **payload 交叉编译**：Makefile 用 `riscv64-linux-musl-gcc -static`
  （本机在 `/usr/local/riscv64-linux-musl-cross/bin/`，需加入 PATH）。
  `xtask` 把产物静态链接后放进 virtio-blk 的 FAT32 镜像，路径 `/sbin/mapfile`，再启动 QEMU 挂该盘。

## 验证
`PATH=/usr/local/riscv64-linux-musl-cross/bin:$PATH bash scripts/test.sh`：
- **riscv64 ✓**、**x86_64 ✓**、**aarch64 ✓**：`all expected output matched`（两行都命中）。
- loongarch64 ✗ —— 预存环境问题 `qemu-system-loongarch64: ram_size must be greater than 1G`
  （configs/xtask 硬编码 `-m 128M`），qemu 启动即失败，与本实现无关。
