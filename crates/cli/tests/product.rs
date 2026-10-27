//! 面向使用者的三个入口：ask 认路径、explain 一屏、project 演进。

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("跑不起 jw");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), s)
}

/// 协议 v1 起 serve 会先发 hello、写库时插 event 帧；回包只认有 id 的行。
fn only_responses(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.contains("\"id\":")).collect()
}

/// 从整段输出里找第一条带 result.events 的帧（跳过 hello/其他）。
fn events_frame(text: &str) -> serde_json::Value {
    for l in text.lines() {
        let v: serde_json::Value = match serde_json::from_str(l) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v["result"]["events"].is_array() {
            return v;
        }
    }
    panic!("没有 events 帧：{text}");
}


/// LSP 真 RA 用例互斥：并行跑时每个测试各起一个 RA，8s 预算会被互相挤爆。
static LSP_RA_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lsp_lock() -> std::sync::MutexGuard<'static, ()> {
    LSP_RA_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 每个真 RA 用例结束都要确认没有 rust-analyzer 残留（只在本机 Windows 上查）。
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
    }
    let _ = child.kill();
}

fn assert_no_ra_residue() {
    #[cfg(windows)]
    {
        for _ in 0..25 {
            let out = std::process::Command::new("tasklist")
                .args(["/FI", "IMAGENAME eq rust-analyzer.exe", "/NH"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            if !out.to_ascii_lowercase().contains("rust-analyzer") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        panic!("rust-analyzer 残留未回收");
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    // 目录名带 pid+序号保证唯一（helper 可能被多个测试复用），但**每轮开跑先清历史残留**：
    // 否则每轮 cargo test 都在 %TEMP% 堆一批新目录（实测累积到 1.1 万个 / 2.8GB）。
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("jw-prod-") && e.path().is_dir() {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    });
    static DIR_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("jw-prod-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("跑不起 git");
    assert!(
        out.status.success(),
        "git {args:?} 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_try(dir: &Path, args: &[&str]) {
    let _ = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("跑不起 git");
}

#[test]
fn timeline_counts_match_change_table() {
    let dir = temp_dir("timeline");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "a").expect("写");
    std::fs::write(dir.join("b.txt"), "b").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    std::fs::write(dir.join("a.txt"), "a2").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c2"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("2 commits"), "{out}");

    let (c, out) = run(&dir, &["timeline", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("已全量扫描"), "全量扫描没标出来：{out}");
    assert!(out.contains("改动    3 次 / 2 文件"), "{out}");
}

#[test]
fn timeline_warns_when_truncated() {
    let dir = temp_dir("timeline-trunc");
    git(&dir, &["init", "-q"]);
    for i in 0..3 {
        std::fs::write(dir.join("a.txt"), format!("v{i}")).expect("写");
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", &format!("c{i}")]);
    }

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "1", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["timeline", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("历史不完整"), "截断没提示：{out}");
    assert!(out.contains("已扫描 1"), "{out}");
}

#[test]
fn find_symbol_shows_line() {
    let dir = temp_dir("find");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src").join("x.rs"), "pub fn hello() {}\n").expect("写");
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["find", "hello", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("src/x.rs:1"), "没带行号：{out}");
    assert!(out.contains("hello"), "{out}");

    // --kind 过滤：hello 不是 doc
    let (c, out) = run(&dir, &["find", "hello", "--kind", "doc", "."]);
    assert_eq!(c, 2, "{out}");
}

#[test]
fn ask_resolves_prefix_from_kind_registry() {
    // subject 前缀由 kind_registry 决定，不是硬编码的五个——新增 kind 不用改解析器
    let dir = temp_dir("prefix");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["ask", "activity:x", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.starts_with("no_data: activity:x"), "前缀被改写：{out}");
}

#[test]
fn serve_card_accepts_directory() {
    // 同一条连接既能取单卡也能取范围卡（都走 card_value）
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve-range");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src").join("a.rs"), "pub fn a() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"{\"id\":1,\"method\":\"card\",\"path\":\"src\"}\n{\"id\":2,\"method\":\"quit\"}\n")
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("\"mode\":\"range\""), "{text}");
    assert!(text.contains("\"files\":1"), "{text}");
    assert!(text.contains("\"v\":1"), "回包缺版本号：{text}");
}

#[test]
fn range_history_shows_days() {
    // 范围 + history：同一目录的改动按天拆开
    let dir = temp_dir("range-hist");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src").join("a.rs"), "pub fn a() {}\n").expect("写");
    std::fs::write(dir.join("src").join("b.rs"), "pub fn b() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["card", "src", "--history", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("范围（history）"), "{out}");
    assert!(out.contains("按天"), "{out}");
    assert!(out.contains("×2"), "同一天两条改动应合成 ×2：{out}");
}

#[test]
fn range_card_aggregates_directory() {
    // 范围选择器：目录 → 聚合卡（数已有的，不画图、不猜结构）
    let dir = temp_dir("range");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src").join("a.rs"), "pub fn a() {}\n").expect("写");
    std::fs::write(dir.join("src").join("b.rs"), "pub fn b() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["card", "src", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("范围"), "{out}");
    assert!(out.contains("文件 2"), "{out}");
    assert!(out.contains("符号 2"), "{out}");
    assert!(out.contains("判断 4 条"), "判断数不对：{out}");

    let (_, out) = run(&dir, &["card", "src", "--json", "."]);
    assert!(out.contains("\"mode\": \"range\""), "{out}");
    assert!(out.contains("\"files\": 2"), "{out}");
    // 前 5 条（一次 GROUP BY）+ 还有多少文件没列出：计数不许抽样
    assert!(out.contains("\"top\": ["), "{out}");
    assert!(out.contains("\"top_more\": 0"), "{out}");
    assert!(out.contains("\"changes\": 2"), "{out}");
    assert!(out.contains("\"judgments\": 4"), "{out}");
    // 普通范围态 days 是空数组（history 态才有内容）——协议文档写的就是这个
    assert!(out.contains("\"days\": []"), "{out}");
}

#[test]
fn merge_commit_expands_and_dedupes() {
    // merge 展开（-m）：冲突解决只出现在 merge commit 里的改动也要采到；两个父的重复行只留一条
    let dir = temp_dir("merge");
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "1").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);

    git(&dir, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.join("a.txt"), "2").expect("写");
    git(&dir, &["commit", "-q", "-a", "-m", "f1"]);

    git(&dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("a.txt"), "3").expect("写");
    git(&dir, &["commit", "-q", "-a", "-m", "m1"]);

    // 冲突 → 解决成 4，再提交 merge
    git_try(&dir, &["merge", "feature"]);
    std::fs::write(dir.join("a.txt"), "4").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "merge-resolve"]);
    let sha = git_head(&dir);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0, "{out}");

    let (c, out) = run(&dir, &["card", &sha, "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("merge-resolve"), "{out}");
    assert!(out.contains("文件 1"), "merge 提交的文件没展开：{out}");
    assert!(out.contains("a.txt"), "{out}");

    // 同一 merge 的两个父视图去重后，a.txt 只多一条改动（c1 A / f1 M / m1 M / merge M = 4）
    let (_, out) = run(&dir, &["card", "file:a.txt", "."]);
    assert!(out.contains("改动 4"), "改动数不对：{out}");
}

#[test]
fn serve_serves_cards_in_one_connection() {
    // UI 的主路径：一条连接取 N 张卡（不再每卡一个进程）
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve-card");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    std::fs::write(dir.join("b.md"), "# b\n").expect("写");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(
                b"{\"id\":1,\"method\":\"card\",\"path\":\"a.md\"}\n{\"id\":2,\"method\":\"card\",\"path\":\"b.md\"}\n{\"id\":3,\"method\":\"quit\"}\n",
            )
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines = only_responses(&text);
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].contains("\"id\":1") && lines[0].contains("doc:a.md"), "{text}");
    assert!(lines[1].contains("\"id\":2") && lines[1].contains("doc:b.md"), "{text}");
    assert!(lines[0].contains("\"exit\":0"), "{text}");
}

#[test]
fn serve_answers_ndjson() {
    // 长驻服务：一行一请求，ping/stat 有回包，quit 结束；不推送、只报 write_seq
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(
                b"{\"id\":1,\"method\":\"ping\"}\n{\"id\":2,\"method\":\"stat\"}\n{\"id\":3,\"method\":\"quit\"}\n",
            )
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let all: Vec<&str> = text.lines().collect();
    // 协议 v1：启动先发一条无 id 的 hello 帧；回包只数有 id 的帧
    assert!(all.first().map(|l| l.contains("\"type\":\"hello\"")).unwrap_or(false), "{text}");
    let lines: Vec<&str> = all.iter().copied().filter(|l| l.contains("\"id\":")).collect();
    assert_eq!(lines.len(), 2, "应只有 2 条回包：{text}");
    assert!(lines[0].contains("\"pong\":true"), "{text}");
    assert!(lines[0].contains("\"write_seq\":1"), "{text}");
    assert!(lines[1].contains("\"judgment\":1"), "{text}");
}

#[test]
fn serve_watch_long_polls() {
    // 长轮询：seq 已涨过 since 立刻回 changed:true；超过 since 等不到就超时回 changed:false
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve-watch");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(
                b"{\"id\":1,\"method\":\"watch\",\"since\":0,\"timeout_ms\":200}\n{\"id\":2,\"method\":\"watch\",\"since\":999,\"timeout_ms\":150}\n{\"id\":3,\"method\":\"quit\"}\n",
            )
            .expect("写请求");
    }
    let started = std::time::Instant::now();
    let out = child.wait_with_output().expect("等 serve");
    let waited = started.elapsed();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines = only_responses(&text);
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].contains("\"changed\":true"), "{text}");
    assert!(lines[0].contains("\"write_seq\":1"), "{text}");
    // 事件要带"哪些 subject 变了"，客户端只重查受影响的卡
    assert!(lines[0].contains("\"doc:a.md\""), "事件缺 subject：{text}");
    assert!(lines[0].contains("\"gap\":false"), "不该断档：{text}");
    assert!(lines[1].contains("\"changed\":false"), "{text}");
    assert!(waited.as_millis() >= 150, "第二条没等超时：{waited:?}");
}

#[test]
fn build_facts_show_up_in_all_views() {
    // 口径一致：deps 落的判定，时间线与 stat 都要显示
    let dir = temp_dir("deps-views");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"v\"\n\n[dependencies]\nanyhow = \"1\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("Cargo.lock"),
        "[[package]]\nname = \"anyhow\"\nversion = \"1.0.86\"\n",
    )
    .expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["deps", "."]);
    assert_eq!(c, 0);

    let (_, out) = run(&dir, &["timeline", "."]);
    assert!(out.contains("构建"), "时间线没有构建行：{out}");
    assert!(out.contains("依赖 1"), "{out}");

    let (_, out) = run(&dir, &["stat", "."]);
    assert!(out.contains("依赖(当前)"), "{out}");
    assert!(out.contains("normal 1"), "{out}");
}

#[test]
fn dep_card_and_manifest_summary() {
    let dir = temp_dir("dep-card");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"d\"\n\n[dependencies]\nanyhow = \"1\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("Cargo.lock"),
        "[[package]]\nname = \"anyhow\"\nversion = \"1.0.86\"\n",
    )
    .expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["deps", "."]);
    assert_eq!(c, 0);

    // 依赖卡：版本 + 类型，不套文件卡那套字段
    let (_, out) = run(&dir, &["card", "dep:anyhow", "."]);
    assert!(out.contains("anyhow  1.0.86"), "{out}");
    assert!(out.contains("类型 normal"), "{out}");
    assert!(out.lines().count() <= 6, "{out}");

    // Cargo.toml 的卡带一句依赖概况
    let (_, out) = run(&dir, &["card", "Cargo.toml", "."]);
    assert!(out.contains("依赖 1（直接 1"), "{out}");
}

