# exercise-altalloc · 双端 bump 分配器 —— 实现笔记

验证（riscv64）：`cargo xtask run --arch=riscv64` →
```
Running bump tests...
Bump tests run OK!
```

## 设计：双端 bump 分配器（`modules/bump_allocator/src/lib.rs`）

同一结构体同时实现 `BaseAllocator` + `ByteAllocator` + `PageAllocator` 三个 trait（来自 `allocator` crate）。内存区间 `[start, end)` 两端各开一个游标：

```
[ 字节区(b_pos→)  |   空闲   |   (←p_pos)页区 ]
start            b_pos      p_pos          end
```

- 字段：`start / end / b_pos(字节游标，向上) / p_pos(页游标，向下) / count(字节分配计数)`。
- `BaseAllocator::init(start, size)`：记录区间，`b_pos=start`，`p_pos=end`。
- `ByteAllocator`：
  - `alloc(layout)`：`b_pos` 对齐到 `layout.align()`，`next = aligned + size`；若 `next > p_pos` → OOM（`AllocError`）；否则 `b_pos = next`，`count += 1`，返回旧对齐位。
  - `dealloc`：`count -= 1`；当 `count == 0` 时整体回退 `b_pos = start`（经典 bump：全部释放才回收）。
  - `total/used/available_bytes`：区间大小 / `b_pos-start` / `p_pos-b_pos`。
- `PageAllocator`（PAGE_SIZE=4096）：
  - `alloc_pages(num, align)`：从 `p_pos` 向下取 `num` 页、按 `align` 对齐；越过 `b_pos` → OOM；返回新 `p_pos`。
  - `dealloc_pages`：no-op（bump 页区不单独回收）。

## 关键点
- **同时实现三 trait** 是本练习与参考实现（各只做一个）的区别：bump 既当字节分配器又当页分配器，靠双端游标互不干扰。
- OOM 判据统一为「两游标相遇」`b_pos > p_pos`。
- 测试分配并排序约 3M 个整数（走字节区 `alloc`），完成后打印 `Bump tests run OK!`。

## 跨 crate
- `allocator` crate 的 `BaseAllocator / ByteAllocator / PageAllocator` trait、`AllocError / AllocResult`。
- `modules/axalloc/src/default_impl.rs` 在 `bump_allocator` feature 下选用本类型；本练习的 `Cargo.toml` 经 `[patch.crates-io] axalloc` 指向本地 `modules/axalloc`。

> 注：环境里 `xtask` 对 loongarch64 硬编码 `-m 128M`，而该机器要求 RAM>1G，故 loongarch64 启动即失败（与本实现无关）；riscv64/x86_64/aarch64 正常。
