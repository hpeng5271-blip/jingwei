use crate::error::{Error, Result};
use crate::model::{Change, Evidence, Judgment, Source};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

const SCHEMA: &str = include_str!("schema.sql");

/// judgment 的插入语句前缀；两种写入策略（DO NOTHING / DO UPDATE）共用。
const JUDGMENT_INSERT: &str = "INSERT INTO judgment
    (id, subject, verdict, kind, scope, role, expires_at, at, coverage_read, coverage_total)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";

pub struct Store {
    conn: Connection,
    repo: String,
    db_path: PathBuf,
}

/// 一条改动的只读视图。
#[derive(Debug, Clone)]
pub struct ChangeRow {
    pub commit_hash: String,
    pub path: String,
    pub status: String,
    pub author: String,
    pub at: i64,
    pub subject: String,
}

/// 一条依据的只读视图。
#[derive(Debug, Clone)]
pub struct EvidenceRow {
    pub premise: String,
    pub polarity: String,
    pub instrument: String,
}

impl Store {
    /// 打开（或创建）仓库下的 .jingwei/jingwei.db。
    /// 只读打开：**没有 `.jingwei/jingwei.db` 就报错**，绝不建库（只读命令不许有副作用）。
    pub fn open_existing(root: &Path) -> Result<Self> {
        let repo = repo_name(root);
        let dir = root.join(".jingwei");
        let db_path = dir.join("jingwei.db");
        if !db_path.is_file() {
            return Err(crate::Error::Msg(format!(
                "no_db: {} 没有 .jingwei/jingwei.db（先 init / ingest / parse）",
                root.display()
            )));
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;")?;
        Ok(Self { conn, repo, db_path })
    }

    pub fn open(root: &Path) -> Result<Self> {
        let repo = repo_name(root);
        let dir = root.join(".jingwei");
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("jingwei.db");
        let conn = Connection::open(&db_path)?;
        // WAL + NORMAL：大仓库批量写时避免每次 fsync（FULL 下 4ms/语句，实测 k8s 65 行/秒）
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(SCHEMA)?;
        ensure_change_columns(&conn)?;
        ensure_edge_leaf(&conn)?;
        seed_kinds(&conn)?;
        Ok(Self { conn, repo, db_path })
    }

    /// G2 不变量：LSP 结果绝不能进历史链（evidence 的 premise 或 source.instrument 不许出现 lsp）。
    /// 注意：evidence 表没有 instrument 列（instrument 在 source 表），旧 SQL 直接查会报错。
    pub fn count_lsp_evidence(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM evidence e LEFT JOIN source s ON e.source_id = s.id \
             WHERE ifnull(s.instrument, '') LIKE 'lsp%' OR e.premise LIKE 'lsp:%'",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn repo(&self) -> &str {
        &self.repo
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn put_snapshot(&self, commit: &str, created_at: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO snapshot(id, commit_hash, parent_hash, created_at)
             VALUES (?1, ?1, NULL, ?2)",
            params![commit, created_at],
        )?;
        Ok(())
    }

    pub fn put_change(&self, c: &Change) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO change(id, commit_hash, path, status, author, at, subject, old_path, blob)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                c.id,
                c.commit_hash,
                c.path,
                c.status,
                c.author,
                c.at,
                c.subject,
                c.old_path,
                c.blob
            ],
        )?;
        Ok(())
    }

    pub fn put_source(&self, s: &Source) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO source(id, instrument, instrument_version, env, time)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![s.id, s.instrument, s.instrument_version, s.env, s.time],
        )?;
        Ok(())
    }

    /// 写一条判断。同 id 已存在时**什么也不做**。
    ///
    /// 不用 `INSERT OR REPLACE`：REPLACE 是“先删后插”，会触发
    /// `evidence.judgment_id` 的 ON DELETE CASCADE，把 verify/expire 之后的
    /// 证据和结论静默冲掉（.pro/out/e0d_reingest.txt：refuted→held、证据 2→1）。
    pub fn put_judgment(&self, j: &Judgment) -> Result<()> {
        let scope = serde_json::to_string(&j.scope)?;
        self.conn.execute(
            &format!("{JUDGMENT_INSERT} ON CONFLICT(id) DO NOTHING"),
            params![
                j.id,
                j.subject,
                j.verdict.as_str(),
                j.kind,
                scope,
                j.role,
                j.expires_at,
                j.at,
                j.coverage_read,
                j.coverage_total
            ],
        )?;
        Ok(())
    }

    /// 显式覆盖已有判断（不删行、不删证据）。
    ///
    /// 只在明确要改历史时用（数据修复、resolve）；普通采集路径一律用
    /// [`Store::put_judgment`]。
    /// 判断是否已存在且被标过期（重采时要 force 复活，否则 DO NOTHING 会永久哑火）。
    pub fn judgment_expired(&self, id: &str) -> Result<bool> {
        let row: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT expires_at FROM judgment WHERE id = ?1",
                params![id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?;
        Ok(matches!(row, Some(Some(_))))
    }

    pub fn put_judgment_force(&self, j: &Judgment) -> Result<()> {
        let scope = serde_json::to_string(&j.scope)?;
        self.conn.execute(
            &format!(
                "{JUDGMENT_INSERT} ON CONFLICT(id) DO UPDATE SET
                   subject=excluded.subject, verdict=excluded.verdict, kind=excluded.kind,
                   scope=excluded.scope, role=excluded.role, expires_at=excluded.expires_at,
                   at=excluded.at, coverage_read=excluded.coverage_read,
                   coverage_total=excluded.coverage_total"
            ),
            params![
                j.id,
                j.subject,
                j.verdict.as_str(),
                j.kind,
                scope,
                j.role,
                j.expires_at,
                j.at,
                j.coverage_read,
                j.coverage_total
            ],
        )?;
        Ok(())
    }

    pub fn put_evidence(&self, e: &Evidence) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO evidence(id, judgment_id, premise, polarity, source_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![e.id, e.judgment_id, e.premise, e.polarity.as_str(), e.source_id],
        )?;
        Ok(())
    }

    pub fn find_judgment(&self, subject: &str) -> Result<Option<Judgment>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, subject, verdict, kind, scope, role, expires_at, at, coverage_read, coverage_total
                 FROM judgment WHERE subject = ?1 ORDER BY at DESC LIMIT 1",
                params![subject],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<i64>>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, i64>(8)?,
                        r.get::<_, i64>(9)?,
                    ))
                },
            )
            .optional()?;

        Ok(match row {
            Some((id, subject, verdict, kind, scope, role, expires_at, at, cr, ct)) => {
                let scope: crate::model::Scope = serde_json::from_str(&scope)
                    .map_err(|e| Error::Msg(format!("scope 解析失败（库内数据坏）: {subject}: {e}")))?;
                Some(Judgment {
                    id,
                    subject,
                    verdict: match verdict.as_str() {
                        "held" => crate::model::Verdict::Held,
                        "refuted" => crate::model::Verdict::Refuted,
                        _ => crate::model::Verdict::Uncertain,
                    },
                    kind,
                    scope,
                    role,
                    expires_at,
                    at,
                    coverage_read: cr,
                    coverage_total: ct,
                })
            }
            None => None,
        })
    }

    pub fn changes_for(&self, path: &str) -> Result<Vec<ChangeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT commit_hash, path, status, author, at, subject
             FROM change WHERE path = ?1 ORDER BY at DESC",
        )?;
        let rows = stmt
            .query_map(params![path], |r| {
                Ok(ChangeRow {
                    commit_hash: r.get(0)?,
                    path: r.get(1)?,
                    status: r.get(2)?,
                    author: r.get(3)?,
                    at: r.get(4)?,
                    subject: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn evidence_for(&self, judgment_id: &str) -> Result<Vec<EvidenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.premise, e.polarity, s.instrument
             FROM evidence e JOIN source s ON s.id = e.source_id
             WHERE e.judgment_id = ?1 ORDER BY e.polarity",
        )?;
        let rows = stmt
            .query_map(params![judgment_id], |r| {
                Ok(EvidenceRow {
                    premise: r.get(0)?,
                    polarity: r.get(1)?,
                    instrument: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// 记一次写：seq 单调递增 + 最后写入时间 + 影响条数。
    /// 订阅（v1.5）的接口先落到这里：客户端只要比较 seq 就知道库变没变。
    /// 记一次写：seq 单调递增 + 最后写入时间 + 本批 subject（写进有界日志）。
    /// 日志给 `serve watch` 用：客户端拿到 seq > since 的事件，只重查受影响的卡。
    pub fn note_write(&self, subjects: &[String]) -> Result<i64> {
        let seq = self
            .get_meta("write_seq")?
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
            + 1;
        self.set_meta("write_seq", &seq.to_string())?;
        self.set_meta("last_write_at", &now_secs().to_string())?;
        self.set_meta("last_write_subjects", &subjects.len().to_string())?;

        let mut log: Vec<serde_json::Value> = self
            .get_meta("write_log")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        log.push(serde_json::json!({"seq": seq, "subjects": subjects}));
        if log.len() > 50 {
            let drop = log.len() - 50;
            log.drain(0..drop);
        }
        self.set_meta("write_log", &serde_json::to_string(&log)?)?;
        Ok(seq)
    }

    /// seq > since 的写事件（seq, subjects）+ 是否断档（日志被截断，客户端应全量重取）。
    pub fn watch_events(&self, since: i64) -> Result<(Vec<(i64, Vec<String>)>, bool)> {
        let log: Vec<serde_json::Value> = self
            .get_meta("write_log")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let first_seq = log.first().and_then(|e| e["seq"].as_i64()).unwrap_or(0);
        let gap = first_seq > since + 1;
        let mut out = Vec::new();
        for e in log {
            let seq = e["seq"].as_i64().unwrap_or(0);
            if seq <= since {
                continue;
            }
            let subjects = e["subjects"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_str().unwrap_or("").to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            out.push((seq, subjects));
        }
        Ok((out, gap))
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let v = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(v)
    }

    /// 某个 subject 的全部判断（按时间倒序）。
    pub fn judgments_for(&self, subject: &str) -> Result<Vec<Judgment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, subject, verdict, kind, scope, role, expires_at, at, coverage_read, coverage_total
             FROM judgment WHERE subject = ?1 ORDER BY at DESC",
        )?;
        let rows = stmt
            .query_map(params![subject], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        rows.into_iter()
            .map(|(id, subject, verdict, kind, scope, role, expires_at, at, cr, ct)| {
                let scope: crate::model::Scope = serde_json::from_str(&scope)
                    .map_err(|e| Error::Msg(format!("scope 解析失败（库内数据坏）: {subject}: {e}")))?;
                Ok(Judgment {
                    id,
                    subject,
                    verdict: match verdict.as_str() {
                        "held" => crate::model::Verdict::Held,
                        "refuted" => crate::model::Verdict::Refuted,
                        _ => crate::model::Verdict::Uncertain,
                    },
                    kind,
                    scope,
                    role,
                    expires_at,
                    at,
                    coverage_read: cr,
                    coverage_total: ct,
                })
            })
            .collect()
    }

    pub fn count_evidence(&self, judgment_id: &str, polarity: &str) -> Result<i64> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM evidence WHERE judgment_id = ?1 AND polarity = ?2",
            params![judgment_id, polarity],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// 记录一对冲突；同一对不重复写。
    pub fn put_conflict(&self, a: &str, b: &str) -> Result<()> {
        let id = format!("cf:{}:{}", a, b);
        self.conn.execute(
            "INSERT OR IGNORE INTO conflict(id, judgment_a, judgment_b, detected_at, resolved)
             VALUES (?1, ?2, ?3, ?4, 0)",
            params![id, a, b, now_secs()],
        )?;
        Ok(())
    }

    pub fn list_conflicts(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT judgment_a, judgment_b FROM conflict WHERE resolved = 0")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 扫描所有同 subject 的判断，登记冲突。过期判断（expires_at <= now）不参与。
    pub fn scan_conflicts(&self, now: i64) -> Result<usize> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT subject FROM judgment")?;
        let subjects = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut found = 0;
        for s in subjects {
            let all = self.judgments_for(&s)?;
            // 与 ask 同一基准：max(墙钟, 该 subject 最新 at)（见 query.rs 的注释）
            let base = all.iter().map(|j| j.at).max().map_or(now, |m| now.max(m));
            let js: Vec<crate::model::Judgment> = all
                .into_iter()
                .filter(|j| match j.expires_at {
                    Some(e) => e > base,
                    None => true,
                })
                .collect();
            for i in 0..js.len() {
                for k in (i + 1)..js.len() {
                    if crate::compose::is_conflict(&js[i], &js[k]) {
                        self.put_conflict(&js[i].id, &js[k].id)?;
                        found += 1;
                    }
                }
            }
        }
        Ok(found)
    }

    /// 按 subject 子串搜索（最多 20 条）。`%`/`_` 按字面匹配（转义 + ESCAPE）。
    pub fn search_subjects(&self, term: &str) -> Result<Vec<(String, String)>> {
        let escaped = term
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let like = format!("%{escaped}%");
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT subject, verdict FROM judgment
             WHERE subject LIKE ?1 ESCAPE '\\' ORDER BY subject LIMIT 20",
        )?;
        let rows = stmt
            .query_map(params![like], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 某路径下的符号判断数（explain 用）。
    pub fn count_symbols_for_path(&self, path: &str) -> Result<i64> {
        let escaped = path.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let like = format!("symbol:{escaped}#%");
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM judgment WHERE subject LIKE ?1 ESCAPE '\\'",
            params![like],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// 被 git 碰过的不同文件数（项目概览用）。
    pub fn distinct_change_paths(&self) -> Result<i64> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(DISTINCT path) FROM change", [], |r| r.get(0))?;
        Ok(n)
    }

    /// 按扩展名统计「改动过的文件」（只数去重后的路径），给"语言覆盖"用。
    pub fn changed_files_by_ext(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT path FROM change")?;
        let paths: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut m: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for p in paths {
            let ext = p
                .rsplit_once('.')
                .map(|(_, e)| e.to_string())
                .unwrap_or_else(|| "(none)".to_string());
            *m.entry(ext).or_default() += 1;
        }
        let mut v: Vec<(String, i64)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(v)
    }

    /// 改动过但没有 live 符号判断的 .rs 文件：(总数, 前 limit 条)。
    pub fn unparsed_rust(&self, limit: usize) -> Result<(i64, Vec<String>)> {
        let sql_count = "SELECT COUNT(*) FROM (
                SELECT DISTINCT path FROM change WHERE path LIKE '%.rs'
                EXCEPT
                SELECT DISTINCT substr(subject, 8, instr(subject, '#') - 8)
                FROM judgment WHERE kind='symbol' AND expires_at IS NULL AND subject LIKE 'symbol:%#%')";
        let total: i64 = self.conn.query_row(sql_count, [], |r| r.get(0))?;
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT path FROM change WHERE path LIKE '%.rs'
             EXCEPT
             SELECT DISTINCT substr(subject, 8, instr(subject, '#') - 8)
             FROM judgment WHERE kind='symbol' AND expires_at IS NULL AND subject LIKE 'symbol:%#%'
             ORDER BY path LIMIT ?1",
        )?;
        let list: Vec<String> = stmt
            .query_map(params![limit as i64], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok((total, list))
    }

    /// 当前 live 的依赖 subject（deps 重跑时用来过期"已删掉的依赖"）。
    pub fn live_dependency_subjects(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT subject FROM judgment WHERE kind='dependency' AND expires_at IS NULL",
        )?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 有符号判断（live）的文件数（按 subject 里的路径去重），给"语言覆盖"用。
    pub fn symbol_path_count(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(DISTINCT substr(subject, 8, instr(subject, '#') - 8))
             FROM judgment WHERE kind='symbol' AND expires_at IS NULL AND subject LIKE 'symbol:%#%'",
            [],
            |r| r.get(0),
        )?)
    }

    /// 某一语言（scope.env.lang）有符号的文件数。
    pub fn symbol_paths_lang(&self, lang: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(DISTINCT substr(subject, 8, instr(subject, '#') - 8))
             FROM judgment WHERE kind='symbol' AND expires_at IS NULL
               AND json_extract(scope, '$.env.lang') = ?1",
            params![lang],
            |r| r.get(0),
        )?)
    }

    /// 按天统计改动数（升序）：(当天 00:00 的 unix 秒, 次数)。
    pub fn changes_per_day(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT (at / 86400) * 86400 AS d, count(*) FROM change GROUP BY d ORDER BY d")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 改动的时间范围（最早, 最晚）。
    pub fn change_span(&self) -> Result<Option<(i64, i64)>> {
        let row: Option<(Option<i64>, Option<i64>)> = self
            .conn
            .query_row("SELECT min(at), max(at) FROM change", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        Ok(match row {
            Some((Some(a), Some(b))) => Some((a, b)),
            _ => None,
        })
    }

    /// 每天：新增 / 改动 / 删除（change.status 首字母）。
    pub fn change_days(&self) -> Result<Vec<(i64, i64, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT (at / 86400) * 86400 AS d,
                    sum(CASE WHEN status LIKE 'A%' THEN 1 ELSE 0 END),
                    sum(CASE WHEN status NOT LIKE 'A%' AND status NOT LIKE 'D%' THEN 1 ELSE 0 END),
                    sum(CASE WHEN status LIKE 'D%' THEN 1 ELSE 0 END)
             FROM change GROUP BY d ORDER BY d",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 每天：提交数（按 snapshot.created_at）。
    pub fn commit_days(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT (created_at / 86400) * 86400 AS d, count(*) FROM snapshot GROUP BY d ORDER BY d",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 每天：首次出现的文件数（每个 path 取最早的改动时间）。
    pub fn file_first_days(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT (first_at / 86400) * 86400 AS d, count(*) FROM (
                SELECT path, min(at) AS first_at FROM change GROUP BY path
             ) GROUP BY d ORDER BY d",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 每天：decision 判断数。
    pub fn decisions_by_day(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT (at / 86400) * 86400 AS d, count(*) FROM judgment
             WHERE kind = 'decision' GROUP BY d ORDER BY d",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 每天：全部判断数（doc/symbol 的 at 是 mtime，只代表当前批次）。
    pub fn judgments_by_day(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT (at / 86400) * 86400 AS d, count(*) FROM judgment GROUP BY d ORDER BY d",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 按名字找判断（每个 subject 取最新一条），返回 (subject, kind, verdict, 首条依据)。
    pub fn find(&self, term: &str, kind: Option<&str>) -> Result<Vec<(String, String, String, String)>> {
        let escaped = term
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let like = format!("%{escaped}%");
        let mut stmt = self.conn.prepare(
            "SELECT j.subject, j.kind, j.verdict,
                    COALESCE((SELECT e.premise FROM evidence e
                              WHERE e.judgment_id = j.id ORDER BY e.premise LIMIT 1), '')
             FROM judgment j
             WHERE j.subject LIKE ?1 ESCAPE '\\'
               AND (?2 IS NULL OR j.kind = ?2)
               AND (j.expires_at IS NULL OR j.expires_at >= ?3)
               AND j.at = (SELECT max(j2.at) FROM judgment j2 WHERE j2.subject = j.subject)
             ORDER BY j.kind, j.subject LIMIT 60",
        )?;
        let now = now_secs();
        let rows = stmt
            .query_map(params![like, kind, now], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 某文件的按天改动数（升序）：(当天 00:00, 次数)。C1 的 history 态用。
    pub fn change_days_for_path(&self, path: &str) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT (at / 86400) * 86400 AS d, count(*) FROM change
             WHERE path = ?1 GROUP BY d ORDER BY d",
        )?;
        let rows = stmt
            .query_map(params![path], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 某路径下的符号定义（名字, kind, 行号）。
    /// 行号与 kind 从 `def:<path>:<line>:<kind>:<at>` 依据里解析，不另存。
    pub fn symbol_definitions_for_path(&self, path: &str) -> Result<Vec<(String, String, String)>> {
        let escaped = path
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let like = format!("symbol:{escaped}#%");
        let mut stmt = self.conn.prepare(
            "SELECT j.subject, e.premise
             FROM judgment j JOIN evidence e ON e.judgment_id = j.id
             WHERE j.kind = 'symbol' AND j.subject LIKE ?1 ESCAPE '\\'
             ORDER BY j.subject, e.premise",
        )?;
        let rows = stmt
            .query_map(params![like], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows
            .into_iter()
            .map(|(subject, premise)| {
                let name = subject
                    .split_once('#')
                    .map(|(_, n)| n.to_string())
                    .unwrap_or(subject);
                let (line, kind) = def_parts(&premise);
                (name, kind, line)
            })
            .collect())
    }

    /// 可折叠的"同向连续段"：同 subject、同 verdict、连续、条数 ≥ min_run，
    /// 且**段首已过期** ≥ age_secs（未过期的一律不折）。
    pub fn fold_plans(
        &self,
        now: i64,
        min_run: usize,
        age_secs: i64,
    ) -> Result<Vec<(String, String, Option<String>, usize, i64, i64)>> {
        let mut subjects: Vec<String> = self
            .conn
            .prepare("SELECT DISTINCT subject FROM judgment")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        subjects.sort();

        let entity_of = |j: &crate::model::Judgment| j.scope.env.get("entity").cloned();

        let mut out = Vec::new();
        for s in subjects {
            let mut rows = self.judgments_for(&s)?; // at DESC
            rows.reverse(); // ASC

            let mut i = 0usize;
            while i < rows.len() {
                let v = rows[i].verdict;
                let e = entity_of(&rows[i]);
                // 段内必须同 verdict **且同 entity**：路径被不同实体复用时不许合并历史
                let mut k = i + 1;
                while k < rows.len() && rows[k].verdict == v && entity_of(&rows[k]) == e {
                    k += 1;
                }
                let run = &rows[i..k];
                let head = &run[0];
                let old_enough = match head.expires_at {
                    Some(t) => t <= now - age_secs,
                    None => false,
                };
                if run.len() >= min_run && old_enough {
                    out.push((
                        s.clone(),
                        v.as_str().to_string(),
                        e,
                        run.len(),
                        head.at,
                        run[run.len() - 1].at,
                    ));
                }
                i = k;
            }
        }
        Ok(out)
    }

    /// 输入不是完整 subject 时，库里有哪几种可能的完整形式（给用户挑，不替用户猜）。
    pub fn subject_candidates(&self, input: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for s in [format!("file:{input}"), format!("doc:{input}")] {
            if self.find_judgment(&s)?.is_some() {
                out.push(s);
            }
        }
        if input.contains('#') {
            let s = format!("symbol:{input}");
            if self.find_judgment(&s)?.is_some() {
                out.push(s);
            }
        }
        Ok(out)
    }

    /// 某提交的作者 / 标题 / 时间（从 change 表取；没采集到 → None）。
    pub fn commit_meta(&self, sha: &str) -> Result<Option<(String, String, i64)>> {
        let like = format!("{sha}%");
        let row = self
            .conn
            .query_row(
                "SELECT author, subject, at FROM change WHERE commit_hash LIKE ?1 LIMIT 1",
                params![like],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
            )
            .optional()?;
        Ok(row)
    }

    /// 某提交改到的文件（路径, 状态），按路径排序。
    pub fn files_for_commit(&self, sha: &str) -> Result<Vec<(String, String)>> {
        let like = format!("{sha}%");
        let mut stmt = self.conn.prepare(
            "SELECT path, status FROM change WHERE commit_hash LIKE ?1 ORDER BY path",
        )?;
        let rows = stmt
            .query_map(params![like], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 按内容 hash 找已有实体（改名/移动后复用同一 entity）。
    pub fn find_entity_by_content(
        &self,
        content_hash: &str,
    ) -> Result<Option<(String, String)>> {
        let row = self
            .conn
            .query_row(
                "SELECT json_extract(scope, '$.env.entity'),
                        json_extract(scope, '$.env.entity_first_path')
                 FROM judgment
                 WHERE json_extract(scope, '$.env.entity_content') = ?1
                 ORDER BY at LIMIT 1",
                params![content_hash],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    /// 执行一次折叠：同 subject 的 [t0,t1] 段、同 verdict，留首尾，删中间，另立 gist。
    ///
    /// gist 用**新 kind `gist`**（id 里含 kind，不会与段尾撞 id）；只由折叠产生。
    pub fn apply_fold(
        &self,
        subject: &str,
        verdict: &str,
        entity: Option<&str>,
        t0: i64,
        t1: i64,
    ) -> Result<usize> {
        let mut rows = self.judgments_for(subject)?;
        rows.reverse(); // ASC
        let entity_of = |j: &crate::model::Judgment| j.scope.env.get("entity").cloned();
        let run: Vec<&crate::model::Judgment> = rows
            .iter()
            .filter(|j| {
                j.at >= t0
                    && j.at <= t1
                    && j.verdict.as_str() == verdict
                    && entity_of(j).as_deref() == entity
            })
            .collect();
        if run.len() < 3 {
            return Ok(0);
        }
        let last = run[run.len() - 1];

        let mut deleted = 0usize;
        for j in &run[1..run.len() - 1] {
            self.conn
                .execute("DELETE FROM judgment WHERE id = ?1", params![j.id])?;
            deleted += 1;
        }
        if deleted > 0 {
            // 中间段被删后，段首必须能替整段回答：存活到段尾（否则 as-of 会落进空洞）
            self.conn.execute(
                "UPDATE judgment SET expires_at = ?1 WHERE id = ?2",
                params![t1, run[0].id],
            )?;
        }

        let g = crate::ids::judgment_row(
            self.repo(),
            subject,
            "gist",
            last.verdict,
            t1,
            1,
            1,
            last.scope.clone(),
        );
        self.put_judgment(&g)?;

        let sid = crate::ids::source_id(self.repo(), "compact", t1);
        self.put_source(&crate::model::Source {
            id: sid.clone(),
            instrument: "compact".into(),
            instrument_version: None,
            env: format!("repo={}", self.repo()),
            time: t1,
        })?;

        let premise = format!("gist:{}:{}:{}..{}", subject, run.len(), t0, t1);
        self.put_evidence(&crate::model::Evidence {
            id: crate::ids::evidence_id(&g.id, &premise, "supports"),
            judgment_id: g.id.clone(),
            premise,
            polarity: crate::model::Polarity::Supports,
            source_id: sid,
        })?;

        let cur: i64 = self
            .get_meta("folded_judgments")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        self.set_meta("folded_judgments", &(cur + deleted as i64).to_string())?;
        Ok(deleted)
    }

    /// 当前 live 依赖按类型计数（normal/dev/build）。
    pub fn dep_counts(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(json_extract(scope,'$.env.dep_kind'),'?') AS k, COUNT(*)
             FROM judgment WHERE kind='dependency' AND expires_at IS NULL GROUP BY k",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 目录范围统计（范围选择器用）：文件数 / 改动数 / 符号判断 / 文档判断 / 判断总数。
    pub fn range_stats(&self, dir: &str) -> Result<(i64, i64, i64, i64, i64)> {
        // 用前缀区间（>= lo AND < hi）而不是 LIKE '%'：能走 ix_change_path，k8s 上 2198ms → 18ms
        let lo = format!("{}/", dir.trim_end_matches('/'));
        let hi = format!("{}0", dir.trim_end_matches('/'));
        let files: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT path) FROM change WHERE path >= ?1 AND path < ?2",
            params![lo, hi],
            |r| r.get(0),
        )?;
        let changes: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM change WHERE path >= ?1 AND path < ?2",
            params![lo, hi],
            |r| r.get(0),
        )?;
        // 三路子查询各走 (subject, expires_at) 覆盖索引：k8s 上 300ms → 10ms
        let (file_j, docs, symbols): (i64, i64, i64) = self.conn.query_row(
            "SELECT
               (SELECT COUNT(*) FROM judgment WHERE subject >= ?1 AND subject < ?2 AND expires_at IS NULL),
               (SELECT COUNT(*) FROM judgment WHERE subject >= ?3 AND subject < ?4 AND expires_at IS NULL),
               (SELECT COUNT(*) FROM judgment WHERE subject >= ?5 AND subject < ?6 AND expires_at IS NULL)",
            params![
                format!("file:{lo}"), format!("file:{hi}"),
                format!("doc:{lo}"), format!("doc:{hi}"),
                format!("symbol:{lo}"), format!("symbol:{hi}")
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let jobs = file_j + docs + symbols;
        Ok((files, changes, symbols, docs, jobs))
    }


    /// 范围里的符号：(判断数, 定义数=不同名字数)。
    pub fn range_symbols(&self, dir: &str) -> Result<(i64, i64)> {
        let lo = format!("symbol:{}/", dir.trim_end_matches('/'));
        let hi = format!("symbol:{}0", dir.trim_end_matches('/'));
        self.conn
            .query_row(
                "SELECT COUNT(*), COUNT(DISTINCT substr(subject, instr(subject,'#')+1))
                 FROM judgment WHERE kind='symbol' AND expires_at IS NULL
                   AND subject >= ?1 AND subject < ?2",
                params![lo, hi],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(Into::into)
    }

    /// 范围里最近改动的文件（按 at 倒序，最多 n 个）。
    pub fn range_recent_files(&self, dir: &str, n: usize) -> Result<Vec<(String, i64)>> {
        let lo = format!("{}/", dir.trim_end_matches('/'));
        let hi = format!("{}0", dir.trim_end_matches('/'));
        let mut stmt = self.conn.prepare(
            "SELECT path, MAX(at) AS t FROM change WHERE path >= ?1 AND path < ?2
             GROUP BY path ORDER BY t DESC LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![lo, hi, n as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 范围里的"决策"（gist = 折叠后的判断）。
    pub fn range_decisions(&self, dir: &str) -> Result<i64> {
        let lo = format!("{}/", dir.trim_end_matches('/'));
        let hi = format!("{}0", dir.trim_end_matches('/'));
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM judgment WHERE kind='gist' AND expires_at IS NULL
                   AND subject >= ?1 AND subject < ?2",
                params![format!("file:{lo}"), format!("file:{hi}")],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }

    /// 范围里的依据条数。
    pub fn range_evidence(&self, dir: &str) -> Result<i64> {
        let lo = format!("{}/", dir.trim_end_matches('/'));
        let hi = format!("{}0", dir.trim_end_matches('/'));
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM evidence e JOIN judgment j ON j.id = e.judgment_id
                 WHERE (j.subject >= ?1 AND j.subject < ?2)
                    OR (j.subject >= ?3 AND j.subject < ?4)
                    OR (j.subject >= ?5 AND j.subject < ?6)",
                params![
                    format!("file:{lo}"), format!("file:{hi}"),
                    format!("symbol:{lo}"), format!("symbol:{hi}"),
                    format!("doc:{lo}"), format!("doc:{hi}")
                ],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }

    /// 范围卡里的"改动最多的前 N 个文件"（一次 GROUP BY，最多 5 条）。
    pub fn range_top_files(&self, dir: &str, limit: usize) -> Result<Vec<(String, i64)>> {
        let lo = format!("{}/", dir.trim_end_matches('/'));
        let hi = format!("{}0", dir.trim_end_matches('/'));
        let mut stmt = self.conn.prepare(
            "SELECT path, COUNT(*) AS n FROM change
             WHERE path >= ?1 AND path < ?2
             GROUP BY path ORDER BY n DESC, path LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![lo, hi, limit as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 目录范围按天的改动数（范围 + history 用）。
    pub fn change_days_for_prefix(&self, dir: &str) -> Result<Vec<(i64, i64)>> {
        let lo = format!("{}/", dir.trim_end_matches('/'));
        let hi = format!("{}0", dir.trim_end_matches('/'));
        let mut stmt = self.conn.prepare(
            "SELECT (at / 86400) * 86400 AS day, COUNT(*) FROM change
             WHERE path >= ?1 AND path < ?2 GROUP BY day ORDER BY day",
        )?;
        let rows = stmt
            .query_map(params![lo, hi], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 判断用了未注册的 kind（kind_registry 里没有）。
    pub fn unregistered_kinds(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM judgment j
             LEFT JOIN kind_registry k ON k.kind = j.kind
             WHERE k.kind IS NULL",
            [],
            |r| r.get(0),
        )?)
    }

    /// 依据指向了不存在的 source（FK 没开时可能产生）。
    pub fn dangling_sources(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM evidence e
             LEFT JOIN source s ON s.id = e.source_id
             WHERE s.id IS NULL",
            [],
            |r| r.get(0),
        )?)
    }

    /// write_log 自洽：seq 严格递增，且最后一条就是 write_seq。
    pub fn write_log_ok(&self) -> Result<bool> {
        let Some(seq) = self.get_meta("write_seq")?.and_then(|s| s.parse::<i64>().ok()) else {
            return Ok(true);
        };
        let log: Vec<serde_json::Value> = self
            .get_meta("write_log")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if log.is_empty() {
            return Ok(true);
        }
        let mut last = 0i64;
        for e in &log {
            let s = e["seq"].as_i64().unwrap_or(0);
            if s <= last {
                return Ok(false);
            }
            last = s;
        }
        Ok(last == seq)
    }

    /// 批量写的事务边界（大仓库 ingest/parse 必需：逐条 autocommit 会慢 100 倍以上）。
    /// 用 SQL 层 BEGIN/COMMIT，方法保持 `&self`；中途出错则连接关闭时自动回滚。
    pub fn begin(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    pub fn commit(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }


    /// 一个文件里的符号定义：(name, kind, line)。
    pub fn symbol_defs(&self, path: &str) -> Result<Vec<(String, String, i64)>> {
        let subject = format!("symbol:{path}#");
        let mut stmt = self.conn.prepare(
            "SELECT j.subject, j.kind,
                    COALESCE((SELECT e.premise FROM evidence e WHERE e.judgment_id = j.id LIMIT 1), '')
             FROM judgment j
             WHERE j.kind='symbol' AND j.expires_at IS NULL AND j.subject LIKE ?1
             ORDER BY j.subject",
        )?;
        let rows = stmt
            .query_map(params![format!("{subject}%")], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for (subject, kind, premise) in rows {
            let name = subject.split_once('#').map(|(_, n)| n.to_string()).unwrap_or(subject);
            // premise = def:path:line:kind:at
            let line = premise
                .split(':')
                .nth(2)
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            out.push((name, kind, line));
        }
        Ok(out)
    }

    /// 同名符号在多文件里定义：[(name, 文件数)]（只看本文件出现过的名字）。
    pub fn symbol_dups(&self, path: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT substr(subject, instr(subject,'#')+1) AS name, COUNT(DISTINCT subject)
             FROM judgment WHERE kind='symbol' AND expires_at IS NULL AND subject LIKE 'symbol:%#%'
               AND substr(subject, instr(subject,'#')+1) IN (
                 SELECT substr(j2.subject, instr(j2.subject,'#')+1) FROM judgment j2
                 WHERE j2.kind='symbol' AND j2.expires_at IS NULL AND j2.subject LIKE ?1)
             GROUP BY name HAVING COUNT(DISTINCT subject) > 1
             ORDER BY COUNT(DISTINCT subject) DESC, name LIMIT 20",
        )?;
        let rows = stmt
            .query_map(params![format!("symbol:{path}#%")], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 已有符号判断的文件集合（parse 断点续跑用）。
    pub fn symbol_paths(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT substr(subject, 8, instr(subject, '#') - 8)
             FROM judgment WHERE kind='symbol' AND subject LIKE 'symbol:%#%'",
        )?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<std::collections::HashSet<_>, _>>()?;
        Ok(rows)
    }

    /// 已入库的提交（断点续跑用：这些提交可以跳过）。
    pub fn snapshot_hashes(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT commit_hash FROM snapshot")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<std::collections::HashSet<_>, _>>()?;
        Ok(rows)
    }

    /// 写一条关系（关系不是判断，单独一张表；同 src/dst/kind 幂等）。
    pub fn put_edge(&self, src: &str, dst: &str, kind: &str, at: i64, source_id: &str, leaf: Option<&str>, line: Option<i64>) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO edge(src, dst, kind, at, source_id, leaf, line) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![src, dst, kind, at, source_id, leaf, line],
        )?;
        Ok(())
    }


    /// 内部 trait 定义：name → 定义文件集合（可按数量判唯一）。
    pub fn trait_paths(&self) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "SELECT substr(subject, instr(subject,'#')+1), substr(subject, 8, instr(subject,'#')-8)
             FROM judgment WHERE kind='symbol' AND expires_at IS NULL AND subject LIKE 'symbol:%#%'",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut m: std::collections::HashMap<String, Vec<String>> = Default::default();
        for (name, path) in rows {
            let v = m.entry(name).or_default();
            if !v.contains(&path) {
                v.push(path);
            }
        }
        Ok(m)
    }

    /// impl 边：(src, dst, line, leaf)。line 保留 NULL（不许折叠成 0——0 不是行号）。
    pub fn impl_edges(&self) -> Result<Vec<(String, String, Option<i64>, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT src, dst, line, leaf FROM edge WHERE kind='impl'")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }





    /// 从没拿到过 blob 的路径数（未提交/被忽略的文件，`--files` 覆盖不到）。
    pub fn pending_blob_paths(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(DISTINCT path) FROM change WHERE blob IS NULL OR blob = ''",
            [],
            |r| r.get(0),
        )?)
    }




    /// use 边最多的前 n 个文件：[(src, n)]。
    pub fn top_use_files(&self, n: usize) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT src, COUNT(*) c FROM edge WHERE kind='use' GROUP BY src ORDER BY c DESC, src LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![n as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 改动最多的前 n 个文件：[(path, n)]。
    pub fn top_files(&self, n: usize) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, COUNT(*) c FROM change GROUP BY path ORDER BY c DESC, path LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![n as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// timeline 三数：(历史路径, 当前存在, 已删)。当前 = 该路径最后一条状态不是 D。
    pub fn timeline_file_stats(&self) -> Result<(i64, i64, i64)> {
        let historical: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT path) FROM change",
            [],
            |r| r.get(0),
        )?;
        let current: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM (
               SELECT path, status FROM change c
               WHERE at = (SELECT MAX(at) FROM change c2 WHERE c2.path = c.path)
             ) WHERE status NOT LIKE 'D%'",
            [],
            |r| r.get(0),
        )?;
        Ok((historical, current, historical - current))
    }

    /// 每个路径最新的 blob（纯内容键：git blob = 内容寻址的 SHA-1）。
    pub fn latest_blobs(&self) -> Result<std::collections::HashMap<String, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, blob FROM change c
             WHERE blob IS NOT NULL AND blob <> ''
               AND at = (SELECT MAX(at) FROM change c2 WHERE c2.path = c.path AND c2.blob IS NOT NULL)",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()?;
        Ok(rows)
    }

    /// 当前依赖的版本：(name, declared, resolved)；版本缺失给 None。
    pub fn dep_versions(&self) -> Result<Vec<(String, Option<String>, Option<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT subject, scope FROM judgment WHERE kind='dependency' AND expires_at IS NULL",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for (subject, scope) in rows {
            let rest = subject.strip_prefix("dep:").unwrap_or(&subject);
            // cargo 的 dev/build 后缀要剥掉；npm 的 scoped 名（@vue/core）不能按 @ 切
            let name = rest
                .trim_end_matches("@dev")
                .trim_end_matches("@build")
                .to_string();
            let v: serde_json::Value = serde_json::from_str(&scope)
                .map_err(|e| Error::Msg(format!("scope 解析失败（依赖数据坏）: {subject}: {e}")))?;
            let declared = v["env"]["version"].as_str().map(|s| s.to_string());
            let resolved = v["env"]["resolved"].as_str().map(|s| s.to_string());
            out.push((name, declared, resolved));
        }
        Ok(out)
    }

    /// 一个 src 的边摘要：(use 内部, use ext, impl 数)。
    pub fn edge_summary_src(&self, src: &str) -> Result<(i64, i64, i64)> {
        let internal: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE src = ?1 AND kind='use'
               AND dst NOT LIKE 'ext:%' AND dst NOT LIKE '?%'",
            params![src],
            |r| r.get(0),
        )?;
        let ext: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE src = ?1 AND kind='use' AND dst LIKE 'ext:%'",
            params![src],
            |r| r.get(0),
        )?;
        let impl_n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE leaf = ?1 AND kind='impl'",
            params![src],
            |r| r.get(0),
        )?;
        Ok((internal, ext, impl_n))
    }

    /// 一个目录前缀下的边摘要（C4）：src LIKE 'dir/%'。
    pub fn edge_summary_prefix(&self, dir: &str) -> Result<(i64, i64, i64)> {
        let lo = format!("{}/", dir.trim_end_matches('/'));
        let hi = format!("{}0", dir.trim_end_matches('/'));
        let internal: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind='use' AND src >= ?1 AND src < ?2
               AND dst NOT LIKE 'ext:%' AND dst NOT LIKE '?%'",
            params![lo, hi],
            |r| r.get(0),
        )?;
        let ext: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind='use' AND src >= ?1 AND src < ?2 AND dst LIKE 'ext:%'",
            params![lo, hi],
            |r| r.get(0),
        )?;
        let impl_n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind='impl' AND leaf >= ?1 AND leaf < ?2",
            params![lo, hi],
            |r| r.get(0),
        )?;
        Ok((internal, ext, impl_n))
    }

    /// 某文件导入最多的前 n 个目标（`--state deps` 用）。
    pub fn top_use_targets(&self, src: &str, n: usize) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT dst, COUNT(*) c FROM edge WHERE src = ?1 AND kind='use'
             GROUP BY dst ORDER BY c DESC, dst LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![src, n as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// impl 边三分类：(internal, ext, ?)。
    pub fn impl_stats(&self) -> Result<(i64, i64, i64)> {
        let internal: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind='impl' AND dst NOT LIKE 'ext:%' AND dst NOT LIKE '?%'",
            [],
            |r| r.get(0),
        )?;
        let ext: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind='impl' AND dst LIKE 'ext:%'",
            [],
            |r| r.get(0),
        )?;
        let unknown: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind='impl' AND dst LIKE '?%'",
            [],
            |r| r.get(0),
        )?;
        Ok((internal, ext, unknown))
    }

    /// 某文件出发的 use 边的 (leaf,line) 多重集（不变量用）。
    pub fn use_leaves_for(&self, src: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(leaf,''), COALESCE(line,0) FROM edge WHERE src = ?1 AND kind='use' ORDER BY line, leaf",
        )?;
        let rows = stmt
            .query_map(params![src], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 某个文件出发的边：(dst, kind)。
    pub fn edges_for(&self, src: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT dst, kind FROM edge WHERE src = ?1 ORDER BY kind, dst")?;
        let rows = stmt
            .query_map(params![src], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 带位置的多态边：(dst, kind, leaf, line)。loc 的语义由 CLI 按 kind 投影。
    /// test/test_macro 的 src 是合成 subject（`test:<file>#<fn>`）、call 的是 `symbol:<file>#<fn>`，
    /// 所以这两类还要按 dst / src 前缀反查进来。
    pub fn edges_located(
        &self,
        src: &str,
    ) -> Result<Vec<(String, String, String, Option<String>, Option<i64>)>> {
        let escaped = src
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let like = format!("symbol:{escaped}#%");
        let mut stmt = self.conn.prepare(
            "SELECT src, dst, kind, leaf, line FROM (
                 SELECT src, dst, kind, leaf, line FROM edge WHERE src = ?1
                 UNION ALL
                 SELECT src, dst, kind, leaf, line FROM edge
                  WHERE kind IN ('test','test_macro') AND dst = ?1
                 UNION ALL
                 SELECT src, dst, kind, leaf, line FROM edge
                  WHERE kind = 'call' AND src LIKE ?2 ESCAPE '\\'
             ) ORDER BY dst, kind, line",
        )?;
        let rows = stmt
            .query_map(params![src, like], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 符号判断的 subject（`symbol:path#name`），给边解析建 name→file。
    pub fn symbol_subjects(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT subject FROM judgment WHERE kind='symbol' AND expires_at IS NULL",
        )?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 某个 kind 的全部 subject（含过期行）——能力核对要看到"声明过什么"。
    pub fn subjects_by_kind(&self, kind: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT subject FROM judgment WHERE kind = ?1")?;
        let rows = stmt
            .query_map(params![kind], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 某一类边的条数与其未解析条数（dst 以 `?` 开头）。
    pub fn edge_kind_stats(&self, kind: &str) -> Result<(i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind = ?1",
            params![kind],
            |r| r.get(0),
        )?;
        let unresolved: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind = ?1 AND dst LIKE '?%'",
            params![kind],
            |r| r.get(0),
        )?;
        Ok((total, unresolved))
    }

    /// use 绑定表：(src_file, leaf) → dst（已解析的 use 边）。call v1.5 的作用域优先依据。
    pub fn use_bindings(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT src, COALESCE(leaf,''), dst FROM edge
             WHERE kind='use' AND leaf IS NOT NULL AND dst NOT LIKE '?%'",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 全部 call 边：(src, dst, leaf, line)。v1.5 要按绑定表重写（含已解析的）。
    pub fn call_edges_all(&self) -> Result<Vec<(String, String, Option<String>, Option<i64>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT src, dst, leaf, line FROM edge WHERE kind='call'")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 未解析的 call 边：(src, dst, leaf, line)。
    pub fn call_edges_unresolved(&self) -> Result<Vec<(String, String, Option<String>, Option<i64>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT src, dst, leaf, line FROM edge WHERE kind='call' AND dst LIKE '?call %'",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 全库 test 边：(file, name, line, kind)。tests-for 要按符号级取交集。
    pub fn test_edges_all(&self) -> Result<Vec<(String, String, i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT dst, COALESCE(leaf,''), COALESCE(line,0), kind FROM edge
             WHERE kind IN ('test','test_macro') ORDER BY dst, line, leaf",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 范围内 test 聚合：(total, standard, macro)。dir="." 表示全仓。
    pub fn range_tests(&self, dir: &str) -> Result<(i64, i64, i64)> {
        let like = if dir == "." { "%".to_string() } else { format!("{dir}/%") };
        let one = |kind: &str| -> Result<i64> {
            Ok(self.conn.query_row(
                "SELECT COUNT(*) FROM edge WHERE kind = ?1 AND dst LIKE ?2",
                params![kind, like],
                |r| r.get(0),
            )?)
        };
        let std_n = one("test")?;
        let macro_n = one("test_macro")?;
        Ok((std_n + macro_n, std_n, macro_n))
    }

    /// 范围内 call 聚合：(total, resolved, unresolved)。dir="." 表示全仓。
    pub fn range_calls(&self, dir: &str) -> Result<(i64, i64, i64)> {
        let like = if dir == "." { "%".to_string() } else { format!("symbol:{dir}/%") };
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind='call' AND src LIKE ?1",
            params![like],
            |r| r.get(0),
        )?;
        let unresolved: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind='call' AND src LIKE ?1 AND dst LIKE '?%'",
            params![like],
            |r| r.get(0),
        )?;
        Ok((total, total - unresolved, unresolved))
    }

    /// 指向某前缀（目录）内符号的 call 边：(caller_src, callee_dst, line)。range impact 用。
    pub fn callers_into_prefix(&self, prefix: &str) -> Result<Vec<(String, String, i64)>> {
        let like = format!("symbol:{prefix}/%");
        let mut stmt = self.conn.prepare(
            "SELECT src, dst, COALESCE(line,0) FROM edge
             WHERE kind='call' AND dst LIKE ?1
             ORDER BY src, dst, line",
        )?;
        let rows = stmt
            .query_map(params![like], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 指向某前缀内文件的 use 边：(src, leaf, dst)。range impact 的 importers。
    pub fn importers_of_prefix(&self, prefix: &str) -> Result<Vec<(String, String, String)>> {
        let like = format!("{prefix}/%");
        let mut stmt = self.conn.prepare(
            "SELECT src, COALESCE(leaf,''), dst FROM edge
             WHERE kind='use' AND dst LIKE ?1 ORDER BY src, leaf, dst",
        )?;
        let rows = stmt
            .query_map(params![like], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 指向某文件的 use 边（谁 import 了它）：(src, leaf)。
    pub fn use_importers_of(&self, path: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT src, COALESCE(leaf,'') FROM edge WHERE kind='use' AND dst = ?1 ORDER BY src, leaf",
        )?;
        let rows = stmt
            .query_map(params![path], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// incoming calls：谁调用了这个 subject（call 边的 dst）。
    pub fn call_edges_to(&self, dst: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT src, COALESCE(line,0) FROM edge WHERE kind='call' AND dst = ?1 ORDER BY src, line",
        )?;
        let rows = stmt
            .query_map(params![dst], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 某文件的 test 边（测试函数名 + 行号 + 类别）：学习"这个文件有哪些测试"、项目"测试覆盖面"。
    pub fn tests_for_path(&self, path: &str) -> Result<Vec<(String, i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(leaf,''), COALESCE(line,0), kind FROM edge
             WHERE kind IN ('test','test_macro') AND dst = ?1 ORDER BY line",
        )?;
        let rows = stmt
            .query_map(params![path], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 全库 test 边按文件聚合（给定 kind；B 对总量用）。
    pub fn tests_by_file(&self, kind: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT dst, COUNT(*) FROM edge WHERE kind = ?1 GROUP BY dst ORDER BY dst",
        )?;
        let rows = stmt
            .query_map(params![kind], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 全库符号定义索引：(name, path, kind)。跨文件 call 解析要按"名字唯一"判定。
    pub fn symbol_def_index(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT j.subject, e.premise FROM judgment j
             JOIN evidence e ON e.judgment_id = j.id
             WHERE j.kind = 'symbol' AND j.expires_at IS NULL",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(subject, premise)| {
                let rest = subject.strip_prefix("symbol:").unwrap_or(&subject);
                match rest.split_once('#') {
                    Some((path, name)) => {
                        let kind = def_parts(&premise).1;
                        (name.to_string(), path.to_string(), kind)
                    }
                    None => (subject.clone(), String::new(), String::new()),
                }
            })
            .collect())
    }

    /// 全部边的最简三元：(kind, src, leaf)——能力核对用（impl 的 src 是 type:X，文件在 leaf）。
    pub fn edges_brief(&self) -> Result<Vec<(String, String, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, src, leaf FROM edge")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }



    /// 未解析的 TS import 边：(src, dst, line, leaf)。
    pub fn unresolved_ts_edges(&self) -> Result<Vec<(String, String, i64, Option<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT src, dst, COALESCE(line,0), leaf FROM edge WHERE kind='import' AND dst LIKE '?ts:%'",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 未解析的 Python import 边：(src, dst, line)。
    pub fn unresolved_py_edges(&self) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT src, dst, COALESCE(line,0) FROM edge WHERE kind='import' AND dst LIKE '?py:%'",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 未解析的 use 边：(src, dst)。
    pub fn unresolved_use_edges(&self) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT src, dst, COALESCE(line,0) FROM edge WHERE kind='use' AND dst LIKE '?%'",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 删一条具体的 use 边（后置解析重写用）；必须带 line，否则同 dst 的其它行会被一起删掉。
    pub fn delete_use_edge(&self, src: &str, dst: &str, line: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM edge WHERE src = ?1 AND dst = ?2 AND kind = 'use' AND COALESCE(line,0) = ?3",
            params![src, dst, line],
        )?;
        Ok(())
    }

    /// 全量重解析前清空边（边是可重算的派生数据）。
    pub fn clear_edges(&self) -> Result<()> {
        self.conn.execute("DELETE FROM edge", [])?;
        Ok(())
    }

    /// 重解析某个文件前删掉它出发的边（避免旧 dst 残留成重复行）。
    pub fn delete_edges_from(&self, src: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM edge WHERE src = ?1", params![src])?;
        Ok(())
    }

    pub fn edge_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM edge", [], |r| r.get(0))?)
    }

    /// 边解析统计：(internal, ext, unresolved, total)。internal = dir: / 已解析到文件。
    pub fn edge_stats(&self) -> Result<(i64, i64, i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind IN ('use','import')",
            [],
            |r| r.get(0),
        )?;
        let ext: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind IN ('use','import') AND dst LIKE 'ext:%'",
            [],
            |r| r.get(0),
        )?;
        let unresolved: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE kind IN ('use','import') AND dst LIKE '?%'",
            [],
            |r| r.get(0),
        )?;
        let internal = total - ext - unresolved;
        Ok((internal, ext, unresolved, total))
    }

    /// import 边（后置把 ext:<module>/x 重映射成 dir:x 用）。
    pub fn import_edges(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT src, dst FROM edge WHERE kind='import' AND dst LIKE 'ext:%'")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 删一条边（重映射后清旧行）。
    pub fn delete_edge(&self, src: &str, dst: &str, kind: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM edge WHERE src = ?1 AND dst = ?2 AND kind = ?3",
            params![src, dst, kind],
        )?;
        Ok(())
    }

    /// 端点为空的边（不变量：src/dst 都不许为空）。
    pub fn edge_empty(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE src = '' OR dst = ''",
            [],
            |r| r.get(0),
        )?)
    }

    /// 各 kind 的判断数（按数量降序）。
    pub fn kind_counts(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, count(*) FROM judgment GROUP BY kind ORDER BY count(*) DESC")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 更新判断的结论（verify 翻转时用）。
    pub fn update_verdict(&self, id: &str, verdict: crate::model::Verdict) -> Result<()> {
        self.conn.execute(
            "UPDATE judgment SET verdict = ?1 WHERE id = ?2",
            params![verdict.as_str(), id],
        )?;
        Ok(())
    }

    /// 设置过期时间（unix 秒）。
    pub fn set_expiry(&self, id: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE judgment SET expires_at = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
        Ok(())
    }

    /// 某 subject 现有判断的最大 at（没有 → None）。
    ///
    /// verify / resolve 写新判断前必须把 at 推到 `max(at)+1`：judgment_id 只到秒，
    /// 同秒撞 id 时 `put_judgment` 的 DO NOTHING 会**静默丢判断**（E3 v2 抓到的）。
    pub fn max_at(&self, subject: &str) -> Result<Option<i64>> {
        let v: Option<i64> = self.conn.query_row(
            "SELECT max(at) FROM judgment WHERE subject = ?1",
            params![subject],
            |r| r.get(0),
        )?;
        Ok(v)
    }

    /// 把 subject 下除 `keep` 之外的判断都设为 `at` 过期，返回改了几条。
    ///
    /// 已经过期的（expires_at <= at）不动；**at 比自己 at 还新的判断也不动**：
    /// mtime 可能早于老库用 now 写的判断（B4.2），不挡会撞 `expires_at >= at`，
    /// 或把未来的判断误杀。
    pub fn expire_older_than(&self, subject: &str, keep: Option<&str>, at: i64) -> Result<usize> {
        let n = match keep {
            Some(k) => self.conn.execute(
                "UPDATE judgment SET expires_at = ?1
                 WHERE subject = ?2 AND id <> ?3 AND at <= ?1
                   AND (expires_at IS NULL OR expires_at > ?1)",
                params![at, subject, k],
            )?,
            None => self.conn.execute(
                "UPDATE judgment SET expires_at = ?1
                 WHERE subject = ?2 AND at <= ?1
                   AND (expires_at IS NULL OR expires_at > ?1)",
                params![at, subject],
            )?,
        };
        Ok(n)
    }

    /// 已清账的冲突对数（conflicts 命令用）。
    pub fn resolved_conflict_count(&self) -> Result<i64> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM conflict WHERE resolved = 1", [], |r| {
                r.get(0)
            })?;
        Ok(n)
    }

    /// 某个 subject 的未解决冲突（两端都还属于该 subject）。
    pub fn unresolved_conflict_for(&self, subject: &str) -> Result<Option<(String, String)>> {
        let row = self
            .conn
            .query_row(
                "SELECT c.judgment_a, c.judgment_b FROM conflict c
                 JOIN judgment a ON a.id = c.judgment_a
                 JOIN judgment b ON b.id = c.judgment_b
                 WHERE a.subject = ?1 AND b.subject = ?1 AND c.resolved = 0
                 ORDER BY c.detected_at LIMIT 1",
                params![subject],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    /// 把一对冲突标为已解决。
    pub fn mark_conflict_resolved(&self, a: &str, b: &str, at: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE conflict SET resolved = 1, resolved_at = ?1 WHERE id = ?2",
            params![at, format!("cf:{a}:{b}")],
        )?;
        Ok(())
    }

    /// 人工裁决冲突：写新判断 → 过期旧的 → 冲突清账。
    ///
    /// 返回 `None` 表示没有未解决冲突；否则返回 (新判断 id, 过期了几条)。
    /// 机器不许自动调用（`by`/`why` 由命令层校验并记进依据）。
    pub fn resolve_conflict(
        &self,
        repo: &str,
        subject: &str,
        verdict: crate::model::Verdict,
        by: &str,
        why: &str,
        now: i64,
    ) -> Result<Option<(String, usize)>> {
        let Some((a, b)) = self.unresolved_conflict_for(subject)? else {
            return Ok(None);
        };
        let Some(latest) = self.find_judgment(subject)? else {
            return Ok(None);
        };

        // id 只到秒：同秒再写会撞 id 被 DO NOTHING 吞掉 → at 至少比现有最大 at 大 1
        let at = now.max(latest.at + 1);

        let j = crate::ids::judgment_row(
            repo,
            subject,
            &latest.kind,
            verdict,
            at,
            1,
            1,
            latest.scope.clone(),
        );
        self.put_judgment(&j)?;

        let sid = crate::ids::source_id(repo, "resolve", at);
        self.put_source(&crate::model::Source {
            id: sid.clone(),
            instrument: "resolve".into(),
            instrument_version: None,
            env: format!("repo={repo};by={by}"),
            time: at,
        })?;

        let reason: String = why.chars().take(120).collect();
        let premise = format!("resolve:cf:{}:{}:{}", a, b, reason);
        let polarity = if verdict == crate::model::Verdict::Held {
            crate::model::Polarity::Supports
        } else {
            crate::model::Polarity::Contradicts
        };
        self.put_evidence(&crate::model::Evidence {
            id: crate::ids::evidence_id(&j.id, &premise, polarity.as_str()),
            judgment_id: j.id.clone(),
            premise,
            polarity,
            source_id: sid,
        })?;

        // 过期时刻 = 新观测的 at：DDL 要求 expires_at >= 该判断的 at，而 at 可能被
        // 推到现在之后（躲同秒 id 撞车）；代价是那一秒内旧判断仍在，见 B3.1 记录。
        let n = self.expire_older_than(subject, Some(&j.id), at)?;
        self.mark_conflict_resolved(&a, &b, now)?;
        Ok(Some((j.id, n)))
    }

    /// 记录“候选 → 决策”的关系。两边都必须是已存在的判断。
    pub fn put_alternative(&self, child: &str, parent: &str) -> Result<()> {
        if !self.judgment_exists(child)? || !self.judgment_exists(parent)? {
            return Err(Error::Msg("alternative_of 两端必须是已存在的判断".into()));
        }
        if child == parent {
            return Err(Error::Msg("候选不能是它自己".into()));
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO alternative_of(judgment_id, parent_id) VALUES (?1, ?2)",
            params![child, parent],
        )?;
        Ok(())
    }

    /// 某个决策下登记过的候选（按候选自己的 subject 排序：/1、/2、/3 是自然序）。




    /// 前向跟随：这个旧路径被改名到哪（status R*，链式由调用方循环）。
    pub fn moved_to(&self, old_path: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM change WHERE old_path = ?1 AND status LIKE 'R%' ORDER BY at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![old_path], |r| r.get::<_, String>(0))?;
        Ok(rows.next().transpose()?)
    }

    /// 家谱：沿 change.old_path 向上追（≤max_hops），返回路径集（含当前路径）。
    pub fn lineage_paths(&self, path: &str, max_hops: usize) -> Result<Vec<String>> {
        let mut out: Vec<String> = vec![path.to_string()];
        let mut cur = path.to_string();
        for _ in 0..max_hops {
            let old: Option<String> = self
                .conn
                .query_row(
                    "SELECT old_path FROM change WHERE path = ?1 AND old_path IS NOT NULL AND old_path <> ''
                     ORDER BY at DESC LIMIT 1",
                    params![cur],
                    |r| r.get(0),
                )
                .optional()?;
            match old {
                Some(o) if !out.contains(&o) => {
                    out.push(o.clone());
                    cur = o;
                }
                _ => break,
            }
        }
        Ok(out)
    }

    /// 一组路径上的改动总数。
    pub fn changes_for_paths(&self, paths: &[String]) -> Result<i64> {
        if paths.is_empty() {
            return Ok(0);
        }
        let mut total = 0i64;
        for p in paths {
            total += self
                .conn
                .query_row("SELECT COUNT(*) FROM change WHERE path = ?1", params![p], |r| {
                    r.get::<_, i64>(0)
                })?;
        }
        Ok(total)
    }

    /// 一条判断的第一条依据（决策卡的 `record:<id>`）。
    pub fn first_premise(&self, judgment_id: &str) -> Result<String> {
        let p: Option<String> = self
            .conn
            .query_row(
                "SELECT premise FROM evidence WHERE judgment_id = ?1 ORDER BY premise LIMIT 1",
                params![judgment_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(p.unwrap_or_default())
    }

    /// 按 subject 前缀找决策（`decision:<repo>#D-...` 这种）。
    pub fn find_by_subject_prefix(&self, prefix: &str) -> Result<Option<crate::model::Judgment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM judgment WHERE subject LIKE ?1 ORDER BY at DESC LIMIT 1",
        )?;
        let id: Option<String> = stmt
            .query_map(params![format!("{prefix}%")], |r| r.get::<_, String>(0))?
            .next()
            .transpose()?;
        match id {
            Some(id) => {
                let subject: Option<String> = self
                    .conn
                    .query_row(
                        "SELECT subject FROM judgment WHERE id = ?1",
                        params![id],
                        |r| r.get(0),
                    )
                    .optional()?;
                match subject {
                    Some(s) => self.find_judgment(&s),
                    None => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    /// 决策的候选：(subject, verdict, 是否选中)。
    pub fn alternatives_detail(
        &self,
        parent: &str,
    ) -> Result<Vec<(String, String, bool)>> {
        let mut stmt = self.conn.prepare(
            "SELECT j.subject, j.verdict FROM alternative_of a
             JOIN judgment j ON j.id = a.judgment_id
             WHERE a.parent_id = ?1 ORDER BY j.subject",
        )?;
        let rows = stmt
            .query_map(params![parent], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(s, v)| {
                let picked = v == "held";
                (s, v, picked)
            })
            .collect())
    }

    pub fn list_alternatives(&self, parent: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.judgment_id FROM alternative_of a
             JOIN judgment j ON j.id = a.judgment_id
             WHERE a.parent_id = ?1 ORDER BY j.subject",
        )?;
        let rows = stmt
            .query_map(params![parent], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn judgment_exists(&self, id: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM judgment WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn judgment_subject(&self, id: &str) -> Result<Option<String>> {
        let s = self
            .conn
            .query_row(
                "SELECT subject FROM judgment WHERE id = ?1",
                params![id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(s)
    }

    /// 某个 kind 登记在 kind_registry 的哪一层（没登记 → None）。
    pub fn kind_layer(&self, kind: &str) -> Result<Option<String>> {
        let v = self
            .conn
            .query_row(
                "SELECT layer FROM kind_registry WHERE kind = ?1",
                params![kind],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(v)
    }

    /// 数一张表的行数；表名只允许白名单，防注入。
    pub fn count(&self, table: &str) -> Result<i64> {
        let allowed = [
            "judgment",
            "evidence",
            "source",
            "change",
            "snapshot",
            "conflict",
            "kind_registry",
            "alternative_of",
            "edge",
        ];
        if !allowed.contains(&table) {
            return Err(Error::Msg(format!("unknown table: {table}")));
        }
        let n: i64 = self
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        Ok(n)
    }

    /// 孤儿依据：指向不存在的判断。
    ///
    /// 正常路径删不掉（FK 有 ON DELETE CASCADE），但拿 sqlite3 / python 直接改库
    /// 且没开 `PRAGMA foreign_keys=ON` 时会留下（实测 1262 条，见 HANDOFF §7）。
    pub fn orphan_evidence(&self) -> Result<i64> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM evidence e
             LEFT JOIN judgment j ON j.id = e.judgment_id
             WHERE j.id IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// 孤儿候选关系：任一端指向不存在的判断。
    pub fn orphan_alternative(&self) -> Result<i64> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM alternative_of a
             LEFT JOIN judgment j1 ON j1.id = a.judgment_id
             LEFT JOIN judgment j2 ON j2.id = a.parent_id
             WHERE j1.id IS NULL OR j2.id IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// 假覆盖检查：说 held/refuted 却 coverage_read < coverage_total。
    pub fn false_coverage(&self) -> Result<i64> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM judgment
             WHERE verdict IN ('held','refuted') AND coverage_read < coverage_total",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// refuted 门槛检查（DDL 已挡，这里做二次校验，防旧库）。
    pub fn refuted_without_full_coverage(&self) -> Result<i64> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM judgment
             WHERE verdict = 'refuted' AND coverage_read < coverage_total",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    }
}

/// `def:<path>:<line>:<kind>:<at>` → (行号, kind)。解析不了就返回 ("?", "?")。
fn def_parts(premise: &str) -> (String, String) {
    let Some(rest) = premise.strip_prefix("def:") else {
        return ("?".into(), "?".into());
    };
    let mut it = rest.rsplitn(3, ':');
    let _at = it.next();
    let kind = it.next().unwrap_or("?");
    let Some(path_line) = it.next() else {
        return ("?".into(), kind.to_string());
    };
    let line = path_line
        .rsplit_once(':')
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| "?".into());
    (line, kind.to_string())
}

/// 旧库补列（加法迁移）：`change.blob` / `change.old_path`。新库由 schema.sql 直接带。
fn ensure_change_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(change)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (name, ddl) in [
        ("blob", "ALTER TABLE change ADD COLUMN blob TEXT"),
        ("old_path", "ALTER TABLE change ADD COLUMN old_path TEXT"),
    ] {
        if !cols.iter().any(|c| c == name) {
            conn.execute(ddl, [])?;
        }
    }
    Ok(())
}

fn seed_kinds(conn: &Connection) -> Result<()> {
    let kinds = [

        ("change", "code", None::<&str>, "git 改动"),
        ("symbol", "code", None, "源码符号"),
        ("file", "code", None, "文件"),
        ("doc", "code", None, "Markdown 文档"),
        ("activity", "human", None, "人的动作"),
        ("check", "human", None, "检查结果"),
        ("rationale", "human", Some("change"), "为什么"),
        ("impact", "code", Some("change"), "影响面"),
        ("candidate-source", "tx", Some("change"), "根因候选"),
        ("time-series", "code", Some("change"), "趋势"),
        ("mastery", "human", Some("file"), "熟练度"),
        ("decision", "human", Some("rationale"), "人做的决策（人工记录）"),
        ("hypothesis", "human", Some("decision"), "决策里被考虑过的选项"),
        ("gist", "meta", None, "折叠摘要（只由 compact 产生）"),
        ("dependency", "code", Some("file"), "依赖（Cargo.toml/lock 采集）"),
    ];
    let now = now_secs();
    for (k, layer, parent, desc) in kinds {
        conn.execute(
            "INSERT OR IGNORE INTO kind_registry(kind, layer, parent, description, since)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![k, layer, parent, desc, now],
        )?;
    }
    Ok(())
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn repo_name(root: &Path) -> String {
    let abs = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    abs.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo")
        .to_string()
}

/// 老库补 edge.leaf（主键变化只能重建表；边是可重算的派生数据）。
fn ensure_edge_leaf(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(edge)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if cols.iter().any(|c| c == "leaf") && cols.iter().any(|c| c == "line") {
        return Ok(());
    }
    let has_leaf = cols.iter().any(|c| c == "leaf");
    let copy_leaf = if has_leaf { "leaf" } else { "NULL" };
    conn.execute_batch(&format!(
        "ALTER TABLE edge RENAME TO edge_old;
         CREATE TABLE edge (
           src TEXT NOT NULL, dst TEXT NOT NULL, kind TEXT NOT NULL,
           at INTEGER NOT NULL, source_id TEXT, leaf TEXT, line INTEGER,
           PRIMARY KEY (src, dst, kind, leaf, line));
         INSERT OR IGNORE INTO edge(src,dst,kind,at,source_id,leaf,line)
           SELECT src,dst,kind,at,source_id,{copy_leaf},NULL FROM edge_old;
         DROP TABLE edge_old;"
    ))?;
    Ok(())
}

#[cfg(test)]
mod lsp_tests {
    use super::Store;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "jw-lsp-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn count_lsp_evidence_three_legs() {
        let d = tmp("legs");
        let s = Store::open(&d).unwrap();
        // FK off: 允许造悬空 source_id（真实库未强制 FK，可能残留）
        s.conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        s.conn
            .execute(
                "INSERT INTO source(id, instrument, instrument_version, env, time) VALUES ('s1','lsp:ra',NULL,'',0)",
                [],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO evidence(id, judgment_id, premise, polarity, source_id) VALUES ('e1','j1','x','supports','s1')",
                [],
            )
            .unwrap();
        assert!(s.count_lsp_evidence().unwrap() >= 1, "instrument leg");
        s.conn
            .execute(
                "INSERT INTO source(id, instrument, instrument_version, env, time) VALUES ('s2','parse',NULL,'',0)",
                [],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO evidence(id, judgment_id, premise, polarity, source_id) VALUES ('e2','j1','lsp:hover','supports','s2')",
                [],
            )
            .unwrap();
        assert!(s.count_lsp_evidence().unwrap() >= 2, "premise leg");
        s.conn
            .execute(
                "INSERT INTO evidence(id, judgment_id, premise, polarity, source_id) VALUES ('e3','j1','lsp:def','supports','ghost')",
                [],
            )
            .unwrap();
        assert!(s.count_lsp_evidence().unwrap() >= 3, "left join leg");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod silent_tests {
    use super::Store;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "jw-silent-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn dropped_table_is_err_not_zero() {
        let d = tmp("drop-edge");
        let s = Store::open(&d).unwrap();
        s.conn.execute_batch("DROP TABLE edge").unwrap();
        assert!(s.count("edge").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn bad_scope_is_error_not_default() {
        let d = tmp("bad-scope");
        let s = Store::open(&d).unwrap();
        s.conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        s.conn
            .execute(
                "INSERT INTO judgment(id, subject, verdict, kind, scope, at, coverage_read, coverage_total) \
                 VALUES ('j1','file:x','held','file','{bad',0,0,0)",
                [],
            )
            .unwrap();
        assert!(s.find_judgment("file:x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}