#[test]
fn deps_reads_manifest_and_lock() {
    // 采·构建：Cargo.toml 直接依赖 + Cargo.lock 版本 → dep:<name> 判断
    let dir = temp_dir("deps");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\"]\n\n[workspace.dependencies]\nanyhow = \"1\"\nserde = { version = \"1\", features = [\"derive\"] }\n",
    )
    .expect("写 toml");
    std::fs::create_dir_all(dir.join("a")).expect("建 a");
    std::fs::write(
        dir.join("a").join("Cargo.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = { workspace = true }\n\n[dev-dependencies]\ntempfile = \"3\"\n",
    )
    .expect("写成员");
    std::fs::write(
        dir.join("Cargo.lock"),
        "[[package]]\nname = \"anyhow\"\nversion = \"1.0.86\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.210\"\n",
    )
    .expect("写 lock");

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["deps", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("deps 2"), "依赖数不对：{out}");
    assert!(out.contains("开发 1"), "{out}");
    assert!(out.contains("工作区成员 1"), "{out}");
    // workspace.dependencies 是候选版本声明，不是项目依赖：只计数，不入 deps
    assert!(out.contains("workspace 声明 2 条"), "{out}");

    let (c, out) = run(&dir, &["ask", "dep:serde", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.starts_with("held: dep:serde"), "{out}");
    assert!(out.contains("1.0.210"), "lock 版本没进判断：{out}");

    let (c, out) = run(&dir, &["ask", "dep:anyhow", "."]);
    assert_eq!(c, 2, "只在 workspace.dependencies 里声明的条目不该成依赖：{out}");
}

fn write_seq(dir: &Path) -> i64 {
    let (_, out) = run(dir, &["stat", "."]);
    let line = out
        .lines()
        .find(|l| l.contains("write_seq"))
        .unwrap_or_default();
    let rest = line.trim_start_matches("write_seq").trim();
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().unwrap_or(-1)
}

#[test]
fn card_says_unparsed_for_non_rust() {
    // 诚实的三态：能力缺失 ≠ 没符号；有能力但没解析 ≠ 有符号
    let dir = temp_dir("card-lang");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.py"), "def f():\n    pass\n").expect("写");
    std::fs::write(dir.join("b.js"), "export const x = 1;\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);

    // .py 有符号能力（只是还没 parse）→ no_symbols，不许说"rust only"
    let (c, out) = run(&dir, &["card", "a.py", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(!out.contains("rust only"), "旧文案不许再出现：{out}");
    assert!(out.contains("符号 0（文件里没有可映射的定义）"), "{out}");
    let (c, out) = run(&dir, &["card", "a.py", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["symbols"]["status"].as_str(), Some("no_symbols"), "{out}");

    // .js 没有符号能力 → no_capability（能力缺失不是"文件没符号"）
    let (c, out) = run(&dir, &["card", "b.js", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["symbols"]["status"].as_str(), Some("no_capability"), "{out}");
    let (_, out) = run(&dir, &["card", "b.js", "."]);
    assert!(out.contains("无能力（javascript 未支持符号解析）"), "{out}");

    // ask：file: 存在性仍是 held，但必须加能力说明；符号类问题落 uncertain(no_capability)
    let (c, out) = run(&dir, &["ask", "file:b.js", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("能力"), "file: 要加能力说明：{out}");
    assert!(out.contains("javascript 未支持符号解析"), "{out}");
    let (c, out) = run(&dir, &["ask", "symbol:b.js#x", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("no_capability"), "{out}");
    let (_, out) = run(&dir, &["ask", "symbol:b.js#x", "--json", "."]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["reason"].as_str(), Some("no_capability"), "{out}");

    // timeline 也要报语言覆盖（rust 文件数 + 实际有符号的文件数）
    let (_, out) = run(&dir, &["timeline", "."]);
    assert!(out.contains("语言"), "{out}");
    assert!(out.contains("有符号 0"), "{out}");
}

#[test]
fn ingest_warns_on_shallow_clone() {
    // 浅克隆必须当场说：否则 timeline 的"已全量扫描"会骗人
    let origin = temp_dir("shallow-origin");
    git(&origin, &["init", "-q"]);
    std::fs::write(origin.join("a.txt"), "1").expect("写");
    git(&origin, &["add", "-A"]);
    git(&origin, &["commit", "-q", "-m", "c1"]);
    std::fs::write(origin.join("a.txt"), "2").expect("写");
    git(&origin, &["commit", "-q", "-a", "-m", "c2"]);

    let base = temp_dir("shallow");
    let dst = base.join("repo");
    let url = format!("file://{}", origin.display());
    let out = Command::new("git")
        .args(["clone", "-q", "--depth", "1", &url])
        .arg(&dst)
        .output()
        .expect("git clone");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let (c, _) = run(&dst, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dst, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("浅克隆"), "浅克隆没被提示：{out}");
}

#[test]
fn coverage_lists_unparsed_and_macro_counts() {
    // 覆盖：macro_rules! 也算符号（补映射），只有 use 的文件会被列进"未解析"
    let dir = temp_dir("coverage");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").expect("写");
    std::fs::write(dir.join("src/b.rs"), "macro_rules! b { () => {} }\n").expect("写");
    std::fs::write(dir.join("src/c.rs"), "use std::fmt;\n").expect("写");
    std::fs::write(dir.join("src/d.rs"), "macro_call!(a, b);\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("覆盖：rust 4 文件 / 2 有符号"), "{out}");

    let (_, out) = run(&dir, &["coverage", "."]);
    assert!(out.contains("2 未解析"), "{out}");
    assert!(out.contains("src/c.rs  —— 只有 use/mod/impl"), "c.rs 原因不对：{out}");
    assert!(
        out.contains("src/d.rs  —— 有宏调用 1 处，token 树未展开"),
        "d.rs 原因不对：{out}"
    );
    assert!(!out.contains("src/b.rs"), "macro_rules 不该算未解析：{out}");

    // 宏进了符号表
    let (_, out) = run(&dir, &["find", "b", "--kind", "symbol", "."]);
    assert!(out.contains("src/b.rs"), "{out}");

    // JSON 契约：字段与 reason 枚举都钉住
    let (_, out) = run(&dir, &["coverage", "--json", "."]);
    for key in ["\"v\": 1", "\"lang\": \"rust\"", "\"total\"", "\"parsed\"", "\"unparsed\"", "\"by_ext\""] {
        assert!(out.contains(key), "缺键 {key}：{out}");
    }
    assert!(out.contains("\"reason\": \"no_mapped_item\""), "{out}");
    assert!(out.contains("\"reason\": \"macro_unexpanded\""), "{out}");
    assert!(out.contains("\"macro_calls\": 1"), "{out}");
}

#[test]
fn serve_edge_stats_matches_cli() {
    // serve 的 edge_stats 三行数字必须与 `jw edges --stats` 完全一致
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve-edge-stats");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("main.go"), "package main\n\nimport \"fmt\"\n\nfunc Hello() {}\n").expect("写");
    std::fs::write(
        dir.join("lib.rs"),
        "mod a;\nuse std::fmt;\npub fn f() {}\n",
    )
    .expect("写");
    std::fs::write(dir.join("a.rs"), "pub fn a() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"{\"id\":1,\"method\":\"edge_stats\"}\n{\"id\":2,\"method\":\"quit\"}\n")
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();

    let (_, cli) = run(&dir, &["edges", "--stats", "."]);
    // CLI 文本与 serve JSON 逐项对应：use 1 条（ext:std::fmt）+ import 1 条（ext:fmt）
    assert!(cli.contains("internal 0/2"), "{cli}");
    assert!(cli.contains("ext 外部 2"), "{cli}");
    assert!(cli.contains("未解析 ? 0"), "{cli}");
    assert!(text.contains("\"internal\":0"), "{text}");
    assert!(text.contains("\"ext\":2"), "{text}");
    assert!(text.contains("\"unresolved\":0"), "{text}");
    assert!(text.contains("\"total\":2"), "{text}");
}

#[test]
fn serve_edges_matches_cli_shape() {
    // 边层的消费端：字段名跟 `jw edges` 同构（kind/dst），测试钉住
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve-edges");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/lib.rs"), "mod a;\nuse std::fmt;\n").expect("写");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"{\"id\":1,\"method\":\"edges\",\"path\":\"src/lib.rs\"}\n{\"id\":2,\"method\":\"quit\"}\n")
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("\"kind\":\"mod\""), "{text}");
    assert!(text.contains("\"dst\":\"src/a.rs\""), "{text}");
    assert!(text.contains("\"total\":2"), "{text}");
    assert!(text.contains("\"v\":1"), "{text}");

    // 与 CLI 同构：同一个文件，两边列出的 dst 必须一致
    let (_, cli) = run(&dir, &["edges", "src/lib.rs", "."]);
    for dst in ["src/a.rs", "ext:std::fmt"] {
        assert!(cli.contains(dst), "CLI 缺 {dst}：{cli}");
        assert!(text.contains(dst), "serve 缺 {dst}：{text}");
    }
}

#[test]
fn go_parse_finds_func_and_import_edges() {
    // Go 最小语言层：注册表分派（不是 if lang=="go"）、go.func、import → ext:
    let dir = temp_dir("go-parse");
    git(&dir, &["init", "-q"]);
    std::fs::write(
        dir.join("main.go"),
        "package main\n\nimport \"fmt\"\n\nfunc Hello() { fmt.Println(\"hi\") }\n\ntype Thing struct{}\n",
    )
    .expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("覆盖 go 1/1"), "Go 覆盖行没打：{out}");
    assert!(out.contains("import 1"), "import 边没统计：{out}");

    let (c, out) = run(&dir, &["find", "Hello", "--kind", "symbol", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("main.go:5"), "Go 函数行号不对：{out}");

    let (c, out) = run(&dir, &["edges", "main.go", "--kind", "import", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("ext:fmt"), "外部包没标 ext：{out}");

    let (c, out) = run(&dir, &["check", "."]);
    assert_eq!(c, 0, "{out}");
}

#[test]
fn edges_mod_use_impl_are_recorded() {
    // 最小边层：mod 包含 / use 引用 / impl 关系（关系不是判断，单独一张表）
    let dir = temp_dir("edges");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src/lib.rs"),
        "mod a;\nuse std::fmt;\nuse crate::a;\npub struct S;\nimpl std::fmt::Display for S {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, \"s\") }\n}\n",
    )
    .expect("写");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("边：mod 1 · use 2 · impl 1"), "{out}");

    let (c, out) = run(&dir, &["edges", "src/lib.rs", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("mod   src/a.rs"), "mod 目标没解析成路径：{out}");
    assert!(out.contains("use   ext:std::fmt"), "std 应标 ext：{out}");
    assert!(out.contains("use   src/a.rs"), "crate::a 应解析到文件：{out}");

    // impl 是"类型 → trait"，从类型这一端查
    let (c, out) = run(&dir, &["edges", "type:S", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("impl  ext:std::fmt::Display"), "外部 trait 应标 ext：{out}");

    // 只看一类
    let (c, out) = run(&dir, &["edges", "src/lib.rs", "--kind", "mod", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(!out.contains("?use std::fmt"), "{out}");

    let (c, out) = run(&dir, &["check", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("edge_empty=0"), "{out}");
}

#[test]
fn deps_says_no_cargo_for_npm_repo() {
    // 没有 Cargo.toml 的 JS 仓库：必须说"npm 未采集"，不许打"依赖 0"
    let dir = temp_dir("deps-nocargo");
    std::fs::write(dir.join("package.json"), "{}").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["deps", "."]);
    assert_eq!(c, 0, "npm 已采集，不该再报错：{out}");
    assert!(out.contains("npm deps 0"), "{out}");
    assert!(!out.contains("未采集"), "{out}");
}

#[test]
fn deps_with_same_name_in_dev_keeps_both() {
    // serde 同时在 [dependencies] 与 [dev-dependencies]：subject 要带类型，否则撞 id
    let dir = temp_dir("deps-dup");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"d\"\n\n[dependencies]\nserde = \"1\"\n\n[dev-dependencies]\nserde = \"1\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("Cargo.lock"),
        "[[package]]\nname = \"serde\"\nversion = \"1.0.210\"\n",
    )
    .expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["deps", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("deps 2"), "{out}");
    assert!(out.contains("开发 1"), "{out}");

    let (_, out) = run(&dir, &["stat", "."]);
    assert!(out.contains("normal 1"), "{out}");
    assert!(out.contains("dev 1"), "{out}");
}

#[test]
fn deps_says_npm_uncollected() {
    let dir = temp_dir("deps-npm");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"d\"\n\n[dependencies]\nanyhow = \"1\"\n",
    )
    .expect("写");
    std::fs::write(dir.join("package.json"), "{}").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["deps", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("npm deps 0"), "混仓也要真采 npm：{out}");
}

#[test]
fn find_json_lists_hits() {
    // find 也有 JSON：每条带 subject/kind/verdict，符号额外带 path/line/name
    let dir = temp_dir("find-json");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src").join("x.rs"), "pub fn hello() {}\n").expect("写");
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["find", "hello", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    for key in ["\"v\": 1", "\"hits\"", "\"subject\"", "\"path\"", "\"line\"", "\"name\""] {
        assert!(out.contains(key), "缺键 {key}：{out}");
    }
    assert!(out.contains("src/x.rs"), "{out}");

    let (c, out) = run(&dir, &["find", "zzznope", "--json", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("\"total\": 0"), "{out}");
}

#[test]
fn timeline_json_matches_text() {
    // timeline 也有 JSON 契约：与文本同口径（改动总数 = 各天之和）
    let dir = temp_dir("timeline-json");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "1").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    std::fs::write(dir.join("a.txt"), "2").expect("写");
    git(&dir, &["commit", "-q", "-a", "-m", "c2"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["timeline", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    for key in ["\"v\": 1", "\"days\"", "\"commits_total\"", "\"changes\"", "\"cumulative_files\""] {
        assert!(out.contains(key), "缺键 {key}：{out}");
    }
    // 两次提交：一次新增、一次改动
    assert!(out.contains("\"added\": 1"), "{out}");
    assert!(out.contains("\"modified\": 1"), "{out}");
    assert!(out.contains("\"commits\": 2"), "{out}");
}

#[test]
fn serve_compacts_when_idle() {
    // 空闲折叠：serve 空闲够久会在后台跑 compact（不往 stdout 打），并记 last_compact_at
    use std::io::{Read, Write};
    use std::process::Stdio;

    let dir = temp_dir("serve-idle");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .env("JW_IDLE_COMPACT_SECS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"{\"id\":1,\"method\":\"ping\"}\n")
            .expect("写请求");
        stdin.flush().expect("flush");
    }
    // 读一行回包（确认服务活着），然后等它空闲
    {
        let stdout = child.stdout.as_mut().expect("stdout");
        let mut buf = [0u8; 1];
        stdout.read_exact(&mut buf).expect("读回包");
    }
    std::thread::sleep(std::time::Duration::from_millis(2200));

    let (_, out) = run(&dir, &["stat", "."]);
    assert!(out.contains("last_compact"), "空闲折叠没跑：{out}");

    drop(child.stdin.take());
    let _ = child.wait_with_output();
}

#[test]
fn serve_rejects_unknown_version() {
    // 协议版本：客户端声明 v:2 → 直接拒，不猜语义（旧服务端不假装兼容）
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve-v");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"{\"id\":1,\"method\":\"ping\",\"v\":2}\n{\"id\":2,\"method\":\"quit\"}\n")
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("unsupported_version"), "{text}");
    assert!(text.contains("\"ok\":false"), "{text}");
    assert!(!text.contains("pong"), "{text}");
}

#[test]
fn serve_card_takes_many_at_once() {
    // 一次请求取 N 张卡（和 CLI 的 --also 对齐）
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve-also");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    std::fs::write(dir.join("b.md"), "# b\n").expect("写");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(
                b"{\"id\":1,\"method\":\"card\",\"path\":\"a.md\",\"also\":[\"b.md\"]}\n{\"id\":2,\"method\":\"quit\"}\n",
            )
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("\"cards\""), "{text}");
    assert!(text.contains("doc:a.md"), "{text}");
    assert!(text.contains("doc:b.md"), "{text}");
}

#[test]
fn serve_refresh_keeps_stream_clean() {
    // refresh 在 serve 里必须安静：stdout 只能有 JSON 行（不能混进 ingested/parsed 的打印）
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("serve-refresh");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(
                b"{\"id\":1,\"method\":\"refresh\"}\n{\"id\":2,\"method\":\"stat\"}\n{\"id\":3,\"method\":\"quit\"}\n",
            )
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines = only_responses(&text);
    assert_eq!(lines.len(), 2, "流被污染了：{text}");
    for l in &lines {
        assert!(l.trim_start().starts_with('{'), "不是 JSON 行：{l}");
    }
    assert!(lines[0].contains("\"write_seq\":3"), "{text}");
    assert!(lines[1].contains("\"judgment\":2"), "{text}");
}

#[test]
fn refresh_runs_all_three_inflows() {
    // 编辑器保存后的入口：一条命令把 docs/parse/ingest 都跑一遍，seq 前进 3
    let dir = temp_dir("refresh");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src").join("x.rs"), "pub fn x() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["refresh", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("docs"), "{out}");
    assert!(out.contains("symbols"), "{out}");
    assert!(out.contains("file-changes"), "{out}");
    assert!(out.contains("refresh done"), "{out}");
    assert_eq!(write_seq(&dir), 3, "三条流入应推进 3 次：{out}");

    // 再跑一次：数据幂等，但每条流入仍记一次写（客户端能感知"跑过"）
    let (c, _) = run(&dir, &["refresh", "."]);
    assert_eq!(c, 0);
    assert_eq!(write_seq(&dir), 6);
}

#[test]
fn every_write_command_bumps_seq() {
    // 收口前的安全网：任何写命令漏了 note_write，这条就红
    let dir = temp_dir("seq-all");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"s\"\n\n[dependencies]\nanyhow = \"1\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("Cargo.lock"),
        "[[package]]\nname = \"anyhow\"\nversion = \"1.0.86\"\n",
    )
    .expect("写");

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);

    let mut last = write_seq(&dir);
    assert_eq!(last, -1, "还没写过不该有 seq");

    for args in [
        vec!["docs", "."],
        vec!["deps", "."],
        vec!["parse", "."],
        vec!["ingest", "--limit", "0", "."],
        vec!["verify", "doc:a.md", "."],
        vec!["expire", "doc:a.md", "."],
    ] {
        let (c, out) = run(&dir, &args);
        assert_eq!(c, 0, "{args:?} 失败：{out}");
        let now = write_seq(&dir);
        assert!(now > last, "{args:?} 没推进 seq：{last} -> {now}");
        last = now;
    }
}

#[test]
fn writes_bump_write_seq() {
    // 订阅接口：每次写 seq+1；客户端只比较 seq 就知道库变没变
    let dir = temp_dir("write-seq");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");

    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    let (_, out) = run(&dir, &["stat", "."]);
    assert!(out.contains("write_seq   1"), "{out}");

    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    let (_, out) = run(&dir, &["stat", "."]);
    assert!(out.contains("write_seq   2"), "{out}");

    let (c, _) = run(&dir, &["verify", "doc:a.md", "."]);
    assert_eq!(c, 0);
    let (_, out) = run(&dir, &["stat", "."]);
    assert!(out.contains("write_seq   3"), "{out}");
}

#[test]
fn preview_and_pin_slots() {
    // 两个槽：preview 一个（浏览时复用），pinned 一个；resume 先 pinned 后 preview
    let dir = temp_dir("slots-pin");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    std::fs::write(dir.join("b.md"), "# b\n").expect("写");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, _) = run(&dir, &["card", "a.md", "--pin", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["card", "b.md", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["resume", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.starts_with("[pinned] a.md"), "{out}");

    let (c, _) = run(&dir, &["unpin", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["resume", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.starts_with("[preview] b.md"), "{out}");
}

#[test]
fn card_batch_takes_many_in_one_process() {
    // 批量：一次进程取 N 张卡（避免每卡一进程的往返成本）
    let dir = temp_dir("card-batch");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    std::fs::write(dir.join("b.md"), "# b\n").expect("写");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["card", "a.md", "--also", "b.md", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.trim_start().starts_with('['), "多张应输出数组：{out}");
    assert!(out.contains("doc:a.md"), "{out}");
    assert!(out.contains("doc:b.md"), "{out}");
}

fn git_head(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("git rev-parse");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn entity_of(out: &str) -> String {
    let key = "\"entity\": \"";
    let i = out.find(key).expect("输出里没有 entity");
    let rest = &out[i + key.len()..];
    rest[..rest.find('"').unwrap()].to_string()
}

#[test]
fn git_rename_keeps_entity() {
    // git rename：blob 不变 → entity 不变；新路径行带 old_path
    let dir = temp_dir("git-rename");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "same").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    git(&dir, &["mv", "a.txt", "b.txt"]);
    git(&dir, &["commit", "-q", "-m", "rename"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0, "{out}");

    let (_, a) = run(&dir, &["card", "file:a.txt", "--json", "."]);
    let (_, b) = run(&dir, &["card", "file:b.txt", "--json", "."]);
    assert_eq!(entity_of(&a), entity_of(&b), "rename 后 entity 变了：\n{a}\n---\n{b}");
    assert!(b.contains("\"old_path\": \"a.txt\""), "{b}");
}

fn dwidth(s: &str) -> usize {
    s.chars()
        .map(|c| if (c as u32) > 0x2E80 { 2 } else { 1 })
        .sum()
}

#[test]
fn timeline_fits_one_screen() {
    // 一屏：行数 ≤ 12、每行 ≤ 48 列（不然"时间线"就变成刷屏）
    let dir = temp_dir("timeline-screen");
    git(&dir, &["init", "-q"]);
    for (name, body) in [("a.txt", "1"), ("b.txt", "2"), ("c.txt", "3")] {
        std::fs::write(dir.join(name), body).expect("写");
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", name]);
    }
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["timeline", "."]);
    assert_eq!(c, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.len() <= 12, "行数 {} 超了：{out}", lines.len());
    for line in &lines {
        assert!(dwidth(line) <= 48, "行宽超 48：{line:?}（{}）", dwidth(line));
    }
    assert!(out.contains("累计"), "{out}");
}

#[test]
fn entity_survives_rename() {
    // 改名/移动后身份不变：同内容 → 复用同一 entity 与首次路径
    let dir = temp_dir("entity");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# same body\n").expect("写");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    std::fs::rename(dir.join("a.md"), dir.join("b.md")).expect("改名");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (_, a) = run(&dir, &["card", "doc:a.md", "--json", "."]);
    let (_, b) = run(&dir, &["card", "doc:b.md", "--json", "."]);
    assert_eq!(entity_of(&a), entity_of(&b), "改名后 entity 变了：\n{a}\n---\n{b}");
    assert!(a.contains("\"entity_first_path\": \"a.md\""), "{a}");
    assert!(b.contains("\"entity_first_path\": \"a.md\""), "{b}");
}

#[test]
fn card_commit_shows_files_and_why() {
    let dir = temp_dir("card-commit");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "v1").expect("写");
    std::fs::write(dir.join("b.txt"), "v1").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first shot"]);
    let sha = git_head(&dir);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["card", &sha, "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("first shot"), "{out}");
    assert!(out.contains("文件 2"), "文件数不对：{out}");
    assert!(out.contains("a.txt"), "{out}");

    // 没采集的提交 → 空态，不报错也不编
    let (c, out) = run(&dir, &["card", "deadbeefdeadbeef", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("没采集"), "{out}");
}

#[test]
fn card_history_shows_days() {
    let dir = temp_dir("card-history");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "v1").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    std::fs::write(dir.join("a.txt"), "v2").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c2"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["card", "a.txt", "--history", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("改动 2 次（history）"), "{out}");
    assert!(out.contains("按天"), "{out}");

    let (c, out) = run(&dir, &["card", "a.txt", "--history", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"mode\": \"history\""), "{out}");
    assert!(out.contains("\"days\""), "{out}");
}

#[test]
fn card_json_is_the_ui_contract() {
    // UI 层只读这份 JSON：字段名是契约，改动要同步规格
    let dir = temp_dir("card-json");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["card", "a.md", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    for key in [
        "\"v\"",
        "\"subject\"",
        "\"state\"",
        "\"exit\"",
        "\"judgment\"",
        "\"scope\"",
        "\"evidence\"",
        "\"symbols\"",
        "\"definitions\"",
    ] {
        assert!(out.contains(key), "契约缺字段 {key}：{out}");
    }
}

#[test]
fn card_shows_structure_and_definitions() {
    let dir = temp_dir("card");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src").join("x.rs"),
        "pub fn hello() {}\nstruct S;\nconst C: u8 = 1;\n",
    )
    .expect("写");
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    // 只有符号判断、没有 file: 判断 → 状态诚实报 uncertain(no_data)，卡片照出结构
    let (c, out) = run(&dir, &["card", "src/x.rs", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("符号 3（3 处定义"), "符号统计不对：{out}");
    assert!(out.contains("fn 1"), "{out}");
    assert!(out.contains("struct 1"), "{out}");
    assert!(out.contains("const 1"), "{out}");
    assert!(out.contains("文档 无"), "{out}");
}

#[test]
fn ask_does_not_guess_paths() {
    // ask 只答给定 subject：不把 "a.md" 偷偷当成 doc:a.md（猜=配错对象）
    let dir = temp_dir("ask-path");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写文件");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["ask", "a.md", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.starts_with("no_data: a.md"), "{out}");
    assert!(out.contains("候选 subject"), "没给候选：{out}");
    assert!(out.contains("doc:a.md"), "{out}");

    // 用完整 subject 才作答
    let (c, out) = run(&dir, &["ask", "doc:a.md", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.starts_with("held: doc:a.md"), "{out}");

    // 路径工具仍然接受路径（它们按 file → doc 的文档规则解析）
    let (c, out) = run(&dir, &["card", "a.md", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("doc:a.md"), "{out}");
}

#[test]
fn explain_gives_one_screen_state() {
    let dir = temp_dir("explain");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src").join("x.rs"), "pub fn hello() {}\n").expect("写文件");
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    // 只有符号判断、没有 file: 判断 → 状态诚实报 no_data（exit 2），但其他信息照给
    let (c, out) = run(&dir, &["explain", "src/x.rs", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("状态"), "{out}");
    assert!(out.contains("no_data"), "{out}");
    assert!(out.contains("符号    1 个"), "{out}");
    assert!(out.contains("文档    无"), "{out}");
}

#[test]
fn project_shows_growth() {
    let dir = temp_dir("project");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    std::fs::write(dir.join("a.md"), "# a\n").expect("写文件");
    std::fs::write(dir.join("b.rs"), "pub fn f() {}\n").expect("写文件");
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["project", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("文档    1"), "{out}");
    assert!(out.contains("符号    1"), "{out}");
    assert!(out.contains("决策    0"), "{out}");
}

#[test]
fn preview_replaces_same_slot() {
    // 槽语义：preview 只有一个（浏览即替换）、pin 不被 preview 顶掉、focus 从 meta 恢复
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("slots");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").expect("写");
    std::fs::write(dir.join("src/b.rs"), "pub fn b() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(
                b"{\"id\":1,\"method\":\"pin\",\"path\":\"src/a.rs\"}\n                  {\"id\":2,\"method\":\"open\",\"path\":\"src/b.rs\"}\n                  {\"id\":3,\"method\":\"open\",\"path\":\"src/a.rs\"}\n                  {\"id\":4,\"method\":\"focus\"}\n                  {\"id\":5,\"method\":\"quit\"}\n",
            )
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();

    // pin 是 a.rs；两次 open 后 preview 应是最后一次（a.rs），pin 仍是 a.rs
    assert!(text.contains("\"slot\":\"pinned\""), "{text}");
    assert!(text.contains("\"pinned\":\"src/a.rs\""), "pin 被顶掉了：{text}");
    assert!(text.contains("\"preview\":\"src/a.rs\""), "preview 不是最后一次浏览：{text}");
    assert!(text.contains("\"last_focus\":\"src/a.rs\""), "focus 没从 meta 恢复：{text}");
}

fn slot_run(dir: &std::path::Path, reqs: &str) -> String {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(reqs.as_bytes())
        .expect("写请求");
    let out = child.wait_with_output().expect("等 serve");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn pin_survives_preview() {
    let dir = temp_dir("pin-survives");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let text = slot_run(
        &dir,
        "{\"id\":1,\"method\":\"pin\",\"path\":\"src/a.rs\"}\n         {\"id\":2,\"method\":\"open\",\"path\":\"src/b.rs\"}\n         {\"id\":3,\"method\":\"focus\"}\n         {\"id\":4,\"method\":\"quit\"}\n",
    );
    assert!(text.contains("\"pinned\":\"src/a.rs\""), "pin 被 preview 顶掉：{text}");
    assert!(text.contains("\"preview\":\"src/b.rs\""), "preview 没替换：{text}");
    let (_, resume) = run(&dir, &["resume", "."]);
    assert!(resume.contains("[pinned] src/a.rs"), "resume 没优先 pin：{resume}");
}

#[test]
fn focus_restored_from_meta() {
    let dir = temp_dir("focus-restore");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let _ = slot_run(
        &dir,
        "{\"id\":1,\"method\":\"open\",\"path\":\"src/a.rs\"}\n{\"id\":2,\"method\":\"quit\"}\n",
    );
    // 新进程（新连接）只读 focus：必须从 meta 恢复
    let text = slot_run(
        &dir,
        "{\"id\":1,\"method\":\"focus\"}\n{\"id\":2,\"method\":\"quit\"}\n",
    );
    assert!(text.contains("\"last_focus\":\"src/a.rs\""), "focus 没从 meta 恢复：{text}");
}

#[test]
fn card_state_def_lists_symbols() {
    let dir = temp_dir("card-def");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn alpha() {}\npub struct Beta;\n").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["card", "src/a.rs", "--state", "def", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("定义"), "{out}");
    assert!(out.contains("alpha"), "{out}");
    let (_, js) = run(&dir, &["card", "src/a.rs", "--state", "def", "--json", "."]);
    assert!(js.contains("\"mode\": \"def\""), "{js}");
    assert!(js.contains("\"total\": 2"), "{js}");
}

#[test]
fn card_state_dups_finds_multi_definitions() {
    let dir = temp_dir("card-dups");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn shared() {}\n").expect("写");
    std::fs::write(dir.join("src/b.rs"), "pub fn shared() {}\n").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["card", "src/a.rs", "--state", "dups", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"mode\": \"dups\""), "{out}");
    assert!(out.contains("shared"), "同名多定义没找到：{out}");
    assert!(out.contains("\"files\": 2"), "{out}");
}

#[test]
fn card_file_history_counts_match() {
    let dir = temp_dir("card-hist");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.rs"), "pub fn a() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    std::fs::write(dir.join("a.rs"), "pub fn a() {}\npub fn b() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c2"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["card", "a.rs", "--state", "history", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"mode\": \"history\""), "{out}");
    // 两次提交 → 改动 2 次、天数 1 天（同一秒）
    assert!(out.contains("\"changes\": 2"), "{out}");
    assert!(out.contains("\"days\":"), "{out}");
}

#[test]
fn go_import_maps_module_to_dir() {
    // go.mod 的 module 前缀 → dir:<相对目录>，不是 ext:
    let dir = temp_dir("go-dir");
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(dir.join("go.mod"), "module example.com/m\n\ngo 1.22\n").expect("写");
    std::fs::write(
        dir.join("main.go"),
        "package main\n\nimport \"example.com/m/pkg\"\n\nfunc main() {}\n",
    )
    .expect("写");
    std::fs::write(dir.join("pkg/x.go"), "package pkg\n\nfunc X() {}\n").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["edges", "main.go", "--kind", "import", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("dir:pkg"), "内部 import 没映射到 dir:：{out}");
}

#[test]
fn find_hides_expired_symbol() {
    let dir = temp_dir("find-expired");
    std::fs::write(dir.join("a.rs"), "pub fn gone() {}\n").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["expire", "symbol:a.rs#gone", "."]);
    assert_eq!(c, 0);
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let (_, out) = run(&dir, &["find", "gone", "--kind", "symbol", "."]);
    assert!(!out.contains("a.rs"), "过期符号不该出现在 find 里：{out}");
}

#[test]
fn card_range_top_lists_capped_files() {
    let dir = temp_dir("card-top");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(dir.join("pkg/a.rs"), "pub fn a() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["card", "pkg", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"top\": ["), "{out}");
    assert!(out.contains("pkg/a.rs"), "{out}");
    assert!(out.contains("\"top_more\": 0"), "{out}");
}

fn symbol_entity_of(dir: &std::path::Path, subject: &str) -> String {
    let (c, out) = run(dir, &["ask", subject, "--json", "."]);
    assert_eq!(c, 0, "ask 失败：{out}");
    for line in out.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("\"entity\": \"") {
            return rest.trim_end_matches('"').to_string();
        }
    }
    panic!("没读到 entity：{out}");
}

#[test]
fn symbol_entity_survives_content_change() {
    let dir = temp_dir("sym-entity");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn hello() {}\n").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let e1 = symbol_entity_of(&dir, "symbol:src/a.rs#hello");
    assert!(e1.starts_with("s:"), "符号 entity 不是 s: 前缀：{e1}");

    // 内容变了、路径/名字/kind 没变 → 同一实体（长链才能折）
    std::fs::write(dir.join("src/a.rs"), "pub fn hello() {\n    let _x = 1;\n}\n").expect("写");
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let e2 = symbol_entity_of(&dir, "symbol:src/a.rs#hello");
    assert_eq!(e1, e2, "内容变了 entity 就断了：{e1} -> {e2}");
}

#[test]
fn symbol_entity_changes_on_rename_path() {
    let dir = temp_dir("sym-rename");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn hello() {}\n").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let e1 = symbol_entity_of(&dir, "symbol:src/a.rs#hello");

    std::fs::rename(dir.join("src/a.rs"), dir.join("src/b.rs")).expect("改名");
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let e2 = symbol_entity_of(&dir, "symbol:src/b.rs#hello");
    assert_ne!(e1, e2, "改路径后 entity 不该相同：{e1}");
}

#[test]
fn module_card_counts_match_sql() {
    let dir = temp_dir("module-card");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\npub struct A;\n").expect("写");
    std::fs::write(dir.join("src/b.rs"), "pub fn b() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["card", "src", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"files\": 2"), "{out}");
    assert!(out.contains("\"changes\": 2"), "{out}");
    // 3 个符号判断、3 个不同名字
    assert!(out.contains("\"judgments\": 5"), "{out}");
    assert!(out.contains("\"definitions\": 3"), "{out}");
    assert!(out.contains("\"symbols\""), "{out}");
    assert!(out.contains("\"recent\": ["), "{out}");
    assert!(out.contains("\"decisions\": 0"), "{out}");
    assert!(out.contains("\"evidence\": 5"), "依据数应与判断数一致：{out}");
}

#[test]
fn module_card_omits_empty_deps() {
    let dir = temp_dir("module-deps");
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(dir.join("pkg/a.rs"), "pub fn a() {}\n").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    // 没有 Cargo.toml：整节省略（不是"依赖 0"）
    let (_, out) = run(&dir, &["card", "pkg", "--json", "."]);
    assert!(!out.contains("\"deps\""), "没有 Cargo.toml 却出现 deps 节：{out}");

    // 加上 Cargo.toml 后 deps 节出现
    std::fs::write(
        dir.join("pkg/Cargo.toml"),
        "[package]\nname = \"pkg\"\n\n[dependencies]\nserde = \"1\"\nanyhow = \"1\"\n",
    )
    .expect("写");
    let (_, out) = run(&dir, &["card", "pkg", "--json", "."]);
    assert!(out.contains("\"deps\""), "有 Cargo.toml 却没 deps 节：{out}");
    assert!(out.contains("serde"), "{out}");
}

fn decision_doc(title: &str, picked: &str) -> String {
    format!(
        "# D-20261021-05 · {title}\n\n- 日期：2026-10-21\n- 状态：**已落地**\n\n## 决策\n\n主路径固定为 8 个动词。\n\n## 三个选项与取舍\n\n| 选项 | 做法 | 代价 | 结论 |\n|---|---|---|---|\n| A | 固定 8 个 | 小 | {picked} |\n| B | 不删不改 | 中 | 否决 |\n| C | 不动 | 大 | 否决 |\n"
    )
}

#[test]
fn decision_card_candidates_match_sql() {
    let dir = temp_dir("decision-card");
    std::fs::create_dir_all(dir.join("docs/decisions")).expect("建目录");
    std::fs::write(
        dir.join("docs/decisions/D-20261021-05-主路径八动词.md"),
        decision_doc("主路径八动词", "选它"),
    )
    .expect("写");
    let (c, out) = run(&dir, &["init", "."]);
    assert_eq!(c, 0, "{out}");
    let (c, out) = run(&dir, &["record", "docs/decisions"]);
    assert_eq!(c, 0, "{out}");

    let (c, out) = run(&dir, &["card", "decision:D-20261021-05", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"candidates\": 3"), "{out}");
    assert!(out.contains("\"picked\": 1"), "{out}");
    assert!(out.contains("\"rejected\": 2"), "{out}");
    assert!(out.contains("\"uncertain\": 0"), "{out}");
    assert!(out.contains("record:D-20261021-05"), "{out}");
}

#[test]
fn decision_card_reads_original_lazily() {
    let dir = temp_dir("decision-original");
    std::fs::create_dir_all(dir.join("docs/decisions")).expect("建目录");
    let doc = dir.join("docs/decisions/D-20261021-05-主路径八动词.md");
    std::fs::write(&doc, decision_doc("主路径八动词", "选它")).expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["record", "docs/decisions"]);
    assert_eq!(c, 0);

    // 有原文：标题 + 决策句
    let (c, out) = run(&dir, &["card", "decision:D-20261021-05", "--state", "original", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"title\""), "{out}");
    assert!(out.contains("主路径固定为 8 个动词"), "{out}");

    // 删掉原文：显示"未采集"，exit 不变（不崩）
    std::fs::remove_file(&doc).expect("删");
    let (c, out) = run(&dir, &["card", "decision:D-20261021-05", "--state", "original", "--json", "."]);
    assert_eq!(c, 0, "原文缺失不该改变 exit：{out}");
    assert!(out.contains("未采集"), "{out}");
}

#[test]
fn decision_card_flags_unmarked_options() {
    // 散文式选项（没有 选它/否决）→ 明确标"未标注结论"，不假装 1/2
    let dir = temp_dir("decision-prose");
    std::fs::create_dir_all(dir.join("docs/decisions")).expect("建目录");
    std::fs::write(
        dir.join("docs/decisions/D-20261021-05-主路径八动词.md"),
        "# D-20261021-05 · 主路径八动词\n\n- 日期：2026-10-21\n- 状态：**已落地**\n\n## 决策\n\n主路径固定为 8 个动词。\n\n## 为什么不是别的\n\n- B. 直接删 28 个：一轮做不完。\n- C. 不动：用户每次先做决策。\n",
    )
    .expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["record", "docs/decisions"]);
    assert_eq!(c, 0);

    let (c, out) = run(&dir, &["card", "decision:D-20261021-05", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("未标注结论"), "{out}");
    let (_, js) = run(&dir, &["card", "decision:D-20261021-05", "--json", "."]);
    assert!(js.contains("\"picked\": 0"), "{js}");
    assert!(js.contains("\"rejected\": 0"), "{js}");
    assert!(js.contains("\"uncertain\": 2"), "{js}");

    // lint：散文式选项被点名，exit 仍是 0
    let (c, out) = run(&dir, &["record", "docs/decisions", "--lint"]);
    assert_eq!(c, 0, "lint 不该改 exit：{out}");
    assert!(out.contains("散文式选项，无结论标注"), "{out}");
}

#[test]
fn record_lint_clean_for_structured_table() {
    let dir = temp_dir("decision-lint-clean");
    std::fs::create_dir_all(dir.join("docs/decisions")).expect("建目录");
    std::fs::write(
        dir.join("docs/decisions/D-20261023-05-事件归属.md"),
        "# D-20261023-05 · 事件归属\n\n- 日期：2026-10-23\n- 状态：**已落地**\n\n## 决策\n\n按仓库归属。\n\n## 三个选项与取舍\n\n| 选项 | 做法 | 代价 | 结论 |\n|---|---|---|---|\n| A | 按仓库 | 小 | 选它 |\n| B | key 复合 | 中 | 否决 |\n| C | 不做 | 零 | 否决 |\n",
    )
    .expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["record", "docs/decisions", "--lint"]);
    assert_eq!(c, 0, "{out}");
    assert!(!out.contains("散文式选项"), "结构化表格被误报：{out}");
    assert!(out.contains("0 条无结论标注"), "{out}");
}

/// 显示宽度：CJK / 全角按 2 列算（终端口径）。
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let u = c as u32;
            let wide = (0x1100..=0x115f).contains(&u)
                || (0x2e80..=0xa4cf).contains(&u)
                || (0xac00..=0xd7a3).contains(&u)
                || (0xf900..=0xfaff).contains(&u)
                || (0xfe30..=0xfe6f).contains(&u)
                || (0xff00..=0xff60).contains(&u)
                || (0xffe0..=0xffe6).contains(&u);
            if wide {
                2
            } else {
                1
            }
        })
        .sum()
}

fn card_lines(out: &str) -> Vec<String> {
    out.lines().map(|l| l.to_string()).collect()
}

#[test]
fn cards_fit_width_budget() {
    let dir = temp_dir("cards-width");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src/a.rs"),
        "use std::fmt;\npub trait Speak {}\npub struct A;\nimpl Speak for A {}\npub fn a() {}\npub fn b() {}\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"demo\"\n\n[dependencies]\nserde = \"1\"\n",
    )
    .expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["deps", "."]);
    assert_eq!(c, 0);

    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dir)
        .output()
        .expect("git rev-parse");
    let head = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(!head.is_empty(), "没拿到提交哈希");

    let cases: Vec<(&str, usize)> = vec![
        ("src/a.rs", 6),      // C1 文件卡
        (head.as_str(), 5),   // C2 提交卡
        ("dep:serde", 6),     // C3 依赖卡
        ("src", 8),           // C4 模块卡
    ];
    for (sel, max_lines) in cases {
        let (c, out) = run(&dir, &["card", sel, "."]);
        assert_eq!(c, 0, "{sel}: {out}");
        let lines = card_lines(&out);
        assert!(lines.len() <= max_lines, "{sel} 行数 {} > {max_lines}：{out}", lines.len());
        for l in &lines {
            assert!(display_width(l) <= 48, "{sel} 超宽 {}：{l}", display_width(l));
        }
    }
}

#[test]
fn cards_density_in_range() {
    let dir = temp_dir("cards-density");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src/a.rs"),
        "use std::fmt;\npub trait Speak {}\npub struct A;\npub enum E { X }\nimpl Speak for A {}\npub fn a() {}\npub fn b() {}\n",
    )
    .expect("写");
    std::fs::write(dir.join("src/b.rs"), "pub fn c() {}\npub const K: i32 = 1;\n").expect("写");
    std::fs::write(
        dir.join("src/Cargo.toml"),
        "[package]\nname = \"demo-src\"\n\n[dependencies]\nserde = \"1\"\nanyhow = \"1\"\nquote = \"1\"\n",
    )
    .expect("写");
    std::fs::create_dir_all(dir.join("docs")).expect("建 docs");
    std::fs::write(dir.join("docs/x.md"), "# x\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\npub fn b() {}\npub struct A;\npub enum E { X }\n// v2\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c2"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["docs", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    for sel in ["src/a.rs", "src"] {
        let (_, out) = run(&dir, &["card", sel, "."]);
        let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
        let ink: usize = lines.iter().map(|l| display_width(l)).sum();
        let capacity = lines.len() * 48;
        let rho = ink as f64 / capacity as f64;
        assert!(
            (0.45..=0.75).contains(&rho),
            "{sel} 密度 ρ={rho:.2} 不在 0.45–0.75（ink={ink} capacity={capacity}）：{out}"
        );
    }
}

#[test]
fn subscribe_sees_write_within_one_second() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    let dir = temp_dir("subscribe");
    std::fs::create_dir_all(dir.join("docs/decisions")).expect("建目录");
    std::fs::write(
        dir.join("docs/decisions/D-20261021-05-主路径八动词.md"),
        "# D-20261021-05 · x\n\n- 日期：2026-10-21\n- 状态：**已落地**\n\n## 决策\n\n固定 8 个。\n",
    )
    .expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    // 先订阅（长轮询 1s），再在另一进程里写
    stdin
        .write_all(b"{\"id\":1,\"method\":\"subscribe\",\"since\":0,\"timeout_ms\":1500}\n")
        .expect("写 subscribe");
    stdin.flush().expect("flush");

    std::thread::sleep(std::time::Duration::from_millis(120));
    let (c, out) = run(&dir, &["record", "docs/decisions"]);
    assert_eq!(c, 0, "{out}");

    let mut line = String::new();
    let v: serde_json::Value = loop {
        line.clear();
        reader.read_line(&mut line).expect("读事件");
        let cand: serde_json::Value = serde_json::from_str(line.trim()).expect("NDJSON");
        if cand["result"]["events"].is_array() {
            break cand;
        }
    };
    let evs = v["result"]["events"].as_array().expect("events");
    let subjects: Vec<String> = evs
        .iter()
        .flat_map(|e| e["subjects"].as_array().cloned().unwrap_or_default())
        .map(|s| s.as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        subjects.iter().any(|s| s.contains("decision:")),
        "1s 内没收到写入事件：{line}"
    );

    stdin
        .write_all(b"{\"id\":2,\"method\":\"quit\"}\n")
        .expect("quit");
    let _ = child.wait();
}

#[test]
fn subscribe_events_match_writes() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("subscribe-count");
    std::fs::write(dir.join("a.rs"), "pub fn hello() {}\n").expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);

    // 先记下 parse 之后的 seq（parse 本身也算一次写），再写 40 次（不超上限 50，不该 gap）
    let (_, st) = run(&dir, &["stat", "."]);
    let seq0: i64 = st
        .lines()
        .find(|l| l.starts_with("write_seq"))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(|v| v.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    assert!(seq0 > 0, "没读到 write_seq：{st}");
    for i in 0..40 {
        let (c, out) = run(&dir, &["expire", "symbol:a.rs#hello", "--at", &(1800000000 + i).to_string(), "."]);
        assert_eq!(c, 0, "{out}");
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        let req = format!(
            "{{\"id\":1,\"method\":\"subscribe\",\"since\":{seq0},\"timeout_ms\":200}}\n{{\"id\":2,\"method\":\"quit\"}}\n"
        );
        stdin.write_all(req.as_bytes()).expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let v: serde_json::Value = events_frame(&text);
    let evs = v["result"]["events"].as_array().expect("events");
    let mut subjects: Vec<String> = evs
        .iter()
        .flat_map(|e| e["subjects"].as_array().cloned().unwrap_or_default())
        .map(|s| s.as_str().unwrap_or("").to_string())
        .collect();
    let total = subjects.len();
    subjects.sort();
    subjects.dedup();
    assert_eq!(total, 40, "事件条数不等于写入次数：{text}");
    assert_eq!(subjects.len(), 1, "同一 subject 被去重成一条：{text}");
    assert_eq!(v["result"]["gap"], false, "没溢出却报了 gap：{text}");
}

#[test]
fn subscribe_can_be_disabled() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = temp_dir("subscribe-off");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let mut child = Command::new(env!("CARGO_BIN_EXE_jw"))
        .args(["serve", ".", "--no-subscribe"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("起 serve");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"{\"id\":1,\"method\":\"subscribe\",\"since\":0}\n{\"id\":2,\"method\":\"quit\"}\n")
            .expect("写请求");
    }
    let out = child.wait_with_output().expect("等 serve");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("subscribe disabled"), "{text}");
    assert!(text.contains("\"ok\":false"), "{text}");
}

