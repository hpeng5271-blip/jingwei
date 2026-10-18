//! Node / 前端那一摊：**用哪个包管理器、依赖装到哪去了、版本要求写在哪**。
//!
//! ## 为什么必须有这一层
//!
//! 之前 Node 项目只有一条判据：`node_modules/<名字>` 在不在。这一条在两种常见
//! 布局里会**给出错误结论**——而这是这套系统最不许犯的错（误报）：
//!
//! | 布局 | `node_modules` 长什么样 | 只看那一条判据会怎样 |
//! |---|---|---|
//! | npm / yarn classic | 平铺在项目根 | 对 |
//! | pnpm（默认隔离布局） | `<名字>` 是指向 `node_modules/.pnpm/…` 的**符号链接** | 对（跟着链接走） |
//! | **yarn PnP** | **压根没有 node_modules** | **全部报成"未安装"** |
//! | **workspace / monorepo** | 装在**仓库根**，成员目录里没有 | **成员项目全部报成"未安装"** |
//!
//! 后两种是**实打实的误报**。所以这一层的活儿是：先把"这个项目用哪种布局"
//! 弄清楚，再决定"能不能查、怎么查"。
//!
//! ## 事实的出处（都能查）
//!
//! - npm 锁文件与字段含义：`docs.npmjs.com/cli/v10/configuring-npm/package-lock-json`
//!   ——`packages` 的键是"相对根项目的路径"，根项目用 `""`；`npm-shrinkwrap.json`
//!   优先于 `package-lock.json`；隐藏锁文件在 `node_modules/.package-lock.json`。
//! - corepack 的 `packageManager` 字段：`github.com/nodejs/corepack`
//!   ——`名字@版本` 必填，`+sha224.…` 可选；corepack 认的是 yarn / npm / pnpm。
//! - pnpm：设置从 `pnpm-workspace.yaml` 读，`nodeLinker` 这类**决定 node_modules
//!   结构**的项只能写在那里（`.npmrc` 只读认证与源）。
//! - yarn：`.yarnrc.yml` 的 `nodeLinker` 取值 `pnp` / `node-modules`。
//! - bun：`bun install` 生成 `bun.lock`；**v1.2 起默认从二进制 `bun.lockb` 换成文本**。
//! - 版本要求：`.nvmrc` / `.node-version` / `package.json` 的 `volta` 与 `engines` /
//!   `.tool-versions` / `mise.toml`（`.mise.toml` 覆盖 `mise.toml`）。
//!
//! 只读文件，不 spawn、不联网——打开项目第一帧要用，快是第一位的。

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebFacts {
    /// 包管理器（npm / pnpm / yarn / bun）以及**这个结论的凭据**。
    pub package_manager: Option<String>,
    pub pm_from: Option<String>,
    /// corepack 的 `packageManager` 里写的版本（`pnpm@9.1.0+sha224.…` → `9.1.0`）。
    pub pm_pinned: Option<String>,
    /// 依赖装到哪去了：`node_modules` / `pnpm 隔离布局` / `yarn PnP` / `还没装`
    pub link_kind: String,
    pub link_from: String,
    /// 项目对 Node 版本的要求，一处一条（来源 + 内容）。
    pub node_pins: Vec<Pin>,
    /// monorepo 的迹象（`package.json` 的 `workspaces` / `pnpm-workspace.yaml`）。
    pub workspaces: Option<String>,
    pub typescript: bool,
    /// 常用脚本（dev / build / test / lint 这几个）。
    pub scripts: Vec<(String, String)>,
    /// 认出来的框架。
    pub frameworks: Vec<String>,
    /// **需要说清楚的地方**：多个锁文件并存、没有锁文件、PnP 的特殊性……
    /// 一条都不许省——它们正是"这个结论凭什么成立 / 为什么可能不对"的答案。
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pin {
    pub from: String,
    pub spec: String,
}

/// 认一个目录里是不是 Node 项目。只有一个判据：`package.json`。
///
/// 为什么不把 `node_modules` 也算上：**没有 `package.json` 却有一堆
/// `node_modules` 的目录**是依赖被装错地方的现场，不是项目。
pub fn is_node_project(root: &Path) -> bool {
    root.join("package.json").is_file()
}

/// 项目声明的包管理器（没声明就是 `None`，不猜）。
///
/// 顺序按"凭据有多硬"排：`packageManager` 是项目**自己写下来的**要求
/// （corepack 认它）；锁文件是"上次装的时候用的是谁"，是事实但不如前者硬。
///
/// 返回 `(包管理器, 凭据, 版本)`。**多个锁文件并存时不许挑一个**：
/// 那是两拨人用两套工具装过的现场，报出来比替用户做决定有用。
pub fn package_manager(root: &Path) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(text) = read(root, "package.json") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(crate::util::bom::strip_bom(&text)) {
            if let Some(s) = json.get("packageManager").and_then(|v| v.as_str()) {
                let (name, ver) = split_pm(s);
                if !name.is_empty() {
                    return (
                        Some(name),
                        Some("package.json 的 packageManager（corepack 认这个字段）".to_string()),
                        ver,
                    );
                }
            }
        }
    }
    // 锁文件。**顺序有讲究**：npm 官方写明 `npm-shrinkwrap.json` 优先于
    // `package-lock.json`，所以两个都在时报 shrinkwrap。
    let locks: [(&str, &str); 6] = [
        ("npm-shrinkwrap.json", "npm"),
        ("package-lock.json", "npm"),
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
    ];
    let hit: Vec<(&str, &str)> = locks
        .iter()
        .filter(|(f, _)| root.join(f).is_file())
        .copied()
        .collect();
    match hit.len() {
        0 => (None, None, None),
        1 => (
            Some(hit[0].1.to_string()),
            Some(format!("{}（锁文件）", hit[0].0)),
            None,
        ),
        _ => {
            let names: Vec<&str> = hit.iter().map(|(f, _)| *f).collect();
            (
                None,
                Some(format!(
                    "{} 个锁文件并存（{}）——谁后装的说了算，这个不猜",
                    hit.len(),
                    names.join(" / ")
                )),
                None,
            )
        }
    }
}

