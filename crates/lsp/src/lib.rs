//! LSP 客户端（D7 第一件）：**只做诊断**，给 `jw lsp-check` 当引擎。
//!
//! 与壳里那张长驻会话的区别：这里是一次性会话（跑完就 shutdown → exit → kill tree），
//! 只求"审一批文件、拿到 errors/warnings"，不做 hover/definition。等 L3 再把两者合并。
//!
//! 硬要求（都是踩过的）：
//! - JSON-RPC 用 **`Content-Length` 头**，不是 NDJSON；
//! - **stderr 必须排空**（后台线程留最后 50 行），只读 stdout 会管道满双死锁；
//! - 顺序固定：`initialize` → 等响应 → `initialized` → `didOpen`；
//! - `positionEncoding` 明确要 `utf-16`（默认值不可假设）；
//! - 退出：`shutdown` → `exit` → `taskkill /T`（Windows 上 RA 会带子进程）；
//! - server 发来的请求（`client/registerCapability` 之类）一律回 `null`，否则它会等在那里。

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Default, Clone)]
pub struct CheckReport {
    pub diagnostics: Vec<Diagnostic>,
    pub errors: usize,
    pub warnings: usize,
    /// 会话没起来/超时的原因（有值就说明"没查到"，不是"没问题"）
    pub note: Option<String>,
}

impl CheckReport {
    fn recount(&mut self) {
        self.errors = self.diagnostics.iter().filter(|d| d.severity == "error").count();
        self.warnings = self.diagnostics.iter().filter(|d| d.severity == "warning").count();
    }
}

/// 默认等待上限：RA 第一次索引整仓可能几十秒。
pub const DEFAULT_TIMEOUT_S: u64 = 90;

pub fn timeout_from_env() -> Duration {
    let s = std::env::var("JW_LSP_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_S);
    Duration::from_secs(s.max(5))
}

/// 从任意路径往上找 `Cargo.toml` 所在目录，作为 RA 的 workspace 根。
pub fn workspace_root(p: &Path) -> Option<PathBuf> {
    let start = if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent()?.to_path_buf()
    };
    // 必须先用绝对路径：相对路径的 ancestors 最后会给出空路径 ""，
    // 而 "" + "Cargo.toml" 会被当成本目录的 Cargo.toml（root 变成空串 → spawn 报 os error 123）。
    let start = std::fs::canonicalize(&start).unwrap_or(start);
    let mut first: Option<PathBuf> = None;
    let mut ws: Option<PathBuf> = None;
    for a in start.ancestors() {
        let c = a.join("Cargo.toml");
        if !c.is_file() {
            continue;
        }
        if first.is_none() {
            first = Some(a.to_path_buf());
        }
        // 真正的 workspace 根：Cargo.toml 里带 [workspace] 的那一层（最外层优先）
        if let Ok(s) = std::fs::read_to_string(&c) {
            if s.contains("[workspace]") {
                ws = Some(a.to_path_buf());
            }
        }
    }
    ws.or(first)
}

/// 找 rust-analyzer：`JW_RA_PATH` → PATH。
pub fn find_ra() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("JW_RA_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let name = if cfg!(windows) {
        "rust-analyzer.exe"
    } else {
        "rust-analyzer"
    };
    let path = std::env::var_os("PATH")?;
    for d in std::env::split_paths(&path) {
        let c = d.join(name);
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

fn file_uri(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = if s.starts_with('/') { s } else { format!("/{s}") };
    let mut out = String::from("file://");
    for ch in s.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            c => out.push(c),
        }
    }
    out
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    next_id: i64,
}

impl Session {
    fn start(root: &Path) -> Result<Session, String> {
        let exe = find_ra().ok_or_else(|| "没找到 rust-analyzer（JW_RA_PATH 或 PATH）".to_string())?;
        let mut cmd = Command::new(&exe);
        cmd.current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("起不来 {}：{e}", exe.display()))?;
        let stdin = child.stdin.take().ok_or("拿不到 stdin")?;
        let mut stdout = child.stdout.take().ok_or("拿不到 stdout")?;
        let stderr = child.stderr.take().ok_or("拿不到 stderr")?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut b = [0u8; 8192];
            loop {
                match stdout.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(b[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        // stderr 必须读：RA 会往里写索引日志，不读就写满管道、双死锁。
        let tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let tw = Arc::clone(&tail);
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(l) = line else { break };
                if let Ok(mut t) = tw.lock() {
                    if t.len() >= 50 {
                        t.pop_front();
                    }
                    t.push_back(l);
                }
            }
        });