#[test]
fn card_lineage_aggregates_renames() {
    let dir = temp_dir("card-lineage");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c1"]);
    git(&dir, &["mv", "src/a.rs", "src/b.rs"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c2"]);
    git(&dir, &["mv", "src/b.rs", "src/c.rs"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c3"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["card", "src/c.rs", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"changes\": 3"), "改名前后没聚合：{out}");
    assert!(out.contains("\"lineage\""), "{out}");
    let lineage = out.matches("src/").count();
    assert!(lineage >= 3, "lineage 少于 3 条：{out}");
}

#[test]
fn card_follows_forward_rename() {
    let dir = temp_dir("card-forward");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c1"]);
    git(&dir, &["mv", "src/a.rs", "src/b.rs"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c2"]);

    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);

    // 旧路径（树里已不存在）→ 跟随到新路径
    let (c, out) = run(&dir, &["card", "src/a.rs", "--json", "."]);
    assert_eq!(c, 0, "旧路径 card 应该仍能出结果：{out}");
    assert!(out.contains("\"moved_to\": \"src/b.rs\""), "{out}");
    assert!(out.contains("\"changes\": 2"), "{out}");

    // 新路径：不该带 moved_to 提示
    let (_, out) = run(&dir, &["card", "src/b.rs", "--json", "."]);
    assert!(out.contains("\"moved_to\": null"), "{out}");
}

#[test]
fn impl_edges_resolve_internal_trait() {
    let dir = temp_dir("impl-resolve");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/t.rs"), "pub trait Speak {}\n").expect("写 t");
    std::fs::write(
        dir.join("src/a.rs"),
        "use crate::t::Speak;\n\npub struct A;\n\nimpl Speak for A {}\n",
    )
    .expect("写 a");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    // impl 的目标应被解析成文件（src/t.rs），不再是 trait:Speak
    let (c, out) = run(&dir, &["edges", "type:A", "--kind", "impl", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("src/t.rs"), "trait 没解析到文件：{out}");
    assert!(!out.contains("trait:Speak"), "还留着未解析的 trait：{out}");
}

#[test]
fn edge_stats_impl_partition() {
    let dir = temp_dir("impl-partition");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/t.rs"), "pub trait Speak {}\n").expect("写 t");
    std::fs::write(
        dir.join("src/a.rs"),
        "use crate::t::Speak;\n\npub struct A;\n\nimpl Speak for A {}\n",
    )
    .expect("写 a");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (_, stats) = run(&dir, &["edges", "--stats", "."]);
    let nums: Vec<i64> = stats
        .lines()
        .find(|l| l.contains("impl internal"))
        .map(|l| {
            l.split_whitespace()
                .filter_map(|t| t.parse::<i64>().ok())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(nums.len(), 3, "没读到 impl 三分类：{stats}");
    assert_eq!(nums[0] + nums[1] + nums[2], 1, "三分类之和不等于 impl 边数：{stats}");
}

#[test]
fn card_edges_counts_match_sql() {
    let dir = temp_dir("card-edges");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/t.rs"), "pub trait Speak {}\n").expect("写 t");
    std::fs::write(
        dir.join("src/a.rs"),
        "use std::fmt;\nuse crate::t::Speak;\n\npub struct A;\n\nimpl Speak for A {}\n",
    )
    .expect("写 a");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (_, out) = run(&dir, &["card", "src/a.rs", "--json", "."]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["edges"]["use"]["internal"].as_i64(), Some(1), "{out}");
    assert_eq!(v["edges"]["use"]["ext"].as_i64(), Some(1), "{out}");
    assert_eq!(v["edges"]["impl"].as_i64(), Some(1), "{out}");
    let (_, text) = run(&dir, &["card", "src/a.rs", "."]);
    assert!(text.contains("导入 2（内部 1 / ext 1）· 实现 1"), "{text}");
}

#[test]
fn card_module_edges_aggregate() {
    let dir = temp_dir("card-edges-dir");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/t.rs"), "pub trait Speak {}\n").expect("写 t");
    std::fs::write(
        dir.join("src/a.rs"),
        "use std::fmt;\nuse crate::t::Speak;\n\npub struct A;\n\nimpl Speak for A {}\n",
    )
    .expect("写 a");
    std::fs::write(dir.join("src/b.rs"), "use std::io;\n").expect("写 b");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    let (_, out) = run(&dir, &["card", "src", "--json", "."]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    // a.rs: 1 ext + 1 internal + 1 impl；b.rs: 1 ext
    assert_eq!(v["edges"]["use"]["internal"].as_i64(), Some(1), "{out}");
    assert_eq!(v["edges"]["use"]["ext"].as_i64(), Some(2), "{out}");
    assert_eq!(v["edges"]["impl"].as_i64(), Some(1), "{out}");
}

#[test]
fn impl_leaf_is_container_file() {
    // impl 边的 leaf = 所在文件（kind 相关语义）；src 仍是 type:X
    let dir = temp_dir("impl-leaf");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/t.rs"), "pub trait Speak {}\n").expect("写 t");
    std::fs::write(
        dir.join("src/a.rs"),
        "use crate::t::Speak;\n\npub struct A;\n\nimpl Speak for A {}\n",
    )
    .expect("写 a");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0);
    // 文件卡里能数到实现（说明 leaf 记的是容器文件）
    let (_, out) = run(&dir, &["card", "src/a.rs", "--json", "."]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["edges"]["impl"].as_i64(), Some(1), "{out}");
    // 从类型这一端查，dst 已解析到 trait 定义文件
    let (_, e) = run(&dir, &["edges", "type:A", "--kind", "impl", "."]);
    assert!(e.contains("src/t.rs"), "{e}");
}

fn cargo_repo(deps: &str) -> std::path::PathBuf {
    let dir = temp_dir("cross-dep");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"x\"\n\n[dependencies]\n{deps}\n"),
    )
    .expect("写 Cargo.toml");
    dir
}

#[test]
fn cross_dep_shared_and_diff() {
    let a = cargo_repo("serde = \"1\"\nanyhow = \"1\"\nonly_a = \"1\"");
    let b = cargo_repo("serde = \"1\"\nanyhow = \"2\"\nonly_b = \"1\"");
    for d in [&a, &b] {
        assert_eq!(run(d, &["init", "."]).0, 0);
        assert_eq!(run(d, &["deps", "."]).0, 0);
    }
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--dep", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["shared"].as_u64(), Some(2), "{out}");
    assert_eq!(v["diff"].as_u64(), Some(1), "{out}");
    let rows = v["rows"].as_array().expect("rows");
    assert!(rows.iter().any(|r| r["name"] == "anyhow" && r["diff"] == true), "{out}");
}

#[test]
fn cross_dep_unknown_marked() {
    // path 依赖没有版本号 → 输出 ?，不猜
    let a = cargo_repo("shared = { path = \"../c\" }");
    let b = cargo_repo("shared = { path = \"../c\" }");
    for d in [&a, &b] {
        assert_eq!(run(d, &["init", "."]).0, 0);
        assert_eq!(run(d, &["deps", "."]).0, 0);
    }
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--dep", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let rows = v["rows"].as_array().expect("rows");
    assert!(!rows.is_empty(), "{out}");
    assert!(rows.iter().all(|r| r["a"] == "?" && r["b"] == "?"), "{out}");
}

#[test]
fn cross_dep_drift_grades() {
    let a = cargo_repo("x = \"1.2.0\"\ny = \"1.2.0\"\nz = \"0.8.0\"\nw = \"2.0.0\"\ns = \"1.2.0\"");
    let b = cargo_repo("x = \"2.0.0\"\ny = \"1.3.0\"\nz = \"0.9.0\"\nw = \"2.0.0\"\ns = \"*\"");
    for d in [&a, &b] {
        assert_eq!(run(d, &["init", "."]).0, 0);
        assert_eq!(run(d, &["deps", "."]).0, 0);
    }
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--dep", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let d = &v["drift"];
    assert_eq!(d["major"].as_u64(), Some(2), "x(1→2) 与 z(0.8→0.9) 都该是 major：{out}");
    assert_eq!(d["minor"].as_u64(), Some(1), "y(1.2→1.3) 该是 minor：{out}");
    assert_eq!(d["same_major"].as_u64(), Some(1), "w 同版本该是 same-major：{out}");
    assert_eq!(d["unknown"].as_u64(), Some(1), "s=\"*\" 该是 unknown：{out}");

    // 排序：major-drift 在前
    let rows = v["rows"].as_array().expect("rows");
    assert_eq!(rows[0]["drift"], "major-drift", "{out}");
}

#[test]
fn cross_dep_drift_zero_x_uses_minor() {
    // 0.8 vs 0.9：按 minor 比，算 major-drift
    let a = cargo_repo("p = \"0.8.1\"\nq = \"0.8.1\"");
    let b = cargo_repo("p = \"0.9.0\"\nq = \"0.8.2\"");
    for d in [&a, &b] {
        assert_eq!(run(d, &["init", "."]).0, 0);
        assert_eq!(run(d, &["deps", "."]).0, 0);
    }
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--dep", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["drift"]["major"].as_u64(), Some(1), "0.8→0.9 该是 major：{out}");
    assert_eq!(v["drift"]["minor"].as_u64(), Some(1), "0.8.1→0.8.2 该是 minor：{out}");
}

fn npm_repo(pkg: &str) -> std::path::PathBuf {
    let dir = temp_dir("npm-dep");
    std::fs::write(dir.join("package.json"), pkg).expect("写 package.json");
    dir
}

#[test]
fn deps_npm_collects_and_marks_unknown() {
    let a = npm_repo(
        "{\"name\":\"a\",\"dependencies\":{\"react\":\"^18.0.0\",\"local\":\"file:../x\"},\"devDependencies\":{\"vitest\":\"^1.0.0\"}}",
    );
    let (c, _) = run(&a, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&a, &["deps", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("npm deps 3"), "{out}");
    assert!(out.contains("版本未知 1"), "{out}");
    let (_, js) = run(&a, &["cross", a.to_str().unwrap(), a.to_str().unwrap(), "--dep", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&js).expect("json");
    assert_eq!(v["shared"].as_u64(), Some(3), "{js}");
    let rows = v["rows"].as_array().expect("rows");
    let unknown: Vec<&serde_json::Value> =
        rows.iter().filter(|r| r["a"] == "?").collect();
    assert_eq!(unknown.len(), 1, "只有 file: 那条该是 ?：{js}");
    assert!(unknown[0]["name"].as_str().unwrap().contains("local"), "{js}");
}

#[test]
fn cross_dep_mixed_ecosystems() {
    let a = cargo_repo("serde = \"1\"");
    std::fs::write(
        a.join("package.json"),
        "{\"name\":\"a\",\"dependencies\":{\"react\":\"^18.0.0\"}}",
    )
    .expect("写 package.json");
    let b = npm_repo("{\"name\":\"b\",\"dependencies\":{\"react\":\"^19.0.0\"}}");
    for d in [&a, &b] {
        assert_eq!(run(d, &["init", "."]).0, 0);
        assert_eq!(run(d, &["deps", "."]).0, 0);
    }
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--dep", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["shared"].as_u64(), Some(1), "跨生态共有 react：{out}");
    assert_eq!(v["drift"]["major"].as_u64(), Some(1), "18→19 是 major：{out}");
}

#[test]
fn deps_py_requirements_collected() {
    let dir = temp_dir("py-deps");
    std::fs::write(
        dir.join("requirements.txt"),
        "requests==2.31.0\ndjango>=4.2\n# 注释\n-r other.txt\n",
    )
    .expect("写");
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["deps", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("py deps 2"), "{out}");

    // 只有 pyproject 的仓：必须说未采集，不许报 0 依赖
    let dir2 = temp_dir("py-only-pyproject");
    std::fs::write(dir2.join("pyproject.toml"), "[project]\nname = \"x\"\n").expect("写");
    let (c, _) = run(&dir2, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir2, &["deps", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("pyproject 未采集"), "{out}");
}

#[test]
fn cross_dep_eco_py_only() {
    let a = temp_dir("py-cross-a");
    std::fs::write(a.join("requirements.txt"), "requests==2.31.0\nflask>=2.0\n").expect("写");
    let b = temp_dir("py-cross-b");
    std::fs::write(b.join("requirements.txt"), "requests==2.32.0\n").expect("写");
    for d in [&a, &b] {
        assert_eq!(run(d, &["init", "."]).0, 0);
        assert_eq!(run(d, &["deps", "."]).0, 0);
    }
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--dep", "--eco", "py", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["eco"].as_str(), Some("py"), "{out}");
    assert_eq!(v["shared"].as_u64(), Some(1), "{out}");
    let rows = v["rows"].as_array().expect("rows");
    assert!(rows.iter().all(|r| r["name"].as_str().unwrap().starts_with("py:")), "{out}");

    // 同一对仓用 --eco cargo：没有 cargo 行
    let (_, out2) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--dep", "--eco", "cargo", "--json"]);
    let v2: serde_json::Value = serde_json::from_str(&out2).expect("json");
    assert_eq!(v2["shared"].as_u64(), Some(0), "{out2}");
}

fn git_repo_with(files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = temp_dir("cross-files");
    git(&dir, &["init", "-q"]);
    for (name, content) in files {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("建目录");
        }
        std::fs::write(&p, content).expect("写");
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c1"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, _) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0);
    dir
}

#[test]
fn cross_files_detects_same_content() {
    let same = "fn shared() {}\n// identical\n";
    let a = git_repo_with(&[("src/a.rs", same), ("src/only_a.rs", "fn a() {}\n")]);
    let b = git_repo_with(&[("lib/b.rs", same), ("lib/only_b.rs", "fn b() {}\n")]);
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--files", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["count"].as_u64(), Some(1), "应检出 1 对同内容：{out}");
    let pairs = v["same_content"].as_array().expect("pairs");
    assert!(pairs[0]["a"].as_str().unwrap().contains("a.rs"), "{out}");
    assert!(pairs[0]["b"].as_str().unwrap().contains("b.rs"), "{out}");
}

#[test]
fn cross_files_reports_zero() {
    let a = git_repo_with(&[("src/a.rs", "fn a() {}\n")]);
    let b = git_repo_with(&[("lib/b.rs", "fn b() {}\n")]);
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--files"]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("相同内容 0 个"), "0 也要照报：{out}");
}

#[test]
fn cross_files_excludes_empty_by_default() {
    let a = git_repo_with(&[("src/empty_a.rs", "")]);
    let b = git_repo_with(&[("lib/empty_b.rs", "")]);
    let (c, out) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--files", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["count"].as_u64(), Some(0), "空文件默认不算同内容：{out}");
    assert_eq!(v["excluded_empty"].as_u64(), Some(1), "{out}");
    assert!(v["no_blob"].as_u64().is_some(), "no_blob 必须报：{out}");

    let (_, out2) = run(&a, &["cross", a.to_str().unwrap(), b.to_str().unwrap(), "--files", "--include-empty", "--json"]);
    let v2: serde_json::Value = serde_json::from_str(&out2).expect("json");
    assert_eq!(v2["count"].as_u64(), Some(1), "--include-empty 应该算 1 对：{out2}");
}

#[test]
fn timeline_files_now_matches_head() {
    let dir = temp_dir("timeline-files");
    git(&dir, &["init", "-q"]);
    for (name, content) in [("a.rs", "fn a() {}\n"), ("b.rs", "fn b() {}\n"), ("c.rs", "fn c() {}\n")] {
        std::fs::write(dir.join(name), content).expect("写");
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c1"]);
    // 删一个、改名一个
    std::fs::remove_file(dir.join("c.rs")).expect("删");
    git(&dir, &["rm", "-q", "c.rs"]);
    git(&dir, &["mv", "b.rs", "d.rs"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c2"]);
    let (c, _) = run(&dir, &["init", "."]);
    assert_eq!(c, 0);
    let (c, out) = run(&dir, &["ingest", "--limit", "0", "."]);
    assert_eq!(c, 0, "{out}");
    let (c, out) = run(&dir, &["timeline", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let fc = &v["file_counts"];
    let hist = fc["historical"].as_i64().expect("historical");
    let cur = fc["current"].as_i64().expect("current");
    let del = fc["deleted"].as_i64().expect("deleted");
    assert_eq!(hist, cur + del, "历史必须 == 当前 + 已删：{out}");
    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "HEAD", "--name-only"])
        .current_dir(&dir)
        .output()
        .expect("git");
    let n = String::from_utf8_lossy(&tree.stdout).lines().filter(|l| !l.trim().is_empty()).count() as i64;
    assert_eq!(cur, n, "当前必须等于 git ls-tree：{out}");
    assert!(del >= 1, "至少删/改名掉一个：{out}");
}

fn py_fixture() -> std::path::PathBuf {
    let dir = temp_dir("py-parse");
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(
        dir.join("pkg/mod.py"),
        "import os\nfrom collections import OrderedDict\n\nclass Greeter:\n    def hello(self):\n        return 1\n\ndef top(a):\n    return a\n",
    )
    .expect("写");
    dir
}

#[test]
fn py_parse_finds_func_class_import_edges() {
    let dir = py_fixture();
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    let (_, edges) = run(&dir, &["edges", "pkg/mod.py", "."]);
    assert!(edges.contains("ext:os"), "{edges}");
    assert!(edges.contains("ext:collections"), "{edges}");
    let (_, stats) = run(&dir, &["raw-stats", "."]);
    assert!(stats.contains("py func 1"), "{stats}");
    assert!(stats.contains("method 1"), "{stats}");
    assert!(stats.contains("class 1"), "{stats}");
    assert!(stats.contains("import 2"), "{stats}");
}

#[test]
fn find_py_function_has_line() {
    let dir = py_fixture();
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["find", "top", "--kind", "symbol", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let hits = v["hits"].as_array().or_else(|| v.as_array()).expect("hits");
    let h = hits.iter().find(|h| h["name"] == "top").expect("找到 top");
    assert_eq!(h["line"].as_str().or(h["line"].as_i64().map(|_| "x")), Some("8"), "{out}");
}

#[test]
fn py_coverage_counts_files() {
    let dir = py_fixture();
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c1"]);
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["ingest", "--limit", "0", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (_, out) = run(&dir, &["coverage", "--json", "."]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["py"]["total"].as_i64(), Some(1), "{out}");
    assert_eq!(v["py"]["parsed"].as_i64(), Some(1), "{out}");
}

#[test]
fn py_internal_import_resolves_to_file() {
    let dir = temp_dir("py-internal");
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(dir.join("pkg/__init__.py"), "").expect("写 init");
    std::fs::write(dir.join("pkg/b.py"), "def f():\n    return 1\n").expect("写 b");
    std::fs::write(
        dir.join("pkg/a.py"),
        "from . import b\nimport os\n\ndef g():\n    return b.f()\n",
    )
    .expect("写 a");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    let (_, edges) = run(&dir, &["edges", "pkg/a.py", "--kind", "import", "."]);
    assert!(edges.contains("py:pkg/b.py"), "相对导入没解析到文件：{edges}");
    assert!(edges.contains("ext:os"), "外部导入应保持 ext：{edges}");
}

#[test]
fn py_absolute_import_resolves_src_layout() {
    let dir = temp_dir("py-src-layout");
    std::fs::create_dir_all(dir.join("src/pkg")).expect("建 src");
    std::fs::write(dir.join("src/pkg/__init__.py"), "").expect("写 init");
    std::fs::write(dir.join("src/pkg/b.py"), "def f():\n    return 1\n").expect("写 b");
    std::fs::write(dir.join("src/pkg/a.py"), "import pkg.b\n\ndef g():\n    return pkg.b.f()\n").expect("写 a");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (_, edges) = run(&dir, &["edges", "src/pkg/a.py", "--kind", "import", "."]);
    assert!(edges.contains("py:src/pkg/b.py"), "src 布局的绝对导入没解析：{edges}");
}

#[test]
fn py_multi_name_import_makes_one_edge_per_name() {
    let dir = temp_dir("py-multi-name");
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(dir.join("pkg/__init__.py"), "").expect("写 init");
    std::fs::write(
        dir.join("pkg/a.py"),
        "from collections import OrderedDict, defaultdict\n\ndef f():\n    return 1\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (_, edges) = run(&dir, &["edges", "pkg/a.py", "--kind", "import", "."]);
    // 一行两个名字 → 两条边（不是一条）
    let count = edges.lines().filter(|l| l.contains("import")).count();
    assert_eq!(count, 2, "一行两名应产生两条边：{edges}");
    let (_, raw) = run(&dir, &["raw-stats", "."]);
    assert!(raw.contains("import 2"), "raw 也按导入名计：{raw}");
}

#[test]
fn py_star_import_single_column() {
    let dir = temp_dir("py-star");
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(dir.join("pkg/__init__.py"), "").expect("写 init");
    std::fs::write(dir.join("pkg/a.py"), "from os.path import *\n\ndef f():\n    return 1\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (_, edges) = run(&dir, &["edges", "pkg/a.py", "--kind", "import", "."]);
    assert!(edges.contains("?py:star:"), "星号导入要单列：{edges}");
    assert!(!edges.contains("py:pkg/"), "星号导入不展开：{edges}");
}

#[test]
fn check_fails_when_rules_version_missing() {
    let dir = temp_dir("rules-check");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    // 从未 parse：meta 里没有 parse_rules_version → check 必须红
    let (c, out) = run(&dir, &["check", "."]);
    assert_ne!(c, 0, "没有 rules 版本时 check 应该失败：{out}");
    assert!(out.contains("rules_version_bad=1"), "{out}");
}

#[test]
fn parse_rebuilds_when_rules_version_differs() {
    let dir = temp_dir("rules-rebuild");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "use std::fmt;\n\npub fn a() {}\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["check", "."]);
    assert_eq!(c, 0, "{out}");

    // 手工把 meta 改回旧版
    let db = dir.join(".jingwei").join("jingwei.db");
    let conn = rusqlite::Connection::open(&db).expect("开库");
    conn.execute(
        "INSERT INTO meta(key, value) VALUES('parse_rules_version','0') ON CONFLICT(key) DO UPDATE SET value='0'",
        [],
    )
    .expect("改 meta");

    let (c, out) = run(&dir, &["check", "."]);
    assert_ne!(c, 0, "版本不符 check 应红：{out}");
    assert!(out.contains("rules_version_bad=1"), "{out}");

    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("重建：rules"), "要打印版本变化：{out}");
    let (c, out) = run(&dir, &["check", "."]);
    assert_eq!(c, 0, "重建后 check 应变绿：{out}");
}

fn ts_fixture() -> std::path::PathBuf {
    let dir = temp_dir("ts-parse");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/util.ts"), "export function helper(): number { return 1; }\n").expect("写");
    std::fs::write(
        dir.join("src/main.ts"),
        "import { helper } from './util';\nimport * as fs from 'node:fs';\n\ninterface P { x: number }\ntype Q = string;\nclass C {}\nexport function main(): void {}\n",
    )
    .expect("写");
    dir
}

#[test]
fn ts_parse_finds_symbols_and_import_edges() {
    let dir = ts_fixture();
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    let (c, out) = run(&dir, &["parse", "."]);
    assert_eq!(c, 0, "{out}");
    let (_, raw) = run(&dir, &["raw-stats", "."]);
    assert!(raw.contains("interface 1"), "{raw}");
    assert!(raw.contains("type 1"), "{raw}");
    assert!(raw.contains("class 1"), "{raw}");
    let (_, edges) = run(&dir, &["edges", "src/main.ts", "--kind", "import", "."]);
    assert!(edges.contains("ts:src/util.ts"), "相对导入没解析到文件：{edges}");
    assert!(edges.contains("ext:node:fs"), "裸包应标 ext：{edges}");
}

#[test]
fn ts_find_function_has_line() {
    let dir = ts_fixture();
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["find", "main", "--kind", "symbol", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let hits = v["hits"].as_array().expect("hits");
    let h = hits.iter().find(|h| h["name"] == "main").expect("找到 main");
    assert_eq!(h["line"].as_str(), Some("7"), "{out}");
    assert_eq!(h["path"].as_str(), Some("src/main.ts"), "{out}");
}

#[test]
fn ts_coverage_counts_files() {
    let dir = ts_fixture();
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "c1"]);
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["ingest", "--limit", "0", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (_, out) = run(&dir, &["coverage", "--json", "."]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["ts"]["total"].as_i64(), Some(2), "{out}");
    assert_eq!(v["ts"]["parsed"].as_i64(), Some(2), "{out}");
}

#[test]
fn ts_alias_import_keeps_two_edges() {
    let dir = temp_dir("ts-alias");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/util.ts"), "export const A = 1;\n").expect("写");
    std::fs::write(
        dir.join("src/main.ts"),
        "import { A, A as B } from './util';\n\nexport function main(): void {}\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (_, edges) = run(&dir, &["edges", "src/main.ts", "--kind", "import", "."]);
    let count = edges.lines().filter(|l| l.contains("import")).count();
    assert_eq!(count, 2, "同名+别名应是两条边：{edges}");
}

#[test]
fn rust_use_alias_leaf_is_local_binding() {
    let dir = temp_dir("rust-alias");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "use std::fmt as f;\n\npub fn a() {}\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let db = dir.join(".jingwei").join("jingwei.db");
    let conn = rusqlite::Connection::open(&db).expect("开库");
    let leaf: Option<String> = conn
        .query_row(
            "SELECT leaf FROM edge WHERE src='src/a.rs' AND kind='use' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    assert_eq!(leaf.as_deref(), Some("f"), "alias leaf = local binding");
}

/// resolve 后置 pass 会重写 `?use` 行：别名必须活下来（dst 求路径时剥 `as`，leaf 取别名）。
#[test]
fn rust_use_alias_leaf_survives_resolve() {
    let dir = temp_dir("rust-alias-resolve");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/foo.rs"), "pub fn x() {}\n").expect("写");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod foo;\nuse foo::bar::Baz as Q;\nuse foo::bar::{Nope as R, Other as S};\nuse foo::{self as F2, x as y};\nuse foo::bar::Baz as _;\nuse foo::{x as X1, x as X2};\n\npub fn a() {}\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let db = dir.join(".jingwei").join("jingwei.db");
    let conn = rusqlite::Connection::open(&db).expect("开库");
    let leaves = |line: i64| -> Vec<String> {
        let mut st = conn
            .prepare("SELECT COALESCE(leaf,'') FROM edge WHERE kind='use' AND src='src/lib.rs' AND line=?1 ORDER BY leaf")
            .expect("查");
        st.query_map([line], |r| r.get::<_, String>(0))
            .expect("读")
            .map(|x| x.expect("行"))
            .collect()
    };
    assert_eq!(leaves(2), vec!["Q".to_string()], "resolve 后 leaf 应仍是别名");
    assert_eq!(leaves(3), vec!["R".to_string(), "S".to_string()], "组内别名");
    assert_eq!(leaves(4), vec!["F2".to_string(), "y".to_string()], "`self as x` 的 leaf 应是别名");
    assert_eq!(leaves(5), vec!["_".to_string()], "`as _` 的 leaf 是 _");
    assert_eq!(leaves(6), vec!["X1".to_string(), "X2".to_string()], "同行同名多别名不合并");
    // 二次 parse：`?use` 行要原样保留别名，否则 leaf 退回原名（非幂等）
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    assert_eq!(leaves(2), vec!["Q".to_string()], "二次 parse 后 leaf 退化了");
    assert_eq!(leaves(3), vec!["R".to_string(), "S".to_string()], "二次 parse 后 leaf 退化了");
    assert_eq!(leaves(4), vec!["F2".to_string(), "y".to_string()], "二次 parse 后 leaf 退化了");
    assert_eq!(leaves(6), vec!["X1".to_string(), "X2".to_string()], "二次 parse 后 leaf 退化了");
}

/// E1 位置多态：每种 kind 都带可点击位置 loc{file,line}，且不越界。
#[test]
fn edges_expose_loc_for_every_kind() {
    let dir = temp_dir("edges-loc");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src/a.rs"),
        "pub struct S;\nimpl std::fmt::Display for S {\n    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }\n}\n",
    )
    .expect("写");
    std::fs::write(dir.join("src/lib.rs"), "mod a;\nuse crate::a::S;\n\npub fn f() {}\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["edges", "src/lib.rs", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let lines = |p: &str| -> i64 {
        std::fs::read_to_string(dir.join(p)).expect("读").lines().count() as i64
    };
    let mut kinds = Vec::new();
    for e in v["edges"].as_array().expect("edges") {
        let k = e["kind"].as_str().expect("kind");
        kinds.push(k.to_string());
        let loc = &e["loc"];
        let file = loc["file"].as_str().unwrap_or_else(|| panic!("{k} 缺 loc.file: {out}"));
        let line = loc["line"].as_i64().unwrap_or_else(|| panic!("{k} 缺 loc.line: {out}"));
        assert!(dir.join(file).is_file(), "{k} 的 loc.file 不存在：{file}");
        assert!(line >= 1 && line <= lines(file), "{k} 的 loc.line 越界：{file}:{line}");
    }
    assert!(kinds.iter().any(|k| k == "mod"), "mod 边也该有位置：{out}");
    assert!(kinds.iter().any(|k| k == "use"), "use 边：{out}");

    // impl 的 loc 指向声明所在文件（leaf），不是调用方
    let (c, out) = run(&dir, &["edges", "type:S", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let impls: Vec<&serde_json::Value> = v["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .filter(|e| e["kind"] == "impl")
        .collect();
    assert_eq!(impls.len(), 1, "应有 1 条 impl：{out}");
    assert_eq!(impls[0]["loc"]["file"].as_str(), Some("src/a.rs"), "{out}");
    assert_eq!(impls[0]["leaf"].as_str(), Some("src/a.rs"), "{out}");
    let iline = impls[0]["loc"]["line"].as_i64().expect("impl 缺 loc.line");
    assert!(iline >= 1, "impl 行号不许是 0：{out}");
    let src = std::fs::read_to_string(dir.join("src/a.rs")).expect("读");
    let body = src.lines().nth((iline - 1) as usize).unwrap_or("");
    assert!(body.contains("impl"), "impl 的 loc.line 应指向 impl 行：{body}");
}

/// mod 边：子模块目录按 `foo.rs → foo/`；inline `mod x { }` 不产生文件边。
#[test]
fn mod_child_paths_and_inline_mods() {
    let dir = temp_dir("mod-paths");
    std::fs::create_dir_all(dir.join("src/bundle")).expect("建 src/bundle");
    std::fs::write(dir.join("src/bundle/category.rs"), "pub fn c() {}\n").expect("写");
    std::fs::write(dir.join("src/bundle/kmp.rs"), "pub fn k() {}\n").expect("写");
    std::fs::write(
        dir.join("src/bundle.rs"),
        "mod category;\nmod kmp;\n\npub fn b() {}\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod bundle;\n\nmod inline_mod {\n    pub fn x() {}\n}\n\npub fn a() {}\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["edges", "src/bundle.rs", "--kind", "mod", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let dsts: Vec<&str> = v["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .map(|e| e["dst"].as_str().unwrap_or(""))
        .collect();
    assert!(dsts.contains(&"src/bundle/category.rs"), "子模块应在 bundle/ 下：{out}");
    assert!(dsts.contains(&"src/bundle/kmp.rs"), "子模块应在 bundle/ 下：{out}");

    let (c, out) = run(&dir, &["edges", "src/lib.rs", "--kind", "mod", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let dsts: Vec<&str> = v["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .map(|e| e["dst"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(dsts, vec!["src/bundle.rs"], "inline mod 不该有文件边：{out}");

    // 集成测试的 crate 根不是 lib.rs：`tests/tests.rs` 里的 `mod util;` → tests/util.rs
    std::fs::create_dir_all(dir.join("tests")).expect("建 tests");
    std::fs::write(dir.join("tests/util.rs"), "pub fn u() {}\n").expect("写");
    std::fs::write(dir.join("tests/tests.rs"), "mod util;\n\npub fn t() {}\n").expect("写");
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["edges", "tests/tests.rs", "--kind", "mod", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["edges"][0]["dst"].as_str(), Some("tests/util.rs"), "crate 根布局：{out}");

    // 原始标识符：`mod r#type;` → type.rs
    std::fs::write(dir.join("src/type.rs"), "pub fn ty() {}\n").expect("写");
    std::fs::write(dir.join("src/rw.rs"), "mod r#type;\n").expect("写");
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["edges", "src/rw.rs", "--kind", "mod", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["edges"][0]["dst"].as_str(), Some("src/type.rs"), "r# 不是文件名：{out}");
}

/// A：能力闸（precedence）——声明需要符号能力的 judgment 落在不支持的语言上必须被 mask。
#[test]
fn capability_masks_symbol_judgment_on_unsupported_lang() {
    let dir = temp_dir("cap-mask");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.js"), "export const x = 1;\n").expect("写");
    std::fs::write(dir.join("docs.md"), "# t\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["ingest", "--limit", "0", "."]).0, 0);

    // 直接插一条"js 上的 symbol 判断"——正常情况下 parse 不会产生它，这里造违规
    {
        let db = dir.join(".jingwei").join("jingwei.db");
        let conn = rusqlite::Connection::open(&db).expect("开库");
        conn.execute(
            "INSERT INTO judgment(id,subject,verdict,kind,scope,at,coverage_read,coverage_total)\
             VALUES('t-sym','symbol:a.js#f','held','symbol','{\"objects\":[],\"time_window\":null,\"env\":{},\"granularity\":\"unknown\"}',1,1,1)",
            [],
        )
        .expect("插判断");
    }

    // 读时被 mask：不是 no_data，是 no_capability
    let (c, out) = run(&dir, &["ask", "symbol:a.js#f", "."]);
    assert_eq!(c, 2, "{out}");
    assert!(out.contains("no_capability"), "{out}");

    // check：能力核对必须抓到这条
    let (c, out) = run(&dir, &["check", "."]);
    assert_eq!(c, 1, "{out}");
    assert!(out.contains("capability_violation=1"), "{out}");

    // 恢复：删掉违规行 → 能力核对回到 0（直接改库会让 write_log 失衡，故只断言这一项）
    {
        let db = dir.join(".jingwei").join("jingwei.db");
        let conn = rusqlite::Connection::open(&db).expect("开库");
        conn.execute("DELETE FROM judgment WHERE id='t-sym'", []).expect("删");
    }
    let (_, out) = run(&dir, &["check", "."]);
    assert!(out.contains("capability_violation=0"), "{out}");

    // 能力与具体语言无关的判断不许被误杀：doc 是 md（未登记），仍按原样回答
    assert_eq!(run(&dir, &["docs", "."]).0, 0);
    let (c, out) = run(&dir, &["ask", "doc:docs.md", "."]);
    assert_eq!(c, 0, "doc 判断不该被能力闸误杀：{out}");
    assert!(!out.contains("no_capability"), "{out}");
}

/// 能力表的对外契约：`jw capability --json` 必须能读出 js 无符号能力。
#[test]
fn capability_command_lists_languages() {
    let dir = temp_dir("cap-cmd");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    let (c, out) = run(&dir, &["capability", "--json"]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let langs = v["langs"].as_array().expect("langs");
    let find = |ext: &str| -> &serde_json::Value {
        langs
            .iter()
            .find(|l| l["ext"] == ext)
            .unwrap_or_else(|| panic!("能力表缺 {ext}"))
    };
    assert_eq!(find("rs")["symbol"], true);
    assert_eq!(find("ts")["import"], true);
    assert_eq!(find("js")["symbol"], false, "js 不该声明符号能力");
    assert_eq!(find("vue")["symbol"], false);
}

/// serve 协议 v1：主动 hello、未知 feature 显式降级、events_since 重放幂等、ack 裁剪。
#[test]
fn serve_hello() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let dir = temp_dir("serve-hello");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/lib.rs"), "pub fn a() {}\n").expect("写");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(
        Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(),
        Some(0)
    );
    assert_eq!(
        Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(),
        Some(0)
    );

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    let hello: serde_json::Value = serde_json::from_str(&lines.next().expect("hello").expect("read"))
        .expect("hello json");
    assert_eq!(hello["type"].as_str(), Some("hello"), "{hello}");
    assert_eq!(hello["proto"].as_i64(), Some(1), "{hello}");
    assert!(hello["features"].as_array().map(|a| a.len()).unwrap_or(0) > 5, "{hello}");
    assert!(!hello["rules"].as_str().unwrap_or("").is_empty(), "{hello}");

    // 事件帧可能插在响应之间（watcher/写命令）：按 id 读，忽略无 id 的事件
    fn read_id(
        lines: &mut impl Iterator<Item = std::io::Result<String>>,
        id: i64,
    ) -> serde_json::Value {
        loop {
            let line = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&line).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    // 未知 feature 必须显式降级，不静默
    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"hello","features":["hello","nope"]})).expect("write");
    stdin.flush().expect("flush");
    let hs: serde_json::Value = read_id(&mut lines, 1);
    assert_eq!(hs["ok"].as_bool(), Some(true), "{hs}");
    let degraded: Vec<&str> = hs["result"]["degraded"].as_array().expect("degraded")
        .iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(degraded, vec!["nope"], "{hs}");

    // 写一次（refresh）→ 有事件；同一 since 重放必须一致
    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"refresh"})).expect("write");
    stdin.flush().expect("flush");
    let rf: serde_json::Value = read_id(&mut lines, 2);
    assert_eq!(rf["ok"].as_bool(), Some(true), "{rf}");
    for id in [3, 4] {
        writeln!(stdin, "{}", serde_json::json!({"id":id,"method":"events_since","since":0})).expect("write");
        stdin.flush().expect("flush");
    }
    let e1: serde_json::Value = read_id(&mut lines, 3);
    let e2: serde_json::Value = read_id(&mut lines, 4);
    assert_eq!(e1["result"]["events"], e2["result"]["events"], "同 since 重放必须幂等");
    let n = e1["result"]["events"].as_array().map(|a| a.len()).unwrap_or(0);
    assert!(n >= 1, "refresh 后应有事件：{e1}");

    // 6) 未知 method 不崩、回 ok:false；坏 JSON 也不崩，随后 ping 仍可用
    writeln!(stdin, "{}", serde_json::json!({"id":90,"method":"no_such_method"})).expect("write");
    stdin.flush().expect("flush");
    let um: serde_json::Value = read_id(&mut lines, 90);
    assert_eq!(um["ok"].as_bool(), Some(false), "{um}");
    writeln!(stdin, "not-json-at-all").expect("write");
    stdin.flush().expect("flush");
    // 坏 JSON 的回包没有 id：按 ok:false 读，直到读到它
    let bj: serde_json::Value = loop {
        let line = lines.next().expect("bj line").expect("read");
        let v: serde_json::Value = serde_json::from_str(&line).expect("bj json");
        if v["ok"].as_bool() == Some(false) && v.get("error").is_some() {
            break v;
        }
    };
    assert_eq!(bj["ok"].as_bool(), Some(false), "{bj}");
    writeln!(stdin, "{}", serde_json::json!({"id":91,"method":"ping"})).expect("write");
    stdin.flush().expect("flush");
    let pg: serde_json::Value = read_id(&mut lines, 91);
    assert_eq!(pg["ok"].as_bool(), Some(true), "未知/坏帧后进程必须还活着：{pg}");

    // ack 裁剪
    let last = e1["result"]["events"].as_array().and_then(|a| a.last()).and_then(|v| v["seq"].as_i64()).unwrap_or(0);
    writeln!(stdin, "{}", serde_json::json!({"id":5,"method":"ack","seq":last})).expect("write");
    stdin.flush().expect("flush");
    let ack: serde_json::Value = read_id(&mut lines, 5);
    assert!(ack["result"]["dropped"].as_i64().unwrap_or(0) >= 1, "{ack}");
    let _ = child.kill();
}

/// range impact：目录目标的 direct_total 只算"前缀外 → 前缀内"的 call 边；tests 是文件级。
#[test]
fn impact_range_direct_total_and_scope() {
    let dir = temp_dir("impact-range");
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(
        dir.join("pkg/a.rs"),
        "pub fn f1() {\n    f2();\n}\n\npub fn f2() {}\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("o.rs"),
        "pub fn outside() {\n    f1();\n}\n\npub fn other() {\n    outside();\n}\n\n#[test]\nfn to() {\n    f1();\n}\n",
    )
    .expect("写");
    std::fs::write(dir.join("lib.rs"), "pub mod pkg;\npub mod o;\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["impact", "pkg", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["target_kind"].as_str(), Some("range"), "{out}");
    assert_eq!(v["targets"].as_i64(), Some(2), "pkg 下有 2 个 fn：{out}");
    // "指向前缀内"的边全算：outside→f1、to→f1、pkg 内 f1→f2 = 3
    assert_eq!(v["direct_total"].as_i64(), Some(3), "前缀内 + 前缀外指向前缀：{out}");
    for d in v["direct"].as_array().expect("direct") {
        assert!(
            d["callee"].as_str().unwrap_or("").starts_with("symbol:pkg/"),
            "direct 的 dst 必须落在前缀内：{out}"
        );
    }
    let items = v["tests"]["items"].as_array().expect("tests");
    assert!(
        items.iter().any(|x| x["name"] == "to" && x["scope"] == "file"),
        "受影响文件里的测试要带 file 级标注：{out}"
    );
    assert_eq!(v["tests"]["scope"].as_str(), Some("file"), "{out}");
}

/// range card：模块视图的聚合必须等于逐文件求和（files/definitions/tests/call/use/impl）。
#[test]
fn range_card_aggregates_match_per_file() {
    let dir = temp_dir("range-card");
    std::fs::create_dir_all(dir.join("pkg")).expect("建 pkg");
    std::fs::write(
        dir.join("pkg/a.rs"),
        "pub fn a() {\n    b();\n}\n\n#[test]\nfn ta() {\n    a();\n}\n",
    )
    .expect("写");
    std::fs::write(dir.join("pkg/b.rs"), "pub fn b() {}\n").expect("写");
    std::fs::write(dir.join("src.rs"), "").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["card", "pkg", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let mut files = 0i64;
    let mut defs = 0i64;
    let mut tests = 0i64;
    let mut call = 0i64;
    let mut use_in = 0i64;
    let mut impls = 0i64;
    for f in ["pkg/a.rs", "pkg/b.rs"] {
        let (_, fo) = run(&dir, &["card", f, "--json", "."]);
        let fv: serde_json::Value = serde_json::from_str(&fo).expect("json");
        if fv["subject"].as_str().unwrap_or("").starts_with("file:") {
            files += 1;
        }
        defs += fv["symbols"]["definitions"].as_i64().unwrap_or(0);
        tests += fv["tests"]["n"].as_i64().unwrap_or(0);
        use_in += fv["edges"]["use"]["internal"].as_i64().unwrap_or(0);
        impls += fv["edges"]["impl"].as_i64().unwrap_or(0);
        let (_, eo) = run(&dir, &["edges", f, "--kind", "call", "--json", "."]);
        if eo.trim_start().starts_with('{') {
            let ev: serde_json::Value = serde_json::from_str(&eo).expect("json");
            call += ev["edges"].as_array().map(|a| a.len()).unwrap_or(0) as i64;
        }
    }
    assert_eq!(v["files"].as_i64(), Some(files), "files 聚合：{out}");
    assert_eq!(v["symbols"]["definitions"].as_i64(), Some(defs), "definitions 聚合：{out}");
    assert_eq!(v["tests"]["total"].as_i64(), Some(tests), "tests 聚合：{out}");
    assert_eq!(v["call"]["total"].as_i64(), Some(call), "call 聚合：{out}");
    assert_eq!(
        v["edges"]["use"]["internal"].as_i64(),
        Some(use_in),
        "use.internal 聚合：{out}"
    );
    assert_eq!(v["edges"]["impl"].as_i64(), Some(impls), "impl 聚合：{out}");
    assert!(tests >= 1, "fixture 里应有测试：{out}");
    assert!(call >= 1, "fixture 里应有调用：{out}");
}

/// tests-for v0：符号级"哪些测试能静态到达它"；没路径的测试不许出现；文案不许说成覆盖。
#[test]
fn tests_for_static_reachability() {
    let dir = temp_dir("tests-for");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src/a.rs"),
        "pub fn target() {}\n\npub fn helper() {\n    target();\n}\n\npub fn unrelated() {}\n\npub fn c1() {\n    c2();\n}\n\npub fn c2() {\n    c1();\n}\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("src/t.rs"),
        "#[test]\nfn t1() {\n    helper();\n}\n\n#[test]\nfn t2() {\n    unrelated();\n}\n\n#[test]\nfn t3() {\n    c2();\n}\n",
    )
    .expect("写");
    std::fs::write(dir.join("src/lib.rs"), "pub mod a;\npub mod t;\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["tests-for", "symbol:src/a.rs#target", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let names: Vec<&str> = v["tests"]
        .as_array()
        .expect("tests")
        .iter()
        .map(|t| t["name"].as_str().unwrap_or(""))
        .collect();
    assert!(names.contains(&"t1"), "t1 → helper → target（depth 2）：{out}");
    assert!(!names.contains(&"t2"), "无关测试不许出现（文件级不算命中）：{out}");
    assert!(!names.contains(&"t3"), "t3 只到 c2，离 target 无路：{out}");
    assert_eq!(v["note"].as_str(), Some("静态可达，不等于运行覆盖"), "{out}");
    assert_eq!(v["completeness"]["depth_cap"].as_i64(), Some(3), "{out}");
    // 循环：目标 c1 ← c2 ← t3（depth 2），不许死循环
    let (c, out) = run(&dir, &["tests-for", "symbol:src/a.rs#c1", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let names: Vec<&str> = v["tests"].as_array().expect("tests").iter().map(|t| t["name"].as_str().unwrap_or("")).collect();
    assert!(names.contains(&"t3"), "循环里 t3 → c2 → c1（depth 2）：{out}");

    // depth=1 时 t1/t3（depth 2）都不该出现在 cap 内
    let (c, out) = run(&dir, &["tests-for", "symbol:src/a.rs#target", "--depth", "1", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["tests"].as_array().expect("tests").len(), 0, "{out}");
    // 目标本身是测试 → depth 0 命中自己
    let (c, out) = run(&dir, &["tests-for", "symbol:src/t.rs#t1", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let one = &v["tests"].as_array().expect("tests")[0];
    assert_eq!(one["name"].as_str(), Some("t1"), "{out}");
    assert_eq!(one["depth"].as_i64(), Some(0), "{out}");
    // 文案：文本输出必须有"静态可达"
    let (c, out) = run(&dir, &["tests-for", "symbol:src/a.rs#target", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("静态可达，不等于运行覆盖"), "{out}");
    // 目标不存在 → exit 2 + no_symbol
    let (c, out) = run(&dir, &["tests-for", "symbol:src/a.rs#nope", "--json", "."]);
    assert_eq!(c, 2, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["reason"].as_str(), Some("no_symbol"), "{out}");
}

/// M13：真 RA codeAction 只读展示——chips 有、apply 永远 0、降级隐藏。
#[test]
fn lsp_action() {
    let _lsp_guard = lsp_lock();
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn read_id(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: i64) -> serde_json::Value {
        loop {
            let l = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&l).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    let dir = temp_dir("lsp-action");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"ra_action\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("src/main.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\nfn main() {\n    let unused = 1;\n    println!(\"hi\");\n}\n",
    )
    .expect("写");
    let appdata = temp_dir("lsp-action-appdata");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(), Some(0));
    assert_eq!(Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(), Some(0));

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"lsp_actions","path":"src/main.rs","line":4,"col":9,"message":"unused variable","severity":2})).expect("write");
    stdin.flush().expect("flush");
    let no = read_id(&mut lines, 1);
    assert_eq!(no["result"]["source"].as_str(), Some("db"), "{no}");
    assert_eq!((no["result"]["actions"].as_array()).map(|a| a.len()), Some(0), "{no}");

    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"lsp_trust","action":"grant"})).expect("write");
    stdin.flush().expect("flush");
    let _ = read_id(&mut lines, 2);
    writeln!(stdin, "{}", serde_json::json!({"id":3,"method":"lsp_actions","path":"src/main.rs","line":4,"col":9,"message":"unused variable: `unused`","severity":2})).expect("write");
    stdin.flush().expect("flush");
    let r = read_id(&mut lines, 3);
    if r["result"]["skip"].as_str() == Some("no_server") {
        assert_eq!(r["result"]["source"].as_str(), Some("db"), "{r}");
    } else {
        assert_eq!(r["result"]["source"].as_str(), Some("lsp"), "{r}");
        let acts = r["result"]["actions"].as_array().expect("actions");
        assert!(!acts.is_empty(), "RA 应有 quickfix（如 Rename unused）：{r}");
        assert!(acts.iter().all(|a| a["source"].as_str() == Some("lsp")), "{r}");
        assert!(acts.iter().all(|a| !a["title"].as_str().unwrap_or("").is_empty()), "{r}");
        // 只读：即使 action 带 edit，也不允许被应用
        assert_eq!(r["result"]["applied"].as_i64(), Some(0), "只读展示，apply 必须 0：{r}");
    }

    writeln!(stdin, "{}", serde_json::json!({"id":4,"method":"lsp_actions","path":"src/main.rs","line":4,"col":9,"message":"unused","severity":2,"force_down":true})).expect("write");
    stdin.flush().expect("flush");
    let d = read_id(&mut lines, 4);
    assert_eq!(d["result"]["source"].as_str(), Some("db"), "{d}");
    assert_eq!((d["result"]["actions"].as_array()).map(|a| a.len()), Some(0), "{d}");
    let _ = child.kill();

    let chk = Command::new(exe).args(["check", "."]).current_dir(&dir).output().expect("check");
    assert!(String::from_utf8_lossy(&chk.stdout).contains("lsp_in_history=0"));
}

/// M12：真 RA inlayHint——可视区请求、上限 200、降级不猜。
#[test]
fn lsp_hint() {
    let _lsp_guard = lsp_lock();
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn read_id(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: i64) -> serde_json::Value {
        loop {
            let l = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&l).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    let dir = temp_dir("lsp-hint");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"ra_hint\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("src/main.rs"),
        "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 { a + b }\n\nfn main() {\n    let x = add(1, 2);\n    println!(\"{x}\");\n}\n",
    )
    .expect("写");
    let appdata = temp_dir("lsp-hint-appdata");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(), Some(0));
    assert_eq!(Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(), Some(0));

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"lsp_hints","path":"src/main.rs","start":1,"end":40})).expect("write");
    stdin.flush().expect("flush");
    let no = read_id(&mut lines, 1);
    assert_eq!(no["result"]["source"].as_str(), Some("db"), "{no}");
    assert_eq!(no["result"]["skip"].as_str(), Some("untrusted_workspace"), "{no}");
    assert_eq!((no["result"]["hints"].as_array()).map(|a| a.len()), Some(0), "{no}");

    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"lsp_trust","action":"grant"})).expect("write");
    stdin.flush().expect("flush");
    let _ = read_id(&mut lines, 2);
    writeln!(stdin, "{}", serde_json::json!({"id":3,"method":"lsp_hints","path":"src/main.rs","start":1,"end":40})).expect("write");
    stdin.flush().expect("flush");
    let r = read_id(&mut lines, 3);
    if r["result"]["skip"].as_str() == Some("no_server") {
        assert_eq!(r["result"]["source"].as_str(), Some("db"), "{r}");
    } else {
        assert_eq!(r["result"]["source"].as_str(), Some("lsp"), "{r}");
        let hints = r["result"]["hints"].as_array().expect("hints");
        assert!(!hints.is_empty(), "应有类型/参数 hint：{r}");
        assert!(hints.len() <= 200, "单屏上限 200：{r}");
        let h0 = &hints[0];
        assert_eq!(h0["source"].as_str(), Some("lsp"), "{h0}");
        assert!(h0["line"].as_i64().unwrap_or(0) >= 1, "{h0}");
        assert!(h0["label"].as_str().unwrap_or("").len() >= 1, "{h0}");
        // 边界：超出文件末尾的 range 必须被夹住而不是让 RA 报错
        writeln!(stdin, "{}", serde_json::json!({"id":4,"method":"lsp_hints","path":"src/main.rs","start":1,"end":100000})).expect("write");
        stdin.flush().expect("flush");
        let big = read_id(&mut lines, 4);
        assert_eq!(big["result"]["source"].as_str(), Some("lsp"), "越界 range 不能让 RA 报错：{big}");
    }

    writeln!(stdin, "{}", serde_json::json!({"id":5,"method":"lsp_hints","path":"src/main.rs","start":1,"end":40,"force_down":true})).expect("write");
    stdin.flush().expect("flush");
    let d = read_id(&mut lines, 5);
    assert_eq!(d["result"]["source"].as_str(), Some("db"), "{d}");
    assert_eq!(d["result"]["badge"].as_str(), Some("db"), "{d}");
    let _ = child.kill();

    let chk = Command::new(exe).args(["check", "."]).current_dir(&dir).output().expect("check");
    assert!(String::from_utf8_lossy(&chk.stdout).contains("lsp_in_history=0"));
}

/// M11：真 RA references——去重稳定、按文件分组、降级给 coverage、不写历史。
#[test]
fn lsp_ref() {
    let _lsp_guard = lsp_lock();
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn read_id(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: i64) -> serde_json::Value {
        loop {
            let l = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&l).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    let dir = temp_dir("lsp-ref");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"ra_ref\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("src/main.rs"),
        "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 { a + b }\n\nfn main() {\n    let x = add(1, 2);\n    let y = add(3, 4);\n    println!(\"{x}{y}\");\n}\n",
    )
    .expect("写");
    let appdata = temp_dir("lsp-ref-appdata");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(), Some(0));
    assert_eq!(Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(), Some(0));

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"lsp_ref","path":"src/main.rs","line":2,"col":8})).expect("write");
    stdin.flush().expect("flush");
    let no = read_id(&mut lines, 1);
    assert_eq!(no["result"]["source"].as_str(), Some("db"), "{no}");
    assert_eq!(no["result"]["precision"].as_str(), Some("low"), "{no}");
    assert!(no["result"]["coverage"].is_object(), "降级必须给覆盖率：{no}");

    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"lsp_trust","action":"grant"})).expect("write");
    stdin.flush().expect("flush");
    let _ = read_id(&mut lines, 2);
    writeln!(stdin, "{}", serde_json::json!({"id":3,"method":"lsp_ref","path":"src/main.rs","line":2,"col":8})).expect("write");
    stdin.flush().expect("flush");
    let r = read_id(&mut lines, 3);
    if r["result"]["skip"].as_str() == Some("no_server") {
        assert_eq!(r["result"]["source"].as_str(), Some("db"), "{r}");
    } else {
        assert_eq!(r["result"]["source"].as_str(), Some("lsp"), "{r}");
        let targets = r["result"]["targets"].as_array().expect("targets");
        assert!(!targets.is_empty(), "{r}");
        // 声明 + 两次调用 = 3（includeDeclaration 默认 true）
        assert_eq!(targets.len(), 3, "引用条数应与 RA 一致：{r}");
        let mut keys: Vec<String> = targets.iter().map(|t| format!("{}:{}:{}", t["file"].as_str().unwrap_or(""), t["line"], t["col"])).collect();
        let before = keys.clone();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), targets.len(), "引用必须去重：{r}");
        assert_eq!(keys, before, "引用必须稳定排序：{r}");
        assert_eq!(r["result"]["include_declaration"].as_bool(), Some(true), "{r}");
    }

    writeln!(stdin, "{}", serde_json::json!({"id":4,"method":"lsp_ref","path":"src/main.rs","line":2,"col":8,"force_down":true})).expect("write");
    stdin.flush().expect("flush");
    let d = read_id(&mut lines, 4);
    assert_eq!(d["result"]["source"].as_str(), Some("db"), "{d}");
    assert!(d["result"]["coverage"].is_object(), "{d}");
    let _ = child.kill();

    let chk = Command::new(exe).args(["check", "."]).current_dir(&dir).output().expect("check");
    assert!(String::from_utf8_lossy(&chk.stdout).contains("lsp_in_history=0"));
}

/// M10：真 RA definition——单目标/多目标都只算一跳，降级走自研低精度。
#[test]
fn lsp_def() {
    let _lsp_guard = lsp_lock();
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn read_id(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: i64) -> serde_json::Value {
        loop {
            let l = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&l).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    let dir = temp_dir("lsp-def");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"ra_def\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("src/main.rs"),
        "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 { a + b }\n\nfn main() {\n    let x = add(1, 2);\n    println!(\"{x}\");\n}\n",
    )
    .expect("写");
    let appdata = temp_dir("lsp-def-appdata");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(), Some(0));
    assert_eq!(Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(), Some(0));

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    // 未信任：自研兜底且 precision=low
    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"lsp_def","path":"src/main.rs","line":5,"col":13})).expect("write");
    stdin.flush().expect("flush");
    let no = read_id(&mut lines, 1);
    assert_eq!(no["result"]["source"].as_str(), Some("db"), "{no}");
    assert_eq!(no["result"]["precision"].as_str(), Some("low"), "{no}");
    assert_eq!(no["result"]["skip"].as_str(), Some("untrusted_workspace"), "{no}");

    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"lsp_trust","action":"grant"})).expect("write");
    stdin.flush().expect("flush");
    let _ = read_id(&mut lines, 2);
    writeln!(stdin, "{}", serde_json::json!({"id":3,"method":"lsp_def","path":"src/main.rs","line":5,"col":13})).expect("write");
    stdin.flush().expect("flush");
    let d = read_id(&mut lines, 3);
    if d["result"]["skip"].as_str() == Some("no_server") {
        assert_eq!(d["result"]["source"].as_str(), Some("db"), "{d}");
    } else {
        assert_eq!(d["result"]["source"].as_str(), Some("lsp"), "{d}");
        assert_eq!(d["result"]["precision"].as_str(), Some("high"), "{d}");
        let targets = d["result"]["targets"].as_array().expect("targets");
        assert!(!targets.is_empty(), "{d}");
        assert_eq!(targets[0]["file"].as_str(), Some("src/main.rs"), "{d}");
        assert_eq!(targets[0]["line"].as_i64(), Some(2), "定义在 add 那一行：{d}");
        assert_eq!(d["result"]["lsp_line"].as_i64(), Some(4), "UI 5 → LSP 4：{d}");
        assert_eq!(d["result"]["lsp_char"].as_i64(), Some(12), "UI 13 → LSP 12：{d}");
    }

    // force_down：降级且 precision=low，不给"高精度"假象
    writeln!(stdin, "{}", serde_json::json!({"id":4,"method":"lsp_def","path":"src/main.rs","line":5,"col":13,"force_down":true})).expect("write");
    stdin.flush().expect("flush");
    let f = read_id(&mut lines, 4);
    assert_eq!(f["result"]["source"].as_str(), Some("db"), "{f}");
    assert_eq!(f["result"]["precision"].as_str(), Some("low"), "{f}");
    let _ = child.kill();

    let chk = Command::new(exe).args(["check", "."]).current_dir(&dir).output().expect("check");
    assert!(String::from_utf8_lossy(&chk.stdout).contains("lsp_in_history=0"));
}