/// 依赖装到哪去了。**这是把"未安装"误报掉的关键一步。**
pub fn link_kind(root: &Path) -> (String, String) {
    // ① PnP：有 .pnp.cjs（或 .pnp.loader.mjs）就是 PnP，**没有 node_modules 也正常**。
    for f in [".pnp.cjs", ".pnp.loader.mjs"] {
        if root.join(f).is_file() {
            return (
                "yarn PnP（没有 node_modules 是正常的）".to_string(),
                f.to_string(),
            );
        }
    }
    // ② `.yarnrc.yml` 明说
    if let Some(text) = read(root, ".yarnrc.yml") {
        let t = text.to_lowercase();
        let squashed: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        if squashed.contains("nodelinker:pnp") {
            return (
                "yarn PnP（没有 node_modules 是正常的）".to_string(),
                ".yarnrc.yml 的 nodeLinker: pnp".to_string(),
            );
        }
        if squashed.contains("nodelinker:node-modules") {
            return (
                "node_modules".to_string(),
                ".yarnrc.yml 的 nodeLinker: node-modules".to_string(),
            );
        }
    }
    // ③ pnpm 的隔离布局：`node_modules/.pnpm` 是它的指纹
    if root.join("node_modules").join(".pnpm").is_dir() {
        return (
            "pnpm 隔离布局（依赖在 .pnpm 里，node_modules 下是符号链接）".to_string(),
            "node_modules/.pnpm".to_string(),
        );
    }
    if root.join("node_modules").is_dir() {
        return (
            "node_modules".to_string(),
            "node_modules 目录存在".to_string(),
        );
    }
    (
        "还没装".to_string(),
        "既没有 node_modules 也没有 .pnp.cjs".to_string(),
    )
}

