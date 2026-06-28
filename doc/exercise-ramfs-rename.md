# exercise-ramfs-rename

目标：`src/main.rs` 在 `/tmp` 下建文件 → `fs::rename("/tmp/f1","/tmp/f2")` → 读回。
发行版的 `axfs` / `axfs_ramfs` 没有可用的 `rename`，需要本地补丁实现。

## VfsNodeOps::rename 签名

定义在 `axfs_vfs` 0.1.2 的 `VfsNodeOps` trait（`src/lib.rs`）里，默认实现返回 `Unsupported`：

```rust
fn rename(&self, _src_path: &str, _dst_path: &str) -> VfsResult {
    ax_err!(Unsupported)
}
```

`VfsResult = AxResult<()>`。注意：ramfs 的 `DirNode` 用 `axfs_vfs::impl_vfs_dir_default!{}`
宏补默认目录方法，但该宏**不含** `rename`，所以可以直接在 `impl VfsNodeOps for DirNode`
里新增 `rename` 方法，不会与宏冲突。axfs_vfs 本身无需改动（trait 已带该方法）。

## ramfs 如何移动目录项（axfs_ramfs/src/dir.rs）

`DirNode` 的目录是 `children: RwLock<BTreeMap<String, VfsNodeRef>>`，名字只是 map 的 key。
同目录 rename = 在 map 里改 key，节点 Arc（及其内容）原样保留：

- `rsplit_path` 把路径拆成 (父目录, 末段名)，如 `"tmp/f1"` → `("tmp","f1")`。
- 用 `self.this.upgrade()` 拿到 `Arc<DirNode>`，对 src/dst 父目录路径分别 `lookup` 解析；
  父路径为空时即当前目录。
- 仅支持同目录：`Arc::ptr_eq(&src_parent, &dst_parent)` 不等则返回 `Unsupported`
  （符合 main.rs「Only support rename, NOT move」语义）。
- 把父目录 downcast 成 `DirNode`，对其 `children` 写锁：`remove(src_name)` 取出节点 Arc，
  以 `dst_name` 重新 `insert`。dst 已存在则 `AlreadyExists`。

## axfs 如何转发 rename（axfs/src/root.rs）

模块级 `pub(crate) fn rename(old,new)`（已存在）会先按需删掉已存在的 dst，再调用
`parent_node_of(None, old).rename(old, new)`；绝对路径下 `parent_node_of` 返回 `ROOT_DIR`
（`RootDirectory`）。但 `RootDirectory` 的 `VfsNodeOps` 用了 `impl_vfs_dir_default!{}`，
没有 rename → 落到 trait 默认的 `Unsupported`。

修复：给 `impl VfsNodeOps for RootDirectory` 新增 `rename`，照搬 `create`/`remove` 的转发模式：

```rust
fn rename(&self, src_path: &str, dst_path: &str) -> VfsResult {
    let src = self.normalize_path(src_path);
    let dst = self.normalize_path(dst_path);
    if let Some((mount_fs, src_rest)) = self.find_best_mount(src) {
        let dst_rest = self.find_best_mount(dst).map(|(_, r)| r).unwrap_or(dst);
        mount_fs.root_dir().rename(src_rest, dst_rest)
    } else {
        self.main_fs.root_dir().rename(src, dst)
    }
}
```

本测试里 ramfs 是 `main_fs`（不在 mounts 列表，mounts 只有 /proc、/sys），
`/tmp/f1`→`/tmp/f2` 走 else 分支：`main_fs.root_dir().rename("tmp/f1","tmp/f2")`
即 ramfs 根 `DirNode::rename`。

## 补丁设置（patch.crates-io）

1. `cargo fetch` 把发行版源拉到 registry（tuna 镜像），
   从 `~/.cargo/registry/src/.../{axfs-0.3.0-preview.1, axfs_ramfs-0.1.2}` 拷到
   本练习目录 `./axfs`、`./axfs_ramfs`（chmod u+w）。
2. 练习根 `Cargo.toml` 末尾加：

   ```toml
   [patch.crates-io]
   axfs = { path = "./axfs" }
   axfs_ramfs = { path = "./axfs_ramfs" }
   ```

   （axfs_vfs 不用 patch，trait 已含 rename 默认方法。）
   `[patch.crates-io]` 在 source-replacement（tuna 镜像）下仍生效。
3. 改动文件：`axfs_ramfs/src/dir.rs`（加 `DirNode::rename` + `rsplit_path`）、
   `axfs/src/root.rs`（加 `RootDirectory::rename` 转发）。

## 通过输出

`bash scripts/test.sh` 中 riscv64（= `cargo xtask run --arch=riscv64`）：

```
Create directory '/tmp' ...
Create '/tmp/f1' and write [hello] ...
Read '/tmp/f1' content: [hello] ok!
Rename '/tmp/f1' to '/tmp/f2' ...
Read '/tmp/f2' content: [hello] ok!

[Ramfs-Rename]: ok!
```

riscv64 / x86_64 / aarch64 三个架构均 `✓ all expected output matched`。

loongarch64 失败与本实现无关：xtask 用 `-m 128M` 启动 QEMU，本机
`qemu-system-loongarch64` 要求 `ram_size must be greater than 1G`，内核还没启动就被
QEMU 拒绝（环境/xtask 配置问题，非 rename 代码问题）。任务判定以 riscv64 为准，已通过。
