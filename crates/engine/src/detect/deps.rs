//! 依赖：把「清单里声明的」和「机器上实际的」摆成两列。
//!
//! 为什么单独一个模块：每种语言的清单长得都不一样。拿 Python 那套去读
//! 别的语言，界面就会一本正经地显示「未声明依赖」——那不是简洁，是撒谎。
//!
//! 另一条底线：**确认不了就说确认不了**。C++ 的包可能来自系统包管理器、
//! vcpkg、Conan 或者源码树，我们没有可靠办法知道它装没装；
//! 那就显示「无法确认」，绝不报成「未安装」。

use crate::detect::lang::Lang;
use std::path::Path;

// Node 的锁文件解析放在 `nodelock.rs`（那里只干"解析一种外部格式"这一件事）。
// **这里再导出一遍**：对外仍然是"依赖这一层提供传递依赖图"，
// 调用方不必知道它被拆到了哪个文件里。
pub use super::nodelock::{web_tree, WebTree};

/// 一条依赖边：谁把谁捎带进来了。
/// 这就是 EMI 那条「recipe tree」的形状——把一件复杂的事拆开，
/// 让你看见每一步、以及最后到底拉进来多少个包。
#[derive(Debug, Clone, PartialEq)]
pub struct TreeEdge {
    pub from: String,
    pub to: String,
}

/// 从 Cargo.lock 读依赖图。
///
/// 为什么不用猜：Cargo.lock 里每个 `[[package]]` 都写着自己的 `dependencies`，
/// 间接依赖是**记在案的事实**，不是推断出来的。
///
/// （原来这里写着"别的语言没有这么一份现成的东西"——**那句话对 npm 是错的**：
/// `package-lock.json` 的 `packages` 段里，键就是"被装到哪儿"的路径，
/// 嵌套关系写在键里。见 `web_tree`。）
pub fn rust_tree(root: &Path) -> (Vec<TreeEdge>, usize) {
    let Some(text) = find_up(root, "Cargo.lock", 3).and_then(|p| std::fs::read_to_string(p).ok())
    else {
        return (Vec::new(), 0);
    };
    let mut pkgs: Vec<(String, Vec<String>)> = Vec::new();
    let mut cur: Option<(String, Vec<String>)> = None;
    let mut in_deps = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            if let Some(p) = cur.take() {
                pkgs.push(p);
            }
            cur = Some((String::new(), Vec::new()));
            in_deps = false;
            continue;
        }
        let Some(p) = cur.as_mut() else { continue };
        if line.starts_with("dependencies = [") {
            in_deps = true;
            continue;
        }
        if in_deps {
            if line.starts_with(']') {
                in_deps = false;
                continue;
            }
            let name = line
                .trim_matches(|c| c == '"' || c == ',' || c == ' ' || c == '\t')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                p.1.push(name);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("name = ") {
            p.0 = rest.trim().trim_matches('"').to_string();
        }
    }
    if let Some(p) = cur.take() {
        pkgs.push(p);
    }
    let total = pkgs.len();
    let mut edges = Vec::new();
    for (name, deps) in &pkgs {
        if name.is_empty() {
            continue;
        }
        for d in deps {
            edges.push(TreeEdge { from: name.clone(), to: d.clone() });
        }
    }
    (edges, total)
}

