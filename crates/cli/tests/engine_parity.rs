//! T2.4 parity：engine facade 与 `jw serve` 的结果必须同构。
//! stats 剔除易变字段 `seq`/`at`；check/files 逐项比。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn tmp_db() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "jw-parity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    datacore::store::Store::open(&d).unwrap();
    d
}

fn serve_request(root: &PathBuf, req: serde_json::Value) -> serde_json::Value {
    let exe = env!("CARGO_BIN_EXE_jw");
    let mut child = Command::new(exe)
        .arg("serve")
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jw serve");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut hello = String::new();
    reader.read_line(&mut hello).unwrap();
    writeln!(stdin, "{}", req).unwrap();
    stdin.flush().unwrap();
    let mut resp = String::new();
    reader.read_line(&mut resp).unwrap();
    let _ = child.kill();
    serde_json::from_str(&resp).expect("json")
}

#[test]
fn stats_facade_matches_serve() {
    let d = tmp_db();
    let served = serve_request(&d, serde_json::json!({"id": 1, "method": "stats"}));
    let eng = engine::Engine::open(&d).unwrap();
    let fac = eng.stats().unwrap();
    for k in ["source", "files", "symbols", "edges", "rules", "write_seq"] {
        assert_eq!(served["result"][k], fac[k], "field {k}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn check_facade_matches_serve() {
    let d = tmp_db();
    let served = serve_request(&d, serde_json::json!({"id": 1, "method": "check"}));
    let eng = engine::Engine::open(&d).unwrap();
    let fac = eng.check().unwrap();
    assert_eq!(served["result"]["rules"], fac["rules"]);
    assert_eq!(served["result"]["ok"], fac["ok"]);
    let a = served["result"]["items"].as_array().unwrap();
    let b = fac["items"].as_array().unwrap();
    assert_eq!(a.len(), 12);
    assert_eq!(b.len(), 12);
    for (x, y) in a.iter().zip(b.iter()) {
        assert_eq!(x["name"], y["name"]);
        assert_eq!(x["value"], y["value"]);
        assert_eq!(x["ok"], y["ok"]);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn files_facade_matches_serve() {
    let d = tmp_db();
    let served = serve_request(&d, serde_json::json!({"id": 1, "method": "files", "path": "src"}));
    let eng = engine::Engine::open(&d).unwrap();
    let fac = eng.files("src").unwrap();
    assert_eq!(served["result"]["path"], fac["path"]);
    assert_eq!(served["result"]["items"], fac["items"]);
    assert_eq!(fac["items"], serde_json::json!(["a.rs"]));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn files_serve_rejects_escape() {
    let d = tmp_db();
    let served = serve_request(&d, serde_json::json!({"id": 1, "method": "files", "path": ".."}));
    assert_eq!(served["ok"], false);
    assert!(
        served["error"].as_str().unwrap().contains("outside root"),
        "{served}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn read_file_facade_matches_serve() {
    let d = tmp_db();
    let served = serve_request(
        &d,
        serde_json::json!({"id": 1, "method": "read_file", "path": "src/a.rs"}),
    );
    let eng = engine::Engine::open(&d).unwrap();
    let fac = eng.read_file("src/a.rs").unwrap();
    assert_eq!(served["result"]["text"], fac["text"]);
    assert_eq!(served["result"]["truncated"], fac["truncated"]);
    let bad = serve_request(
        &d,
        serde_json::json!({"id": 1, "method": "read_file", "path": ".."}),
    );
    assert_eq!(bad["ok"], false);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn machine_facade_matches_serve() {
    let d = tmp_db();
    let served = serve_request(&d, serde_json::json!({"id": 1, "method": "machine"}));
    let fac = engine::Engine::machine();
    for k in ["source", "os", "arch", "cpus"] {
        assert_eq!(served["result"][k], fac[k], "field {k}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn projects_facade_matches_serve() {
    let d = tmp_db();
    let served = serve_request(&d, serde_json::json!({"id": 1, "method": "projects"}));
    let eng = engine::Engine::open(&d).unwrap();
    let fac = eng.projects().unwrap();
    assert_eq!(served["result"]["parent"], fac["parent"]);
    assert_eq!(served["result"]["root"], fac["root"]);
    assert_eq!(served["result"]["items"], fac["items"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn file_serve_rejects_directory() {
    let d = tmp_db();
    let served = serve_request(&d, serde_json::json!({"id": 1, "method": "file", "path": "src"}));
    assert_eq!(served["ok"], false);
    let _ = std::fs::remove_dir_all(&d);
}

fn card_via_serve(root: &PathBuf, req: serde_json::Value) -> (u8, serde_json::Value) {
    let served = serve_request(root, req);
    let exit = served["result"]["exit"].as_u64().unwrap() as u8;
    (exit, served["result"]["card"].clone())
}

#[test]
fn card_facade_matches_serve_4cases() {
    let d = tmp_db();
    let eng = engine::Engine::open(&d).unwrap();

    let (c1, v1) = card_via_serve(&d, serde_json::json!({"id":1,"method":"card","path":"src/a.rs"}));
    let (c1f, v1f) = eng.card("src/a.rs", false, None).unwrap();
    assert_eq!((c1, v1), (c1f, v1f), "file card");

    let (c2, v2) = card_via_serve(&d, serde_json::json!({"id":1,"method":"card","path":"src"}));
    let (c2f, v2f) = eng.card("src", false, None).unwrap();
    assert_eq!((c2, v2), (c2f, v2f), "range card");

    let (c3, v3) = card_via_serve(&d, serde_json::json!({"id":1,"method":"card","path":"src/a.rs","state":"def"}));
    let (c3f, v3f) = eng.card("src/a.rs", false, Some("def")).unwrap();
    assert_eq!((c3, v3), (c3f, v3f), "state=def (params not dropped)");

    let (c4, v4) = card_via_serve(&d, serde_json::json!({"id":1,"method":"card","path":"src/a.rs","history":true}));
    let (c4f, v4f) = eng.card("src/a.rs", true, None).unwrap();
    assert_eq!((c4, v4), (c4f, v4f), "history=true (params not dropped)");

    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn edges_facade_matches_serve() {
    let d = tmp_db();
    let served = serve_request(&d, serde_json::json!({"id":1,"method":"edges","path":"src/a.rs"}));
    let eng = engine::Engine::open(&d).unwrap();
    let r = eng.edges("src/a.rs").unwrap();
    assert_eq!(served["result"], r);
    // §4.4 scope 三件套：单文件边层
    assert_eq!(r["scope"], "file");
    assert_eq!(r["files_considered"], 1);
    assert_eq!(r["truncated"], false);
    assert!(r["next_cursor"].is_null());
    let _ = std::fs::remove_dir_all(&d);
}

