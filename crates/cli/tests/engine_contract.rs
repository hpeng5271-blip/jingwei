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
    // §4.4 scope 三件套：symbol → file
    assert_eq!(v["scope"], "file");
    assert_eq!(v["truncated"], false);
    assert!(v["files_considered"].as_u64().is_some(), "{v}");
    assert!(v["next_cursor"].is_null(), "{v}");
    let mut top: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    top.sort();
    assert_eq!(
        top,
        vec!["completeness", "direct", "files_considered", "importers", "next_cursor", "scope", "target", "target_kind", "targets", "tests", "transitive", "truncated", "v"]
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
    // §4.4：scan 必须带 scope 三件套 + residue（dry-run 数字）
    assert_eq!(
        top,
        vec![
            "depth",
            "files_considered",
            "items",
            "next_cursor",
            "parent",
            "residue",
            "scanned_dirs",
            "scope",
            "truncated"
        ]
    );
    assert!(
        matches!(facade["scope"].as_str(), Some("file") | Some("module") | Some("project") | Some("multi")),
        "scope 越界：{}",
        facade["scope"]
    );
    assert_eq!(facade["truncated"], false, "fixture 只有一个小项目，不该截断");
    assert_eq!(facade["residue"]["apply"], false, "残渣报告默认 dry-run");
    for it in facade["items"].as_array().unwrap() {
        let mut keys: Vec<&str> = it.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "age_days",
                "files",
                "files_truncated",
                "has_contract",
                "has_git",
                "has_index",
                "kind",
                "kinds",
                "markers",
                "name",
                "path",
                "touched"
            ]
        );
        // 卡片「最近改动 N 天前」靠这两个字段；缺了就会显示 undefined
        assert!(it["touched"].as_u64().unwrap_or(0) > 0, "touched 要有值：{it}");
        assert!(it.get("contract").is_none(), "不许出现 T6 假字段: {it}");
    }
    let _ = std::fs::remove_dir_all(&d);
}







/// R2 · clean 换芯（数据面）：项目产物按旧页 JUNK/BUILD 枚举（target/node_modules 不列），字段对齐。
#[test]
fn serve_clean_scan_matches_facade_and_flags_regenerable() {
    let d = fixture("cleanscan");
    std::fs::create_dir_all(d.join("target/debug")).unwrap();
    std::fs::write(d.join("target/debug/x.rlib"), vec![0u8; 2048]).unwrap();
    std::fs::create_dir_all(d.join("node_modules/pkg")).unwrap();
    std::fs::write(d.join("node_modules/pkg/i.js"), "x\n").unwrap();
    std::fs::create_dir_all(d.join("dist")).unwrap();
    std::fs::write(d.join("dist/app.bin"), vec![0u8; 4096]).unwrap();
    std::fs::create_dir_all(d.join("build")).unwrap();
    std::fs::write(d.join("build/o.o"), vec![0u8; 8]).unwrap();
    std::fs::create_dir_all(d.join("__pycache__")).unwrap();
    std::fs::write(d.join("__pycache__/x.pyc"), vec![0u8; 100]).unwrap();
    parse_fixture(&d);

    let e = engine::Engine::open(&d).expect("open");
    let facade = e.clean_scan().expect("clean_scan");
    let req = serde_json::json!({"id": 1, "method": "clean_scan"});
    let resp = serve_once(&d, &req);
    assert_eq!(resp["ok"], true, "{resp}");
    let served = strip(resp["result"].clone());
    // 全量 dir_stat 无截断，serve 与 facade 逐值相等。
    assert_eq!(served, facade, "serve 与 facade 必须同值");

    // 顶层键固定
    let mut top: Vec<&str> = served.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    top.sort();
    assert_eq!(top, ["files_considered", "hasProject", "human", "humanRecommended", "items", "next_cursor", "recommendedBytes", "recommendedCount", "scope", "total_bytes", "truncated", "zoneA"]);

    // 项目产物：target/node_modules 旧页 SKIP 不列；dist/build/__pycache__ 列（旧文案/绝对路径/rec）。
    let items = served["items"].as_array().expect("items");
    let project: Vec<&Value> = items
        .iter()
        .filter(|i| matches!(i["label"].as_str(), Some("target" | "node_modules" | "dist" | "build" | "__pycache__")))
        .collect();
    let labels: Vec<&str> = project.iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(!labels.contains(&"target"), "{labels:?}");
    assert!(!labels.contains(&"node_modules"), "{labels:?}");
    assert!(labels.contains(&"dist"), "{labels:?}");
    assert!(labels.contains(&"__pycache__"), "{labels:?}");
    for it in &project {
        let mut ks: Vec<&str> = it.as_object().unwrap().keys().map(|x| x.as_str()).collect();
        ks.sort();
        assert_eq!(ks, ["action", "consequence", "label", "path", "reason", "recommended", "size_bytes", "zone"], "{it}");
        assert_eq!(it["action"], "delete");
        assert_eq!(it["zone"], "a");
    }
    let dist = project.iter().find(|i| i["label"] == "dist").expect("dist item");
    assert_eq!(dist["recommended"], false, "{dist}");
    assert_eq!(dist["reason"], "打包/构建目录；重新构建就会再生成");
    assert_eq!(dist["consequence"], "如果里面有你要交付的安装包，先拷走再删");
    assert!(dist["path"].as_str().unwrap().ends_with("dist"), "{dist}");
    assert!(dist["size_bytes"].as_u64().unwrap() >= 4096, "{dist}");
    let pyc = project.iter().find(|i| i["label"] == "__pycache__").expect("pyc item");
    assert_eq!(pyc["recommended"], true, "{pyc}");
    assert_eq!(pyc["reason"], "Python 导入时自动生成的字节码；下次导入会重建");
    assert!(pyc["path"].as_str().unwrap().ends_with("__pycache__"), "{pyc}");
    assert!(pyc["size_bytes"].as_u64().unwrap() >= 100, "{pyc}");

    assert_eq!(served["truncated"], false);
    assert_eq!(served["scope"], "project");
    let _ = std::fs::remove_dir_all(&d);
}

