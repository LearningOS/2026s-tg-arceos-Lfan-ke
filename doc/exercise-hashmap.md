# exercise-hashmap — 给 axstd 增加 HashMap

## 任务
`src/main.rs` 里 `use std::collections::HashMap;`（`std` = arceos 的 `axstd`），
构建 5 万条 `String -> u32` 的 HashMap 并校验。但发布版 `axstd 0.3.0-preview.1`
的 `collections` 只是 `pub use alloc::{... collections ...}`，即
`alloc::collections`，其中**没有 HashMap**（HashMap 在 std 里，依赖 OS 的
`RandomState`，no_std 下不可用）。所以需要自己给 axstd 补一个 HashMap。

## 方案：本地 vendor axstd + patch 覆盖

1. 把 registry 里的 `axstd-0.3.0-preview.1` 源码整份拷到本仓库 `./axstd/`
   （源码位置：`~/.cargo/registry/src/<mirror>/axstd-0.3.0-preview.1/`，
   先 `cargo fetch` 把它拉到本地缓存）。

2. 在本练习的 `Cargo.toml` 加 patch，让 axstd 走本地副本：
   ```toml
   [patch.crates-io]
   axstd = { path = "axstd" }
   ```
   本地副本版本号仍是 `0.3.0-preview.1`，满足 `=0.3.0-preview.1` 约束；
   `[patch.crates-io]` 即使在 tuna/ustc 镜像（source replacement）下也生效。

3. 改本地 `axstd/src/lib.rs`：
   - 把 `pub use alloc::{boxed, collections, format, string, vec};`
     里的 `collections` 去掉（避免和自建模块同名冲突）；
   - 新增 `#[cfg(feature = "alloc")] pub mod collections;`。

4. 新增 `axstd/src/collections.rs`：
   - `pub use alloc::collections::*;`（保留 VecDeque/BTreeMap 等原有类型）；
   - 用 `hashbrown` 实现 `HashMap`，并提供 `HashSet`。

5. `axstd/Cargo.toml`：加 `hashbrown`（optional），并把它挂到 `alloc` feature 上，
   只有开了 `alloc`（HashMap 需要堆）才拉进来：
   ```toml
   alloc = ["dep:hashbrown", "arceos_api/alloc", "axfeat/alloc", "axio/alloc"]
   [dependencies.hashbrown]
   version = "0.16"
   optional = true
   default-features = false
   features = ["default-hasher", "allocator-api2"]
   ```
   本练习里 axstd 已带 `alloc` feature，故 HashMap 可用。

## HashMap 实现（hashbrown 而非纯手写）

选 **hashbrown**（std 的 HashMap 底层也是它），而不是从零手写 bucket 数组，
理由：正确性/性能有保证，API 与 std 完全一致，5 万条插入+迭代轻松通过。

难点：`main.rs` 调用的是 `HashMap::new()`，而 hashbrown 的 `new()` 只为默认
hasher（`DefaultHashBuilder` = foldhash）实现，无法直接给自定义 `RandomState`
当默认 hasher 还保留 `new()`。所以用一个**薄 newtype 包一层**：

```rust
pub struct HashMap<K, V, S = RandomState> { base: hashbrown::HashMap<K, V, S> }
impl<K, V> HashMap<K, V, RandomState> {
    pub fn new() -> Self { Self { base: hashbrown::HashMap::with_hasher(RandomState::new()) } }
}
// Deref/DerefMut -> hashbrown::HashMap，于是 insert/get/iter/... 全部直接可用
// 另外实现 IntoIterator（&map 和 map）
```

`m.insert(k, v)` 经 `DerefMut`，`m.iter()` 经 `Deref`，都落到 hashbrown 上。

## 随机种子来源（hasher seeding）

任务建议用 `axhal::random()` / `axhal::misc::random()` 给 hasher 播种，
**但本版 `axhal 0.3.0-preview.1` 根本没有 `random()` 这个 API**
（`grep -rn random` 在 axhal/arceos_api/axplat 全为空）。

退而求其次，用**硬件时钟**作为可用的熵源：axstd 本就依赖 `arceos_api`，
其中 `arceos_api::time::ax_wall_time()`（底层 = `axhal::time::wall_time`）返回开机
以来的时间。`RandomState::new()` 取它的纳秒值，过一遍 splitmix64 雪崩混合得到
64 位种子：

```rust
let nanos = arceos_api::time::ax_wall_time().as_nanos() as u64;
// splitmix64 finalizer -> seed
```

hasher 本体是自带的 `FxHasher`（FxHash 构造，纯 no_std、无额外依赖），
`build_hasher()` 用该 seed 作初始状态。这样每次开机时刻不同 -> 哈希顺序不同，
具备一定的抗 hash-flooding 随机化。

> 备注：hashbrown 默认 foldhash 的 `DefaultHashBuilder` 本身也会自播种，
> 直接 `pub use hashbrown::HashMap` 也能过测；这里特意走时钟播种，
> 以贴合“用 arceos 硬件熵给 hasher 播种”的题意（在没有 random() 的前提下）。

## 改动文件清单
- `Cargo.toml`：新增 `[patch.crates-io] axstd = { path = "axstd" }`
- `axstd/`（本地 vendor 的整份 axstd 源码副本）
  - `Cargo.toml`：`alloc` feature 挂 `dep:hashbrown`；新增 hashbrown 依赖
  - `src/lib.rs`：`collections` 从 alloc re-export 移除；新增 `pub mod collections;`
  - `src/collections.rs`（新增）：HashMap/HashSet/RandomState/FxHasher

> 注意：未做任何 `git add/commit`，仅改工作树。

## 通过输出（riscv64 / `bash scripts/test.sh`）
```
arch = riscv64
platform = riscv64-qemu-virt
...
Initialize global memory allocator...
  use TLSF allocator.
...
Running memory tests...
test_hashmap() OK!
Memory tests run OK!
[axplat_riscv64_qemu_virt::power:25] Shutting down...
```
脚本汇总：`Passed 3/4`（riscv64 ✓ / x86_64 ✓ / aarch64 ✓），`Failed 1/4`（loongarch64）。

> loongarch64 的失败与本练习无关：`qemu-system-loongarch64` 的 `virt` 机器要求
> RAM > 1G，而 `xtask/src/main.rs:141` 写死 `-m 128M`，于是 QEMU 在启动阶段就报
> `ram_size must be greater than 1G.` 退出——内核本身已正常编译链接（`.bin` 产物存在），
> 任何代码都还没跑。这是本机 QEMU/xtask 的环境限制，非 HashMap 实现问题；
> 题目判定口径为 riscv64（= `cargo xtask run --arch=riscv64`），该项通过。
> 若要让脚本整体变绿，可把 loongarch 的 `-m` 调到 >1G（本练习未改，保持最小改动）。
