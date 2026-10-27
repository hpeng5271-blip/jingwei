use crate::model::{Judgment, Verdict};

/// 合成结果。`Uncertain` 带原因码，便于上层区分“没数据 / 覆盖不足 / 冲突”。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Held,
    Refuted,
    Uncertain(&'static str),
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Held => "held",
            Outcome::Refuted => "refuted",
            Outcome::Uncertain(_) => "uncertain",
        }
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Outcome::Uncertain(r) => Some(r),
            _ => None,
        }
    }
}

/// R2 主从 + 矛盾闸 + 覆盖闸（v0.1 默认策略）。
///
/// 判定顺序：
/// 1. 没有任何判断 → uncertain(no_data)
/// 2. 同时有支持与反驳证据 → uncertain(conflict)  ← 矛盾闸：不许吞
/// 3. 覆盖率不足 → uncertain(coverage_short)        ← 覆盖闸：不许假 held
/// 4. 否则取主判断（coverage 最大的那条）的 verdict
pub fn compose(judgments: &[Judgment], supports: i64, contradicts: i64) -> Outcome {
    if judgments.is_empty() {
        return Outcome::Uncertain("no_data");
    }
    if supports > 0 && contradicts > 0 {
        return Outcome::Uncertain("conflict");
    }
    let main = judgments
        .iter()
        .max_by_key(|j| (j.coverage_read, j.at))
        .expect("non-empty checked above");
    if main.coverage_read < main.coverage_total {
        return Outcome::Uncertain("coverage_short");
    }
    match main.verdict {
        Verdict::Held => Outcome::Held,
        Verdict::Refuted => Outcome::Refuted,
        Verdict::Uncertain => Outcome::Uncertain("verdict_uncertain"),
    }
}

/// 两条判断是否构成“冲突”：同 subject、结论相反、都覆盖完整。
pub fn is_conflict(a: &Judgment, b: &Judgment) -> bool {
    a.subject == b.subject
        && a.id != b.id
        && a.verdict != b.verdict
        && a.verdict != Verdict::Uncertain
        && b.verdict != Verdict::Uncertain
        && a.coverage_read >= a.coverage_total
        && b.coverage_read >= b.coverage_total
}
