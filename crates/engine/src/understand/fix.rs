//! 体检的另一半：能安全替你改的，就替你改。
//!
//! 只碰两类东西：忽略文件里缺的那几行、以及纯粹是垃圾的文件。
//! 不删你写的任何一行，不动锁文件，不碰构建脚本——那些要么得你决定，
//! 要么得跑包管理器，工具替你做只会制造第二个你不知道的真相。
use crate::project::ProjectScan;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 一条能自动执行的修改。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Fix {
    /// 对应的体检条目 id
    pub id: String,
    pub title: String,
    /// 改哪个文件（相对项目根）
    pub target: String,
    /// 会追加的整行内容
    pub lines: Vec<String>,
    /// 为什么会加这些
    pub note: String,
}

/// 看得见但不代做的那些，附一句原因。用户有权知道边界在哪。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Skipped {
    pub id: String,
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FixPlan {
    pub fixes: Vec<Fix>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FixReport {
    pub done: Vec<String>,
    pub changed_files: Vec<String>,
    /// 一行都没加（早就写过了）
    pub nothing_to_do: bool,
}

const MARK: &str = "# added by jingwei";
/// 上一代名字留下的章。**不能只认新章**：老项目里那些忽略文件早就盖着
/// `# added by drydock`，只认新章就会再盖一遍，用户的文件里平白多一行。
const MARK_LEGACY: &[&str] = &["# added by drydock"];

/// 这个文件是不是已经盖过章了——新章老章都算。
fn already_marked(text: &str) -> bool {
    text.contains(MARK) || MARK_LEGACY.iter().any(|m| text.contains(m))
}

/// 计划：只读，不动任何文件。界面拿它显示「点了会发生什么」。
pub fn plan(s: &ProjectScan) -> FixPlan {
    let mut fixes = Vec::new();

    // 忽略文件缺行：这是最值得代做的一件事——答案唯一、可逆、且人老是忘。
    let missing: Vec<String> = s
        .unignored
        .iter()
        .map(|p| format!("{}/", p.trim_end_matches('/')))
        .collect();
    if !missing.is_empty() {
        fixes.push(Fix {
            id: if s.has_gitignore {
                "gitignore-missing-entry".into()
            } else {
                "gitignore-missing".into()
            },
            title: if s.has_gitignore {
                format!("把 {} 个产物目录补进 .gitignore", missing.len())
            } else {
                format!("新建 .gitignore，写进 {} 个产物目录", missing.len())
            },
            target: ".gitignore".into(),
            lines: missing,
            note: "只追加，不重排、不删你写过的行。".into(),
        });
    }

    // .env 没被挡住
    if !s.exposed_env.is_empty() {
        fixes.push(Fix {
            id: "env-file-exposed".into(),
            title: format!("把 {} 挡进 .gitignore", s.exposed_env.join("、")),
            target: ".gitignore".into(),
            lines: s.exposed_env.clone(),
            note: "已经提交过的话，工具不会替你 git rm——那一步你自己来，命令在条目里。".into(),
        });
    }

    // IDE 配置目录
    if s.has_git {
        let ide: Vec<String> = [".idea", ".vscode"]
            .iter()
            .filter(|d| s.root_path().join(d).is_dir())
            .map(|d| format!("{}/", d))
            .collect();
        let ide: Vec<String> = ide
            .into_iter()
            .filter(|l| !s.gitignore_covers(l.trim_end_matches('/')))
            .collect();
        if !ide.is_empty() {
            fixes.push(Fix {
                id: "ide-dir-present".into(),
                title: format!("忽略 {}（编辑器本地配置）", ide.join("、")),
                target: ".gitignore".into(),
                lines: ide,
                note: "想让同事共享 .vscode/settings.json 的话，改完自己删掉那一行。".into(),
            });
        }
    }

    // Docker 上下文
    if s.has_dockerfile() && !s.has_dockerignore() {
        fixes.push(Fix {
            id: "docker-no-ignore".into(),
            title: "新建 .dockerignore".into(),
            target: ".dockerignore".into(),
            lines: vec![
                ".git/".into(),
                "node_modules/".into(),
                "target/".into(),
                "__pycache__/".into(),
                ".venv/".into(),
                "*.log".into(),
            ],
            note: "构建上下文越小，docker build 越快；镜像里也不会混进本机垃圾。".into(),
        });
    }

    // 看得见但不代做的
    let mut skipped = Vec::new();
    for item in &s.items {
        let reason = match item.id.as_str() {
            "node-lock-missing" | "node-lock-stale" => {
                "要跑一次包管理器（npm install）才能生成/更新锁文件"
            }
            "py-lock-missing" | "py-unpinned" => "要跑一次 pip/uv 才知道实际装了什么版本",
            "php-lock-missing" | "ruby-lock-missing" => "同上：锁文件只能由包管理器生成",
            "maven-snapshot-dep" | "maven-version-conflict" | "maven-encoding-unset"
            | "maven-no-wrapper" | "gradle-no-wrapper" | "package-json-broken"
            | "node-no-scripts" | "py-multiple-manifests" => "改的是你写的构建描述文件，得你定",
            "detritus" => "删文件走「清理」页，那里会进回收站、能反悔",
            "project-artifacts" => "同上，删产物走「清理」页",
            "long-path" | "project-path-unsafe" => "要挪动整个项目，工具不该动你的目录",
            "no-tests" | "no-src-test-layout" => "这是习惯，不是错误，工具不该替你建目录",
            "large-file" => "可能是你要的数据集，删不删只有你知道",
            "py-manifest-missing" => "要跑 freeze 才知道装了什么",
            _ => continue,
        };
        skipped.push(Skipped {
            id: item.id.clone(),
            title: item.title.clone(),
            reason: reason.to_string(),
        });
    }

    FixPlan { fixes, skipped }
}

/// 执行。默认只做 plan 里那些「加行 / 建文件」的动作，全部幂等：
/// 再跑一次不会重复写。
pub fn apply(root: &Path, s: &ProjectScan, only: &[String]) -> Result<FixReport> {
    let mut report = FixReport::default();
    let mut pending: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut order: Vec<String> = Vec::new();

    for f in plan(s).fixes {
        if !only.is_empty() && !only.contains(&f.id) {
            continue;
        }
        order.push(f.target.clone());
        pending.entry(f.target.clone()).or_default().extend(f.lines);
        report.done.push(format!("{}：{}", f.id, f.title));
    }
    if pending.is_empty() {
        report.nothing_to_do = true;
        return Ok(report);
    }

    // 同一个文件只读一次、写一次：两个条目都指向 .gitignore 是常态。
    let mut seen_files: Vec<String> = Vec::new();
    for target in order {
        if seen_files.contains(&target) {
            continue;
        }
        seen_files.push(target.clone());
        let lines = pending.get(&target).cloned().unwrap_or_default();
        let path = root.join(&target);
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let have: Vec<String> = existing
            .lines()
            .map(|l| l.trim().trim_end_matches('/').to_lowercase())
            .collect();

        let mut added: Vec<String> = Vec::new();
        for l in lines {
            let key = l.trim().trim_end_matches('/').to_lowercase();
            if key.is_empty() || have.contains(&key) || added.iter().any(|a: &String| a.to_lowercase().trim_end_matches('/') == key) {
                continue;
            }
            added.push(l);
        }
        if added.is_empty() {
            continue;
        }
        let mut text = existing.clone();
        if text.is_empty() {
            text.push_str(&format!("{}\n", MARK));
        } else {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            if !already_marked(&text) {
                text.push_str(&format!("\n{}\n", MARK));
            }
        }
        for l in &added {
            text.push_str(l);
            text.push('\n');
        }
        std::fs::write(&path, text)?;
        report.changed_files.push(target.clone());
        report.done.push(format!(
            "{}：新增 {} 行（{}）",
            target,
            added.len(),
            added.join(" ")
        ));
    }
    if report.changed_files.is_empty() {
        report.nothing_to_do = true;
    }
    Ok(report)
}

impl ProjectScan {
    fn gitignore_covers(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        // 这里只看「忽略文件里有没有写过这个名字」，和 project 里
        // 那套匹配规则保持一致的口径：认不出来就当没覆盖。
        match std::fs::read_to_string(self.root_path().join(".gitignore")) {
            Ok(t) => t.lines().any(|l| {
                let l = l.trim().trim_start_matches('/').trim_end_matches('/').to_lowercase();
                !l.is_empty() && (l == n || l.rsplit('/').next().map(|x| x == n).unwrap_or(false))
            }),
            Err(_) => false,
        }
    }
    fn has_dockerfile(&self) -> bool {
        self.all_files
            .iter()
            .any(|f| f.rsplit('/').next().unwrap_or(f).eq_ignore_ascii_case("dockerfile"))
    }
    fn has_dockerignore(&self) -> bool {
        self.all_files
            .iter()
            .any(|f| f.rsplit('/').next().unwrap_or(f).eq_ignore_ascii_case(".dockerignore"))
    }
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dd-fix-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_ignore_lines_are_appended_without_touching_what_was_there() {
        let d = scratch("append");
        std::fs::write(d.join(".gitignore"), "# 我自己写的\n*.log\n").unwrap();
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        let s = crate::project::scan(&d);
        let r = apply(&d, &s, &[]).unwrap();
        assert!(!r.nothing_to_do);
        let t = std::fs::read_to_string(d.join(".gitignore")).unwrap();
        assert!(t.starts_with("# 我自己写的\n*.log\n"), "原有内容必须原样在前面");
        assert!(t.contains("node_modules/"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn applying_twice_changes_nothing_the_second_time() {
        let d = scratch("idempotent");
        std::fs::create_dir_all(d.join("target")).unwrap();
        let s = crate::project::scan(&d);
        apply(&d, &s, &[]).unwrap();
        let first = std::fs::read_to_string(d.join(".gitignore")).unwrap();
        let s2 = crate::project::scan(&d);
        let r2 = apply(&d, &s2, &[]).unwrap();
        let second = std::fs::read_to_string(d.join(".gitignore")).unwrap();
        assert_eq!(first, second, "第二次不该再写一遍");
        assert!(r2.nothing_to_do);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_existing_entry_is_not_duplicated() {
        let d = scratch("dupe");
        std::fs::write(d.join(".gitignore"), "node_modules\n").unwrap();
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        let s = crate::project::scan(&d);
        let r = apply(&d, &s, &[]).unwrap();
        let t = std::fs::read_to_string(d.join(".gitignore")).unwrap();
        assert_eq!(t.matches("node_modules").count(), 1, "不该写第二遍：{}", t);
        assert!(r.nothing_to_do);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dockerignore_is_created_for_a_dockerfile_project() {
        let d = scratch("docker");
        std::fs::write(d.join("Dockerfile"), "FROM node:20\n").unwrap();
        let s = crate::project::scan(&d);
        assert!(plan(&s).fixes.iter().any(|f| f.target == ".dockerignore"));
        apply(&d, &s, &[]).unwrap();
        let t = std::fs::read_to_string(d.join(".dockerignore")).unwrap();
        assert!(t.contains("node_modules/"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn source_files_are_never_touched() {
        let d = scratch("sources");
        std::fs::write(d.join("main.py"), "print(1)\n").unwrap();
        std::fs::create_dir_all(d.join("__pycache__")).unwrap();
        let before = std::fs::read_to_string(d.join("main.py")).unwrap();
        let s = crate::project::scan(&d);
        apply(&d, &s, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("main.py")).unwrap(), before);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 锁文件、pom、项目路径这些工具不代做，但要说清为什么。
    #[test]
    fn work_that_needs_a_decision_is_listed_as_skipped_with_a_reason() {
        let d = scratch("skipped");
        std::fs::write(d.join("package.json"), "{\"name\":\"x\"}").unwrap();
        std::fs::write(d.join("pom.xml"), "<project/>").unwrap();
        let s = crate::project::scan(&d);
        let p = plan(&s);
        let sk = p.skipped.iter().find(|x| x.id == "node-lock-missing").expect("该列出来");
        assert!(sk.reason.contains("npm install"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