/// M9：真 RA hover——类型/文档、位置双映射、降级不猜。
#[test]
fn lsp_hover() {
    let _lsp_guard = lsp_lock();
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn read_id(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: i64) -> serde_json::Value {
        loop {
            let l = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&l).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    let dir = temp_dir("lsp-hover");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"ra_hover\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("src/main.rs"),
        "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 { a + b }\n\nfn main() {\n    let x = add(1, 2);\n    println!(\"{x}\");\n}\n",
    )
    .expect("写");
    let appdata = temp_dir("lsp-hover-appdata");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(), Some(0));
    assert_eq!(Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(), Some(0));

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    // 未信任：显式"类型未知"，不是猜一个
    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"lsp_hover","path":"src/main.rs","line":5,"col":9})).expect("write");
    stdin.flush().expect("flush");
    let no = read_id(&mut lines, 1);
    assert_eq!(no["result"]["type"], serde_json::Value::Null, "{no}");
    assert_eq!(no["result"]["skip"].as_str(), Some("untrusted_workspace"), "{no}");

    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"lsp_trust","action":"grant"})).expect("write");
    stdin.flush().expect("flush");
    let _ = read_id(&mut lines, 2);
    writeln!(stdin, "{}", serde_json::json!({"id":3,"method":"lsp_hover","path":"src/main.rs","line":5,"col":9})).expect("write");
    stdin.flush().expect("flush");
    let h = read_id(&mut lines, 3);
    if h["result"]["skip"].as_str() == Some("no_server") {
        // 机器上没有 RA：允许 skip，但不能是"假类型"
        assert_eq!(h["result"]["type"], serde_json::Value::Null, "{h}");
    } else {
        let ty = h["result"]["type"].as_str().unwrap_or("");
        assert!(ty.contains("i32"), "hover 类型应含 i32：{h}");
        assert_eq!(h["result"]["source"].as_str(), Some("lsp"), "{h}");
        assert_eq!(h["result"]["lsp_line"].as_i64(), Some(4), "UI 5 → LSP 4：{h}");
        assert_eq!(h["result"]["lsp_char"].as_i64(), Some(8), "UI 9 → LSP 8：{h}");
        assert_eq!(h["result"]["capabilities_used"].as_str(), Some("hover"), "{h}");
    }

    // force_down：降级且不猜
    writeln!(stdin, "{}", serde_json::json!({"id":4,"method":"lsp_hover","path":"src/main.rs","line":5,"col":9,"force_down":true})).expect("write");
    stdin.flush().expect("flush");
    let d = read_id(&mut lines, 4);
    assert_eq!(d["result"]["type"], serde_json::Value::Null, "{d}");
    assert_eq!(d["result"]["skip"].as_str(), Some("server_down"), "{d}");
    let _ = child.kill();

    let chk = Command::new(exe).args(["check", "."]).current_dir(&dir).output().expect("check");
    assert!(String::from_utf8_lossy(&chk.stdout).contains("lsp_in_history=0"));
}

