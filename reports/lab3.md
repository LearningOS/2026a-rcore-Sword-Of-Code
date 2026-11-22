## 实现总结

实现了 `sys_spawn`：在 `TaskControlBlock` 上加 `spawn` 方法，直接用 `MemorySet::from_elf` 建新地址空间（不复制父进程），分配 pid / kernel_stack、设 parent，调用方把它挂到父的 `children` 并 `add_task` 入调度队列。
实现了 stride 调度：`TaskControlBlockInner` 加 `priority: isize` 和 `stride: u64`，初始 `priority=16, stride=0`；`TaskManager::fetch` 遍历 `ready_queue` 取 stride 最小者，取出时把它的 stride 加 `BIG_STRIDE / priority`（`wrapping_add`）。`sys_set_priority(prio)` 当 `prio < 2` 返回 -1，否则写回并返回 prio。

## 问答题

### 1. p1.stride=255, p2.stride=250, pass=10, 8-bit u8，p2 跑一个时间片后下一次是谁？

**实际并不会轮到 p1**，仍会是 p2。

原因是 `u8` 溢出：p2 执行后其 stride 变为 `(250 + 10) mod 256 = 4`。

### 2. 为什么 `STRIDE_MAX − STRIDE_MIN ≤ BigStride / 2`？

用归纳法说明：

- **基础情况**：所有进程初始 stride 都为 0，差为 0，显然 ≤ BigStride/2。

- **归纳步骤**：假设某次调度前，`MAX − MIN ≤ BigStride/2`。本次调度器必然挑的是 stride 为 `MIN` 的进程 P 执行，执行后 P 的 stride 变为 `MIN + pass_P`，其中 `pass_P ≤ BigStride/2`。设新一轮的 MIN'、MAX'：

分两种情况讨论 MAX'：
  1. 若 `MIN + pass_P ≤ MAX`（P 跑完后 stride 仍不超过原 MAX），则 MAX' = MAX。由于 MIN' ≥ MIN，所以 `MAX' − MIN' ≤ MAX − MIN ≤ BigStride/2`，继续满足。
  2. 若 `MIN + pass_P > MAX`（P 成了新的 MAX），则 MAX' = `MIN + pass_P`。此时 MIN' 是"剩下进程里最小的那个 stride"，它一定 ≥ 旧 MIN。于是 `MAX' − MIN' ≤ (MIN + pass_P) − MIN = pass_P ≤ BigStride/2`。

两种情形下都有 `MAX' − MIN' ≤ BigStride/2`。归纳成立。

### 3. 带溢出感知的 `partial_cmp`

思路：由上题知任意时刻两个有效 stride 的"真实差值"绝对值不超过 `BigStride/2`。利用 `wrapping_sub` 取两者的环上距离，若 `a - b (wrapping) ≤ BigStride/2` 则 `a` 实际走得更远（更大），反之更小。

```rust
use core::cmp::Ordering;

const BIG_STRIDE: u64 = /* 调度器约定的大常数 */;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // 题目假设两个 Stride 不相等，这里只需要分 Greater / Less。
        // 若 self 环上领先 other 不超过 BigStride/2，判为更大；
        // 否则 other 在环上领先 self，self 更小。
        if self.0.wrapping_sub(other.0) <= BIG_STRIDE / 2 {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}
```

这样放进 `BinaryHeap<Stride>` 时，`pop` 由于默认是最大堆，要么反过来把语义改成"最小 stride"再 pop，要么把 `Greater/Less` 交换一下——核心不变。

## 荣誉准则

1、在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

答：无

2、此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

答：使用 Claude 进行辅助编程，但每一行代码都检视过。

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按"-100"分计。
