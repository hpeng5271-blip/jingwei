//! T2.3: card_value 与最小助手闭包，自 cli/src/main.rs 原样迁入（逻辑/字段不改）。
//! cli 与 Tauri 壳共用这一份取数逻辑，避免两套漂移。

use datacore::query;
use datacore::store::Store;
use std::path::Path;

/// 一张卡的原始数据：JSON 契约与文本渲染共用同一份，避免两套取数逻辑漂移。
pub fn card_value(store: &Store, root: &Path, input: &str, history: bool, state: Option<&str>) -> anyhow::Result<(u8, serde_json::Value)> {
    // 路径形输入但磁盘/库里都没有：给 reason，不用全 0 卡冒充"就是没有"（commit/dep/doc/decision 不在此列）
    let looks_path = !input.is_empty()
        && !input.contains(':')
        && (input.contains('/') || input.contains('\\') || input.ends_with(".rs"));
    if looks_path {
        let clean = input.trim_start_matches("file:").trim_end_matches('/');
        if !root.join(clean).exists()
            && store.find_judgment(&format!("file:{clean}"))?.is_none()
        {
            return Ok((2, serde_json::json!({
                "v": 1, "path": input, "reason": "empty_range", "exit": 2,
            })));
        }
    }
    // 决策卡：decision:<repo>#<id>（摘要不显示标题——库没存）
    if let Some(rest) = input.strip_prefix("decision:") {
        let prefix = if rest.contains('#') {
            format!("decision:{rest}")
        } else {
            format!("decision:%#{rest}")
        };
        let Some(j) = store.find_by_subject_prefix(&prefix)? else {
            return Ok((2, serde_json::json!({
                "v": 1, "mode": "decision", "subject": prefix,
                "error": "no_data", "exit": 2,
            })));
        };
        let alts = store.alternatives_detail(&j.id)?;
        let picked = alts.iter().filter(|a| a.1 == "held").count();
        let rejected = alts.iter().filter(|a| a.1 == "refuted").count();
        let uncertain = alts.len() - picked - rejected;
        let premise = store.first_premise(&j.id)?;
        let mut v = serde_json::json!({
            "v": 1,
            "mode": "decision",
            "subject": j.subject,
            "verdict": j.verdict.as_str(),
            "candidates": alts.len(),
            "picked": picked,
            "rejected": rejected,
            "uncertain": uncertain,
            "evidence": premise,
            "original": serde_json::Value::Null,
            "exit": 0,
            "scope": "project",
            "files_considered": 1,
            "truncated": false,
            "next_cursor": serde_json::Value::Null,
        });
        if state == Some("original") {
            // 原文：只从 <id> 推导（docs/decisions/<id>*.md），不猜别的目录；失败一次不崩
            let id = j.subject.split_once('#').map(|(_, i)| i.to_string()).unwrap_or_default();
            let dir = root.join("docs").join("decisions");
            let original = match std::fs::read_dir(&dir) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    serde_json::json!({"error": "未采集：docs/decisions 目录不存在"})
                }
                Err(e) => serde_json::json!({"error": format!("未采集：读不了目录（{e}）")}),
                Ok(rd) => {
                    let mut found: Option<std::path::PathBuf> = None;
                    for e in rd.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.starts_with(&id) && name.ends_with(".md") {
                            found = Some(e.path());
                            break;
                        }
                    }
                    match found.map(|p| std::fs::read_to_string(p)) {
                None => serde_json::json!({"error": "未采集：docs/decisions 下没有这个 id 的原文"}),
                Some(Err(e)) => serde_json::json!({"error": format!("未采集：读不了（{e}）")}),
                Some(Ok(text)) => {
                    let title = text
                        .lines()
                        .find(|l| l.trim_start().starts_with("# "))
                        .map(|l| l.trim_start_matches('#').trim().to_string())
                        .unwrap_or_default();
                    let mut decision = String::new();
                    let mut in_sec = false;
                    for line in text.lines() {
                        let l = line.trim();
                        if l.starts_with("## ") {
                            in_sec = l.contains("决策");
                            continue;
                        }
                        if in_sec && !l.is_empty() {
                            decision = l.to_string();
                            break;
                        }
                    }
                    serde_json::json!({"title": title, "decision": decision})
                }
                    }
                }
            };
            v["original"] = original;
        }
        return Ok((0, v));
    }

    // 符号态：def（本文件定义）/ dups（同名多定义）
    if let Some(st) = state {
        if st == "def" || st == "dups" {
            let rel = input.replace('\\', "/");
            if st == "def" {
                let syms = store.symbol_defs(&rel)?;
                let list: Vec<serde_json::Value> = syms
                    .iter()
                    .map(|(n, k, line)| serde_json::json!({"name": n, "kind": k, "line": line}))
                    .collect();
                return Ok((0, serde_json::json!({
                    "v": 1, "mode": "def", "path": rel, "total": list.len(),
                    "symbols": list, "exit": 0,
                    "scope": "file", "files_considered": 1, "truncated": false, "next_cursor": serde_json::Value::Null,
                })));
            }
            let dups = store.symbol_dups(&rel)?;
            let list: Vec<serde_json::Value> = dups
                .iter()
                .map(|(n, c)| serde_json::json!({"name": n, "files": c}))
                .collect();
            return Ok((0, serde_json::json!({
                "v": 1, "mode": "dups", "path": rel, "total": list.len(),
                "dups": list, "exit": 0,
                "scope": "file", "files_considered": 1, "truncated": false, "next_cursor": serde_json::Value::Null,
            })));
        }
    }
    // 范围选择器：目录 → 聚合卡（不画图、不猜结构，只数已有的）
    if root.join(input).is_dir() {
        let dir = input
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string();
        let dir = if dir.is_empty() { ".".to_string() } else { dir };
        let (files_hist, changes, symbols, docs, jobs) = store.range_stats(&dir)?;
        // files/definitions 与"逐文件 card"同一口径（facts 重解析）：files = 该前缀下已解析 .rs 数，
        // definitions = Σ 每文件 definitions。超过 300 文件时退回 DB 下界并标注，不静默换口径。
        let mut parsed_files: i64 = 0;
        let mut defs_sum: i64 = 0;
        let mut facts_ok = true;
        for rel in collect::parse::walk_paths(root) {
            let in_range = dir == "."
                || rel == dir
                || rel.strip_prefix(&format!("{dir}/")).is_some();
            if !in_range {
                continue;
            }
            parsed_files += 1;
            if parsed_files <= 300 {
                defs_sum += collect::parse::facts(&root.join(&rel)).mapped as i64;
            } else {
                facts_ok = false;
            }
        }
        let files = parsed_files;
        let _ = files_hist;
        // 列表最多 5 条（SQL GROUP BY，不逐文件扫）
        let top = store.range_top_files(&dir, 5)?;
        let (sym_j, sym_defs) = store.range_symbols(&dir)?;
        let recent = store.range_recent_files(&dir, 3)?;
        let decisions = store.range_decisions(&dir)?;
        // 依据数要 join evidence 全表（k8s 上 1.3s）：大范围显式标"未统计"，不抽样
        let (evidence, evidence_note) = if files > 1000 {
            (None, Some("范围过大（>1000 文件），依据数未统计"))
        } else {
            (Some(store.range_evidence(&dir)?), None)
        };
        // deps 节只在真有 Cargo.toml 时出现（没有就整节省略，不是"依赖 0"）
        let (edge_in, edge_ext, edge_impl) = store.edge_summary_prefix(&dir)?;
        let (tests_total, tests_std, tests_macro) = store.range_tests(&dir)?;
        let (call_total, call_ok, call_un) = store.range_calls(&dir)?;
        let manifest = root.join(&dir).join("Cargo.toml");
        let dep_names = if manifest.is_file() {
            Some(collect::cargo::manifest_dep_names(&manifest))
        } else {
            None
        };
        let days = if history {
            store.change_days_for_prefix(&dir)?
        } else {
            Vec::new()
        };
        let mut v = serde_json::json!({
            "v": 1,
            "mode": if history { "range_history" } else { "range" },
            "path": dir,
            "files": files,
            "changes": changes,
            "symbols": symbols,
            "docs": docs,
            "judgments": jobs,
            "top": top.iter().map(|(p, n)| serde_json::json!({"path": p, "n": n})).collect::<Vec<_>>(),
            "top_more": (files - top.len() as i64).max(0),
            "edges": {"use": {"internal": edge_in, "ext": edge_ext}, "impl": edge_impl,
                      "call": {"total": call_total, "resolved": call_ok, "unresolved": call_un}},
            "tests": {"total": tests_total, "standard": tests_std, "macro": tests_macro},
            "call": {"total": call_total, "resolved": call_ok, "unresolved": call_un},
            "symbols": {"judgments": sym_j, "definitions": if facts_ok { defs_sum } else { sym_defs },
                        "definitions_note": if facts_ok { serde_json::Value::Null } else { serde_json::json!("范围过大（>300 文件）：definitions 退回 DB 下界（未重解析）") }},
            "recent": recent.iter().map(|(p, t)| serde_json::json!({"path": p, "at": t})).collect::<Vec<_>>(),
            "decisions": decisions,
            "evidence": evidence,
            "evidence_note": evidence_note,
            "deps": dep_names.as_ref().map(|names| serde_json::json!({
                "names": names, "more": 0,
            })),
            "days": days.iter().map(|(d, n)| serde_json::json!({"day": day_str(*d), "n": n})).collect::<Vec<_>>(),
            "exit": 0,
            "scope": if dir == "." { "project" } else { "module" },
            "files_considered": files,
            "truncated": files > 1000 || !facts_ok,
            "next_cursor": serde_json::Value::Null,
        });
        if dep_names.is_none() {
            if let Some(o) = v.as_object_mut() {
                o.remove("deps");
            }
        }
        return Ok((0, v));
    }

    // C3：提交选择器