/// M8：真 rust-analyzer（只诊断）——无 RA 时显式 skip，不冒充"无诊断"。
#[test]
fn lsp_real() {
    let _lsp_guard = lsp_lock();
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn read_id(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: i64) -> serde_json::Value {
        loop {
            let l = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&l).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    let dir = temp_dir("lsp-real");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"ra_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("写 Cargo.toml");
    std::fs::write(dir.join("src/main.rs"), "fn main() {\n    let x: i32 = ;\n}\n").expect("写");
    let appdata = temp_dir("lsp-real-appdata");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(), Some(0));
    assert_eq!(Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(), Some(0));

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    // 先确认发现结果：没有 RA 的机器必须显式 skip（不是 0 诊断）
    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"lsp_ra_info"})).expect("write");
    stdin.flush().expect("flush");
    let info = read_id(&mut lines, 1);
    let found = info["result"]["found"].as_bool().unwrap_or(false);
    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"lsp_trust","action":"grant"})).expect("write");
    stdin.flush().expect("flush");
    let _ = read_id(&mut lines, 2);
    writeln!(stdin, "{}", serde_json::json!({"id":3,"method":"lsp_real","path":"src/main.rs"})).expect("write");
    stdin.flush().expect("flush");
    let r = read_id(&mut lines, 3);
    if found {
        assert_eq!(r["ok"].as_bool(), Some(true), "{r}");
        assert_eq!(r["result"]["source"].as_str(), Some("lsp"), "{r}");
        let items = r["result"]["items"].as_array().expect("items");
        assert!(!items.is_empty(), "RA 存在时应 >=1 条诊断：{r}");
        assert_eq!(items[0]["source"].as_str(), Some("lsp"), "{r}");
        assert_eq!(r["result"]["degraded"].as_bool(), Some(false), "{r}");
    } else {
        assert_eq!(r["result"]["skip"].as_str(), Some("no_server"), "无 RA 必须显式 skip：{r}");
    }
    let _ = child.kill();

    let chk = Command::new(exe).args(["check", "."]).current_dir(&dir).output().expect("check");
    assert!(String::from_utf8_lossy(&chk.stdout).contains("lsp_in_history=0"));
}