/// R2 · 全局缓存枚举（hermetic）：显式路径，不碰真实 %LOCALAPPDATA% 等；字段含 command/command_note。
#[test]
fn global_caches_enumerate_hermetic_fields_and_sizes() {
    let d = std::env::temp_dir().join(format!("jw-caches-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let specs = engine::CACHES_MAIN.iter().chain(engine::CACHES_TAIL.iter());
    let mut roots: Vec<(&engine::CacheSpec, std::path::PathBuf)> = Vec::new();
    for (i, spec) in specs.enumerate() {
        let p = d.join(format!("c{i}"));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("f.bin"), vec![0u8; 100 + i]).unwrap();
        roots.push((spec, p));
    }
    let items = engine::enumerate_global_caches(&roots);
    assert_eq!(items.len(), engine::CACHES_MAIN.len() + engine::CACHES_TAIL.len(), "{items:?}");
    for (it, spec) in items.iter().zip(engine::CACHES_MAIN.iter().chain(engine::CACHES_TAIL.iter())) {
        assert_eq!(it["zone"], "a", "{it}");
        assert_eq!(it["action"], spec.action, "{it}");
        assert_eq!(it["label"].as_str().unwrap(), spec.label, "{it}");
        assert!(it["size_bytes"].as_u64().unwrap() > 0, "{it}");
        assert_eq!(it["recommended"].as_bool().unwrap(), spec.recommended, "{it}");
        assert_eq!(it["command"].is_string(), spec.command.is_some(), "{it}");
        assert_eq!(it["command_note"].is_string(), spec.command_note.is_some(), "{it}");
    }
    // 带 command 的项键集固定 10 个（command 在、command_note 无则缺键）
    let pip = items.iter().find(|i| i["label"] == "pip 下载缓存").expect("pip item");
    let mut ks: Vec<&str> = pip.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    ks.sort();
    assert_eq!(ks, ["action", "command", "consequence", "label", "path", "reason", "recommended", "size_bytes", "zone"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// R2 · clean 特殊项（hermetic）：回收站（固定 command）/ 缩略图（文件量）/ 临时文件（7 天以上）。
#[test]
fn clean_scan_special_items_recycle_thumb_temp() {
    let rb = engine::recycle_bin_item();
    assert_eq!(rb["action"], "command");
    assert_eq!(rb["zone"], "a");
    assert_eq!(rb["command"], "Clear-RecycleBin -Force");
    assert_eq!(rb["command_note"], "在 PowerShell 里跑");
    assert_eq!(rb["recommended"], false);
    assert_eq!(rb["size_bytes"], 0);

    let d = std::env::temp_dir().join(format!("jw-thumb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("thumbcache_1.db"), vec![0u8; 512]).unwrap();
    std::fs::write(d.join("iconcache_2.db"), vec![0u8; 256]).unwrap();
    std::fs::write(d.join("readme.txt"), vec![0u8; 999]).unwrap();
    let th = engine::scan_thumbcache(&d).expect("thumb item");
    assert_eq!(th["size_bytes"].as_u64().unwrap(), 768, "{th}");
    assert_eq!(th["label"], "缩略图 / 图标缓存");
    assert_eq!(th["action"], "delete");
    let _ = std::fs::remove_dir_all(&d);

    let t = std::env::temp_dir().join(format!("jw-tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&t);
    std::fs::create_dir_all(&t).unwrap();
    std::fs::write(t.join("fresh.bin"), vec![0u8; 100]).unwrap();
    assert!(engine::scan_temp(&t).is_none(), "刚建的文件不该列");
    let old = t.join("old.bin");
    std::fs::write(&old, vec![0u8; 200]).unwrap();
    let old_t = std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(8 * 24 * 3600)).unwrap();
    std::fs::OpenOptions::new().write(true).open(&old).unwrap().set_times(std::fs::FileTimes::new().set_modified(old_t)).unwrap();
    let tm = engine::scan_temp(&t).expect("temp item with old file");
    assert!(tm["size_bytes"].as_u64().unwrap() >= 200, "{tm}");
    assert_eq!(tm["action"], "delete");
    let _ = std::fs::remove_dir_all(&t);
}

/// R2 · 项目垃圾（hermetic）：junk 后缀 / 日志≥1MB / 空目录 / 未用 venv。
#[test]
fn clean_scan_project_junk_enumerates_files_logs_empty_venv() {
    let d = fixture("junk");
    std::fs::write(d.join("x.bak"), vec![0u8; 10]).unwrap();
    std::fs::write(d.join("y.tmp"), vec![0u8; 20]).unwrap();
    std::fs::write(d.join("big.log"), vec![0u8; 1024 * 1024 + 1]).unwrap();
    std::fs::create_dir_all(d.join("empty_dir")).unwrap();
    std::fs::create_dir_all(d.join("env-bak")).unwrap();
    std::fs::write(d.join("env-bak/pyvenv.cfg"), "home = x\n").unwrap();
    parse_fixture(&d);

    let items = engine::scan_project_junk(&d);
    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(labels.iter().any(|l| l.contains("*.bak · 备份文件")), "{labels:?}");
    assert!(labels.iter().any(|l| l.contains("*.tmp · 临时文件")), "{labels:?}");
    assert!(labels.iter().any(|l| l.contains("日志文件")), "{labels:?}");
    assert!(labels.iter().any(|l| l.contains("空目录")), "{labels:?}");
    assert!(labels.iter().any(|l| l.contains("没在用的虚拟环境：env-bak")), "{labels:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// R2 · machine 硬件换芯：走外部 PowerShell CIM，成功给 hw，失败给 fails（不许两头空）。
#[test]
fn machine_hw_reports_either_data_or_reason() {
    let v = engine::Engine::machine_hw();
    assert_eq!(v["scope"], "project");
    assert_eq!(v["truncated"], false);
    let has_hw = !v["hw"].is_null();
    let fails = v["fails"].as_array().map(|a| a.len()).unwrap_or(0);
    assert!(has_hw || fails > 0, "要么有数据要么有失败原因：{v}");
    if has_hw {
        assert!(v["hw"]["cpu"].is_string() || v["hw"]["cpu"].is_null(), "{v}");
        assert!(v["hw"].get("mem_gb").is_some(), "内存字段要在：{v}");
        assert!(v["hw"].get("disks").is_some(), "磁盘字段要在：{v}");
    }
}

/// R2 · note 换芯：读 `.jingwei/notes/*.md`，frontmatter 五键与旧壳同规则，排序 updated 倒序。
#[test]
fn serve_notes_matches_facade_and_parses_frontmatter() {
    let d = fixture("notes");
    let nd = d.join(".jingwei/notes");
    std::fs::create_dir_all(&nd).unwrap();
    std::fs::write(
        nd.join("a.md"),
        "---\nkind: idea\ntitle: 第一个想法\ncreated: 2026-10-01T00:00:00Z\nupdated: 2026-10-02T00:00:00Z\ntags: [x, y]\n---\n\n正文 A\n",
    )
    .unwrap();
    std::fs::write(
        nd.join("b.md"),
        "---\nkind: plan\ntitle: 后面的计划\ncreated: 2026-10-03T00:00:00Z\nupdated: 2026-10-04T00:00:00Z\ntags: \n---\n\n正文 B\n",
    )
    .unwrap();
    parse_fixture(&d);

    let e = engine::Engine::open(&d).expect("open");
    let facade = e.notes().expect("notes");
    let req = serde_json::json!({"id": 1, "method": "notes"});
    let resp = serve_once(&d, &req);
    assert_eq!(resp["ok"], true, "{resp}");
    let served = strip(resp["result"].clone());
    assert_eq!(served, facade, "serve 与 facade 必须同值");

    let ns = served["notes"].as_array().expect("notes");
    assert_eq!(ns.len(), 2);
    // updated 倒序：b(10-04) 在 a(10-02) 前
    assert_eq!(ns[0]["slug"], "b");
    assert_eq!(ns[0]["title"], "后面的计划");
    assert_eq!(ns[1]["slug"], "a");
    assert_eq!(ns[1]["kind"], "idea");
    let tags: Vec<&str> = ns[1]["tags"].as_array().unwrap().iter().filter_map(|x| x.as_str()).collect();
    assert_eq!(tags, vec!["x", "y"]);
    assert!(ns[1]["body"].as_str().unwrap().contains("正文 A"));
    // 键固定 7 个
    for n in ns {
        let mut ks: Vec<&str> = n.as_object().unwrap().keys().map(|x| x.as_str()).collect();
        ks.sort();
        assert_eq!(ks, vec!["body", "created", "kind", "slug", "tags", "title", "updated"], "{n}");
    }
    assert_eq!(served["scope"], "project");
    assert_eq!(served["files_considered"], 2);
    let _ = std::fs::remove_dir_all(&d);
}

/// R2 · editor 换芯：文件清单要扁平、带 `/`、跳过 target/node_modules，且带 scope 三件套。
#[test]
fn serve_source_files_matches_facade_and_skips_noise() {
    let d = fixture("srcfiles");
    std::fs::create_dir_all(d.join("src/nested")).unwrap();
    std::fs::write(d.join("src/nested/c.rs"), "pub fn c() {}\n").unwrap();
    std::fs::create_dir_all(d.join("target/debug")).unwrap();
    std::fs::write(d.join("target/debug/junk.rs"), "x\n").unwrap();
    std::fs::create_dir_all(d.join("node_modules/x")).unwrap();
    std::fs::write(d.join("node_modules/x/i.js"), "x\n").unwrap();
    parse_fixture(&d);

    let e = engine::Engine::open(&d).expect("open");
    let facade = e.source_files(2000).expect("source_files");
    let req = serde_json::json!({"id": 1, "method": "source_files", "limit": 2000});
    let resp = serve_once(&d, &req);
    assert_eq!(resp["ok"], true, "{resp}");
    let served = strip(resp["result"].clone());
    assert_eq!(served, facade, "serve 与 facade 必须同值");

    let files: Vec<&str> = served["files"].as_array().expect("files").iter().filter_map(|x| x.as_str()).collect();
    assert!(files.contains(&"src/a.rs"), "{files:?}");
    assert!(files.contains(&"src/nested/c.rs"), "递归收子目录：{files:?}");
    assert!(!files.iter().any(|f| f.starts_with("target/")), "target 不该进来：{files:?}");
    assert!(!files.iter().any(|f| f.contains("node_modules")), "node_modules 不该进来：{files:?}");
    assert!(files.iter().all(|f| !f.contains('\\')), "路径统一用 /：{files:?}");

    assert_eq!(served["scope"], "module");
    assert_eq!(served["truncated"], false);
    assert!(served["files_considered"].as_u64().unwrap_or(0) > 0);
    let _ = std::fs::remove_dir_all(&d);
}

/// R2 · run 换芯：四步计划必须显式给"有没有这一步/为什么没有"。
#[test]
fn serve_run_plan_matches_facade_and_marks_missing_steps() {
    let d = fixture("runplan");
    std::fs::write(
        d.join("env.contract.json"),
        r#"{"name":"fx","commands":{"run":"cargo run","build":"cargo build"}}"#,
    )
    .unwrap();
    parse_fixture(&d);

    let e = engine::Engine::open(&d).expect("open");
    let facade = e.run_plan().expect("run_plan");
    let req = serde_json::json!({"id": 1, "method": "run_plan"});
    let resp = serve_once(&d, &req);
    assert_eq!(resp["ok"], true, "{resp}");
    let served = strip(resp["result"].clone());
    assert_eq!(served, facade, "serve 与 facade 必须同值");

    let steps = served["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 4, "四步固定：install/build/test/run");
    let kinds: Vec<&str> = steps.iter().filter_map(|s| s["kind"].as_str()).collect();
    assert_eq!(kinds, vec!["install", "build", "test", "run"]);
    // build/run 有命令；install/test 没有 → available=false 且给理由，不许静默省略
    assert_eq!(steps[1]["available"], true);
    assert_eq!(steps[1]["cmd"], "cargo build");
    assert_eq!(steps[3]["available"], true);
    assert_eq!(steps[3]["cmd"], "cargo run");
    assert_eq!(steps[0]["available"], false);
    assert!(steps[0]["reason"].as_str().unwrap().contains("没有"));
    assert_eq!(steps[2]["available"], false);
    // 每步 4 个键（kind/cmd/available/reason），null 也算键
    for s in steps {
        let mut ks: Vec<&str> = s.as_object().unwrap().keys().map(|x| x.as_str()).collect();
        ks.sort();
        assert_eq!(ks, vec!["available", "cmd", "kind", "reason"], "{s}");
    }
    // §4.4 三件套
    assert_eq!(served["scope"], "project");
    assert_eq!(served["truncated"], false);
    let _ = std::fs::remove_dir_all(&d);
}

/// R2 · inspect 换芯第一刀：5 组已迁字段 + scope 三件套；未迁的三组必须**显式列出**。
#[test]
fn serve_inspect_matches_facade_and_keeps_keys() {
    let d = fixture("inspect");
    // 给 fixture 一份契约：run/install/test 三个字段才有值可比
    std::fs::write(
        d.join("env.contract.json"),
        r#"{"name":"fx","commands":{"run":"cargo run","install":"cargo fetch","test":"cargo test"}}"#,
    )
    .unwrap();
    parse_fixture(&d);

    let e = engine::Engine::open(&d).expect("open");
    let facade = e.inspect().expect("inspect");
    let req = serde_json::json!({"id": 1, "method": "inspect"});
    let resp = serve_once(&d, &req);
    assert_eq!(resp["ok"], true, "{resp}");
    let served = strip(resp["result"].clone());
    assert_eq!(served, facade, "serve 与 facade 必须同值");

    // 5 组已迁：路径 / 类型 / 最近改动 / 怎么跑 / 契约
    assert_eq!(served["root"].as_str().unwrap().is_empty(), false);
    assert_eq!(served["run_command"], "cargo run");
    assert_eq!(served["install_command"], "cargo fetch");
    assert_eq!(served["test_command"], "cargo test");
    assert_eq!(served["contract_name"], "fx");
    assert!(served["recent"]["days"].as_u64().is_some(), "最近改动要给天数：{}", served["recent"]);
    assert_eq!(served["has_test"], true);

    // §4.4 三件套
    assert_eq!(served["scope"], "project");
    assert!(served["files_considered"].as_u64().is_some());
    assert_eq!(served["truncated"], false);
    assert!(served["next_cursor"].is_null());

    // 未迁的三组必须明说，不许静默缺席
    let un: Vec<&str> = served["unavailable"]
        .as_array()
        .expect("unavailable")
        .iter()
        .filter_map(|x| x.as_str())
        .collect();
    assert_eq!(un, vec!["findings", "hygiene", "guide"]);
    for k in ["findings", "hygiene", "guide"] {
        assert!(served.get(k).is_none(), "还没迁的字段不该出现假值：{k}");
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
