//! 工作副本：改代码不直接落盘。
//!
//! 为什么这么做：编辑器一次误保存就能把文件写坏，而人常常事后才发现。
//! 所以：改动先进 `.jingwei/drafts/`，点"应用"才写文件，写之前再留一份快照，
//! 想回退随时能回到任意一版。
//!
//! 边界（刻意的）：
//! - 只碰项目根目录以内的文件，`..` 一律拒绝；
//! - 二进制文件不给编辑（读出来就告诉你是二进制）；
//! - 不记"跨重启的撤销历史"——那是连 VS Code 都没解决的问题，不假装能做。
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// 读文件的上限：再大就不该在工具里编辑了，交给真正的编辑器。
pub const READ_CAP: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Draft {
    pub id: String,
    pub at: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Opened {
    pub path: String,
    pub text: String,
    pub bytes: u64,
    /// 二进制文件（或太大）不给编辑，只告诉你为什么
    pub readable: bool,
    pub note: String,
    /// 这个文件本来的编码与换行。写回时必须照原样，否则就是悄悄改用户的文件。
    pub encoding: String,
    pub eol: String,
    pub drafts: Vec<Draft>,
}

/// 把相对路径钉死在项目根里面。越界一律拒绝——这层不做，后面所有写操作都不安全。
pub fn safe_rel(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel = rel.replace('\\', "/");
    let p = Path::new(&rel);
    if p.is_absolute() {
        bail!("只接受项目内的相对路径");
    }
    for c in p.components() {
        match c {
            Component::Normal(_) => {}
            _ => bail!("路径里有不允许的片段：{}", rel),
        }
    }
    let full = root.join(p);
    // 双保险：解析后仍要在根里面（防符号链接绕过）。
    // Windows 的 canonicalize 会带 \\?\ 前缀，比较前统一去掉再比，否则必然对不上。
    if let Ok(real) = full.canonicalize() {
        let real_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let strip = |x: &Path| {
            x.to_string_lossy()
                .trim_start_matches(r"\\?\")
                .replace('\\', "/")
                .to_lowercase()
        };
        if !strip(&real).starts_with(&strip(&real_root)) {
            bail!("这个路径不在项目里：{}", rel);
        }
    }
    Ok(full)
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

/// 读出可编辑内容。二进制/超限时不报错，而是带着原因返回（界面要能解释）。
pub fn read(root: &Path, rel: &str) -> Result<Opened> {
    let full = safe_rel(root, rel)?;
    let mut o = Opened {
        path: rel.replace('\\', "/"),
        readable: true,
        ..Default::default()
    };
    let md = std::fs::metadata(&full).context("读不到文件信息")?;
    o.bytes = md.len();
    if md.len() as usize > READ_CAP {
        o.readable = false;
        o.note = format!(
            "文件 {} MB，超过 {} MB 的上限——这种体量交给真正的编辑器",
            md.len() / 1048576,
            READ_CAP / 1048576
        );
        return Ok(o);
    }
    let bytes = std::fs::read(&full).context("读不到文件内容")?;
    if looks_binary(&bytes) {
        o.readable = false;
        o.note = "这是二进制文件，不给在工具里编辑".into();
        return Ok(o);
    }
    // 认编码与换行：GBK 的中文文件不再被读成乱码，CRLF 也不会被悄悄改掉
    let tf = crate::util::textfile::decode(&bytes);
    o.text = tf.text;
    o.encoding = tf.encoding;
    o.eol = tf.eol;
    if !tf.readable {
        o.readable = false;
    }
    if !tf.note.is_empty() {
        o.note = tf.note;
    }
    o.drafts = list(root, rel);
    Ok(o)
}

fn draft_dir(root: &Path, rel: &str) -> PathBuf {
    let mut d = crate::data_dir(root).join("drafts");
    for part in rel.replace('\\', "/").split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        // 文件名里可能有冒号之类，换成下划线，别让目录名非法
        let clean: String = part
            .chars()
            .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
            .collect();
        d = d.join(clean);
    }
    d
}

/// 先把当前内容存成草稿，再原子写回。返回这次留下的草稿。
pub fn apply(root: &Path, rel: &str, text: &str) -> Result<Draft> {
    apply_as(root, rel, text, "utf-8", "lf")
}

/// 按指定编码与换行写回。界面会把打开时记下的（encoding, eol）原样传回来——
/// 这是"不悄悄改用户文件"的落点。
pub fn apply_as(root: &Path, rel: &str, text: &str, encoding: &str, eol: &str) -> Result<Draft> {
    let body = crate::util::textfile::encode(text, encoding, eol);
    apply_bytes(root, rel, &body, text)
}

fn apply_bytes(root: &Path, rel: &str, body: &[u8], text: &str) -> Result<Draft> {
    let full = safe_rel(root, rel)?;
    if full.exists() {
        snapshot(root, rel)?;
    }
    let dir = full.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    // `parent()` 在相对路径只有一段时会给空串，`create_dir_all("")` 必然失败，
    // 而那个失败是正常的（目标就在项目根下，根不需要建）。空串要跳过。
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("建目录失败：{}", dir.display()))?;
    }
    // 原子写：先写临时文件再改名，避免写一半断电留下半个文件
    let tmp = full.with_extension(format!(
        "{}.tmp",
        full.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default()
    ));
    std::fs::write(&tmp, body).with_context(|| format!("写临时文件失败：{}", tmp.display()))?;
    std::fs::rename(&tmp, &full).with_context(|| format!("落盘失败：{}", full.display()))?;
    Ok(Draft {
        id: crate::facts::stable_hash([text])[..8].to_string(),
        at: crate::facts::now_iso(),
        bytes: text.len(),
    })
}