/// M7：fixture LSP 贯通诊断——push/pull 映射、降级标 db、历史链 0。
#[test]
fn lsp_diag() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn read_id(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: i64) -> serde_json::Value {
        loop {
            let l = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&l).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    let dir = temp_dir("lsp-diag");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/lib.rs"), "pub fn a() {}\n").expect("写");
    std::fs::write(
        dir.join("diagnostics.json"),
        r#"{"diagnostics":{"src/lib.rs":[{"line":1,"severity":"error","message":"db fallback"}]}}"#,
    )
    .expect("写 fixture");
    let appdata = temp_dir("lsp-diag-appdata");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(), Some(0));
    assert_eq!(Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(), Some(0));

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    // 未信任：不 spawn、不产出诊断
    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"lsp_diagnostics","path":"src/lib.rs"})).expect("write");
    stdin.flush().expect("flush");
    let no = read_id(&mut lines, 1);
    assert_eq!(no["ok"].as_bool(), Some(false), "{no}");
    assert_eq!(no["result"]["reason"].as_str(), Some("untrusted_workspace"), "{no}");

    // grant → push：3 条、来源 lsp、字段映射齐
    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"lsp_trust","action":"grant"})).expect("write");
    stdin.flush().expect("flush");
    let _ = read_id(&mut lines, 2);
    writeln!(stdin, "{}", serde_json::json!({"id":3,"method":"lsp_diagnostics","path":"src/lib.rs","mode":"push"})).expect("write");
    stdin.flush().expect("flush");
    let push = read_id(&mut lines, 3);
    let items = push["result"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 3, "{push}");
    assert_eq!(push["result"]["source"].as_str(), Some("lsp"), "{push}");
    let first = &items[0];
    assert_eq!(first["source"].as_str(), Some("lsp"), "{first}");
    assert_eq!(first["line"].as_i64(), Some(1), "{first}");
    assert_eq!(first["col"].as_i64(), Some(1), "{first}");
    assert_eq!(first["severity"].as_str(), Some("error"), "{first}");

    // pull：同样 3 条
    writeln!(stdin, "{}", serde_json::json!({"id":4,"method":"lsp_diagnostics","path":"src/lib.rs","mode":"pull"})).expect("write");
    stdin.flush().expect("flush");
    let pull = read_id(&mut lines, 4);
    assert_eq!(pull["result"]["items"].as_array().map(|a| a.len()), Some(3), "{pull}");

    // 降级：source=db、degraded=true、拿到 db fixture
    writeln!(stdin, "{}", serde_json::json!({"id":5,"method":"lsp_diagnostics","path":"src/lib.rs","force_down":true})).expect("write");
    stdin.flush().expect("flush");
    let deg = read_id(&mut lines, 5);
    assert_eq!(deg["result"]["degraded"].as_bool(), Some(true), "{deg}");
    assert_eq!(deg["result"]["source"].as_str(), Some("db"), "{deg}");
    assert_eq!(deg["result"]["items"].as_array().map(|a| a.len()), Some(1), "{deg}");
    let _ = child.kill();

    let chk = Command::new(exe).args(["check", "."]).current_dir(&dir).output().expect("check");
    let out = String::from_utf8_lossy(&chk.stdout);
    assert!(out.contains("lsp_in_history=0"), "历史链被污染：{out}");
}