if is_commit_like(input) {
        let files = store.files_for_commit(input)?;
        let meta = store.commit_meta(input)?;
        let mut added = 0;
        let mut modified = 0;
        let mut deleted = 0;
        for (_, st) in &files {
            match st.chars().next() {
                Some('A') => added += 1,
                Some('D') => deleted += 1,
                _ => modified += 1,
            }
        }
        let v = serde_json::json!({
            "v": 1,
            "mode": "commit",
            "commit": input,
            "author": meta.as_ref().map(|m| m.0.clone()),
            "at": meta.as_ref().map(|m| m.2),
            "message": meta.as_ref().map(|m| m.1.clone()),
            "exit": if meta.is_some() { 0 } else { 2 },
            "files": {
                "total": files.len(),
                "added": added,
                "modified": modified,
                "deleted": deleted,
                "list": files.iter().take(20).map(|(p, s)| serde_json::json!({"path": p, "status": s})).collect::<Vec<_>>(),
            },
            "scope": "project",
            "files_considered": files.len(),
            "truncated": files.len() > 20,
            "next_cursor": serde_json::Value::Null,
        });
        return Ok((if meta.is_some() { 0 } else { 2 }, v));
    }

    let mut subject = resolve_subject(store, input);
    let mut path = subject_path(&subject).to_string();
    // 前向跟随：旧路径被改名过（且当前树里已不存在）→ 跟到最新路径
    let mut moved_to: Option<String> = None;
    if !root.join(&path).exists() {
        let mut cur = path.clone();
        for _ in 0..20 {
            match store.moved_to(&cur)? {
                Some(next) if next != cur => {
                    moved_to = Some(next.clone());
                    cur = next;
                }
                _ => break,
            }
        }
        if let Some(new_path) = moved_to.clone() {
            path = new_path;
            subject = format!("file:{path}");
            if subject_path(&subject) == path && !path.is_empty() {
                // 保持 subject 形式一致（file:<path>）
            }
        }
    }
    let ans = query::ask_with(store, &subject, None, &collect::capability::LangCaps)?;

    // 边层：让 C1 卡也能看见导入/实现（imports/implements）
    let (use_in, use_ext, impl_n) = store.edge_summary_src(&path)?;
    if state == Some("deps") {
        let top = store.top_use_targets(&path, 3)?;
        let total: i64 = use_in + use_ext;
        return Ok((0, serde_json::json!({
            "v": 1, "mode": "file_deps", "path": path,
            "use": {"internal": use_in, "ext": use_ext}, "impl": impl_n,
            "top": top.iter().map(|(d, n)| serde_json::json!({"dst": d, "n": n})).collect::<Vec<_>>(),
            "more": (total - top.len() as i64).max(0),
            "exit": 0,
            "scope": "file", "files_considered": 1, "truncated": false, "next_cursor": serde_json::Value::Null,
        })));
    }

    let code = match &ans.outcome {
        datacore::compose::Outcome::Held => 0u8,
        datacore::compose::Outcome::Refuted => 1u8,
        datacore::compose::Outcome::Uncertain(_) => 2u8,
    };
    let state = match &ans.outcome {
        datacore::compose::Outcome::Held => "held".to_string(),
        datacore::compose::Outcome::Refuted => "refuted".to_string(),
        datacore::compose::Outcome::Uncertain(r) => format!("uncertain({r})"),
    };

    let rows = store.changes_for(&path)?;
    if history {
        let days = store.change_days_for_path(&path)?;
        let total: i64 = days.iter().map(|(_, n)| n).sum();
        let v = serde_json::json!({
            "mode": "history",
            "subject": subject,
            "path": path,
            "state": state,
            "exit": code,
            "changes": total,
            "days": days.iter().map(|(d, n)| serde_json::json!({
                "day": day_str(*d), "n": n,
            })).collect::<Vec<_>>(),
            "recent": rows.iter().take(3).map(|r| serde_json::json!({
                "commit": r.commit_hash,
                "status": r.status,
                "at": r.at,
                "author": r.author,
                "subject": r.subject,
            })).collect::<Vec<_>>(),
            "scope": "file", "files_considered": 1, "truncated": false, "next_cursor": serde_json::Value::Null,
        });
        return Ok((code, v));
    }

    // 家谱：移动过的文件把旧路径的改动也算进来（b:<blob> 只表内容同一性，不作家谱）
    let lineage = store.lineage_paths(&path, 20)?;
    let lineage_changes = store.changes_for_paths(&lineage)?;
    let rows = store.changes_for(&path)?;
    let rows = store.changes_for(&path)?;

    let defs = store.symbol_definitions_for_path(&path)?;
    let tests = store.tests_for_path(&path)?;
    let doc = store.find_judgment(&format!("doc:{path}"))?;
    let mut names: Vec<&str> = defs.iter().map(|(n, _, _)| n.as_str()).collect();
    names.sort();
    names.dedup();
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (_, kind, _) in &defs {
        *kinds.entry(kind.clone()).or_default() += 1;
    }

    let mut v = serde_json::json!({
        "v": 1,
        "subject": subject,
        "path": path,
        "state": state,
        "exit": code,
        "judgment": ans.main.as_ref().map(|j| serde_json::json!({
            "id": j.id,
            "kind": j.kind,
            "verdict": j.verdict.as_str(),
            "coverage_read": j.coverage_read,
            "coverage_total": j.coverage_total,
            "at": j.at,
            "expires_at": j.expires_at,
            "scope": {
                "granularity": j.scope.granularity,
                "objects": j.scope.objects,
                "env": j.scope.env,
                "time_window": j.scope.time_window,
            },
        })),
        "evidence": ans.evidence.iter().map(|e| serde_json::json!({
            "polarity": e.polarity,
            "premise": e.premise,
            "instrument": e.instrument,
        })).collect::<Vec<_>>(),
        "changes": lineage_changes,
        "lineage": lineage,
        "moved_to": moved_to,
        "edges": {"use": {"internal": use_in, "ext": use_ext}, "impl": impl_n},
        "authors": top_authors(&rows, 2).iter()
            .map(|(a, n)| serde_json::json!({"name": a, "n": n}))
            .collect::<Vec<_>>(),
        "recent": rows.iter().take(3).map(|r| serde_json::json!({
            "commit": r.commit_hash,
            "status": r.status,
            "at": r.at,
            "author": r.author,
            "subject": r.subject,
        })).collect::<Vec<_>>(),
        "doc": doc.as_ref().map(|j| serde_json::json!({
            "subject": j.subject,
            "verdict": j.verdict.as_str(),
        })),
        "symbols": {
            "names": names.len(),
            "definitions": defs.len(),
            "kinds": kinds,
            "status": collect::capability::symbols_status(&path, names.len() as i64),
            "note": collect::capability::symbols_note(&path),
        },
        "tests": {
            "n": tests.len(),
            "macro_n": tests.iter().filter(|(_, _, k)| k == "test_macro").count(),
            "items": tests.iter().map(|(n, l, k)| serde_json::json!({"name": n, "line": l, "kind": k})).collect::<Vec<_>>(),
        },
        "scope": if subject.starts_with("dep:") { "project" } else { "file" },
        "files_considered": 1,
        "truncated": false,
        "next_cursor": serde_json::Value::Null,
    });
    // Cargo.toml 的卡带一句依赖概况（只读统计，不重复落判断）
    if path.ends_with("Cargo.toml") {
        let counts = store.dep_counts()?;
        let total: i64 = counts.iter().map(|(_, n)| n).sum();
        if total > 0 {
            let map: std::collections::BTreeMap<String, i64> = counts.into_iter().collect();
            v["deps"] = serde_json::json!({
                "total": total,
                "normal": map.get("normal").copied().unwrap_or(0),
                "dev": map.get("dev").copied().unwrap_or(0),
                "build": map.get("build").copied().unwrap_or(0),
            });
        }
    }
    Ok((code, v))
}

