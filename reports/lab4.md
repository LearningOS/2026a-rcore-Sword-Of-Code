## 实现总结

实现了 `sys_linkat / sys_unlinkat / sys_fstat` 三个系统调用，支持硬链接。底层在 `easy-fs/src/vfs.rs` 的 `Inode` 上加了 `link / unlink / nlink_of / inode_id / is_dir` 五个方法：
`link` 在目录里追加一条指向已有 inode_id 的新 dirent；
`unlink` 用 swap-with-last 的方式删除目标 dirent 并缩短目录 `size`，然后重新扫描统计剩余引用，若归零则 `clear_size` 释放数据块、`dealloc_inode` 释放 inode 槽；`nlink_of` 把自己当目录数 inode_id 的出现次数。
`EasyFileSystem` 相应补了 `dealloc_inode` 和 `get_inode_id_from_pos`。OS 侧给 `File` trait 加了默认返回 `None` 的 `stat`，`OSInode` 覆盖后返回 `(ino, is_dir, nlink)`；
`sys_fstat` 通过这个方法拿元数据再 `translated_refmut` 写回用户缓冲区；
`sys_linkat / sys_unlinkat` 把用户路径转成字符串后转发到 `ROOT_INODE.link / unlink`。

## 问答题

### 在我们的 easy-fs 中，root inode 起着什么作用？如果 root inode 中的内容损坏了，会发生什么？

**root inode 的作用**

easy-fs 把编号为 0 的那个 DiskInode 永远分配给根目录。在 `EasyFileSystem::create` 里：

```rust
assert_eq!(efs.alloc_inode(), 0);
// 把 inode 0 初始化为 Directory
disk_inode.initialize(DiskInodeType::Directory);
```

然后 `EasyFileSystem::root_inode` 固定取 `get_disk_inode_pos(0)` 这个位置、包成一个 VFS 层的 `Inode` 交出去。在 OS 侧，`ROOT_INODE` 是一个 `lazy_static!` 单例：

- **命名与查找**：easy-fs 只支持单层目录，所有文件名（`fname / lname0 / ch6b_user_shell` 之类）都作为一条 dirent 放在 root inode 的数据块里。`open_file` 的 `ROOT_INODE.find(name)` 就是遍历这些 dirent 找 inode_id、再定位到文件自身的 DiskInode。
- **文件创建**：`ROOT_INODE.create(name)` 会 `alloc_inode()` 一个新的 DiskInode 存文件、然后往 **root inode 的数据块追加一条 dirent**。root inode 的 `size` = 系统中（可见）文件数量 × `DIRENT_SZ`。
- **硬链接 / 删除**：本实验的 `linkat / unlinkat` 都是在 root inode 的 dirent 列表上做追加 / swap-with-last + 缩 size；`fstat` 里的 `nlink` 也是靠 `ROOT_INODE.nlink_of(ino)` **扫 root 的 dirent 统计引用数**得到的。
- **应用加载**：`sys_exec / sys_spawn` 要加载 ELF 都走 `open_file`，也就是必然先过 root inode；甚至 `list_apps()` 开机列应用列表也是直接 `ROOT_INODE.ls()`。

总结：**root inode 是整个文件系统的入口目录，是唯一一个"位置写死"的 inode**。它的位置可以无条件还原（永远是 inode id = 0），但它里面的 dirent 列表是系统里所有文件名→inode_id 映射的唯一来源。

**如果 root inode 的内容损坏了**

按损坏范围分四种情形看：

1. **类型位（`type_`）被破坏**：如果它不再是 Directory，`find_inode_id` 开头的 `assert!(disk_inode.is_dir())` 会直接 panic，内核启动就起不来——`ROOT_INODE` 这个 `lazy_static` 在第一次 `find("ch6b_initproc")` 时就崩溃。