/// 汇总「声明 → 实际」两列。两条取数的路径（GUI 的完整探测、CLI 的快速探测）
/// 都走这里，免得同一个界面在两处各算一遍、算出不一样的结果。
///
/// `py_installed` 为空 = 这次没有 pip 清单（快速路径）：
/// 那种情况下 Python 依赖标「无法确认」，**不猜成未安装**。
pub fn collect(
    root: &Path,
    langs: &[Lang],
    contract: Option<&crate::understand::contract::Contract>,
    py_installed: &[String],
) -> Vec<crate::facts::DepDetail> {
    let mut decls = declared_all(root, langs);
    if let Some(c) = contract {
        for d in c.deps.required.iter() {
            if !decls.iter().any(|x| &x.name == d) {
                decls.push(Decl {
                    name: d.clone(),
                    want: String::new(),
                    from: "env.contract.json".into(),
                    lang: Lang::Unknown,
                    kind: DepKind::Declared,
                });
            }
        }
    }
    let mut detail = Vec::new();
    for d in decls {
        let v = if d.lang == Lang::Python && py_installed.is_empty() {
            // **"没有 pip 清单"是我们的失败，不是"没装"。**
            // `collect` 的文档写着空 = "这次没有 pip 清单（快速路径）"——
            // 也就是说**问工具的那一步没做或没给答案**，而我们**确知**这一点
            // （它是有意跳过的快速路径，不是"不知道跳没跳"）。所以归 `OurGap` 有依据。
            Verdict::Unknown
        } else {
            installed(root, d.lang, &d.name, py_installed)
        };
        // **这里原来还顺手产出一个 `BTreeMap<String, bool>`（`deps_status`），2026-10-16 删了。**
        //
        // 它是 `detail` 的弱化副本：`Unknown` 插进去就是 `false`，于是"没查过"
        // 在任何读它的地方都等于"没装"。这个仓库为那个错修过两次消费者
        // （规则层的假 Error、`snapshot.rs` 的"依赖 都装了"），第三次直接掐掉来源——
        // **类型装不下三态，就不该存在**。三态全在下面这个 `DepDetail` 里
        // （`ok` / `unknown` / `have` / `from`）。
        let (ok, unknown, have) = match v {
            Verdict::Yes(ver) => (true, false, ver.unwrap_or_default()),
            Verdict::No => (false, false, String::new()),
            Verdict::Unknown => (false, true, String::new()),
        };
        detail.push(crate::facts::DepDetail {
            name: d.name,
            want: d.want,
            have,
            ok,
            unknown,
            from: d.from,
            // **"装没装"是从哪儿查的**——这条结论的证据（2026-10-16）。
            // 由这一层给：只有它知道刚才实际看了哪儿（几种语言各不相同）。
            evidence: evidence_of(root, d.lang, py_installed.is_empty()),
        });
    }
    detail
}
/// **这一条"装没装"是从哪儿查的**——结论的证据（2026-10-16）。
///
/// `AGENTS.md` §2 要求每条结论可反驳，而依赖这一栏原来只有结论没有来源。
/// 四种语言的查法**各不相同**，所以这不是套话——说出来用户就知道去哪儿核对：
///   Rust  → `Cargo.lock`
///   Web   → `node_modules`（可能被提升到仓库根，所以要往上找）
///   Java  → `~/.m2/repository`
///   Python→ `pip list` 的输出（没给就是"没有可查的地方"）
///
/// **"没查过"与"没装"在文字上必须长得不一样**（三态在措辞上的落法）。
fn evidence_of(root: &Path, lang: Lang, no_py_list: bool) -> String {
    if lang == Lang::Python {
        return if no_py_list {
            "没有可查的地方（这次没拿到 pip list）".to_string()
        } else {
            "pip list".to_string()
        };
    }
    match lang {
        Lang::Rust | Lang::Unknown if root.join("Cargo.toml").is_file() => {
            "Cargo.lock".to_string()
        }
        Lang::Web => {
            if crate::detect::web::is_pnp(root) {
                "没有可查的地方（PnP 项目的依赖表在 .pnp.cjs 里，我们还没读它）".to_string()
            } else {
                "node_modules".to_string()
            }
        }
        Lang::Java => "~/.m2/repository".to_string(),
        Lang::Cpp => "没有可查的地方（C++ 的包可能来自系统 / vcpkg / Conan，我们没实现）".to_string(),
        _ => "没有可查的地方".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decl {    /// 依赖名（Rust 用 crate 名、Java 用 groupId:artifactId）
    pub name: String,
    /// 清单里写的版本。空 = 没写
    pub want: String,
    /// 从哪个文件看出来的
    pub from: String,
    /// 这条来自哪门语言的清单。查「装没装」的方式由它决定。
    pub lang: Lang,
    /// 运行时依赖还是只在开发期用的。清单里就写着（dependencies /
    /// devDependencies / dev-dependencies），照着读就行——Mod Menu 的做法
    /// 是「把库从正经内容里筛出去」，同一条道理：混在一起就永远看不清。
    pub kind: DepKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DepKind {
    Runtime,
    Dev,
    /// 契约里额外声明、清单里没有的
    Declared,
}

impl DepKind {
    pub fn id(&self) -> &'static str {
        match self {
            DepKind::Runtime => "runtime",
            DepKind::Dev => "dev",
            DepKind::Declared => "declared",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// 装上了（可能带版本号）
    Yes(Option<String>),
    /// 明确没装
    No,
    /// 这个语言没有可靠的确认手段
    Unknown,
}

pub(super) fn read(root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(root.join(name)).ok()
}

/// 从当前目录往上找某个文件。
///
/// 为什么必须往上找：workspace 的成员（crates/engine 这种）自己目录里
/// 没有 Cargo.lock，锁文件在工作区根上。只看当前目录，就会把已经装好的
/// 依赖报成「未安装」——一个误报就够让整栏不可信了。
fn find_up(root: &Path, name: &str, max_hops: usize) -> Option<std::path::PathBuf> {
    let mut cur = Some(root);
    for _ in 0..=max_hops {
        let dir = cur?;
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
        cur = dir.parent();
    }
    None
}

/// 往上找某个**目录**（`find_up` 的目录版）。
/// `node_modules` 会被包管理器提升到工作区根上，所以它必须往上找。
fn find_dir_up(root: &Path, name: &str, max_hops: usize) -> Option<std::path::PathBuf> {
    let mut cur = Some(root);
    for _ in 0..=max_hops {
        let dir = cur?;
        let p = dir.join(name);
        if p.is_dir() {
            return Some(p);
        }
        cur = dir.parent();
    }
    None
}

/// Node 的一个依赖装没装，装在**哪个 node_modules** 里。
///
/// 版本从 `node_modules/<名字>/package.json` 的 `version` 读——
/// 这是"实际装了什么版本"唯一的就地凭据（锁文件说的是"该装什么"）。
/// pnpm 的隔离布局下 `<名字>` 是指向 `.pnpm/…` 的符号链接，跟着走就对。
fn node_installed(nm: &Path, name: &str) -> Verdict {
    let p = nm.join(name);
    if !p.is_dir() {
        return Verdict::No;
    }
    let ver = std::fs::read_to_string(p.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|j| {
            j.get("version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
    Verdict::Yes(ver)
}

/// 声明了哪些依赖（只按一门语言）。
pub fn declared(root: &Path, lang: Lang) -> Vec<Decl> {
    let out = match lang {
        Lang::Python => python_decls(root),
        Lang::Web => node_decls(root),
        Lang::Java => java_decls(root),
        Lang::Rust => rust_decls(root),
        Lang::Cpp => cpp_decls(root),
        // `Unknown` 不再替 Rust 顶班——那是 `lang::Lang` 少一个变体时的绕道，
        // 变体补上之后，这里只留"Cargo.toml 在但语言没认出来"这一种真实情况
        // （比如项目太深、`has_ext` 只扫两层没扫到）。
        Lang::Unknown if root.join("Cargo.toml").is_file() => rust_decls(root),
        _ => Vec::new(),
    };
    // 顺序固定：清单里的顺序不等于人想看的顺序，而且测试和界面都要稳定
    let mut sorted = out;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted.dedup_by(|a, b| a.name == b.name);
    sorted
}

/// 复合项目的依赖：按检测到的**所有**语言取并集。
/// 只挑一个「主语言」是不行的——一个 Tauri 项目同时有 Cargo.toml 和
/// package.json，只报一边就等于告诉用户「另一边不存在」。
pub fn declared_all(root: &Path, langs: &[Lang]) -> Vec<Decl> {
    let mut list: Vec<Lang> = langs.to_vec();
    // `Unknown` 兜底：语言没认出来、但 Cargo.toml 在时，Rust 依赖照样要报
    // （`Lang::Rust` 正常会由检测补进 `langs`，这一条只是保险）。
    if root.join("Cargo.toml").is_file() && !list.contains(&Lang::Unknown) && !list.contains(&Lang::Rust) {
        list.push(Lang::Unknown);
    }
    let mut out: Vec<Decl> = Vec::new();
    for l in list {
        for d in declared(root, l) {
            if !out.iter().any(|x| x.name == d.name) {
                out.push(d);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ── Rust：Cargo.toml 的 [dependencies] 一族 ─────────────────
fn rust_decls(root: &Path) -> Vec<Decl> {
    let Some(text) = read(root, "Cargo.toml") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        let kind = match section.as_str() {
            "dependencies" => DepKind::Runtime,
            "dev-dependencies" | "build-dependencies" => DepKind::Dev,
            _ => continue,
        };
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let name = k.trim().to_string();
        let v = v.trim();
        let want = if v.starts_with('{') {
            // 行内表：{ version = "1.0", features = [...] }
            v.trim_matches(|c| c == '{' || c == '}')
                .split(',')
                .find_map(|kv| {
                    let (k2, v2) = kv.split_once('=')?;
                    if k2.trim() == "version" {
                        Some(v2.trim().trim_matches('"').to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        } else {
            v.trim_matches('"').to_string()
        };
        if !name.is_empty() {
            out.push(Decl { name, want, from: format!("Cargo.toml · {}", section), lang: Lang::Unknown, kind });
        }
    }
    out
}

// ── Node：package.json 的两组依赖 ──────────────────────────
fn node_decls(root: &Path) -> Vec<Decl> {
    let Some(text) = read(root, "package.json") else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, kind) in [("dependencies", DepKind::Runtime), ("devDependencies", DepKind::Dev)] {
        let from = key;
        if let Some(obj) = json.get(key).and_then(|v| v.as_object()) {
            for (name, want) in obj {
                out.push(Decl {
                    name: name.clone(),
                    want: want.as_str().unwrap_or("").to_string(),
                    from: format!("package.json · {}", from),
                    lang: Lang::Web,
                    kind,
                });
            }
        }
    }
    out
}

// ── Java：pom.xml 的 <dependency>（手写扫描，不引 XML 库）──
fn java_decls(root: &Path) -> Vec<Decl> {
    let Some(text) = read(root, "pom.xml") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("<dependency>") {
        let tail = &rest[start + "<dependency>".len()..];
        let Some(end) = tail.find("</dependency>") else {
            break;
        };
        let block = &tail[..end];
        let tag = |name: &str| -> String {
            let open = format!("<{}>", name);
            match block.find(&open) {
                Some(i) => {
                    let t = &block[i + open.len()..];
                    t.split('<').next().unwrap_or("").trim().to_string()
                }
                None => String::new(),
            }
        };
        let g = tag("groupId");
        let a = tag("artifactId");
        let v = tag("version");
        if !a.is_empty() {
            let scope = tag("scope");
            out.push(Decl {
                name: if g.is_empty() { a } else { format!("{}:{}", g, a) },
                want: v,
                from: if scope.is_empty() { "pom.xml".to_string() } else { format!("pom.xml · {}", scope) },
                lang: Lang::Java,
                kind: if scope == "test" { DepKind::Dev } else { DepKind::Runtime },
            });
        }
        rest = &tail[end + "</dependency>".len()..];
    }
    out
}

// ── C++：CMakeLists 里的 find_package ───────────────────────
fn cpp_decls(root: &Path) -> Vec<Decl> {
    let Some(text) = read(root, "CMakeLists.txt") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let lower = line.to_lowercase();
        if !lower.starts_with("find_package(") {
            continue;
        }
        let inner = line
            .trim_start_matches(|c: char| c != '(')
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();
        let name = inner.split_whitespace().next().unwrap_or("").to_string();
        if !name.is_empty() {
            out.push(Decl { name, want: String::new(), from: "CMakeLists.txt".into(), lang: Lang::Cpp, kind: DepKind::Runtime });
        }
    }
    out
}

// ── Python：requirements.txt（由 contract 那侧统一解析）────
fn python_decls(root: &Path) -> Vec<Decl> {
    crate::understand::contract::Contract::declared_deps_spec(root)
        .into_iter()
        .map(|(name, want)| Decl { name, want, from: "requirements.txt".into(), lang: Lang::Python, kind: DepKind::Runtime })
        .collect()
}

/// **Java 依赖"确认不了"的具体原因**（2026-10-16）。
///
/// 两个出口，**都是"我们"这边**（见 `installed` 的 `Lang::Java` 分支）：
///   ① 读不到 `USERPROFILE`（连本地仓库在哪都不知道）；
///   ② `~/.m2/repository` 不在 —— 说明**这台机器还没跑过 Maven**，
///      那么"装没装"这件事**没有可查的地方**（不是用户少装了什么）。
pub fn why_unknown_java() -> String {
    if std::env::var_os("USERPROFILE").is_none() {
        "读不到 USERPROFILE，所以连本地仓库在哪都不知道".to_string()
    } else {
        "本地 Maven 仓库（~/.m2/repository）还不存在——这台机器没跑过 Maven，\
         所以没有能查的地方"
            .to_string()
    }
}

/// **Web 依赖"确认不了"的具体原因**（2026-10-16）。
///
/// 原来调用方只能说一句笼统的"确认不了"，用户分不出是"你没装"还是"我们没查到"。
/// 而这一处**原因很具体**：PnP 项目的依赖可见性写在 `.pnp.cjs` 里，
/// **解析它要跑 JS，不在这条路径上**——那是**我们没做**，不是用户的问题。
///
/// **为什么不直接用 `Unknown::OurGap` 那句话**：`rules.rs` 那边的做法说明了更好的答案——
/// **能说出具体原因的地方，比一个笼统的归属标签更好**。
/// `what_to_do()` 那三句是"不知道该怪谁时"的兜底；这里知道，就说知道的那句。
pub fn why_unknown_web(root: &Path) -> String {
    if crate::detect::web::is_pnp(root) {
        "这是 Yarn PnP 项目：依赖装在 `.pnp.cjs` 那张映射表里，\
         读出它要跑一次 JS，我们还没做这一步"
            .to_string()
    } else {
        // 走到这里说明"未知"来自别处——**不硬编一个原因**（编出来的原因比不编更糟）。
        crate::Unknown::OurGap.what_to_do().to_string()
    }
}

/// 装没装。`py_installed` 是 pip list 的结果（`名字==版本`），只在 Python 下用。
pub fn installed(root: &Path, lang: Lang, name: &str, py_installed: &[String]) -> Verdict {
    // **判据是 `lang`，不是"这个目录里有没有 Cargo.toml"**（2026-10-16 修）。
    //
    // 原来这里写的是 `root.join("Cargo.toml").is_file() && lang != Lang::Python`——
    // 于是**在一个 Rust + 前端的项目里（Tauri 就是），每一条 npm 依赖都被拿去查
    // `Cargo.lock`**：查不到 → `No` → 报「没装」。
    //
    // 实测（这是它怎么被发现的）：`jingwei guide --root D:\projects\jingwei` 报
    // 「1 个依赖没装：@tauri-apps/cli」，而 `package.json` 声明了它、
    // `node_modules/@tauri-apps/cli/package.json` **实实在在在磁盘上（2454 字节）**。
    // **一个假 Error**——用户会去重装一个已经装好的包，然后发现问题依旧。
    //
    // 它与 `Lang::Rust` 变体补上之前那三次"绕道"是同一类错：
    // **用"环境里有什么文件"代替"这条依赖属于哪门语言"**。
    // 依赖的归属由 `Decl.lang` 一路带下来，这里是它该被用上的地方。
    //
    // `Lang::Unknown` 兜底保留旧行为：语言没认出来、但 `Cargo.toml` 在时，
    // 那确实是"Rust 项目没认出来"（见 `declared` 里同一条兜底），按 Rust 查是对的。
    let rust_like = matches!(lang, Lang::Rust)
        || (matches!(lang, Lang::Unknown) && root.join("Cargo.toml").is_file());
    if rust_like {
        // Rust：Cargo.lock 记着实际锁定的版本。没有 lock 就是没装过依赖。
        let Some(lock_path) = find_up(root, "Cargo.lock", 3) else {
            return Verdict::No;
        };
        let Ok(lock) = std::fs::read_to_string(&lock_path) else {
            return Verdict::Unknown;
        };
        let needle = format!("name = \"{}\"", name);
        if !lock.lines().any(|l| l.trim() == needle) {
            return Verdict::No;
        }
        let mut lines = lock.lines();
        while let Some(l) = lines.next() {
            if l.trim() == needle {
                if let Some(v) = lines.next() {
                    let v = v.trim();
                    if let Some(rest) = v.strip_prefix("version = ") {
                        return Verdict::Yes(Some(rest.trim_matches('"').to_string()));
                    }
                }
            }
        }
        return Verdict::Yes(None);
    }
    match lang {
        Lang::Web => {
            // ① **PnP 项目压根没有 node_modules**——照旧逻辑会把它整个报成
            //    "未安装"，那是实打实的误报。依赖可见性是 `.pnp.cjs` 那份映射
            //    表说了算，而解析它要跑 JS，不在这条路径上。所以：确认不了。
            if crate::detect::web::is_pnp(root) {
                return Verdict::Unknown;
            }
            // ② `node_modules` 可能被**提升到仓库根**（npm / yarn / pnpm 的
            //    workspace 都这么干）。只看当前目录，成员项目的依赖会全被报成
            //    未安装——和 Rust 那边为什么要往上找 `Cargo.lock` 是同一个理由。
            match find_dir_up(root, "node_modules", 3) {
                Some(nm) => node_installed(&nm, name),
                None => Verdict::No,
            }
        }
        Lang::Java => {
            let Some(home) = std::env::var_os("USERPROFILE") else {
                return Verdict::Unknown;
            };
            let repo = std::path::PathBuf::from(home).join(".m2").join("repository");
            if !repo.is_dir() {
                return Verdict::Unknown;
            }
            let (g, a) = match name.split_once(':') {
                Some((g, a)) => (g, a),
                None => ("", name),
            };
            let dir = if g.is_empty() {
                repo.join(a)
            } else {
                repo.join(g.replace('.', "/")).join(a)
            };
            if dir.is_dir() {
                Verdict::Yes(None)
            } else {
                Verdict::No
            }
        }
        Lang::Cpp => Verdict::Unknown,
        _ => {
            let norm = crate::understand::contract::norm_name(name);
            py_installed
                .iter()
                .find_map(|s| {
                    let (n, v) = s.split_once("==")?;
                    if crate::understand::contract::norm_name(n) == norm {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                })
                .map(|v| Verdict::Yes(Some(v)))
                .unwrap_or(Verdict::No)
        }
    }
}

#[cfg(test)]
#[path = "deps_tests.rs"]
mod tests;
