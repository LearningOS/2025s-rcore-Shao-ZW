## 功能介绍
实现SV39下的sys_spawn和stride调度算法。
 
## 简答作业：
在这种情况下，p2.stride = 250，执行一个时间片后，p2.stride变为250 + 10 = 260，由于是 8 位无符号整型，会发生溢出，变为4。而p1.stride = 255，此时p2.stride小于p1.stride，所以实际上不是轮到p1执行。

可以采用归纳的思想考察，由于每个pass都严格<=BigStride / 2，当再tasks中选择stride最小的时，即使给它加上pass，和其他任何一个的task的stride都满足上式。
    
use core::cmp::Ordering;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.0.abs_diff(other.0) < BIGSTRIDE / 2 {
            if self.0 < other.0 {
                return Some(Ordering::Less);
            } else {
                return Some(Ordering::Greater);
            }
        } else {
            if self.0 > other.0 {
                return Some(Ordering::Less);
            } else {
                return Some(Ordering::Greater);
            }
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

荣誉准则：

在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容： NONE

此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容： NONE

我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。