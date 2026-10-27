use crate::model::{Change, Judgment, Scope};

/// FNV-1a 64 位：稳定、短、无外部依赖。
pub fn stable_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn judgment_id(repo: &str, kind: &str, subject: &str, at: i64) -> String {
    format!(
        "{repo}:j:{kind}:{at}:{:016x}",
        stable_hash(subject)
    )
}

pub fn evidence_id(judgment_id: &str, premise: &str, polarity: &str) -> String {
    format!("{judgment_id}:e:{polarity}:{:016x}", stable_hash(premise))
}

pub fn source_id(repo: &str, instrument: &str, time: i64) -> String {
    format!("{repo}:s:{instrument}:{time}")
}

pub fn change_id(commit: &str, path: &str) -> String {
    format!("c:{commit}:{:016x}", stable_hash(path))
}

pub fn change_row(
    commit: &str,
    path: &str,
    status: &str,
    author: &str,
    at: i64,
    subject: &str,
    old_path: Option<String>,
    blob: Option<String>,
) -> Change {
    Change {
        id: change_id(commit, path),
        commit_hash: commit.to_string(),
        path: path.to_string(),
        status: status.to_string(),
        author: author.to_string(),
        at,
        subject: subject.to_string(),
        old_path,
        blob,
    }
}

pub fn judgment_row(
    repo: &str,
    subject: &str,
    kind: &str,
    verdict: crate::model::Verdict,
    at: i64,
    coverage_read: i64,
    coverage_total: i64,
    scope: Scope,
) -> Judgment {
    Judgment {
        id: judgment_id(repo, kind, subject, at),
        subject: subject.to_string(),
        verdict,
        kind: kind.to_string(),
        scope,
        role: None,
        expires_at: None,
        at,
        coverage_read,
        coverage_total,
    }
}