        Ok(Session {
            child,
            stdin,
            rx,
            buf: Vec::new(),
            stderr_tail: tail,
            next_id: 1,
        })
    }

    fn tail(&self) -> Vec<String> {
        self.stderr_tail
            .lock()
            .map(|t| t.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn send(&mut self, v: &serde_json::Value) {
        let body = v.to_string();
        let _ = self
            .stdin
            .write_all(format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes());
        let _ = self.stdin.flush();
    }

    fn send_request(&mut self, method: &str, params: serde_json::Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        id
    }

    /// 从缓冲里切出完整的帧（按 `Content-Length`），返回已解析的消息，并保留半帧。
    fn drain_frames(&mut self) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        loop {
            let Some(pos) = find_header_end(&self.buf) else { break };
            let head = String::from_utf8_lossy(&self.buf[..pos]).to_string();
            let mut len = None;
            for line in head.split("\r\n") {
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse::<usize>().ok();
                }
            }
            let Some(len) = len else {
                self.buf.drain(..pos + 4);
                continue;
            };
            if self.buf.len() < pos + 4 + len {
                break;
            }
            let body = self.buf[pos + 4..pos + 4 + len].to_vec();
            self.buf.drain(..pos + 4 + len);
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) {
                out.push(v);
            }
        }
        out
    }

    /// 等一个满足条件的消息；期间把 server 发来的请求一律回 null（不然它会一直等）。
    fn wait_for(
        &mut self,
        deadline: Instant,
        mut want: impl FnMut(&serde_json::Value) -> bool,
    ) -> Option<serde_json::Value> {
        loop {
            for f in self.drain_frames() {
                if f.get("method").is_some() && f.get("id").is_some() {
                    let id = f["id"].clone();
                    self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"result":null}));
                    continue;
                }
                if want(&f) {
                    return Some(f);
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            match self.rx.recv_timeout(Duration::from_millis(120)) {
                Ok(chunk) => self.buf.extend_from_slice(&chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // 进程没了：把剩下的帧处理完再收工
                    for f in self.drain_frames() {
                        if want(&f) {
                            return Some(f);
                        }
                    }
                    return None;
                }
            }
        }
    }

    fn shutdown(mut self) {
        let id = self.send_request("shutdown", serde_json::Value::Null);
        let _ = self.wait_for(Instant::now() + Duration::from_secs(3), |f| {
            f["id"].as_i64() == Some(id)
        });
        self.send(&serde_json::json!({"jsonrpc":"2.0","method":"exit"}));
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .output();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find_header_end(b: &[u8]) -> Option<usize> {
    b.windows(4).position(|w| w == b"\r\n\r\n")
}

fn severity_name(n: i64) -> &'static str {
    match n {
        1 => "error",
        2 => "warning",
        3 => "info",
        _ => "hint",
    }
}