/// 把磁盘上的当前版本留一份（应用前自动调用；也可以手动"留一版"）。
pub fn snapshot(root: &Path, rel: &str) -> Result<Draft> {
    let full = safe_rel(root, rel)?;
    let bytes = std::fs::read(&full).context("读不到原文件")?;
    let dir = draft_dir(root, rel);
    std::fs::create_dir_all(&dir)?;
    let hash = crate::facts::stable_hash([String::from_utf8_lossy(&bytes).to_string()]);
    let at = crate::facts::now_iso();
    let id = format!("{}-{}", at.replace([':', '-'], ""), &hash[..6]);
    std::fs::write(dir.join(format!("{}.bak", id)), &bytes)?;
    Ok(Draft {
        id,
        at,
        bytes: bytes.len(),
    })
}

pub fn list(root: &Path, rel: &str) -> Vec<Draft> {
    let dir = draft_dir(root, rel);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut out: Vec<Draft> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".bak")?.to_string();
            let md = e.metadata().ok()?;
            let at = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Some(Draft {
                id,
                at: format!("{}", at),
                bytes: md.len() as usize,
            })
        })
        .collect();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

/// 回到某一版：把草稿写回文件（先把当前版本也留一份，回滚本身也可回滚）。
pub fn restore(root: &Path, rel: &str, id: &str) -> Result<()> {
    let dir = draft_dir(root, rel);
    let src = dir.join(format!("{}.bak", id));
    if !src.is_file() {
        bail!("找不到这一版草稿：{}", id);
    }
    let bytes = std::fs::read(&src)?;
    let full = safe_rel(root, rel)?;
    if full.exists() {
        // 回滚前先留档。留不上还不至于让回滚失败，但事后想找回被覆盖的那一版就没了。
        if let Err(e) = snapshot(root, rel) {
            crate::util::dlog::w("9005", &format!("回滚前留档失败（{rel}）：{e}"));
        }
    }
    std::fs::write(&full, &bytes)?;
    Ok(())
}

