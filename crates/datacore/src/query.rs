use crate::capability::{feature_required, path_of_subject, Caps};
use crate::compose::{compose, Outcome};
use crate::error::Result;
use crate::model::Judgment;
use crate::store::{EvidenceRow, Store};

/// 一次回答：结论 + 主判断 + 依据链。`masked` = 因能力缺失而被挡掉的判断数（不许静默丢）。
#[derive(Debug)]
pub struct Answer {
    pub outcome: Outcome,
    pub main: Option<Judgment>,
    pub evidence: Vec<EvidenceRow>,
    pub masked: usize,
}

/// 回答“subject 现在是什么结论”（旧语义：不查语言能力）。
///
/// `as_of` 给定时只看 `at <= as_of` 的判断（历史回看）。
/// 过期 = `expires_at <= 基准`（含端点），基准见函数内的 threshold；
/// 若判断全被剔除 → `uncertain(expired)`。
pub fn ask(store: &Store, subject: &str, as_of: Option<i64>) -> Result<Answer> {
    ask_with(store, subject, as_of, &crate::capability::AllowAll)
}

/// 带能力协商的回答：需要能力而该路径不支持的判断先被 mask 掉。
///
/// - 全部被 mask → `uncertain(no_capability)`（不是 no_data：数据在，是我们没能力读）
/// - 部分被 mask → 只对剩下的 compose，`masked` 记下被挡掉的条数（不静默丢）
pub fn ask_with(
    store: &Store,
    subject: &str,
    as_of: Option<i64>,
    caps: &dyn Caps,
) -> Result<Answer> {
    let all = store.judgments_for(subject)?;
    let filtered: Vec<Judgment> = match as_of {
        Some(t) => all.into_iter().filter(|j| j.at <= t).collect(),
        None => all,
    };

    let now = crate::store::now_secs();
    // 过期基准：as_of 给了就用它；否则取 max(墙钟, 该 subject 最新判断的 at)。
    // 为什么不是纯墙钟：verify/resolve 写新判断时 at 可能被推到 now+1（躲同秒 id 撞车），
    // 旧判断的 expires_at 也随之是 now+1；基准只用墙钟会留出 1 秒窗口，
    // 刚 resolve 完 ask 仍看到旧判断（E3 v2 第 6 步）。含端点：expires_at == 基准即过期。
    let max_at = filtered.iter().map(|j| j.at).max().unwrap_or(now);
    let threshold = as_of.unwrap_or(now.max(max_at));
    let mut expired = false;
    let judgments: Vec<Judgment> = filtered
        .into_iter()
        .filter(|j| match j.expires_at {
            Some(t) if t <= threshold => {
                expired = true;
                false
            }
            _ => true,
        })
        .collect();

    if judgments.is_empty() {
        let outcome = if expired {
            Outcome::Uncertain("expired")
        } else {
            Outcome::Uncertain("no_data")
        };
        return Ok(Answer {
            outcome,
            main: None,
            evidence: Vec::new(),
            masked: 0,
        });
    }

    // 能力闸：只对声明了所需能力的 kind 生效（目前只有 symbol）。
    let subject_path = path_of_subject(subject);
    let mut masked = 0usize;
    let mut kept: Vec<Judgment> = Vec::with_capacity(judgments.len());
    for j in judgments {
        let blocked = match (feature_required(&j.kind), subject_path) {
            (Some(f), Some(path)) => !caps.supports(path, f),
            _ => false,
        };
        if blocked {
            masked += 1;
        } else {
            kept.push(j);
        }
    }
    if kept.is_empty() {
        return Ok(Answer {
            outcome: Outcome::Uncertain("no_capability"),
            main: None,
            evidence: Vec::new(),
            masked,
        });
    }

    let mut supports = 0;
    let mut contradicts = 0;
    for j in &kept {
        supports += store.count_evidence(&j.id, "supports")?;
        contradicts += store.count_evidence(&j.id, "contradicts")?;
    }

    let outcome = compose(&kept, supports, contradicts);
    let main = kept
        .iter()
        .max_by_key(|j| (j.coverage_read, j.at))
        .cloned();
    let evidence = match &main {
        Some(j) => store.evidence_for(&j.id)?,
        None => Vec::new(),
    };

    Ok(Answer {
        outcome,
        main,
        evidence,
        masked,
    })
}