fn resolve_subject(store: &Store, input: &str) -> String {
    // ① 库里已有这个 subject 就原样用（前缀是任意标签，不要求等于 kind 名）
    if store.find_judgment(input).ok().flatten().is_some() {
        return input.to_string();
    }
    // ② 已知 kind 前缀（kind_registry 决定，不是硬编码）
    if let Some((prefix, _)) = input.split_once(':') {
        if store.kind_layer(prefix).ok().flatten().is_some() {
            return input.to_string();
        }
    }
    if input.contains('#') {
        return format!("symbol:{input}");
    }
    for s in [format!("file:{input}"), format!("doc:{input}")] {
        if store.find_judgment(&s).ok().flatten().is_some() {
            return s;
        }
    }
    format!("file:{input}")
}


pub(crate) fn subject_path(subject: &str) -> &str {
    if let Some(rest) = subject.strip_prefix("symbol:") {
        return rest.split_once('#').map(|(p, _)| p).unwrap_or(rest);
    }
    for p in ["file:", "doc:"] {
        if let Some(rest) = subject.strip_prefix(p) {
            return rest;
        }
    }
    subject
}


/// 看起来像提交哈希（7–40 位十六进制）→ 走 C3 提交卡。
fn is_commit_like(s: &str) -> bool {
    let n = s.len();
    (7..=40).contains(&n) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// unix 秒 → YYYY-MM-DD（UTC）。
pub fn day_str(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant 的 civil_from_days：1970-01-01 起的天数 → (年, 月, 日)。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 改动次数前 n 名的作者。
fn top_authors(rows: &[datacore::store::ChangeRow], n: usize) -> Vec<(String, usize)> {
    let mut m: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for r in rows {
        *m.entry(r.author.clone()).or_default() += 1;
    }
    let mut v: Vec<(String, usize)> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let u = c as u32;
            let wide = (0x1100..=0x115F).contains(&u)
                || (0x2E80..=0xA4CF).contains(&u)
                || (0xAC00..=0xD7A3).contains(&u)
                || (0xF900..=0xFAFF).contains(&u)
                || (0xFE30..=0xFE4F).contains(&u)
                || (0xFF00..=0xFF60).contains(&u)
                || (0xFFE0..=0xFFE6).contains(&u);
            if wide {
                2
            } else {
                1
            }
        })
        .sum()
}

