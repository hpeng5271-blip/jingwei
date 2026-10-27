use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 三态：成立 / 不成立 / 不确定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Held,
    Refuted,
    Uncertain,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Held => "held",
            Verdict::Refuted => "refuted",
            Verdict::Uncertain => "uncertain",
        }
    }
}

/// 证据极性：支持成立 / 支持不成立。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    Supports,
    Contradicts,
}

impl Polarity {
    pub fn as_str(self) -> &'static str {
        match self {
            Polarity::Supports => "supports",
            Polarity::Contradicts => "contradicts",
        }
    }
}

/// 范围四维：对象 / 时间 / 环境 / 粒度。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scope {
    pub objects: Vec<String>,
    pub time_window: Option<(i64, i64)>,
    pub env: BTreeMap<String, String>,
    pub granularity: String,
}

/// 一条判断。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Judgment {
    pub id: String,
    pub subject: String,
    pub verdict: Verdict,
    pub kind: String,
    pub scope: Scope,
    pub role: Option<String>,
    pub expires_at: Option<i64>,
    pub at: i64,
    pub coverage_read: i64,
    pub coverage_total: i64,
}

/// 依据来源（观测手段 + 环境 + 时刻）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub instrument: String,
    pub instrument_version: Option<String>,
    pub env: String,
    pub time: i64,
}

/// 一条依据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub judgment_id: String,
    pub premise: String,
    pub polarity: Polarity,
    pub source_id: String,
}

/// 人工记录的状态行 → 三态（A2 裁决的优先级）。
///
/// 优先级：作废/撤销/回滚 > 部分完成/待实施/未决 > 已落地/已完成。
/// 读法：只表示「这条决策当前算不算数」，**不表示「代码照做了」**；
/// 没有状态行 → uncertain（没写就是不知道）。
pub fn record_verdict(status_raw: &str) -> Verdict {
    let s = status_raw;
    let has = |keys: &[&str]| keys.iter().any(|k| s.contains(k));

    if has(&["作废", "撤销", "回滚", "废弃"]) {
        return Verdict::Refuted;
    }
    if has(&[
        "部分完成",
        "部分落地",
        "待实施",
        "待执行",
        "待定",
        "未决",
        "待办",
    ]) {
        return Verdict::Uncertain;
    }
    if has(&[
        "已落地",
        "已完成",
        "已实现",
        "已实施",
        "已裁定",
        "已定",
        "已采纳",
        "都已实现",
    ]) {
        return Verdict::Held;
    }
    Verdict::Uncertain
}

/// 一次改动。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    pub commit_hash: String,
    pub path: String,
    pub status: String,
    pub author: String,
    pub at: i64,
    pub subject: String,
    /// rename/copy 的源路径（R/C 才有）。
    pub old_path: Option<String>,
    /// 新内容 blob id（内容身份来源）；删除时为空。
    pub blob: Option<String>,
}

/// 解析规则版本：全仓唯一真相（cli/engine/壳共用）。
pub const PARSE_RULES_VERSION: &str = "27";