/// 审一批文件。`root` 是 RA 的 workspace 根；`files` 是相对/绝对路径。
pub fn check(root: &Path, files: &[PathBuf], timeout: Duration) -> Result<CheckReport, String> {
    let mut report = CheckReport::default();
    if files.is_empty() {
        report.note = Some("没有要查的文件".into());
        return Ok(report);
    }
    let mut s = Session::start(root)?;
    let deadline = Instant::now() + timeout;

    // initialize → 等响应
    let root_uri = file_uri(&std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()));
    let id = s.send_request(
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "jingwei-lsp-check", "version": env!("CARGO_PKG_VERSION")},
            "rootUri": root_uri,
            "capabilities": {
                "general": {"positionEncodings": ["utf-16"]},
                "textDocument": {"publishDiagnostics": {"relatedInformation": false}, "synchronization": {"didSave": false}}
            },
            "workspaceFolders": [{"uri": root_uri, "name": "root"}]
        }),
    );
    let init = s.wait_for(deadline, |f| f["id"].as_i64() == Some(id));
    let Some(init) = init else {
        let tail = s.tail();
        let mut note = String::from("initialize 没有响应");
        if !tail.is_empty() {
            note.push_str("；stderr 尾部：");
            note.push_str(&tail.join(" | "));
        }
        report.note = Some(note);
        s.shutdown();
        return Ok(report);
    };
    if init.get("error").is_some() {
        report.note = Some(format!("initialize 被拒：{}", init["error"]));
        s.shutdown();
        return Ok(report);
    }

    // initialized → didOpen（顺序固定）
    s.send(&serde_json::json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    let mut wanted: Vec<String> = Vec::new();
    for f in files {
        let abs = std::fs::canonicalize(f).unwrap_or_else(|_| f.to_path_buf());
        let Ok(text) = std::fs::read_to_string(&abs) else {
            continue;
        };
        let uri = file_uri(&abs);
        wanted.push(uri.clone());
        s.send(&serde_json::json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri": uri, "languageId":"rust", "version":1, "text": text}
        }}));
    }

    // 收诊断：每来一批就记下（RA 可能分多次推）；直到所有文件都有过一批或超时
    let mut seen: Vec<String> = Vec::new();
    let rel_of = |uri: &str| -> String {
        uri.strip_prefix("file://")
            .map(|p| {
                let p = p.replace("%20", " ").replace("%23", "#").replace("%25", "%");
                let p = p.strip_prefix('/').unwrap_or(&p).to_string();
                p.replace('/', "\\")
            })
            .unwrap_or_else(|| uri.to_string())
    };
    while Instant::now() < deadline {
        let Some(f) = s.wait_for(
            Instant::now() + Duration::from_millis(1500),
            |f| f["method"] == "textDocument/publishDiagnostics",
        ) else {
            if seen.len() >= wanted.len() {
                break;
            }
            continue;
        };
        let params = &f["params"];
        let uri = params["uri"].as_str().unwrap_or_default().to_string();
        if !seen.contains(&uri) {
            seen.push(uri.clone());
        }
        for d in params["diagnostics"].as_array().cloned().unwrap_or_default() {
            let line = d["range"]["start"]["line"].as_u64().unwrap_or(0) as u32 + 1;
            let col = d["range"]["start"]["character"].as_u64().unwrap_or(0) as u32 + 1;
            report.diagnostics.push(Diagnostic {
                file: rel_of(&uri),
                line,
                col,
                severity: severity_name(d["severity"].as_i64().unwrap_or(3)).to_string(),
                message: d["message"].as_str().unwrap_or("").to_string(),
            });
        }
        if seen.len() >= wanted.len() {
            // 再给一小段时间收残余
            let grace = Instant::now() + Duration::from_millis(800);
            while Instant::now() < grace {
                if let Some(f2) = s.wait_for(Instant::now() + Duration::from_millis(300), |f| {
                    f["method"] == "textDocument/publishDiagnostics"
                }) {
                    if let Some(list) = f2["params"]["diagnostics"].as_array() {
                        for d in list {
                            let line = d["range"]["start"]["line"].as_u64().unwrap_or(0) as u32 + 1;
                            let col = d["range"]["start"]["character"].as_u64().unwrap_or(0) as u32 + 1;
                            report.diagnostics.push(Diagnostic {
                                file: rel_of(f2["params"]["uri"].as_str().unwrap_or_default()),
                                line,
                                col,
                                severity: severity_name(d["severity"].as_i64().unwrap_or(3)).to_string(),
                                message: d["message"].as_str().unwrap_or("").to_string(),
                            });
                        }
                    }
                }
            }
            break;
        }
    }
    if seen.len() < wanted.len() {
        report.note = Some(format!(
            "只收到 {}/{} 个文件的诊断（其余超时）",
            seen.len(),
            wanted.len()
        ));
    }
    // 只留**被检查文件**的诊断：RA 会把整个 workspace 的告警一起推过来，
    // 混进报告会让"这次改的文件有没有 error"失真。
    let wanted_abs: Vec<PathBuf> = files
        .iter()
        .map(|f| std::fs::canonicalize(f).unwrap_or_else(|_| f.clone()))
        .collect();
    report.diagnostics.retain(|d| {
        let dp = PathBuf::from(&d.file);
        wanted_abs
            .iter()
            .any(|w| std::fs::canonicalize(w).map(|c| c == dp).unwrap_or(false) || dp.ends_with(w.file_name().unwrap_or_default()))
    });
    report.recount();
    s.shutdown();
    Ok(report)
}