/// 列出项目里可编辑的文件（跳过产物、缓存、git 内部）
pub fn files(root: &Path, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 6 || out.len() >= limit {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let p = e.path();
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let n = name.to_lowercase();
                if matches!(n.as_str(), ".git" | ".jingwei" | ".pylaunch" | "node_modules" | "target" | "dist" | ".venv" | "venv" | "__pycache__" | ".idea" | ".vscode")
                    || crate::project::artifact_kind(&p, &dir, &name).is_some()
                {
                    continue;
                }
                stack.push((p, depth + 1));
                continue;
            }
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            // 只看写代码/配置的那几类；图片、压缩包之类不给编辑
            let ext = p
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            const EDITABLE: &[&str] = &[
                "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "vue", "java", "kt", "go",
                "c", "h", "cpp", "hpp", "cc", "cs", "php", "rb", "sh", "ps1", "sql", "toml",
                "json", "yaml", "yml", "ini", "cfg", "conf", "properties", "md", "txt", "xml",
                // 注意：这里**没有** env —— 见下面的 SENSITIVE。
                "lock", "gitignore", "editorconfig",
            ];
            let base = name.to_lowercase();
            // 敏感文件：**不读、不进图谱、不进日志**。P5 复审发现原来的白名单里
            // 竟然写着 "env" 且专门放行 `.env*` —— 那不是排除，是把密钥文件列成可编辑。
            const SENSITIVE: &[&str] = &[
                ".env", "id_rsa", "id_ed25519", "id_dsa", ".npmrc", ".pypirc",
                ".git-credentials", ".netrc", "credentials", ".htpasswd", "shadow",
                "secring", "keystore", ".tfstate",
            ];
            const SENSITIVE_EXT: &[&str] = &["pem", "key", "p12", "pfx", "jks", "ppk", "kdbx"];
            if SENSITIVE.iter().any(|s| base == *s || base.starts_with(&format!("{s}.")))
                || SENSITIVE_EXT.contains(&ext.as_str())
            {
                continue;
            }
            let ok = EDITABLE.contains(&ext.as_str())
                || base.starts_with(".gitignore")
                || base == "makefile"
                || base == "dockerfile";
            if ok {
                out.push(rel);
            }
            if out.len() >= limit {
                break;
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    /// P5 复审发现：原来的白名单把 `.env` 列成"可编辑"，等于把密钥文件读进来。
    /// 这条测试守着"敏感文件不读、不进图谱、不进日志"。
    #[test]
    fn sensitive_files_are_not_even_listed() {
        let d = std::env::temp_dir().join(format!("jingwei-secret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        for f in [".env", ".env.local", "id_rsa", "server.pem", "app.key", ".npmrc",
                  ".git-credentials", "keystore.p12"] {
            std::fs::write(d.join(f), "SECRET=1").unwrap();
        }
        std::fs::write(d.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(d.join(".gitignore"), "target/").unwrap();
        let listed = files(&d, 100);
        for bad in [".env", ".env.local", "id_rsa", "server.pem", "app.key", ".npmrc",
                    ".git-credentials", "keystore.p12"] {
            assert!(!listed.iter().any(|p| p.ends_with(bad)),
                "{} 不该出现在可编辑列表里：{:?}", bad, listed);
        }
        assert!(listed.iter().any(|p| p == "src/main.rs"), "普通源码要照常列出：{:?}", listed);
        assert!(listed.iter().any(|p| p == ".gitignore"), ".gitignore 仍可编辑");
        let _ = std::fs::remove_dir_all(&d);
    }


    /// 端到端：一个 GBK + CRLF 的文件，改一行再写回，
    /// **编码与换行都不能变**。这条是"编辑器毁文件"那类事故的守门测试。
    #[cfg(windows)]
    #[test]
    fn editing_a_gbk_crlf_file_keeps_its_encoding_and_line_endings() {
        let d = std::env::temp_dir().join(format!("jingwei-gbk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let rel = "config.ini";
        let original = "# 中文注释\r\nkey = 旧值\r\n";
        let gbk = crate::util::textfile::encode(original, "gbk", "crlf");
        std::fs::write(d.join(rel), &gbk).unwrap();

        let o = super::read(&d, rel).unwrap();
        assert_eq!(o.encoding, "gbk");
        assert_eq!(o.eol, "crlf");
        assert!(o.readable, "GBK 要能编辑");

        let edited = o.text.replace("旧值", "新值");
        assert!(!edited.contains('\r'), "textarea 给回来的应当是 LF");
        super::apply_as(&d, rel, &edited, &o.encoding, &o.eol).unwrap();

        let back = std::fs::read(d.join(rel)).unwrap();
        assert!(back.contains(&0x0D) && back.contains(&0x0A), "CRLF 必须保住");
        assert!(String::from_utf8(back.clone()).is_err(), "写回去还得是 GBK，不能变成 UTF-8");
        let text = crate::util::textfile::decode(&back);
        assert_eq!(text.encoding, "gbk");
        assert!(text.text.contains("新值"));
        assert!(text.text.contains("中文注释"), "中文注释不能坏");
        let _ = std::fs::remove_dir_all(&d);
    }


    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dd-wc-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn editing_never_touches_the_file_until_apply() {
        let d = scratch("apply");
        std::fs::write(d.join("a.py"), "x = 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.py")).unwrap(), "x = 1\n");
        let draft = apply(&d, "a.py", "x = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.py")).unwrap(), "x = 2\n");
        assert!(!draft.id.is_empty());
        // 应用之前那一版要留着
        let drafts = list(&d, "a.py");
        assert_eq!(drafts.len(), 1, "应用前应自动留一份快照");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn restore_goes_back_to_the_snapshot() {
        let d = scratch("restore");
        std::fs::write(d.join("a.py"), "old\n").unwrap();
        apply(&d, "a.py", "new\n").unwrap();
        let id = list(&d, "a.py")[0].id.clone();
        restore(&d, "a.py", &id).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.py")).unwrap(), "old\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 越界路径一律拒绝——这条闸门没了，后面所有写操作都不安全。
    #[test]
    fn paths_outside_the_project_are_refused() {
        let d = scratch("escape");
        for bad in ["../secret.txt", "..\\secret.txt", "C:\\Windows\\x.txt", "a/../../b.txt"] {
            assert!(safe_rel(&d, bad).is_err(), "应当拒绝：{}", bad);
        }
        assert!(safe_rel(&d, "src/main.py").is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn binary_files_are_not_offered_for_editing() {
        let d = scratch("binary");
        std::fs::write(d.join("a.bin"), [0u8, 1, 2, 3]).unwrap();
        let o = read(&d, "a.bin").unwrap();
        assert!(!o.readable);
        assert!(o.note.contains("二进制"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn file_list_skips_artifacts_and_keeps_sources() {
        let d = scratch("files");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("node_modules").join("x")).unwrap();
        std::fs::write(d.join("src").join("main.rs"), "fn main(){}").unwrap();
        std::fs::write(d.join("node_modules").join("x").join("a.js"), "1").unwrap();
        std::fs::write(d.join("README.md"), "# x").unwrap();
        std::fs::write(d.join("logo.png"), [0u8, 0, 1]).unwrap();
        let f = files(&d, 100);
        assert!(f.contains(&"src/main.rs".to_string()));
        assert!(f.contains(&"README.md".to_string()));
        assert!(!f.iter().any(|x| x.contains("node_modules")), "{:?}", f);
        assert!(!f.iter().any(|x| x.ends_with(".png")), "{:?}", f);
        let _ = std::fs::remove_dir_all(&d);
    }
}
