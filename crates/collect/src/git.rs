use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct FileChange {
    pub status: String,
    pub path: String,
    /// rename/copy 的源路径（R/C 状态才有）。
    pub old_path: Option<String>,
    /// 新内容的 blob id（内容身份的来源）；删除时为空。
    pub blob: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub at: i64,
    pub subject: String,
    pub files: Vec<FileChange>,
}

fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("spawn git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn is_repo(root: &Path) -> bool {
    run(root, &["rev-parse", "--git-dir"]).is_ok()
}

pub fn is_shallow(root: &Path) -> bool {
    run(root, &["rev-parse", "--is-shallow-repository"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// 仓库总提交数（用来判断我们扫全了没有）。
pub fn total_commits(root: &Path) -> Option<usize> {
    run(root, &["rev-list", "--count", "HEAD"])
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
}



/// HEAD 树里的 gitlink（模式 160000 = submodule 提交）数量。
pub fn ls_tree_gitlinks(root: &Path) -> usize {
    let Ok(out) = Command::new("git")
        .args(["ls-tree", "-r", "HEAD"])
        .current_dir(root)
        .output()
    else {
        return 0;
    };
    if !out.status.success() {
        return 0;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("160000 "))
        .count()
}

/// HEAD 树里的文件条目数（git ls-tree -r --name-only；gitlink 也会出现）。
pub fn ls_tree_files(root: &Path) -> Option<usize> {
    let out = Command::new("git")
        .args(["ls-tree", "-r", "HEAD", "--name-only"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.lines().filter(|l| !l.trim().is_empty()).count())
}

/// 读提交历史：hash / author / 时间 / 标题 / 改动文件。
///
/// `--raw -M -m`：`--raw` 拿 blob 与 rename 源；`-m` 展开 merge（每个父一次），
/// 同一 merge 会重复出现 → 按 hash 合并、按 path 去重，只留第一次的状态。
pub fn log(root: &Path, limit: usize) -> Result<Vec<Commit>, String> {
    let n = limit.to_string();
    // 流式读：大仓库 `git log --raw` 的输出是 GB 级，整段读进 String 会直接吃光内存
    let mut child = Command::new("git")
        .args([
            "-c",
            "core.quotepath=false",
            "log",
            "-n",
            &n,
            "-M",
            "-m",
            "--no-abbrev",
            "--pretty=format:%H%x1f%an%x1f%at%x1f%s",
            "--raw",
        ])
        .current_dir(root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| "no stdout".to_string())?;

    let mut commits: Vec<Commit> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut pending: Vec<FileChange> = Vec::new();
    let mut cur: Option<String> = None;
    // 每个提交已见过的 path（去重用，避免 O(n²) 扫描）
    let mut seen: Vec<std::collections::HashSet<String>> = Vec::new();

    fn flush(
        commits: &mut [Commit],
        index: &std::collections::HashMap<String, usize>,
        pending: &mut Vec<FileChange>,
        cur: &Option<String>,
        seen: &mut [std::collections::HashSet<String>],
    ) {
        let Some(h) = cur else { return };
        let Some(&i) = index.get(h) else { return };
        for f in pending.drain(..) {
            if seen[i].insert(f.path.clone()) {
                commits[i].files.push(f);
            }
        }
    }

    let reader = std::io::BufReader::new(stdout);
    for line in std::io::BufRead::lines(reader) {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.contains('\u{1f}') {
            let parts: Vec<&str> = line.split('\u{1f}').collect();
            if parts.len() >= 4 {
                flush(&mut commits, &index, &mut pending, &cur, &mut seen);
                let hash = parts[0].to_string();
                if !index.contains_key(&hash) {
                    index.insert(hash.clone(), commits.len());
                    commits.push(Commit {
                        hash: hash.clone(),
                        author: parts[1].to_string(),
                        at: parts[2].parse().unwrap_or(0),
                        subject: parts[3].to_string(),
                        files: Vec::new(),
                    });
                    seen.push(std::collections::HashSet::new());
                }
                cur = Some(hash);
            }
            continue;
        }
        let line = line.trim_end_matches('\r');
        if line.is_empty() || !line.starts_with(':') {
            continue;
        }
        // :oldmode newmode oldsha newsha status \t path [\t old_path]
        let Some((meta, rest)) = line.split_once('\t') else {
            continue;
        };
        let f: Vec<&str> = meta.trim_start_matches(':').split_whitespace().collect();
        if f.len() < 5 {
            continue;
        }
        let status = f[4].to_string();
        // R/C 状态有两个路径：old \t new
        let mut paths = rest.split('\t');
        let first = paths.next().unwrap_or("").to_string();
        let second = paths.next().filter(|s| !s.is_empty()).map(|s| s.to_string());
        let (path, old_path) = match second {
            Some(new) => (new, Some(first)),
            None => (first, None),
        };
        if path.is_empty() {
            continue;
        }
        let blob = if f[3].chars().all(|c| c == '0') {
            None
        } else {
            Some(f[3].to_string())
        };
        pending.push(FileChange {
            status,
            path,
            old_path,
            blob,
        });
    }
    flush(&mut commits, &index, &mut pending, &cur, &mut seen);
    let _ = child.wait();
    Ok(commits)
}

/// 某行 blame 的 commit（7+ hex）；解析不到返回 None——调用方必须报 reason，不许造 commit。
pub fn blame_line(root: &Path, file: &str, line: i64) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["blame", "--porcelain", "-L", &format!("{line},{line}"), "--", file])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next()?;
    let h = first.split_whitespace().next()?.trim_start_matches('^');
    if h.len() >= 7 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(h.to_string())
    } else {
        None
    }
}

/// 文件最后一次被提交改动的 commit（`git log -1 --format=%H -- <path>`）。
pub fn last_commit(root: &Path, file: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%H", "--", file])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let h = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if h.len() >= 7 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(h)
    } else {
        None
    }
}