/// 这个项目是不是 yarn PnP。
///
/// **单独抽出来是因为它决定了一件很具体的事**：查依赖装没装时，
/// PnP 项目**不能**去看 `node_modules`——那玩意儿压根不存在。
/// 依赖是否可见这件事，只有 `.pnp.cjs` 那份映射表说得清，而解析它
/// 不在这一层（要跑 JS）。所以结论是**"确认不了"**，不是**"没装"**。
pub fn is_pnp(root: &Path) -> bool {
    if root.join(".pnp.cjs").is_file() || root.join(".pnp.loader.mjs").is_file() {
        return true;
    }
    read(root, ".yarnrc.yml")
        .map(|t| {
            let squashed: String = t.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
            squashed.contains("nodelinker:pnp")
        })
        .unwrap_or(false)
}

/// 项目对 Node 版本的要求。**一处一条，不合并**——
/// 两处写得不一样，这件事本身就是最该看到的信息。
pub fn node_pins(root: &Path) -> Vec<Pin> {
    let mut out = Vec::new();
    if let Some(t) = read(root, ".nvmrc") {
        push_pin(&mut out, ".nvmrc", &t);
    }
    if let Some(t) = read(root, ".node-version") {
        push_pin(&mut out, ".node-version", &t);
    }
    if let Some(t) = read(root, ".tool-versions") {
        for line in t.lines() {
            let mut it = line.split_whitespace();
            // 这一族文件里 node 那一行的键名就是 nodejs
            if it.next() == Some("nodejs") {
                if let Some(v) = it.next() {
                    out.push(Pin {
                        from: ".tool-versions".to_string(),
                        spec: v.to_string(),
                    });
                }
            }
        }
    }
    for f in ["mise.toml", ".mise.toml"] {
        if let Some(t) = read(root, f) {
            if let Some(v) = toml_table_value(&t, "tools", "node") {
                out.push(Pin {
                    from: f.to_string(),
                    spec: v,
                });
            }
        }
    }
    if let Some(text) = read(root, "package.json") {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(crate::util::bom::strip_bom(&text)) {
            if let Some(v) = j
                .get("volta")
                .and_then(|v| v.get("node"))
                .and_then(|v| v.as_str())
            {
                push_pin(&mut out, "package.json · volta.node", v);
            }
            if let Some(v) = j
                .get("engines")
                .and_then(|v| v.get("node"))
                .and_then(|v| v.as_str())
            {
                push_pin(&mut out, "package.json · engines.node", v);
            }
        }
    }
    out
}

/// 这类文件常常带注释（`20 # LTS`）或引号，去掉。
fn push_pin(out: &mut Vec<Pin>, from: &str, text: &str) {
    let spec = text.trim();
    let spec = spec.split('#').next().unwrap_or(spec).trim();
    let spec = spec.trim_matches('"').trim_matches('\'');
    if !spec.is_empty() {
        out.push(Pin {
            from: from.to_string(),
            spec: spec.to_string(),
        });
    }
}

/// 一次把 Node 项目该看的都看了。不是 Node 项目就返回 `None`。
/// **这个 Node 项目"跑起来"该敲什么**（2026-10-18 加）。
///
/// 来源：拿真 Vite 项目跑 `guide`，它说「还查不出来——补一份 env.contract.json」。
/// 而**前端项目的命令本来就在 `package.json` 的 `scripts` 里**（仓库自己的注释也这么写），
/// `launch` 那一层早就会挑。所以这不是"查不了"，是 **`guide` 没去问同一份数据**。
///
/// 脚本名与顺序**必须和 `launch/node.rs` 的 `LIVE_SCRIPTS` 一致**：
/// dev → start → serve → preview。两处不一致，就会出现"首页说 A、按钮跑 B"。
pub fn run_script(root: &Path) -> Option<String> {
    let w = probe_web(root)?;
    let pm = w.package_manager.clone().unwrap_or_else(|| "npm".to_string());
    for want in ["dev", "start", "serve", "preview"] {
        if let Some((k, _)) = w.scripts.iter().find(|(k, _)| k == want) {
            return Some(format!("{pm} run {k}"));
        }
    }
    None
}

