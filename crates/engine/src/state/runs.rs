//! 启动记录：每次跑过什么，机器自己记一份。
//!
//! 这是「机器记事实」的那一半。卡片的用处不在当次——当次你看得见——
//! 而在第二天回来时还能回答「昨天那次到底是哪儿不对」。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEEP: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRec {
    pub index: usize,
    pub title: String,
    /// ok / note / fail
    pub status: String,
    pub detail: String,
    pub ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub at: String,
    /// run / test / install
    pub kind: String,
    pub command: String,
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crux: Option<String>,
    #[serde(default)]
    pub stages: Vec<StageRec>,
    /// 只留尾巴，够看懂就够了
    #[serde(default)]
    pub tail: Vec<String>,
    /// 这次跑的时候内存怎么样。有了它，下次跑才能比。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<crate::sys::mem::MemReport>,
}

fn file(root: &Path) -> PathBuf {
    crate::data_dir(root).join("runs.jsonl")
}

pub fn append(root: &Path, rec: &RunRecord) -> Result<()> {
    let p = file(root);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    // 文件里保持「时间顺序」：list() 读进来是最新在前，这里翻回去再接新的。
    // 之前直接把新记录 push 到「最新在前」的数组末尾，等于每写一次就把顺序搅一次。
    let mut all = list(root, usize::MAX);
    all.reverse();
    all.push(rec.clone());
    if all.len() > KEEP {
        all.drain(0..all.len() - KEEP); // 丢最老的
    }
    let body: String = all
        .iter()
        .filter_map(|r| serde_json::to_string(r).ok())
        .map(|l| l + "\n")
        .collect();
    std::fs::write(&p, body)?;
    Ok(())
}

/// 最新的在前。
pub fn list(root: &Path, limit: usize) -> Vec<RunRecord> {
    let Ok(text) = std::fs::read_to_string(file(root)) else {
        return Vec::new();
    };
    let mut out: Vec<RunRecord> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    out.reverse();
    out.truncate(limit);
    out
}

pub fn clear(root: &Path) -> Result<()> {
    let p = file(root);
    if p.is_file() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use std::path::PathBuf;

    /// 每个测试一个独立目录。共用同一个的话，并行跑时一个测试的
    /// scratch() 会把另一个的数据删掉——那会变成随机失败的测试。
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pl-runs-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rec(at: &str) -> RunRecord {
        RunRecord {
            at: at.into(),
            kind: "run".into(),
            command: "python main.py".into(),
            exit_code: 0,
            ready: None,
            crux: None,
            stages: Vec::new(),
            tail: Vec::new(),
            memory: None,
        }
    }

    /// 写完几次之后，「最新在前」必须还成立。
    /// 之前是把新记录 push 到「最新在前」的数组末尾，写一次顺序就乱一次——
    /// 结果查上一次运行会查到更早的那次。
    #[test]
    fn newest_stays_first_after_several_appends() {
        let d = scratch("order");
        // 按 a、b、c 的顺序写，最新在前就该是 c、b、a
        for t in ["a", "b", "c"] {
            append(&d, &rec(t)).unwrap();
        }
        let got: Vec<String> = list(&d, 10).into_iter().map(|r| r.at).collect();
        assert_eq!(got, vec!["c", "b", "a"], "顺序应该是最后写的排最前，实际 {:?}", got);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unreadable_lines_do_not_kill_the_history() {
        let d = scratch("badline");
        append(&d, &rec("1")).unwrap();
        let p = file(&d);
        let mut t = std::fs::read_to_string(&p).unwrap();
        t.push_str("{ 这不是 JSON\n");
        std::fs::write(&p, t).unwrap();
        append(&d, &rec("2")).unwrap();
        assert_eq!(list(&d, 10).len(), 2, "坏行该被跳过，好记录该留下");
        let _ = std::fs::remove_dir_all(&d);
    }
}