pub fn clip_display(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = display_width(&c.to_string());
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

pub fn density_budget(text: &str, max_lines: usize, width: usize) -> String {
    let mut out = String::new();
    for line in text.lines().take(max_lines) {
        out.push_str(&clip_display(line, width));
        out.push('\n');
    }
    out
}

pub fn render_card_text(v: &serde_json::Value) -> String {
    let raw = render_card_text_raw(v);
    let max_lines = match v["mode"].as_str() {
        Some("commit") => 5,
        Some("range") | Some("range_history") => 10,
        Some("decision") => 7,
        _ => 6,
    };
    // 预算 W=48；实际裁剪留 4 列余量，避免整行贴满（密度 ρ 才能落在 0.45–0.75）
    density_budget(&raw, max_lines, 44)
}

pub fn render_card_text_raw(v: &serde_json::Value) -> String {
    let path = v["path"].as_str().unwrap_or("");
    let state = v["state"].as_str().unwrap_or("");
    let mut out = String::new();

    if v["mode"].as_str() == Some("decision") {
        let mut text = format!(
            "● {}  决策\n{} · {}\n依据 {}\n",
            v["subject"].as_str().unwrap_or(""),
            v["verdict"].as_str().unwrap_or(""),
            if v["picked"].as_i64().unwrap_or(0) == 0 && v["rejected"].as_i64().unwrap_or(0) == 0
            {
                format!("候选 {}（未标注结论；未定 {}）", v["candidates"], v["uncertain"])
            } else {
                format!(
                    "候选 {}（选 {} / 否 {} / 未定 {}）",
                    v["candidates"], v["picked"], v["rejected"], v["uncertain"]
                )
            },
            v["evidence"].as_str().unwrap_or("")
        );
        if let Some(orig) = v["original"].as_object() {
            if let Some(err) = orig.get("error").and_then(|e| e.as_str()) {
                text.push_str(&format!("{err}\n"));
            } else {
                text.push_str(&format!("原文 {}\n", orig.get("title").and_then(|t| t.as_str()).unwrap_or("")));
                if let Some(d) = orig.get("decision").and_then(|d| d.as_str()) {
                    text.push_str(&format!("{d}\n"));
                }
            }
        }
        return density_budget(&text, 7, 48);
    }
    if v["mode"].as_str() == Some("def") || v["mode"].as_str() == Some("dups") {
        let is_def = v["mode"].as_str() == Some("def");
        let mut text = format!(
            "● {}  {}\n",
            v["path"].as_str().unwrap_or(""),
            if is_def { "定义" } else { "同名多定义" }
        );
        let arr = if is_def { &v["symbols"] } else { &v["dups"] };
        if let Some(items) = arr.as_array() {
            if items.is_empty() {
                text.push_str("（无）\n");
            }
            for it in items.iter().take(5) {
                if is_def {
                    text.push_str(&format!(
                        "{}  :{}\n",
                        it["name"].as_str().unwrap_or(""),
                        it["line"].as_i64().unwrap_or(0)
                    ));
                } else {
                    text.push_str(&format!(
                        "{}  ×{}\n",
                        it["name"].as_str().unwrap_or(""),
                        it["files"].as_i64().unwrap_or(0)
                    ));
                }
            }
            if items.len() > 5 {
                text.push_str(&format!("…还有 {} 条\n", items.len() - 5));
            }
        }
        eprintln!("DEBUG_RAW_LINES={} has_call={}", text.lines().count(), text.contains("调用"));
        return density_budget(&text, 12, 48);
    }
    if v["mode"].as_str() == Some("range") || v["mode"].as_str() == Some("range_history") {
        let mut text = format!(
            "● {}  范围{}\n文件 {} · 改动 {}\n符号 {} 条 / {} 定义 · 文档 {}\n判断 {} 条\n",
            v["path"].as_str().unwrap_or(""),
            if v["mode"].as_str() == Some("range_history") {
                "（history）"
            } else {
                ""
            },
            v["files"],
            v["changes"],
            v["symbols"]["judgments"],
            v["symbols"]["definitions"],
            v["docs"],
            v["judgments"]
        );
        text.push_str(&format!(
            "测试 {}（standard {} · macro {}）\n",
            v["tests"]["total"], v["tests"]["standard"], v["tests"]["macro"]
        ));
        if let Some(e) = v["edges"].as_object() {
            let a = e["use"]["internal"].as_i64().unwrap_or(0);
            let b = e["use"]["ext"].as_i64().unwrap_or(0);
            let c = e["impl"].as_i64().unwrap_or(0);
            if a + b + c > 0 {
                text.push_str(&format!("导入 {}（内部 {a} / ext {b}）· 实现 {c}\n", a + b));
            }
            text.push_str(&format!(
                "调用 {}（已解析 {} / 未解析 {}）\n",
                v["call"]["total"], v["call"]["resolved"], v["call"]["unresolved"]
            ));
        }
        if let Some(deps) = v["deps"].as_object() {
            if let Some(names) = deps["names"].as_array() {
                let shown: Vec<String> = names
                    .iter()
                    .take(3)
                    .map(|n| n.as_str().unwrap_or("").to_string())
                    .collect();
                text.push_str(&format!("依赖  {}", shown.join(" · ")));
                if names.len() > 3 {
                    text.push_str(&format!("（还有 {}）", names.len() - 3));
                }
                text.push('\n');
            }
        }
        if let Some(top) = v["top"].as_array() {
            if !top.is_empty() {
                let line = top
                    .iter()
                    .map(|t| {
                        format!(
                            "{} ×{}",
                            t["path"].as_str().unwrap_or(""),
                            t["n"].as_i64().unwrap_or(0)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
                text.push_str(&format!("前 5  {}", line));
                let more = v["top_more"].as_i64().unwrap_or(0);
                if more > 0 {
                    text.push_str(&format!("（还有 {more} 个文件）"));
                }
                text.push('\n');
            }
        }
        if let Some(days) = v["days"].as_array() {
            if !days.is_empty() {
                let line = days
                    .iter()
                    .map(|d| {
                        format!(
                            "{} ×{}",
                            d["day"].as_str().unwrap_or(""),
                            d["n"].as_i64().unwrap_or(0)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
                text.push_str(&format!("按天  {line}\n"));
            }
        }
        return density_budget(&text, 10, 48);
    }

    if v["mode"].as_str() == Some("commit") {
        let sha = v["commit"].as_str().unwrap_or("");
        if v["author"].is_null() {
            out.push_str(&format!("{sha}\n  这个提交没采集（先跑 ingest）\n"));
            return out;
        }
        out.push_str(&format!(
            "{}  {} · {}\n",
            short_commit(sha),
            v["at"].as_i64().map(day_str).unwrap_or_default(),
            v["author"].as_str().unwrap_or("")
        ));
        out.push_str(&format!("{}\n", v["message"].as_str().unwrap_or("")));
        out.push_str(&format!(
            "文件 {}（A {} / M {} / D {}）\n",
            v["files"]["total"],
            v["files"]["added"],
            v["files"]["modified"],
            v["files"]["deleted"]
        ));
        for f in v["files"]["list"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .take(5)
        {
            out.push_str(&format!(
                "  {} {}\n",
                f["path"].as_str().unwrap_or(""),
                f["status"].as_str().unwrap_or("")
            ));
        }
        return out;
    }

    if v["mode"].as_str() == Some("history") {
        let total = v["changes"].as_i64().unwrap_or(0);
        out.push_str(&format!("{path}    {state} · 改动 {total} 次（history）\n"));
        let days = v["days"].as_array().cloned().unwrap_or_default();
        if days.is_empty() {
            out.push_str("按天  无改动记录\n");
        } else {
            let line = days
                .iter()
                .map(|d| {
                    format!(
                        "{} ×{}",
                        d["day"].as_str().unwrap_or(""),
                        d["n"].as_i64().unwrap_or(0)
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ");
            out.push_str(&format!("按天  {line}\n"));
        }
        for r in v["recent"].as_array().cloned().unwrap_or_default() {
            out.push_str(&format!(
                "  {} {} {} <{}> {}\n",
                short_commit(r["commit"].as_str().unwrap_or("")),
                r["status"].as_str().unwrap_or(""),
                r["at"].as_i64().map(day_str).unwrap_or_default(),
                r["author"].as_str().unwrap_or(""),
                r["subject"].as_str().unwrap_or("")
            ));
        }
        return out;
    }

    let changes = v["changes"].as_i64().unwrap_or(0);
    let marker = match state {
        "held" => "●",
        "refuted" => "○",
        _ => "?",
    };
    // 依赖卡：dep:<name> 不是文件，按"声明/解析/类型"三行给
    if v["judgment"]["kind"].as_str() == Some("dependency") {
        let env = &v["judgment"]["scope"]["env"];
        let name = path.strip_prefix("dep:").unwrap_or(path);
        let declared = env["version"].as_str().unwrap_or("—");
        let resolved = env["resolved"].as_str().unwrap_or(declared);
        let dep_kind = env["dep_kind"].as_str().unwrap_or("—");
        let ev = v["evidence"].as_array().map(|a| a.len()).unwrap_or(0);
        out.push_str(&format!("{marker} {name}  {resolved}\n"));
        out.push_str(&format!("声明 {declared} · 解析 {resolved} · 类型 {dep_kind}\n"));
        out.push_str(&format!("依据 {ev} 条（cargo）\n"));
        return density_budget(&out, 6, 48);
    }

    out.push_str(&format!("{marker} {path}  {state}\n"));
    if let Some(mv) = v["moved_to"].as_str() {
        out.push_str(&format!("已改名 → {mv}\n"));
    }
    let old_paths = v["lineage"]
        .as_array()
        .map(|a| a.len().saturating_sub(1))
        .unwrap_or(0);
    let changes_line = if old_paths > 0 {
        format!("改动 {changes}（跨 {old_paths} 个旧路径）")
    } else {
        format!("改动 {changes} 次")
    };
    out.push_str(&format!(
        "{changes_line} · 最近 {}\n",
        v["recent"]
            .as_array()
            .and_then(|a| a.first())
            .map(|r| format!(
                "{} · {}",
                r["at"].as_i64().map(day_str).unwrap_or_default(),
                r["author"].as_str().unwrap_or("")
            ))
            .unwrap_or_else(|| "—".to_string())
    ));

    let syms = &v["symbols"];
    let kinds = syms["kinds"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, n)| format!("{k} {}", n.as_i64().unwrap_or(0)))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .unwrap_or_default();
    let n_syms = syms["names"].as_i64().unwrap_or(0);
    if n_syms == 0 && !path.is_empty() && !path.starts_with("dep:") {
        // 诚实的 0：区分「这门语言没能力」「有能力但文件里没定义」「文件不在 parse 结果里」
        match syms["status"].as_str() {
            Some("no_capability") => {
                out.push_str(&format!(
                    "符号 {}\n",
                    syms["note"].as_str().unwrap_or("无能力（语言未登记）")
                ));
            }
            _ if path.ends_with(".rs") => {
                out.push_str("符号 未解析（不在 parse 结果里，可能超出 --limit）\n");
            }
            _ if !path.contains("://") && !v["judgment"].is_null() => {
                out.push_str("符号 0（文件里没有可映射的定义）\n");
            }
            _ => {}
        }
    } else if n_syms > 0 {
        out.push_str(&format!(
            "符号 {}（{} 处定义{}）",
            n_syms,
            syms["definitions"].as_i64().unwrap_or(0),
            if kinds.is_empty() {
                String::new()
            } else {
                format!("：{kinds}")
            }
        ));
        out.push('\n');
    }

    // 测试：学习要"这个文件有哪些例子"，项目要"测试落点"
    let n_tests = v["tests"]["n"].as_i64().unwrap_or(0);
    if n_tests > 0 {
        let names = v["tests"]["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .take(3)
            .map(|t| t["name"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" / ");
        out.push_str(&format!("测试 {n_tests}（{names}）\n"));
    }

    // 边层：导入（内部/ext）+ 实现
    if let Some(e) = v["edges"].as_object() {
        let a = e["use"]["internal"].as_i64().unwrap_or(0);
        let b = e["use"]["ext"].as_i64().unwrap_or(0);
        let c = e["impl"].as_i64().unwrap_or(0);
        if a + b + c > 0 {
            out.push_str(&format!("导入 {}（内部 {a} / ext {b}）· 实现 {c}\n", a + b));
        }
    }

    if let Some(total) = v["deps"]["total"].as_i64() {
        out.push_str(&format!(
            "依赖 {total}（直接 {} · 开发 {} · 构建 {}）\n",
            v["deps"]["normal"].as_i64().unwrap_or(0),
            v["deps"]["dev"].as_i64().unwrap_or(0),
            v["deps"]["build"].as_i64().unwrap_or(0)
        ));
    }

    // 空态：不是错误、不是候选，只说明"这块还没数据"以及怎么让它有
    let has_deps = v["deps"]["total"].as_i64().unwrap_or(0) > 0;
    if v["judgment"].is_null()
        && changes == 0
        && syms["names"].as_i64().unwrap_or(0) == 0
        && v["doc"].is_null()
        && !has_deps
    {
        out.push_str("这块没数据（先跑 ingest / docs / parse）\n");
    }

    let ev = v["evidence"].as_array().map(|a| a.len()).unwrap_or(0);
    if v["doc"].is_object()
        || ev > 0
        || syms["names"].as_i64().unwrap_or(0) > 0
        || !v["judgment"].is_null()
    {
        match v["doc"].as_object() {
            Some(d) => out.push_str(&format!(
                "文档 {}   依据 {ev} 条\n",
                d.get("verdict").and_then(|x| x.as_str()).unwrap_or("")
            )),
            None => out.push_str(&format!("文档 无   依据 {ev} 条\n")),
        }
    }

    for e in v["evidence"].as_array().cloned().unwrap_or_default() {
        out.push_str(&format!(
            "  [{}] {} ({})\n",
            e["polarity"].as_str().unwrap_or(""),
            e["premise"].as_str().unwrap_or(""),
            e["instrument"].as_str().unwrap_or("")
        ));
    }
    density_budget(&out, 6, 48)
}

pub fn short_commit(s: &str) -> &str {
    &s[..8.min(s.len())]
}
