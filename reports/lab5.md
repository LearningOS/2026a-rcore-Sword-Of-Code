## 实现总结

实现了死锁检测系统调用 `sys_enable_deadlock_detect`（syscall id 469），并把检测挂到 `sys_mutex_lock` 与 `sys_semaphore_down` 请求入口。

**状态存放**：在 `ProcessControlBlockInner` 增加 `deadlock_detect_enabled: bool` 以及两组分配矩阵 `mutex_alloc[tid][mid]` / `sem_alloc[tid][sid]`（`u32`），对 mutex 与 semaphore 分别做检测。Need 不持久化，避免"已被唤醒但尚未恢复"时留下假需求的坑；而是在检测的瞬间从 `Semaphore.inner.wait_queue` 以及 `Mutex::waiter_tids()`（新增 trait 方法，`MutexBlocking` 覆盖返回等待队列里各线程的 tid。

**银行家算法**（`run_bankers` 在 `process.rs` 内）：
- `Available[j]`：mutex 为 `1 - Σ Allocation[*][j]`；semaphore 为 `max(sem.inner.count, 0)`。
- `Allocation[i][j]`：从分配矩阵读。
- `Need[i][j]`：等待队列里的线程推导出来 + 当前请求者加 1。
- `Work = Available`，`Finish = [dead_thread]`；循环找 `Finish[i]==false && Need[i] <= Work` 的线程，命中就 `Work += Allocation[i]` 并 `Finish[i]=true`；全部 `Finish` 为 true 视为安全。

**系统调用整合**：
- `sys_mutex_lock` / `sys_semaphore_down`：开启检测时先跑一次安全检查，不安全直接返回 `-0xDEAD`；安全则照常 `lock() / down()`，返回后把 `*_alloc[tid][id] += 1`。
- `sys_mutex_unlock` / `sys_semaphore_up`：先把 `*_alloc[tid][id]` 做 `saturating_sub(1)`，再调原语的 unlock/up。
- `sys_mutex_create` / `sys_semaphore_create` 新建资源时为已有行扩列。
- `sys_thread_create` 为新 tid 预留分配行并调用 `reset_deadlock_tid`，避免 tid 被 `RecycleAllocator` 复用后残留旧账。
- `sys_waittid` 清理 tid 时同步调 `reset_deadlock_tid`。

## 问答题

1、需要回收的资源有哪些？

主线程退出触发整进程退出的回收，在 `exit_current_and_run_next`（`os/src/task/mod.rs`）里完成，涉及：

- **每个线程的 user resource（`TaskUserRes`）**：tid、trap context 物理页、用户栈所在的虚拟区段。
- **每个线程的内核栈 `KernelStack`**：`TaskControlBlock` 被 drop 时自动把 kstack 对应的内核地址空间区段释放；仅主线程为了"自己正在用 kstack 继续跑 schedule"必须延迟到 `add_stopping_task` 后由下一次切换丢弃。
- **进程地址空间**
- **文件描述符表**
- **线程数组**
- **调度器 / 定时器中的残影**：对每个线程调用 `remove_inactive_task`（= `remove_task` + `remove_timer`）
- **pid → process 映射**：`remove_from_pid2process(pid)`。
- **PID、PCB 自身**：通过 `PidHandle::drop` 与最后一个 `Arc<ProcessControlBlock>` 被 drop 自动归还。
- **进程状态**：标记 `is_zombie = true`、写入 `exit_code`，等父进程 `waitpid` 后再最终丢掉 PCB。

其他线程的 TaskControlBlock 可能在哪些位置被引用，分别是否需要回收，为什么？

- **`process_inner.tasks[tid]`**：PCB 持有的 Arc。必须回收——`tasks.clear()` 做这件事。
- **TASK_MANAGER 的就绪队列（`manager.rs`）**：Ready 状态的线程还被排在队列里。必须回收，由 `remove_inactive_task` 里的 `remove_task` 负责。
- **定时器堆 TIMERS（`timer.rs`）**：`sys_sleep` 把等待到期的线程 Arc 挂在 `BinaryHeap` 里。必须回收，remove_timer` 负责。

2、对比 Mutex1 与 Mutex2 的区别

- **Mutex1 的 unlock 把 `locked` 置为 false 再唤醒**。被唤醒的线程从 `block_current_and_run_next()` 之后重新回到 `loop` 顶，**重新抢**锁。
- **Mutex2 的 unlock 则不改 `locked`**：队列里有人就直接把锁"交接"给队首，`locked` 保持 true；没人等才把 `locked` 置为 false。

可能导致的问题：

1. **饥饿 / 顺序不保证**：Mutex1 下，队列里排前面的线程可能被后到的新线程反复插队，极端情况下永远抢不到锁（而 Mutex2 FIFO 严格）。
2. **唤醒风暴 + 空转**：Mutex1 的 `lock` 是 `loop`，每次被唤醒都要重试一次；一旦遇到上面的插队，线程走一轮 "ready → running → 发现 locked=true → 入队 → block"，纯粹浪费一次调度。
3. **死锁检测的语义偏差**：银行家分析假设"资源被释放后才有线程能接手"。Mutex1 释放后短暂地让 `locked=false`——新来的请求者此时看到的 `Available[mid]=1`，可能在 banker's 里被判成安全而实际上立刻又被抢走。在本实验的分配矩阵式检测里，需要特别小心才能保持与真实状态的一致。

## 荣誉准则

1、在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

答：无

2、此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

答：使用 Claude 进行辅助编程，但每一行代码都检视过。

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按"-100"分计。
