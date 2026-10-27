//! serve/CLI 与 Engine facade 的同值契约（T6 每搬一个命令都加一条）。
//! **纪律**：数据驱动测试一律自建 fixture，不依赖主仓 DB（它会被 parse/ingest 改）。

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn strip(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.remove("seq");
    }
    v
}

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-contract-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/b.rs"), "pub fn b() {}\n").unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn a() { b(); }\n").unwrap();
    d
}

fn parse_fixture(dir: &Path) {
    let jw = env!("CARGO_BIN_EXE_jw");
    let st = Command::new(jw)
        .args(["parse", dir.to_str().unwrap()])
        .status()
        .expect("jw parse");
    assert!(st.success(), "jw parse failed");
}

fn serve_once(dir: &Path, req: &Value) -> Value {
    let jw = env!("CARGO_BIN_EXE_jw");
    let mut child = Command::new(jw)
        .args(["serve", "."])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jw serve");
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut hello = String::new();
    reader.read_line(&mut hello).unwrap();
    stdin.write_all(req.to_string().as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let _ = child.kill();
    let _ = child.wait();
    serde_json::from_str(line.trim()).expect("serve response json")
}

#[test]
fn serve_tests_for_matches_facade() {
    let root = repo_root();
    // tests_for 需要真实 symbol；仍用主仓作为"有库可开"的样本，
    // 但断言只做 keys-only + 同值（内容随 parse 变是允许的）。
    let target = "symbol:crates/engine/src/lib.rs#card";
    let req = serde_json::json!({"id": 1, "method": "tests_for", "target": target, "depth": 3});
    let resp = serve_once(&root, &req);
    let serve_result = strip(resp["result"].clone());
    let e = engine::Engine::open(&root).expect("open facade");
    let facade = strip(e.tests_for(target, 3).unwrap());
    assert_eq!(serve_result, facade, "serve 与 facade 必须同值");

    let mut top: Vec<&str> = facade.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    top.sort();
    assert_eq!(top, vec!["completeness", "note", "target", "tests", "v"]);
    if let Some(tests) = facade["tests"].as_array() {
        for t in tests {
            let mut keys: Vec<&str> = t.as_object().unwrap().keys().map(|s| s.as_str()).collect();
            keys.sort();
            assert_eq!(keys, vec!["depth", "file", "kind", "line", "name", "via"]);
        }
    }
}

#[test]
fn impact_fixture_is_honest_about_symbol_level() {
    // 自建 fixture：parse 只建结构边（mod/use/impl/import），不产 call 边
    // → impact 的 direct 必空，且 completeness 必须自证 call_symbol_level=unsupported
    let d = fixture("impact");
    parse_fixture(&d);
    let e = engine::Engine::open(&d).expect("open fixture");
    let (v, code) = e.impact("symbol:src/b.rs#b").unwrap();

    assert_eq!(code, 0);
    // 正例：a() 调 b() → direct 必须非空且是符号级（call 边连到 symbol）
    let direct = v["direct"].as_array().unwrap();
    assert!(!direct.is_empty(), "正例：fixture 里 a() 调 b()，direct 不许空: {v}");
    assert!(
        direct.iter().any(|d| d["from"] == "symbol:src/a.rs#a"),
        "direct 里应有 symbol:src/a.rs#a: {v}"
    );
    assert_eq!(v["target_kind"], "symbol");
    let mut top: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    top.sort();
    assert_eq!(
        top,
        vec!["completeness", "direct", "importers", "target", "target_kind", "targets", "tests", "transitive", "v"]
    );

    // serve 同值
    let req = serde_json::json!({"id": 1, "method": "impact", "target": "symbol:src/b.rs#b"});
    let resp = serve_once(&d, &req);
    assert_eq!(strip(resp["result"].clone()), strip(v.clone()), "serve 与 facade 必须同值");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn impact_perf_ms() {
    let d = fixture("impact-perf");
    parse_fixture(&d);
    let jw = env!("CARGO_BIN_EXE_jw");
    let t0 = std::time::Instant::now();
    let out = Command::new(jw)
        .args(["impact", "symbol:src/b.rs#b", "--json"])
        .current_dir(&d)
        .output()
        .expect("run jw impact");
    let ms = t0.elapsed().as_millis();
    assert!(out.status.success());
    eprintln!("impact_ms={} (fixture; doc §18.8)", ms);
    assert!(ms < 5000, "impact too slow: {ms}ms");
    let _ = std::fs::remove_dir_all(&d);
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn parse_only(dir: &Path) {
    let jw = env!("CARGO_BIN_EXE_jw");
    let st = Command::new(jw)
        .args(["parse", dir.to_str().unwrap()])
        .status()
        .expect("jw parse");
    assert!(st.success());
}

fn cite_skip_of(dir: &Path, subject: &str) -> Option<String> {
    let e = engine::Engine::open(dir).expect("open");
    let (v, _code) = e.cite(subject).unwrap();
    v.get("skip").and_then(|x| x.as_str()).map(|s| s.to_string())
}

#[test]
fn cite_no_git_skips() {
    let d = std::env::temp_dir().join(format!("jw-cite-nogit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    parse_only(&d);
    assert_eq!(cite_skip_of(&d, "file:src/a.rs").as_deref(), Some("no_git"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cite_no_commit_skips() {
    let d = std::env::temp_dir().join(format!("jw-cite-nocommit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    assert!(git(&d, &["init"]));
    parse_only(&d);
    assert_eq!(cite_skip_of(&d, "file:src/a.rs").as_deref(), Some("no_commit"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cite_detached_head_skips() {
    let d = std::env::temp_dir().join(format!("jw-cite-detached-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    assert!(git(&d, &["init"]));
    assert!(git(&d, &["add", "."]));
    assert!(git(
        &d,
        &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-m", "init"]
    ));
    let out = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(git(&d, &["checkout", &sha]));
    parse_only(&d);
    assert_eq!(cite_skip_of(&d, "file:src/a.rs").as_deref(), Some("detached"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cite_shallow_clone_skips() {
    // 造本地源仓库 → file:// 浅克隆（不走网络）
    let src = std::env::temp_dir().join(format!("jw-cite-src-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&src);
    std::fs::create_dir_all(src.join("src")).unwrap();
    std::fs::write(src.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    assert!(git(&src, &["init"]));
    assert!(git(&src, &["add", "."]));
    assert!(git(
        &src,
        &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-m", "init"]
    ));
    let d = std::env::temp_dir().join(format!("jw-cite-shallow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let url = format!("file://{}", src.to_string_lossy().replace('\\', "/"));
    let ok = Command::new("git")
        .args(["clone", "--depth", "1", &url])
        .arg(&d)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("SKIP: 无法造浅克隆（git clone --depth 1 file:// 不可用）");
        let _ = std::fs::remove_dir_all(&src);
        return;
    }
    parse_only(&d);
    assert_eq!(cite_skip_of(&d, "file:src/a.rs").as_deref(), Some("shallow_clone"));
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cite_normal_keys_and_serve_parity() {
    // 好仓库 fixture：git init + commit + parse → cite 正常分支
    let d = std::env::temp_dir().join(format!("jw-cite-normal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    assert!(git(&d, &["init"]));
    assert!(git(&d, &["add", "."]));
    assert!(git(&d, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-m", "init"]));
    parse_only(&d);

    let e = engine::Engine::open(&d).expect("open");
    let (v, code) = e.cite("file:src/a.rs").unwrap();
    assert_eq!(code, 0, "正常分支不应 skip: {v}");
    let mut top: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    top.sort();
    assert_eq!(top, vec!["rows", "subject", "v"]);
    for row in v["rows"].as_array().unwrap() {
        let mut keys: Vec<&str> = row.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["commit", "evidence_kind", "file", "file_commit", "line", "verdict"]);
        assert!(row["commit"].as_str().unwrap().len() >= 7);
    }

    // serve 同值
    let req = serde_json::json!({"id": 1, "method": "cite", "subject": "file:src/a.rs"});
    let resp = serve_once(&d, &req);
    assert_eq!(strip(resp["result"].clone()), strip(v.clone()), "serve 与 facade 必须同值");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cite_skip_branch_keys() {
    // skip 分支 keys-only：v/subject/skip
    let d = std::env::temp_dir().join(format!("jw-cite-skipkeys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    parse_only(&d);
    let e = engine::Engine::open(&d).expect("open");
    let (v, _) = e.cite("file:src/a.rs").unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys.sort();
    assert_eq!(keys, vec!["skip", "subject", "v"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cite_perf_ms() {
    let d = std::env::temp_dir().join(format!("jw-cite-perf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    assert!(git(&d, &["init"]));
    assert!(git(&d, &["add", "."]));
    assert!(git(&d, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-m", "init"]));
    parse_only(&d);
    let jw = env!("CARGO_BIN_EXE_jw");
    let t0 = std::time::Instant::now();
    let out = Command::new(jw)
        .args(["cite", "file:src/a.rs", "--json"])
        .current_dir(&d)
        .output()
        .expect("run jw cite");
    let ms = t0.elapsed().as_millis();
    assert!(out.status.success());
    eprintln!("cite_ms={} (fixture; doc §18.8)", ms);
    assert!(ms < 5000, "cite too slow: {ms}ms");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn serve_scan_matches_facade() {
    let d = std::env::temp_dir().join(format!("jw-scan-contract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let parent = d.join("parent");
    std::fs::create_dir_all(parent.join("a/.git")).unwrap();
    std::fs::create_dir_all(parent.join("b/.jingwei")).unwrap();
    std::fs::write(parent.join("a/x.rs"), "").unwrap();
    parse_fixture(&d);

    let e = engine::Engine::open(&d).expect("open");
    let facade = e.scan(parent.to_str().unwrap(), 3, 500).unwrap();
    let req = serde_json::json!({"id": 1, "method": "scan", "parent": parent.to_str().unwrap(), "depth": 3, "limit": 500});
    let resp = serve_once(&d, &req);
    assert_eq!(strip(resp["result"].clone()), strip(facade.clone()), "serve 与 facade 必须同值");

    let mut top: Vec<&str> = facade.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    top.sort();
    assert_eq!(top, vec!["depth", "items", "parent", "scanned_dirs"]);
    for it in facade["items"].as_array().unwrap() {
        let mut keys: Vec<&str> = it.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "files",
                "files_truncated",
                "has_contract",
                "has_git",
                "has_index",
                "kind",
                "kinds",
                "markers",
                "name",
                "path"
            ]
        );
        assert!(it.get("contract").is_none(), "不许出现 T6 假字段: {it}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// I2：`env_probe` 是只读探测，serve 与 facade 同值；键集合固定（缺字段不许用"消失"表达）。
#[test]
fn serve_env_probe_matches_facade_and_keeps_keys() {
    let d = fixture("envprob");
    parse_fixture(&d);

    let e = engine::Engine::open(&d).expect("open");
    let facade = e.env_probe().expect("env_probe");
    let req = serde_json::json!({"id": 1, "method": "env_probe"});
    let resp = serve_once(&d, &req);
    assert_eq!(resp["ok"], true, "{resp}");
    let served = strip(resp["result"].clone());

    // 同值：8 项工具，逐个比 name/status（版本会随时间变，不锁死）
    let ft = facade["tools"].as_array().expect("tools");
    let st = served["tools"].as_array().expect("served tools");
    assert_eq!(st.len(), 8, "工具表固定 8 项：{served}");
    assert_eq!(ft.len(), 8);
    for (a, b) in st.iter().zip(ft.iter()) {
        assert_eq!(a["name"], b["name"]);
        assert_eq!(a["status"], b["status"]);
    }
    assert_eq!(served["schema"], facade["schema"]);

    // keys-only：顶层 3 个
    let mut top: Vec<&str> = served.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    top.sort();
    assert_eq!(top, vec!["generated_at", "schema", "tools"]);
    assert_eq!(served["schema"], 1);

    // 每 tool 6 个键：null 也算键（三态不许用"字段消失"冒充"没有"）
    for t in st {
        let mut ks: Vec<&str> = t.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        ks.sort();
        assert_eq!(
            ks,
            vec!["name", "path", "repro", "source", "status", "version"],
            "{t}"
        );
        let status = t["status"].as_str().unwrap();
        assert!(
            matches!(status, "ok" | "timeout" | "absent"),
            "status 只能是三态：{status}"
        );
        match t["source"].as_str() {
            None => {}
            Some(s) => assert!(
                matches!(s, "pin" | "shim" | "common" | "path"),
                "source 越界：{s}"
            ),
        }
        if status == "absent" {
            assert!(t["repro"].is_string(), "absent 要带 repro：{t}");
        }
        if status == "ok" {
            assert!(t["version"].is_string(), "ok 要带 version：{t}");
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}