2. **`size` 字段被改坏**：
   - 改小 → 一部分真实存在的 dirent 被当作越界，对应文件在 `open` 里查不到，表现为"文件凭空消失"；但那些文件的 DiskInode 和数据块仍在，变成**孤儿 inode**，永远没人能触达，相当于硬盘泄露。
   - 改大 → `find / ls / nlink_of` 都会扫出一堆 `size / DIRENT_SZ` 条目，多出来的那段数据块里的字节会被当成 `DirEntry` 解析：`name` 里只要出现 0 字节就被截断成某个名字，`inode_id` 字段则随便指向一个 inode id——**打开某个"文件名"可能映射到任意一个 DiskInode**，读到的内容是别的文件的，或者 inode_id 超出有效范围触发 panic。

3. **`direct / indirect1 / indirect2` 这些块指针被改**：
   - 指到数据区别的合法块 → "目录内容"变成某个数据文件的内容；用户看到的"目录列表"是乱码字节被当 dirent 解析的结果，可能 `name` 碰巧是某个已存在文件名，于是 `find` 返回错误的 inode_id，读到完全不相干的文件甚至写坏别人的数据。
   - 指到无效/未分配块 → 读到全 0，表现为**目录空了**：`list_apps` 打印空列表，所有 `open_file` 全部返回 `None`，`sys_exec("ch6b_initproc")` 失败，内核直接 panic。
   - 指到 inode 区 / bitmap 区 / super block → 对目录的读写会撞上元数据，`write_at` 的时候一次正常的 `create / link / unlink` 就能把 super block 的 magic、bitmap 位图、或者其它 inode 的 size/指针字段覆盖，之后任何操作的结果都不可预测。

4. **dirent 里具体字节出错**（block 指针对、但数据块内容被改）：
   - `inode_id` 字段被改 → `open` 这个名字时拿到的是错 inode：读到别人的数据，写入会污染别人的数据，`unlink` 会把一个不该删的 inode 当作"最后一条链接"直接回收掉，造成活 inode 被释放、bitmap 对不上。
   - `name` 字节被改 → 文件"改名"了；原名字 `open` 失败，用户无法再访问。

**总结**：root inode 是 easy-fs 唯一的全局入口，它的损坏不是局部"一个文件坏了"的问题，而是整棵命名树断根——从内核启动时 `list_apps / open("ch6b_initproc")` 就可能崩溃。哪怕能起来，之后的 `find / create / link / unlink / fstat` 全部基于 root inode 的 dirent，损坏会以**间接寻址**的方式扩散到任意 inode 与数据块，且 easy-fs 没有任何 journal / 校验 / 备份机制，一旦损坏基本无法自愈。

## ch7

### 1. 举出使用 pipe 的一个实际应用的例子

最典型的一个日常例子就是在 shell 里**统计一个文件的行数**：

```bash
cat access.log | wc -l
```

shell 会先 `pipe()` 创建一对读/写文件描述符，然后 `fork` 出两个子进程：

- 左边的 `cat access.log` 把管道的读端 close 掉，`dup2` 把写端装到 fd 1（stdout），然后 `exec` 掉 cat。cat 正常往 stdout `write` 文件内容，字节流其实就落进了管道缓冲区。
- 右边的 `wc -l` 把管道的写端 close 掉，`dup2` 把读端装到 fd 0（stdin），然后 `exec` 掉 wc。wc 从 stdin `read` 到的就是前一个进程通过管道推过来的内容，数完换行符打印出结果。

两个进程完全解耦：cat 不知道下游是谁，wc 不知道上游是谁，shell 只负责搭管道 + fork + exec。好处是：

- **不用中间文件**：不需要把 access.log 另存一份再给 wc 读，省 I/O 也避免并发写的竞态。
- **流式**：cat 一边产出字节、wc 一边消费，不用等 cat 完全读完文件；管道缓冲区满了内核会阻塞 cat，空了会阻塞 wc，天然做好了背压。
- **组合性**：这个套路可以继续叠——比如 `cat access.log | grep 500 | wc -l` 就在三进程间串两根管道，统计 500 响应的条数。

## 荣誉准则

1、在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

答：无

2、此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

答：使用 Claude 进行辅助编程，但每一行代码都检视过。

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按"-100"分计。
