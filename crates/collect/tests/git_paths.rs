//! git 采集的路径编码测试：中文路径必须是真 UTF-8，不是 `\ooo` 转义。
//!
//! 背景（.pro/out/e0b_facts.txt §）：不开 `core.quotepath=false` 时
//! `git log --name-status` 会把非 ASCII 路径写成 `"\344\270\255..."`，
//! 于是 subject 变成 `file:"\344..."`，`ask file:docs/MANIFEST-摘要.md` 查不到。

use std::path::PathBuf;
use std::process::Command;

fn git(dir: &PathBuf, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("跑不起 git");
    assert!(
        out.status.success(),
        "git {args:?} 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn git_log_keeps_utf8_paths() {
    let dir = std::env::temp_dir().join(format!("jw-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时仓库");

    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.email", "t@example.com"]);
    git(&dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("中文.md"), "x").expect("写文件");
    git(&dir, &["add", "."]);
    git(
        &dir,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "add 中文"],
    );

    let commits = collect::git::log(&dir, 10).expect("log");
    assert_eq!(commits.len(), 1, "commits={commits:?}");

    let paths: Vec<&str> = commits[0].files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.iter().any(|p| p.contains("中文")),
        "路径里没有中文: {paths:?}"
    );
    assert!(
        paths.iter().all(|p| !p.contains('"') && !p.contains("\\3")),
        "路径被转义了: {paths:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