pub fn probe_web(root: &Path) -> Option<WebFacts> {
    if !is_node_project(root) {
        return None;
    }
    let (pm, pm_from, pm_pinned) = package_manager(root);
    let (link_kind, link_from) = link_kind(root);
    let mut notes = Vec::new();
    match (&pm, &pm_from) {
        (None, Some(why)) => notes.push(why.clone()),
        (None, None) => notes.push(
            "没有锁文件，也没有 packageManager 字段——还看不出用哪个包管理器".to_string(),
        ),
        _ => {}
    }
    if link_kind.starts_with("yarn PnP") {
        notes.push(
            "PnP 项目不生成 node_modules：判依赖装没装要看 .pnp.cjs，不是看目录".to_string(),
        );
    }
    Some(WebFacts {
        package_manager: pm,
        pm_from,
        pm_pinned,
        link_kind,
        link_from,
        node_pins: node_pins(root),
        workspaces: workspaces(root),
        typescript: has_typescript(root),
        scripts: scripts(root),
        frameworks: frameworks(root),
        notes,
    })
}

/// `pnpm@9.1.0+sha224.abc…` → `("pnpm", Some("9.1.0"))`
fn split_pm(s: &str) -> (String, Option<String>) {
    let s = s.trim();
    let (name, rest) = match s.split_once('@') {
        Some((n, r)) => (n, Some(r)),
        None => (s, None),
    };
    // `+sha224.…` 之后是校验值，不是版本的一部分
    let ver = rest
        .map(|r| r.split('+').next().unwrap_or(r).trim().to_string())
        .filter(|v| !v.is_empty());
    (name.trim().to_string(), ver)
}

fn workspaces(root: &Path) -> Option<String> {
    if root.join("pnpm-workspace.yaml").is_file() {
        return Some("pnpm-workspace.yaml".to_string());
    }
    let text = read(root, "package.json")?;
    let j: serde_json::Value = serde_json::from_str(&text).ok()?;
    match j.get("workspaces") {
        Some(serde_json::Value::Array(a)) if !a.is_empty() => {
            Some(format!("package.json · workspaces（{} 项）", a.len()))
        }
        Some(serde_json::Value::Object(o)) => {
            let n = o
                .get("packages")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(format!("package.json · workspaces.packages（{n} 项）"))
        }
        _ => None,
    }
}

fn has_typescript(root: &Path) -> bool {
    std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n == "tsconfig.json" || (n.starts_with("tsconfig.") && n.ends_with(".json"))
            })
        })
        .unwrap_or(false)
}

/// 一条龙要用到的几个脚本。**按固定顺序取**，界面上不会每次换个样。
fn scripts(root: &Path) -> Vec<(String, String)> {
    const WANT: [&str; 8] = [
        "dev",
        "start",
        "build",
        "test",
        "lint",
        "preview",
        "serve",
        "typecheck",
    ];
    let Some(text) = read(root, "package.json") else {
        return Vec::new();
    };
    let Ok(j) = serde_json::from_str::<serde_json::Value>(crate::util::bom::strip_bom(&text)) else {
        return Vec::new();
    };
    let Some(obj) = j.get("scripts").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    WANT.iter()
        .filter_map(|k| {
            obj.get(*k)
                .and_then(|v| v.as_str())
                .map(|v| ((*k).to_string(), v.to_string()))
        })
        .collect()
}