/// M6：LSP 信任门——未信任 0 spawn、grant 后 spawn（fixture server）、revoke 后拒绝、历史链 0。
#[test]
fn lsp_gate() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn read_id(lines: &mut impl Iterator<Item = std::io::Result<String>>, id: i64) -> serde_json::Value {
        loop {
            let l = lines.next().expect("line").expect("read");
            let v: serde_json::Value = serde_json::from_str(&l).expect("json");
            if v["id"].as_i64() == Some(id) {
                return v;
            }
        }
    }

    let dir = temp_dir("lsp-gate");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/lib.rs"), "pub fn a() {}\n").expect("写");
    let appdata = temp_dir("lsp-appdata");
    let exe = env!("CARGO_BIN_EXE_jw");
    assert_eq!(Command::new(exe).args(["init", "."]).current_dir(&dir).status().expect("init").code(), Some(0));
    assert_eq!(Command::new(exe).args(["parse", "."]).current_dir(&dir).status().expect("parse").code(), Some(0));

    let mut child = Command::new(exe)
        .args(["serve", "."])
        .current_dir(&dir)
        .env("APPDATA", &appdata)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    // 未信任：0 spawn + 明确 reason
    writeln!(stdin, "{}", serde_json::json!({"id":1,"method":"lsp_spawn"})).expect("write");
    stdin.flush().expect("flush");
    let no = read_id(&mut lines, 1);
    assert_eq!(no["ok"].as_bool(), Some(false), "{no}");
    assert_eq!(no["result"]["reason"].as_str(), Some("untrusted_workspace"), "{no}");
    assert_eq!(no["result"]["spawned"].as_bool(), Some(false), "{no}");

    // grant → spawn fixture（会写 marker）
    writeln!(stdin, "{}", serde_json::json!({"id":2,"method":"lsp_trust","action":"grant"})).expect("write");
    stdin.flush().expect("flush");
    let g = read_id(&mut lines, 2);
    assert_eq!(g["result"]["trusted"].as_bool(), Some(true), "{g}");
    writeln!(stdin, "{}", serde_json::json!({"id":3,"method":"lsp_spawn"})).expect("write");
    stdin.flush().expect("flush");
    let sp = read_id(&mut lines, 3);
    assert_eq!(sp["ok"].as_bool(), Some(true), "{sp}");
    assert_eq!(sp["result"]["source"].as_str(), Some("lsp"), "{sp}");
    assert_eq!(sp["result"]["spawned"].as_bool(), Some(true), "{sp}");
    assert!(dir.join(".jingwei/lsp-marker").is_file(), "fixture 没被真的 spawn：{sp}");

    // revoke → 又拒绝
    writeln!(stdin, "{}", serde_json::json!({"id":4,"method":"lsp_trust","action":"revoke"})).expect("write");
    stdin.flush().expect("flush");
    let rv = read_id(&mut lines, 4);
    assert_eq!(rv["result"]["trusted"].as_bool(), Some(false), "{rv}");
    writeln!(stdin, "{}", serde_json::json!({"id":5,"method":"lsp_spawn"})).expect("write");
    stdin.flush().expect("flush");
    let no2 = read_id(&mut lines, 5);
    assert_eq!(no2["ok"].as_bool(), Some(false), "{no2}");
    let _ = child.kill();

    // 历史链必须 0
    let chk = Command::new(exe).args(["check", "."]).current_dir(&dir).output().expect("check");
    let out = String::from_utf8_lossy(&chk.stdout);
    assert!(out.contains("lsp_in_history=0"), "check 缺第 12 项或不为 0：{out}");
}

