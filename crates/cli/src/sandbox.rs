//! **沙箱的最小骨架**（2026-10-21）：写操作只许落在项目内、`.git` 不许碰、写要原子。
//!
//! ## 为什么先做这三条
//!
//! 误导的两大来源是"半成品当结果"和"做了不可逆却说成功"。改名是唯一的**改用户源码**的命令，
//! 所以先给它三条硬边界：
//! 1. **路径边界**：目标必须落在项目根内；带 `..` 一律拒（那是"写到项目外"的前兆）；
//! 2. **`.git` 只读**：绝不写版本库内部（写坏了整个仓库都废）；
//! 3. **原子写**：先写同目录临时文件，再 rename 覆盖——失败时原文件不动，不留半个文件。
//!
//! 这三条不做"确认弹窗"（那是交互层的事），只做**物理边界**：越界的写，代码层面直接拒绝。
//! 后续再长"动作声明 + 网络允许（本次/24h）"，见 `docs/` 里的沙箱决策。

use std::io;
use std::path::{Component, Path, PathBuf};

/// 目标路径必须在项目根内，且不许碰 `.git`。
///
/// **词法归一，不做 fs 解析**：目标文件可能还不存在，canonicalize 会失败；
/// 而 `..` 一律拒绝，就不用担心"解析前/解析后不是同一个地方"。
///
/// 另一处坑（2026-10-21 实测）：`root.canonicalize()` 在 Windows 上会带 `\\?\` 前缀，
/// 而调用方给的是普通路径；两边直接 `starts_with` 会**永远为假**——所以先剥前缀再比。
pub fn ensure_within(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("项目根读不了：{e}"))?;
    let root_plain = strip_verbatim(&root);
    // 输入里带 `..` 一律拒：不猜它最终会落到哪，只认"这是要往项目外写的信号"。
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(format!(
            "拒绝：路径里有 `..`（可能写到项目外面）：{}",
            path.display()
        ));
    }
    let target = if path.is_absolute() {
        strip_verbatim(path)
    } else {
        root_plain.join(path)
    };
    if !target.starts_with(&root_plain) {
        return Err(format!(
            "拒绝：写到项目外面了 {}（项目根 {}）",
            target.display(),
            root_plain.display()
        ));
    }
    if target
        .components()
        .any(|c| matches!(c, Component::Normal(s) if s == ".git"))
    {
        return Err("拒绝：`.git` 只读，不许写版本库内部".to_string());
    }
    Ok(target)
}

/// `\\?\C:\...` → `C:\...`（Windows 的 canonicalize 前缀）
fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() > 2 && b[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    p.to_path_buf()
}

/// 原子写：同目录临时文件 → rename 覆盖。失败时原文件不动，临时文件清掉。
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "路径没有父目录"))?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "out".to_string());
    let tmp = dir.join(format!(".{name}.jingwei-tmp-{}", std::process::id()));
    std::fs::write(&tmp, contents)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows 的 rename 不覆盖已存在目标：先删再换。原始内容在临时文件里，
            // 最坏情况是"短暂缺失"，不会出现"半个文件"。
            std::fs::remove_file(path).ok();
            let r = std::fs::rename(&tmp, path);
            if r.is_err() {
                std::fs::remove_file(&tmp).ok();
            }
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-sandbox-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(d.join("src")).expect("建临时目录");
        d
    }

    /// 项目内的相对路径：放行。
    #[test]
    fn 项目内的路径放行() {
        let d = tmpdir("ok");
        let p = ensure_within(&d, Path::new("src/a.rs")).expect("项目内该放行");
        assert!(p.starts_with(&d));
        std::fs::remove_dir_all(&d).ok();
    }

    /// `..` 一律拒绝：这是"写到项目外"的前兆，**不猜它最终会落到哪**。
    #[test]
    fn 带上级目录的路径被拒() {
        let d = tmpdir("parent");
        let e = ensure_within(&d, Path::new("../evil.rs")).unwrap_err();
        assert!(e.contains(".."), "错误要说明是因为 `..`：{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    /// 绝对路径落在项目外：拒绝。
    #[test]
    fn 项目外的绝对路径被拒() {
        let d = tmpdir("abs");
        let outside = std::env::temp_dir().join("jw-outside.txt");
        let e = ensure_within(&d, &outside).unwrap_err();
        assert!(e.contains("项目外面"), "错误要说明越界：{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    /// `.git` 只读：写坏版本库的代价比任何一次改名都大。
    #[test]
    fn git_目录被拒() {
        let d = tmpdir("git");
        let e = ensure_within(&d, Path::new(".git/config")).unwrap_err();
        assert!(e.contains(".git"), "错误要说明是 .git：{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    /// 原子写：能写、能覆盖，内容完整。
    #[test]
    fn 原子写能覆盖且不留临时文件() {
        let d = tmpdir("atomic");
        let f = d.join("src/a.rs");
        write_atomic(&f, b"one").expect("第一次写");
        write_atomic(&f, b"two").expect("覆盖写");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "two");
        let leftovers: Vec<_> = std::fs::read_dir(d.join("src"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("jingwei-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不许留临时文件：{leftovers:?}");
        std::fs::remove_dir_all(&d).ok();
    }
}