/// 从依赖里认框架。包名出现在 `dependencies` 或 `devDependencies` 都算。
fn frameworks(root: &Path) -> Vec<String> {
    const KNOWN: [(&str, &str); 10] = [
        ("vite", "Vite"),
        ("next", "Next.js"),
        ("nuxt", "Nuxt"),
        ("@vue/cli-service", "Vue CLI"),
        ("react-scripts", "Create React App"),
        ("@sveltejs/kit", "SvelteKit"),
        ("astro", "Astro"),
        ("@angular/core", "Angular"),
        ("expo", "Expo"),
        ("@remix-run/dev", "Remix"),
    ];
    let Some(text) = read(root, "package.json") else {
        return Vec::new();
    };
    let Ok(j) = serde_json::from_str::<serde_json::Value>(crate::util::bom::strip_bom(&text)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (dep, label) in KNOWN {
        let has = j
            .get("dependencies")
            .and_then(|v| v.get(dep))
            .is_some()
            || j.get("devDependencies").and_then(|v| v.get(dep)).is_some();
        if has {
            out.push(label.to_string());
        }
    }
    out
}

fn read(root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(root.join(name)).ok()
}

/// 从一段 TOML 里抠 `[表]` 下面某个键的值。
///
/// 手写扫描而不引 TOML 解析器：这里只要读一个 `node = "20"`，
/// 而解析器读不出来的那些写法会导致"整份配置一片空白"——
/// 那比读得慢糟得多。
fn toml_table_value(text: &str, table: &str, key: &str) -> Option<String> {
    let want = format!("[{table}]");
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_table = t == want;
            continue;
        }
        if !in_table {
            continue;
        }
        let Some((k, v)) = t.split_once('=') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let v = v.trim().split('#').next().unwrap_or("").trim();
        let v = v.trim_matches('"').trim_matches('\'');
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::let_underscore_must_use)]

    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jw-web-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(d: &Path, name: &str, body: &str) {
        std::fs::write(d.join(name), body).unwrap();
    }

    #[test]
    fn 没有_package_json_就不算_node_项目() {
        let d = tmp("empty");
        std::fs::create_dir_all(d.join("node_modules").join("vue")).unwrap();
        assert!(probe_web(&d).is_none(), "只有 node_modules 不是项目");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `packageManager` 是**项目自己写下来的要求**，比锁文件硬。
    #[test]
    fn corepack_字段优先于锁文件() {
        let d = tmp("corepack");
        write(
            &d,
            "package.json",
            r#"{"name":"x","packageManager":"pnpm@9.1.0+sha224.abc123"}"#,
        );
        write(&d, "yarn.lock", "");
        let f = probe_web(&d).unwrap();
        assert_eq!(f.package_manager.as_deref(), Some("pnpm"));
        assert_eq!(f.pm_pinned.as_deref(), Some("9.1.0"), "sha 那段不是版本");
    }

    /// 两个锁文件并存 = 谁后装的说了算，**不猜**。
    #[test]
    fn 两个锁文件并存时不猜() {
        let d = tmp("twolocks");
        write(&d, "package.json", r#"{"name":"x"}"#);
        write(&d, "package-lock.json", "{}");
        write(&d, "pnpm-lock.yaml", "");
        let f = probe_web(&d).unwrap();
        assert!(f.package_manager.is_none(), "两个锁文件不许挑一个");
        assert!(
            f.notes.iter().any(|n| n.contains("并存")),
            "要如实说清为什么没结论：{:?}",
            f.notes
        );
    }

    /// **PnP 项目没有 node_modules 是正常的**——这正是原来会误报的那一类。
    #[test]
    fn pnp_项目被认出来() {
        let d = tmp("pnp");
        write(&d, "package.json", r#"{"name":"x"}"#);
        write(&d, "yarn.lock", "");
        write(&d, ".pnp.cjs", "// pnp\n");
        let f = probe_web(&d).unwrap();
        assert_eq!(f.package_manager.as_deref(), Some("yarn"));
        assert!(f.link_kind.starts_with("yarn PnP"), "{}", f.link_kind);
        assert_eq!(f.link_from, ".pnp.cjs");
    }

    #[test]
    fn pnpm_的隔离布局有指纹() {
        let d = tmp("pnpm");
        write(&d, "package.json", r#"{"name":"x"}"#);
        write(&d, "pnpm-lock.yaml", "");
        std::fs::create_dir_all(d.join("node_modules").join(".pnpm")).unwrap();
        let f = probe_web(&d).unwrap();
        assert_eq!(f.package_manager.as_deref(), Some("pnpm"));
        assert!(f.link_kind.starts_with("pnpm 隔离布局"), "{}", f.link_kind);
    }

    /// 版本要求**一处一条**：`.nvmrc` 和 `engines.node` 写得不一样时，
    /// 这个矛盾本身就是最该看到的信息。
    #[test]
    fn 版本要求一处一条不合并() {
        let d = tmp("pins");
        write(
            &d,
            "package.json",
            r#"{"name":"x","engines":{"node":">=18"}}"#,
        );
        write(&d, ".nvmrc", "20.11.0\n");
        write(&d, ".tool-versions", "nodejs 20.11.0\npython 3.12\n");
        write(&d, "mise.toml", "[tools]\nnode = \"18.19\"\n");
        let f = probe_web(&d).unwrap();
        let got: Vec<(String, String)> = f
            .node_pins
            .iter()
            .map(|p| (p.from.clone(), p.spec.clone()))
            .collect();
        assert!(got.contains(&(".nvmrc".into(), "20.11.0".into())), "{got:?}");
        assert!(
            got.contains(&("package.json · engines.node".into(), ">=18".into())),
            "{got:?}"
        );
        assert!(
            got.contains(&(".tool-versions".into(), "20.11.0".into())),
            "只该取 nodejs 那一行：{got:?}"
        );
        assert!(got.contains(&("mise.toml".into(), "18.19".into())), "{got:?}");
        assert_eq!(got.len(), 4, "四处要求，不许合并：{got:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 框架和脚本都认得出来() {
        let d = tmp("fw");
        write(
            &d,
            "package.json",
            r#"{"name":"x","workspaces":["apps/*"],
                "scripts":{"dev":"vite","build":"vite build","test":"vitest run","typecheck":"tsc --noEmit"},
                "devDependencies":{"vite":"^5","typescript":"^5"}}"#,
        );
        let f = probe_web(&d).unwrap();
        assert_eq!(f.frameworks, vec!["Vite".to_string()]);
        assert!(f.workspaces.is_some());
        let names: Vec<&str> = f.scripts.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, vec!["dev", "build", "test", "typecheck"], "顺序固定");
    }

    /// **`package.json` 带 BOM 也要读得出依赖**（2026-10-18 实测换的）。
    ///
    /// Windows 上记事本、PowerShell 的 `Set-Content -Encoding utf8` 默认写 BOM。
    /// 实测同一个 Node 项目：
    ///
    /// ```text
    /// 带 BOM → 「还查不出来 / their_gap」        ← 错
    /// 无 BOM → 「1 个依赖没装：left-pad」        ← 对
    /// ```
    ///
    /// **一个 BOM 就能让工具从"说准"退化成"查不了"。**
    #[test]
    fn 带bom的package_json也能读出依赖() {
        let d = tmp("bom");
        // 用 `scripts` 钉：它也是从 package.json 读的，BOM 会让它变空。
        let body = r#"{"name":"bomapp","scripts":{"build":"node build.js","start":"node index.js"}}"#;
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(body.as_bytes());
        std::fs::write(d.join("package.json"), &bytes).unwrap();

        assert!(is_node_project(&d), "有 package.json 就是 Node 项目");
        let f = probe_web(&d).expect("该读得出事实");
        // BOM 不许让 scripts 变空——它就是用户读得懂的那几个命令。
        assert!(
            f.scripts.iter().any(|(k, _)| k == "build"),
            "BOM 不许让 scripts 变空：{:?}",
            f.scripts
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