/// impact v0：反向 call 闭包（depth≤2）、循环不爆、测试只标 file 级。
#[test]
fn impact_reverse_closure_and_tests() {
    let dir = temp_dir("impact");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn a() {\n    b();\n}\n").expect("写");
    std::fs::write(dir.join("src/b.rs"), "pub fn b() {\n    c();\n}\n\n#[test]\nfn tb() { b(); }\n").expect("写");
    std::fs::write(dir.join("src/c.rs"), "pub fn c() {\n    c();\n}\n").expect("写");
    std::fs::write(dir.join("src/d.rs"), "pub fn lonely() {}\n").expect("写");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod a;\npub mod b;\npub mod c;\npub mod d;\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["impact", "symbol:src/c.rs#c", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let direct: Vec<&str> = v["direct"]
        .as_array()
        .expect("direct")
        .iter()
        .map(|r| r["from"].as_str().unwrap_or(""))
        .collect();
    assert!(direct.contains(&"symbol:src/b.rs#b"), "b 直接调 c：{out}");
    assert!(direct.contains(&"symbol:src/c.rs#c"), "c 自递归：{out}");
    let trans: Vec<&str> = v["transitive"]
        .as_array()
        .expect("transitive")
        .iter()
        .map(|r| r["from"].as_str().unwrap_or(""))
        .collect();
    assert!(trans.contains(&"symbol:src/a.rs#a"), "a 经 b 间接调 c（depth 2）：{out}");
    assert!(!trans.contains(&"symbol:src/c.rs#c"), "循环不许把目标自己算进传递：{out}");
    // 测试：b.rs 里的 tb 属于受影响文件，且标 scope=file
    let items = v["tests"]["items"].as_array().expect("tests");
    assert!(
        items.iter().any(|t| t["name"] == "tb" && t["scope"] == "file"),
        "受影响文件里的测试要带 file 级标注：{out}"
    );
    assert_eq!(v["tests"]["scope"].as_str(), Some("file"), "{out}");
    assert_eq!(v["completeness"]["depth_cap"].as_i64(), Some(2), "{out}");

    // 无调用者：direct/transitive 都空，但命令成功（不是错误）
    let (c, out) = run(&dir, &["impact", "symbol:src/d.rs#lonely", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["direct"].as_array().expect("direct").len(), 0, "{out}");
    // 查不到目标 → exit 2 + reason
    let (c, out) = run(&dir, &["impact", "symbol:src/d.rs#nope", "--json", "."]);
    assert_eq!(c, 2, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["reason"].as_str(), Some("no_symbol"), "{out}");
}

/// call v1.5：`use` 绑定优先于"同名唯一"；std 绑定必须保持 `?call`（不误导）。
#[test]
fn call_scope_binding_beats_name_match() {
    let dir = temp_dir("call-binding");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src/a.rs"),
        "use std::fs::read;\nuse crate::p::helper3;\n\npub fn caller() {\n    read();\n    helper3();\n}\n",
    )
    .expect("写");
    std::fs::write(dir.join("src/b.rs"), "pub fn read() {}\n").expect("写");
    std::fs::write(dir.join("src/p.rs"), "pub fn helper3() {}\n").expect("写");
    std::fs::write(dir.join("src/q.rs"), "pub fn helper3() {}\n").expect("写");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod a;\npub mod b;\npub mod p;\npub mod q;\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["edges", "src/a.rs", "--kind", "call", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let rows = v["edges"].as_array().expect("edges");
    let dst_of = |name: &str| -> String {
        rows.iter()
            .find(|e| e["leaf"] == name)
            .and_then(|e| e["dst"].as_str())
            .unwrap_or("")
            .to_string()
    };
    assert_eq!(
        dst_of("read"),
        "?call read",
        "std 绑定（std::fs::read）不许解析到本地同名 fn：{out}"
    );
    assert_eq!(
        dst_of("helper3"),
        "symbol:src/p.rs#helper3",
        "内部绑定优先（p.rs），不是唯一名：{out}"
    );
}

/// 跨文件 call v1：全仓 fn 定义唯一才解析；多义保持 `?call`（不猜）。
#[test]
fn call_cross_file_unique_only() {
    let dir = temp_dir("call-cross");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/a.rs"), "pub fn caller() {\n    helper();\n    dupe();\n}\n").expect("写");
    std::fs::write(dir.join("src/b.rs"), "pub fn helper() {}\n").expect("写");
    std::fs::write(dir.join("src/c.rs"), "pub fn dupe() {}\n").expect("写");
    std::fs::write(dir.join("src/d.rs"), "pub fn dupe() {}\n").expect("写");
    std::fs::write(dir.join("src/lib.rs"), "pub mod a;\npub mod b;\npub mod c;\npub mod d;\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["edges", "src/a.rs", "--kind", "call", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let rows = v["edges"].as_array().expect("edges");
    let dsts: Vec<&str> = rows.iter().map(|e| e["dst"].as_str().unwrap_or("")).collect();
    assert!(
        dsts.contains(&"symbol:src/b.rs#helper"),
        "唯一同名定义应跨文件解析：{out}"
    );
    assert!(
        dsts.contains(&"?call dupe"),
        "多义（c.rs/d.rs 都有 dupe）必须保持 ?call：{out}"
    );
}

/// v0 调用图：同文件裸标识符调用才有 call 边；方法调用/未定义 callee 各有说法。
#[test]
fn call_edges_same_file_only() {
    let dir = temp_dir("call-edges");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn a() {\n    b();\n    b();\n    c();\n    unknown_fn();\n    let x = 1; let _ = x.to_string();\n}\n\npub fn b() {\n    a();\n    if true { c(); }\n}\n\npub fn c() {\n    c();\n}\n\n#[test]\nfn t() {\n    a();\n    b();\n    c();\n}\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["edges", "src/lib.rs", "--kind", "call", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let rows = v["edges"].as_array().expect("edges");
    assert_eq!(rows.len(), 10, "手工期望表 10 条 call：{out}");
    let unresolved = rows
        .iter()
        .filter(|e| e["dst"].as_str().unwrap_or("").starts_with("?call "))
        .count();
    assert_eq!(unresolved, 1, "unknown_fn 归 ?call：{out}");
    // 方法调用 `x.to_string()` 不是裸标识符调用，不许入图
    assert!(
        !rows.iter().any(|e| e["leaf"] == "to_string"),
        "方法调用不该入 v0 调用图：{out}"
    );
    // 每条 call 的行确实含 `callee(`
    let body = std::fs::read_to_string(dir.join("src/lib.rs")).expect("读");
    for e in rows {
        let line = e["loc"]["line"].as_i64().expect("loc.line") as usize;
        let name = e["leaf"].as_str().expect("leaf");
        let src_line = body.lines().nth(line - 1).unwrap_or("");
        assert!(src_line.contains(&format!("{name}(")), "{name} 不在第 {line} 行：{src_line}");
        // caller 必须暴露（学习"谁调用了它"、项目"改 X 波及谁"）
        assert!(
            e["from"].as_str().unwrap_or("").starts_with("symbol:src/lib.rs#"),
            "call 行要带 from（caller）：{e}"
        );
    }

    // 反查：c 被 a/b/c/t 各调一次 → 4 条
    let (c, out) = run(
        &dir,
        &["edges", "src/lib.rs", "--kind", "call", "--to", "symbol:src/lib.rs#c", "--json", "."],
    );
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["total"].as_i64(), Some(4), "incoming calls：{out}");
}

/// 研究引用：cite 给的行/提交必须等于 git blame（permalink 语义），且幂等、查不到不造 commit。
#[test]
fn cite_gives_permalink_commit() {
    let dir = temp_dir("cite");
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/lib.rs"), "pub struct Foo;\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c1"]);
    let c1 = String::from_utf8(
        Command::new("git")
            .args(["-C", dir.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("utf8")
    .trim()
    .to_string();
    // 第二个提交：改文件尾部，Foo 那一行仍属 c1
    std::fs::write(dir.join("src/lib.rs"), "pub struct Foo;\n\npub fn bar() {}\n").expect("写");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "c2"]);
    let c2 = String::from_utf8(
        Command::new("git")
            .args(["-C", dir.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("utf8")
    .trim()
    .to_string();

    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["cite", "symbol:src/lib.rs#Foo", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let row = &v["rows"][0];
    assert_eq!(row["line"].as_i64(), Some(1), "{out}");
    assert_eq!(row["commit"].as_str(), Some(c1.as_str()), "commit 必须是 blame 结果：{out}");
    assert_eq!(row["verdict"].as_str(), Some("held"), "{out}");
    assert_eq!(row["evidence_kind"].as_str(), Some("tree-sitter"), "{out}");
    assert_eq!(row["file_commit"].as_str(), Some(c2.as_str()), "file_commit 是文件最后一次变更：{out}");

    // 幂等：连跑两次逐字节相同
    let (_, out2) = run(&dir, &["cite", "symbol:src/lib.rs#Foo", "--json", "."]);
    assert_eq!(out, out2, "cite 必须幂等");

    // 文件级引用：commit = 文件最后一次变更（不是第 1 行的许可证行）
    let (c, out) = run(&dir, &["cite", "file:src/lib.rs", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["rows"][0]["commit"].as_str(), Some(c2.as_str()), "{out}");
    assert_eq!(v["rows"][0]["line"].as_i64(), Some(1), "{out}");

    // 查不到 → exit 2 + reason，不许造 commit
    let (c, out) = run(&dir, &["cite", "symbol:src/lib.rs#Nope", "--json", "."]);
    assert_eq!(c, 2, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["reason"].as_str(), Some("no_line"), "{out}");
    assert!(v["rows"].is_null(), "失败不许给 rows：{out}");
}

/// 测试映射：`#[test]` / `#[tokio::test]` 各一条 test 边（一个测试函数一条，不是按调用数）。
#[test]
fn test_edges_map_test_functions() {
    let dir = temp_dir("test-edges");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[test]\nfn adds() { assert_eq!(add(1, 2), 3); }\n\n#[tokio::test]\nasync fn async_adds() { let _ = add(1, 2); }\n\n#[test]\n// 属性与 fn 之间夹注释（注释是 trivia，仍算同一个属性链）\nfn commented() {}\n\n#[nu_test_support::test]\nfn macro_style() {}\n\n#[rstest]\nfn rstest_style() {}\n",
    )
    .expect("写");
    std::fs::write(dir.join("src/plain.rs"), "pub fn g() {}\n").expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);

    let (c, out) = run(&dir, &["card", "src/lib.rs", "--json", "."]);
    assert!(c == 0 || c == 2, "没 ingest 时 card 的 exit 可以是 2：{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["tests"]["n"].as_i64(), Some(5), "五个测试函数 → 四条 test 边：{out}");
    assert_eq!(v["tests"]["macro_n"].as_i64(), Some(2), "rstest / *::test 归 macro：{out}");
    let names: Vec<&str> = v["tests"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|t| t["name"].as_str().unwrap_or(""))
        .collect();
    for want in ["adds", "async_adds", "commented", "macro_style", "rstest_style"] {
        assert!(names.contains(&want), "缺 {want}：{out}");
    }

    // 非测试文件 0 条；文本卡也要有"测试 2"
    let (_, out) = run(&dir, &["card", "src/plain.rs", "--json", "."]);
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["tests"]["n"].as_i64(), Some(0), "plain.rs 没有测试：{out}");
    let (_, out) = run(&dir, &["card", "src/lib.rs", "."]);
    assert!(out.contains("测试 5"), "{out}");

    // 全库汇总（B 对总量用）
    let (c, out) = run(&dir, &["coverage", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["tests"]["total"].as_i64(), Some(5), "{out}");
    assert_eq!(v["tests"]["standard"].as_i64(), Some(3), "{out}");
    assert_eq!(v["tests"]["macro"].as_i64(), Some(2), "{out}");
}

/// `#[path = "..."] mod x;`：目标由属性给出（相对声明处目录），不许留 `?x`。
#[test]
fn path_attr_mod_target_is_resolved() {
    let dir = temp_dir("path-attr");
    std::fs::create_dir_all(dir.join("src/tls")).expect("建 src/tls");
    std::fs::create_dir_all(dir.join("src/extra")).expect("建 src/extra");
    std::fs::write(dir.join("src/extra/theme.rs"), "pub fn t() {}\n").expect("写");
    std::fs::write(dir.join("src/tls/impl_native.rs"), "pub fn n() {}\n").expect("写");
    std::fs::write(
        dir.join("src/tls/mod.rs"),
        "#[path = \"impl_native.rs\"]\nmod impl_tls;\n\npub fn f() {}\n",
    )
    .expect("写");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod tls;\n\n#[path = \"extra/theme.rs\"]\nmod theme;\n\npub fn g() {}\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    for (file, want) in [
        ("src/tls/mod.rs", "src/tls/impl_native.rs"),
        ("src/lib.rs", "src/extra/theme.rs"),
    ] {
        let (c, out) = run(&dir, &["edges", file, "--kind", "mod", "--json", "."]);
        assert_eq!(c, 0, "{out}");
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        let dsts: Vec<&str> = v["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .map(|e| e["dst"].as_str().unwrap_or(""))
            .collect();
        assert!(dsts.contains(&want), "{file} 应解析到 {want}：{out}");
        assert!(!dsts.iter().any(|d| d.starts_with('?')), "不该留 ?：{out}");
    }
}

/// `use crate as X;` 指向最近 Cargo.toml 的 crate 根文件（src/lib.rs 或 src/main.rs）。
#[test]
fn crate_alias_resolves_to_crate_root_file() {
    let dir = temp_dir("crate-alias");
    std::fs::create_dir_all(dir.join("src/sub")).expect("建 src");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("写");
    std::fs::write(dir.join("src/lib.rs"), "pub mod sub;\n").expect("写");
    std::fs::write(
        dir.join("src/sub/mod.rs"),
        "use crate as me;\n\npub fn f() {}\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["edges", "src/sub/mod.rs", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let use_dsts: Vec<&str> = v["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .filter(|e| e["kind"] == "use")
        .map(|e| e["dst"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(use_dsts, vec!["src/lib.rs"], "crate 别名应指向 crate 根：{out}");
    // 原因码台账里不该再出现 crate_root_alias
    let (c, out) = run(&dir, &["coverage", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let reasons = &v["unresolved_use"]["reasons"];
    assert!(
        reasons.get("crate_root_alias").is_none(),
        "crate 别名应已解析：{out}"
    );
}

/// `use ::win::X;`（extern prelude 绝对路径）必须标 ext，不许当内部模块留 `?`。
#[test]
fn absolute_extern_use_is_external() {
    let dir = temp_dir("extern-abs");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/win.rs"), "pub struct W;\n").expect("写");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod win;\n\nuse ::windows::Win32::X;\nuse win::W;\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["edges", "src/lib.rs", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let dsts: Vec<&str> = v["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .filter(|e| e["kind"] == "use")
        .map(|e| e["dst"].as_str().unwrap_or(""))
        .collect();
    assert!(dsts.contains(&"ext:windows::Win32::X"), "应标 ext：{out}");
    assert!(dsts.contains(&"src/win.rs"), "本地 use 仍要解析到文件：{out}");
    assert!(!dsts.iter().any(|d| d.starts_with("?use ::")), "不许把 :: 当内部：{out}");
}

/// F′：`?use` 必须有原因码，`sum(reasons)==|?use|` 且 `other=0`。
#[test]
fn unresolved_use_reasons_are_closed_set() {
    let dir = temp_dir("gap-reasons");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/foo.rs"), "pub fn f() {}\n").expect("写");
    // 造三类 ?use：glob / crate 内 item 缺失 / 跨 crate 同名模块（foo 是本仓模块名）
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod foo;\n\nuse foo::*;\nuse crate::foo::nope::X;\nuse foo::deep::Y;\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["coverage", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let u = &v["unresolved_use"];
    let total = u["total"].as_i64().expect("total");
    let reasons = u["reasons"].as_object().expect("reasons");
    let sum: i64 = reasons.values().map(|n| n.as_i64().unwrap_or(0)).sum();
    assert_eq!(sum, total, "每条 ?use 恰一个码：{out}");
    assert!(total >= 3, "至少三类样例：{out}");
    assert_eq!(
        reasons.get("other").map(|n| n.as_i64().unwrap_or(0)).unwrap_or(0),
        0,
        "不许有未归类：{out}"
    );
    assert!(reasons.get("glob").is_some_and(|n| n.as_i64().unwrap_or(0) >= 1), "{out}");
    assert!(
        reasons.get("crate_item_missing").is_some_and(|n| n.as_i64().unwrap_or(0) >= 1),
        "{out}"
    );

    // edges --json 也要带原因码（同一份函数）
    let (c, out) = run(&dir, &["edges", "src/lib.rs", "--json", "."]);
    assert_eq!(c, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert!(v["unresolved_reasons"].is_object(), "edges 缺原因码：{out}");
}

/// `pub use` / `pub(crate) use` 的可见性不是路径：dst 不许写成 `ext:pub use ...`。
#[test]
fn pub_use_resolves_like_plain_use() {
    let dir = temp_dir("pub-use");
    std::fs::create_dir_all(dir.join("src")).expect("建 src");
    std::fs::write(dir.join("src/foo.rs"), "pub fn x() {}\n").expect("写");
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub mod foo;\npub use crate::foo::x;\npub(crate) use std::fmt;\n\npub fn a() {}\n",
    )
    .expect("写");
    assert_eq!(run(&dir, &["init", "."]).0, 0);
    assert_eq!(run(&dir, &["parse", "."]).0, 0);
    let (c, out) = run(&dir, &["edges", "src/lib.rs", "."]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("use   src/foo.rs"), "pub use 应解析到文件：{out}");
    assert!(out.contains("use   ext:std::fmt"), "pub(crate) use std 应标 ext：{out}");
    assert!(!out.contains("pub use"), "dst 里不该出现可见性前缀：{out}");
}

