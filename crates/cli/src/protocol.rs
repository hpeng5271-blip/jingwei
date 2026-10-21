//! **统一输出协议**：结论 + 三态 + 依据 + 覆盖度（2026-10-21）。
//!
//! ## 为什么需要它
//!
//! 概览卡、编辑器状态栏、报告窗、LSP 诊断……都要渲染**同一份**信息。没有统一形状，
//! 每个出口就会各拼各的——那是 14 个 UI 文件各拼各数据的根，也是"同一件事两个说法"的根。
//!
//! ## 四样东西各自回答什么
//!
//! - `state`：**结论能不能下**。`yes` 能；`no` 不能（有待处理项，现在下结论会误导）；
//!   `unknown` **查不了**（没有可判断的对象）。三态不许压扁：查不了 ≠ 没有。
//! - `summary`：一句话结论。用户先看它，其余都是它的依据分解。
//! - `evidence`：**凭什么**。每条都要能自己复跑（库 / 命令 / 文件）。空依据不许有结论。
//! - `coverage`：**覆盖了多少**。`read/total` + `partial`；读了一部分就下全称判断，
//!   是这类工具最常见的误导来源（"入口 5 个里只验了 1 个"）。
//!
//! 领域字段不进 `data` 子对象，而是**平铺**在顶层——保持既有 `--json` 字段名不变，
//! 老脚本不用改；新消费者只认这四个键。

use serde_json::{json, Value};

/// 结论的三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// 能下结论
    Yes,
    /// 能下结论，但现在不该下（有待处理项）
    No,
    /// 查不了（没有可判断的对象）
    Unknown,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Yes => "yes",
            State::No => "no",
            State::Unknown => "unknown",
        }
    }
}

/// 一条依据。`kind` 只有三种：`graph`（库）/ `command`（可复跑的命令）/ `file`（文件或目录）。
pub struct Evidence {
    pub kind: &'static str,
    pub at: String,
    pub note: String,
}

impl Evidence {
    pub fn graph(root: &std::path::Path, note: &str) -> Self {
        Self {
            kind: "graph",
            at: format!("{}/.jingwei/graph.db", root.display()),
            note: note.to_string(),
        }
    }

    pub fn command(cmd: &str, note: &str) -> Self {
        Self {
            kind: "command",
            at: cmd.to_string(),
            note: note.to_string(),
        }
    }

    pub fn file(at: &str, note: &str) -> Self {
        Self {
            kind: "file",
            at: at.to_string(),
            note: note.to_string(),
        }
    }
}

/// 覆盖度：这次真的看了多少。
pub struct Coverage {
    pub read: usize,
    /// 全量是多少；**不知道就写 None**（不许把"不知道"报成 0）。
    pub total: Option<usize>,
    /// 只覆盖了一部分（例如图谱还没建、只读了知识层）。
    pub partial: bool,
    pub note: String,
}

impl Coverage {
    pub fn full(read: usize, note: &str) -> Self {
        Self {
            read,
            total: Some(read),
            partial: false,
            note: note.to_string(),
        }
    }

    pub fn partial(read: usize, total: Option<usize>, note: &str) -> Self {
        Self {
            read,
            total,
            partial: true,
            note: note.to_string(),
        }
    }
}

/// 组装一份带协议的 JSON。领域字段平铺在顶层，四件套固定。
pub fn envelope(
    state: State,
    summary: impl Into<String>,
    evidence: Vec<Evidence>,
    coverage: Coverage,
    fields: Value,
) -> Value {
    debug_assert!(
        !evidence.is_empty(),
        "没有依据就不该有结论：envelope() 至少要一条 evidence"
    );
    let summary = summary.into();
    let mut v = json!({
        "state": state.as_str(),
        "summary": summary,
        // **与引擎的 `graph::envelope::Envelope` 对齐**：外壳只认 `state` + `reason`。
        // 同文输出，GUI 与 CLI 才不会各说各的（两套说法正是这套协议要消灭的东西）。
        "reason": summary,
        "evidence": evidence
            .iter()
            .map(|e| json!({"kind": e.kind, "ref": e.at, "note": e.note}))
            .collect::<Vec<_>>(),
        "coverage": {
            "read": coverage.read,
            "total": coverage.total,
            "partial": coverage.partial,
            "note": coverage.note,
        },
    });
    if let (Value::Object(dst), Value::Object(src)) = (&mut v, fields) {
        for (k, val) in src {
            // 四件套是协议本身，领域字段不许覆盖它们
            if matches!(
                k.as_str(),
                "state" | "summary" | "reason" | "evidence" | "coverage"
            ) {
                continue;
            }
            dst.insert(k, val);
        }
    }
    v
}

/// 打印一份协议 JSON（所有命令共用这一个出口，免得有人手写 `println!` 漏字段）。
pub fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}
