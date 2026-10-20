//! 文件的生命周期状态机（2026-10-20，pro 定：唯一值得显式化的第一处）。
//!
//! 为什么需要它：现在的"跳过"是散在 `parse_pool::Skip` 与几个计数器里的**分支**，
//! 没有一张能穷举的表。于是"删了又改""超大之后变小""删掉后又被报成同内容"这类转移
//! 没有地方能断言——而它们恰恰是数据正确性事故点。
//!
//! **最要紧的一条**：`Deleted` 状态下再收到"内容没变"（`ScanSame`）是**非法**的。
//! 文件已经没了，还说内容一样，说明扫描侧看到的东西和库里的认知对不上——
//! 这种时候必须报错，而不是悄悄回到 `Unchanged` 把它盖过去。
//!
//! 三态（是/不是/查不了）是**值域**，不是流程：本模块只管"文件处在哪个阶段"，
//! 不管"结论可不可信"。两者不要混。

use std::fmt;

/// 文件在索引生命周期里的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileLife {
    /// 从没见过（库里没有这行）
    Unseen,
    /// 见过，且内容没变（上次索引后没有再动）
    Unchanged,
    /// 见过，且内容变了（待重解析）
    Modified,
    /// 见过，但现在磁盘上没了（墓碑：节点已删，状态保留）
    Deleted,
}

impl FileLife {
    pub fn as_str(self) -> &'static str {
        match self {
            FileLife::Unseen => "unseen",
            FileLife::Unchanged => "unchanged",
            FileLife::Modified => "modified",
            FileLife::Deleted => "deleted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "unseen" => FileLife::Unseen,
            "unchanged" => FileLife::Unchanged,
            "modified" => FileLife::Modified,
            "deleted" => FileLife::Deleted,
            _ => return None,
        })
    }
}

impl fmt::Display for FileLife {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 作用在文件上的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileEvent {
    /// 扫描到文件，内容哈希与库里一致
    ScanSame,
    /// 扫描到文件，内容哈希与库里不同
    ScanChanged,
    /// 扫描发现文件已经不在了
    Delete,
    /// 文件在删除之后又出现了
    RescanBack,
}

#[derive(Debug, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: FileLife,
    pub event: FileEvent,
}

impl fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "非法转移：{} --{:?}-->（没有定义）", self.from, self.event)
    }
}

/// **唯一的转移函数**：所有状态写入都必须经过它。
///
/// 返回 `Err` = 这个事件在当前状态下不该发生。调用方要**报错**，
/// 不许自己兜底成一个"看起来正常"的状态。
pub fn transition(from: FileLife, event: FileEvent) -> Result<FileLife, IllegalTransition> {
    use FileEvent::*;
    use FileLife::*;
    let to = match (from, event) {
        (Unseen, ScanSame) => Unchanged,
        (Unseen, ScanChanged) => Modified,
        (Unseen, Delete) => Deleted,
        (Unchanged, ScanSame) => Unchanged,
        (Unchanged, ScanChanged) | (Unchanged, RescanBack) => Modified,
        (Unchanged, Delete) => Deleted,
        (Modified, ScanSame) => Unchanged,
        (Modified, ScanChanged) | (Modified, RescanBack) => Modified,
        (Modified, Delete) => Deleted,
        // 删了又重新出现：按"变了"处理（哪怕内容与删除前一样，时间线已经不同）
        (Deleted, RescanBack) | (Deleted, ScanChanged) => Modified,
        (Deleted, Delete) => Deleted,
        // **故意非法**：删掉之后还报"内容没变"——扫描与库的认知对不上
        (Deleted, ScanSame) => return Err(IllegalTransition { from, event }),
        (Unseen, RescanBack) => return Err(IllegalTransition { from, event }),
    };
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::FileEvent::*;
    use super::FileLife::*;
    use super::*;

    /// **穷举 4 状态 × 4 事件**：每一格都写死期望，删掉/改错一条转移测试必红。
    #[test]
    fn 状态乘事件全组合都有明确答案() {
        let table: &[(FileLife, FileEvent, Option<FileLife>)] = &[
            (Unseen, ScanSame, Some(Unchanged)),
            (Unseen, ScanChanged, Some(Modified)),
            (Unseen, Delete, Some(Deleted)),
            (Unseen, RescanBack, None),
            (Unchanged, ScanSame, Some(Unchanged)),
            (Unchanged, ScanChanged, Some(Modified)),
            (Unchanged, Delete, Some(Deleted)),
            (Unchanged, RescanBack, Some(Modified)),
            (Modified, ScanSame, Some(Unchanged)),
            (Modified, ScanChanged, Some(Modified)),
            (Modified, Delete, Some(Deleted)),
            (Modified, RescanBack, Some(Modified)),
            (Deleted, ScanSame, None),
            (Deleted, ScanChanged, Some(Modified)),
            (Deleted, Delete, Some(Deleted)),
            (Deleted, RescanBack, Some(Modified)),
        ];
        for (from, event, want) in table {
            let got = transition(*from, *event).ok();
            assert_eq!(&got, want, "{from} --{event:?}--> 期望 {want:?} 实际 {got:?}");
        }
    }

    /// 最要紧的一条非法转移：删掉的文件还能"内容没变"。
    #[test]
    fn 删除之后内容没变是非法转移() {
        assert_eq!(
            transition(Deleted, ScanSame),
            Err(IllegalTransition {
                from: Deleted,
                event: ScanSame
            })
        );
    }

    #[test]
    fn 枚举与字符串互转是闭合的() {
        for s in [Unseen, Unchanged, Modified, Deleted] {
            assert_eq!(FileLife::parse(s.as_str()), Some(s));
        }
        assert_eq!(FileLife::parse("nope"), None);
    }
}
