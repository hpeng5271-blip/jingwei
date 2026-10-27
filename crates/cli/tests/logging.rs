//! D5：`--debug` 的日志只能走 stderr；stdout 必须仍是纯 JSON；文件模式按天滚并保留 7 天。

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-log-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Run {
    child: Child,
    stdout: BufReader<std::process::ChildStdout>,
    stdin: std::process::ChildStdin,
}

fn start_serve(root: &Path, debug: bool, envs: &[(&str, &str)]) -> Run {
    let jw = env!("CARGO_BIN_EXE_jw");
    let mut cmd = Command::new(jw);
    cmd.args(["serve", root.to_str().unwrap()]);
    if debug {
        cmd.arg("--debug");
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn jw serve");
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    Run { child, stdout, stdin }
}

impl Run {
    fn hello(&mut self) -> String {
        let mut s = String::new();
        self.stdout.read_line(&mut s).unwrap();
        s
    }
    fn send(&mut self, req: &Value) -> String {
        self.stdin.write_all(req.to_string().as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
        let mut s = String::new();
        self.stdout.read_line(&mut s).unwrap();
        s
    }
    /// 杀掉并回收 stderr（must_use：不读 stderr 会看不到断言失败原因）。
    fn finish(mut self) -> String {
        let mut err = String::new();
        if let Some(mut e) = self.child.stderr.take() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = e.read_to_string(&mut err);
        }
        err
    }
}

#[test]
fn debug_keeps_stdout_pure_json_and_logs_events_to_stderr() {
    let root = tmp("stderr");
    let mut r = start_serve(&root, true, &[]);
    let hello: Value = serde_json::from_str(r.hello().trim()).expect("hello 是 JSON");
    assert_eq!(hello["type"], "hello");

    // ① scan：必须有 scan.start / scan.end
    let parent = root.to_string_lossy().to_string();
    let line = r.send(&serde_json::json!({"id": 1, "method": "scan", "parent": parent, "depth": 2}));
    serde_json::from_str::<Value>(line.trim()).expect("scan 的 stdout 必须是纯 JSON");
    // ② env_probe：必须有 env.probe
    let line2 = r.send(&serde_json::json!({"id": 2, "method": "env_probe"}));
    let v2: Value = serde_json::from_str(line2.trim()).expect("env_probe 的 stdout 必须是纯 JSON");
    assert_eq!(v2["ok"], true, "{v2}");

    let err = r.finish();
    for ev in ["serve.call", "scan.start", "scan.end", "env.probe"] {
        assert!(err.contains(ev), "stderr 缺少事件 {ev}：\n{err}");
    }
    // 单行格式：`ts level event k=v`
    assert!(err.contains(" debug "), "缺少级别字段：\n{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn without_debug_stderr_stays_quiet() {
    let root = tmp("quiet");
    let mut r = start_serve(&root, false, &[]);
    let _ = r.hello();
    let line = r.send(&serde_json::json!({"id": 1, "method": "env_probe"}));
    serde_json::from_str::<Value>(line.trim()).expect("stdout 纯 JSON");
    let err = r.finish();
    assert!(
        !err.contains("serve.call"),
        "没开 --debug 不该有调试事件：\n{err}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn file_mode_writes_today_log_and_prunes_old_ones() {
    let root = tmp("file");
    let lap = tmp("lap"); // 假的 LOCALAPPDATA，别写到真实用户目录
    let logs = lap.join("jingwei").join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    // 造一个 9 天前的日志：init 时应被删（保留 7 天）
    std::fs::write(logs.join("jw-20200101.log"), "old").unwrap();

    let lap_s = lap.to_string_lossy().to_string();
    let mut r = start_serve(
        &root,
        true,
        &[("LOCALAPPDATA", &lap_s), ("JW_LOG_FILE", "1")],
    );
    let _ = r.hello();
    let line = r.send(&serde_json::json!({"id": 1, "method": "env_probe"}));
    serde_json::from_str::<Value>(line.trim()).expect("stdout 纯 JSON");
    let _ = r.finish();

    assert!(!logs.join("jw-20200101.log").exists(), "9 天前的日志该被删");
    let today: Vec<PathBuf> = std::fs::read_dir(&logs)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("jw-") && n.ends_with(".log"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(today.len(), 1, "该只剩当天一个日志：{today:?}");
    let body = std::fs::read_to_string(&today[0]).unwrap();
    assert!(body.contains("env.probe"), "日志里该有 env.probe：\n{body}");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&lap);
}
