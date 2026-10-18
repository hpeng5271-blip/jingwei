//! 项目生命周期里「还没进去」的那一段：发现项目、读懂它、替它起草契约。
//!
//! 新手最需要被省掉的一步就是「先写一份配置」。所以这里反着来：
//! 先扫，再猜，猜出来的每一条都说清是从哪儿看出来的，最后交给人改。
use crate::understand::contract::{Contract, ReadyWhen};
use crate::detect::lang::Lang;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// 文件名就是判据：出现这些，这个目录就算一个项目根。
//
// **判据已经不在这个文件里了**（2026-10-17）。
//
// 这里原来有个 `MARKERS` 常量，它自己的注释写着
// 「**这是唯一一份标记表**……两处各写一份标记表，迟早会出现
// '这个目录算不算项目' 两种答案」——
// **它预言了那个 bug，却没阻止它**：`project.rs` 的 `manifest_kind` 是第二份，
// 两份还不一样（`manifest_kind` 认 `settings.gradle.kts` / `requirements-dev.txt` /
// `.fsproj` / `.sln`，这份不认）。
// 后果实测：只含 `settings.gradle.kts` 的目录，`doctor` 找到清单，
// `guide` 却说「**没有任何项目标记文件**」。
//
// **教训：注释说"只有一份"不等于只有一份——只有一处实现才算。**
// 现在判据只有 `project::manifest_kind`，本文件与 `container.rs` 都问它。

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Found {
    pub path: String,
    pub name: String,
    /// 判它是个项目的那些文件
    pub markers: Vec<String>,
    /// 语言标签（和体检用同一套判定）
    pub kinds: Vec<String>,
    pub has_git: bool,
    pub has_contract: bool,
    /// 最近改动（秒），用来排序：刚碰过的排前面
    pub touched: u64,
    /// 离今天多少天
    pub age_days: u64,
}

fn mtime_secs(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 这个目录里的项目标志文件（小写比较，返回原始文件名）。
fn markers_in(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        // **"什么算项目标记"只有一处判据**（`project::manifest_kind`）。
        //
        // 原来这里是另一份（`MARKERS` 常量 + 特判 `.csproj`/`.sln`/`.fsproj`），
        // 它**没有** `settings.gradle.kts`、也没有 `requirements-dev.txt`——
        // 而 `manifest_kind` 有。后果（实测）：只含 `settings.gradle.kts` 的目录，
        // `doctor` 找到清单，`guide` 却说「没有任何项目标记文件」。
        if crate::understand::project::manifest_kind(&name).is_some() {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// 这个目录**看起来像不像一个项目**：有没有项目标记文件（或自己是个 git 仓库）。
///
/// 放宽到 `pub(crate)`（2026-10-16）：`guide` 要用它区分两句话——
/// 「**这里不是一个项目**」与「**这是一个项目、但你还没写契约**」。
/// 那两句对用户要做的事完全不同：前者他不该动，后者他补一份就有答案。
/// 原来它是私有的，于是 `guide` 只能一律说"你缺了 env.contract.json"，
/// **对语言安装目录（Python 的 `Lib/`）就是错的**——照做换来的是
/// 一个"假项目能跑"的结论，比不判断更坏。
pub(crate) fn looks_like_project(dir: &Path) -> bool {
    // **`env.contract.json` 也算**（2026-10-17，被界面门禁的新断言打出来的）。
    //
    // 那个夹具里只有 `env.contract.json` 与一个 `target/`——于是被判成"不像一个项目"，
    // 体检页当场折叠，屏幕上写「这里不像一个项目——所以下面不给项目建议」。
    //
    // 可 `env.contract.json` 是**我们自己让用户建的**（`jingwei init`）。
    // 建完之后再说"这不是项目"，等于**把用户刚做对的那一步作废**。
    !markers_in(dir).is_empty() || dir.join(".git").is_dir() || dir.join("env.contract.json").is_file()
}

/// 从某个父目录往下找出所有项目。`depth` 是往下看几层。
///
/// ## 2026-10-18：**它现在是索引之上的一层皮**
///
/// 原来这里是"一边走一边判"（`walk_from` 命中项目就 `push` + `return`）。
/// 那样有两个后果：**规则一改就得重扫**、**"为什么这个目录被跳过"没有记录**。
/// 现在改成：`dirs_index` 先把目录表建好（每行带 mtime / 标记文件 / 跳过原因），
/// 这里只做两件事——**查"最外层项目"** + 给每个补上界面要的字段（`describe`）。
///
/// **口径没变**（遇项目就不再往里算），变的是它从哪儿来。
/// 为什么必须统一：`jingwei projects` 已经走索引了，界面还走旧的会**两套口径并存**——
/// 那正是这个仓库最熟的事故形状（两份清单各写各的，必然漂）。
pub fn discover(parent: &Path, depth: usize) -> Vec<Found> {
    if !parent.is_dir() {
        return Vec::new();
    }
    let now = now_secs();
    let (idx, _reuse) =
        crate::understand::dirs_index::build_cached(parent, depth.max(1), None);
    let mut out: Vec<Found> = idx
        .top_level_projects()
        .iter()
        .map(|p| describe(p, now))
        .collect();
    out.sort_by(|a, b| b.touched.cmp(&a.touched).then(a.name.cmp(&b.name)));
    out
}

/// **体量**：文件数 + 字节数，**有上限**（4000 个文件或 120ms 就停）。
///
/// 为什么要有上限：首页按体量分档只需要"小/中/大/超大"这个量级，
/// 为一个档位把 node_modules 全读一遍是**拿用户的风扇换一个数字**。
/// 返回 `(文件数, 字节数, 是否被上限截断)`——截断了就说截断了，不假装是真值。
pub fn measure(dir: &Path) -> (u64, u64, bool) {
    let t0 = std::time::Instant::now();
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if files >= 4000 || t0.elapsed().as_millis() > 120 {
            return (files, bytes, true);
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || crate::util::scan::is_not_source_dir(&name) {
                    continue;
                }
                stack.push(e.path());
            } else if ft.is_file() {
                files += 1;
                bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
                if files >= 4000 {
                    return (files, bytes, true);
                }
            }
        }
    }
    (files, bytes, false)
}

/// 给外壳用：判一个目录是不是项目、是什么类型（**与 discover 同一套**）。
pub fn describe_public(dir: &Path) -> Found {
    describe(dir, now_secs())
}

/// **默认扫描根**：常见放代码/研究/克隆的地方（2026-10-19）。
///
/// 为什么要有它：界面的默认父目录原来是"存在哪个用哪个"，于是
/// **用户真实项目常常一个都没扫到**，却看不出是"没扫"还是"没有"。
pub fn default_scan_roots() -> Vec<std::path::PathBuf> {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for c in [r"D:\projects", r"D:\research", r"D:\code", r"D:\src"] {
        out.push(std::path::PathBuf::from(c));
    }
    if !home.is_empty() {
        out.push(std::path::Path::new(&home).join("source").join("repos"));
        out.push(std::path::Path::new(&home).join("Desktop"));
    }
    out.retain(|p| p.is_dir());
    out
}

fn describe(dir: &Path, now: u64) -> Found {
    let markers = markers_in(dir);
    let mut touched = mtime_secs(dir);
    for m in &markers {
        touched = touched.max(mtime_secs(&dir.join(m)));
    }
    Found {
        path: dir.to_string_lossy().to_string(),
        name: dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string()),
        has_git: dir.join(".git").is_dir(),
        has_contract: dir.join("env.contract.json").is_file(),
        kinds: crate::detect::lang::detect_all(dir)
            .iter()
            .map(|l| l.name().to_string())
            .collect(),
        age_days: if touched > 0 {
            now.saturating_sub(touched) / 86400
        } else {
            0
        },
        touched,
        markers,
    }
}

/// 给一个项目起草契约。返回 (契约, 依据) —— 依据是给人看的：
/// 「我是从哪个文件哪一行看出来的」。看不懂的推导不去猜。
pub fn draft(root: &Path) -> (Contract, Vec<String>) {
    let mut why: Vec<String> = Vec::new();
    let has = |f: &str| root.join(f).is_file();
    let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap_or_default();
    let mut c = Contract {
        schema_version: Some(1),
        name: root
            .file_name()
            .map(|s| s.to_string_lossy().to_string()),
        description: Some("由 jingwei 扫描项目后起草，命令可以改。".into()),
        ..Default::default()
    };
    c.deps.required = Contract::declared_deps(root);

    if has("package.json") {
        let text = read("package.json");
        c.commands.install = Some("npm install".into());
        why.push("有 package.json：装依赖用 npm install".into());
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(crate::util::bom::strip_bom(&text)) {
            let scripts = v.get("scripts").and_then(|s| s.as_object());
            if scripts.map(|s| s.contains_key("build")).unwrap_or(false) {
                c.commands.build = Some("npm run build".into());
            }
            for k in ["dev", "start", "serve", "build"] {
                if scripts.map(|s| s.contains_key(k)).unwrap_or(false) {
                    c.commands.run = Some(format!("npm run {}", k));
                    why.push(format!("package.json 的 scripts 里有 {}，所以用它启动", k));
                    break;
                }
            }
            if scripts.map(|s| s.contains_key("test")).unwrap_or(false) {
                c.commands.test = Some("npm test".into());
            }
            // 前端项目的默认端口
            if v.get("dependencies")
                .map(|d| d.to_string().contains("vite") || d.to_string().contains("vue"))
                .unwrap_or(false)
            {
                c.ready_when = Some(ReadyWhen {
                    port: Some(5173),
                    timeout_s: Some(60),
                    ..Default::default()
                });
                why.push("Vite 默认端口 5173，就用它判断「起来了没有」".into());
            }
        }
    }

    if has("requirements.txt") || has("pyproject.toml") || has("setup.py") || has("Pipfile") {
        if c.commands.install.is_none() {
            c.commands.install = Some(if has("requirements.txt") {
                why.push("有 requirements.txt：用它装依赖".into());
                "python -m pip install -r requirements.txt".into()
            } else {
                why.push("有 pyproject.toml：按项目自己声明的装".into());
                "python -m pip install -e .".into()
            });
        }
        // Python 的"编译"= 语法检查 + 字节码，不出产物，所以用 compileall 当构建位
        c.commands.build = Some("python -m compileall -q .".into());
        let venvs = crate::detect::probe::scan_venvs(root);
        if let Some(v) = venvs.first() {
            let exe = Path::new(v).join("Scripts").join("python.exe");
            let exe_unix = Path::new(v).join("bin").join("python");
            let chosen = if exe.is_file() { Some(exe) } else if exe_unix.is_file() { Some(exe_unix) } else { None };
            if let Some(p) = chosen {
                c.runtime.python_exe = Some(p.to_string_lossy().to_string());
                why.push(format!("项目里有虚拟环境 {}，直接绑它——不用 activate", v));
            }
        }
        if has("manage.py") {
            c.commands.run = Some("python manage.py runserver".into());
            c.ready_when = Some(ReadyWhen { port: Some(8000), timeout_s: Some(60), ..Default::default() });
            why.push("有 manage.py：这是 Django 项目".into());
        } else if has("main.py") {
            c.commands.run = Some("python main.py".into());
            why.push("有 main.py：先按脚本跑".into());
        } else if has("app.py") {
            c.commands.run = Some("python app.py".into());
            why.push("有 app.py：先按脚本跑".into());
        }
        if has("tests") || read("requirements.txt").contains("pytest") {
            c.commands.test = Some("python -m pytest".into());
            why.push("有测试目录或 pytest 依赖".into());
        }
    }

    if has("pom.xml") {
        let text = read("pom.xml");
        c.commands.install = Some("mvn -q dependency:go-offline".into());
        c.commands.build = Some("mvn -q -DskipTests package".into());
        c.commands.test = Some("mvn test".into());
        // 真实 pom 里两种写法都有：artifactId 是 spring-boot-starter-*，
        // groupId 是 org.springframework.boot。
        if text.contains("spring-boot") || text.contains("springframework.boot") {
            c.commands.run = Some("mvn spring-boot:run".into());
            c.ready_when = Some(ReadyWhen { port: Some(8080), timeout_s: Some(120), ..Default::default() });
            why.push("pom.xml 里有 spring-boot：用 spring-boot:run 启动，默认 8080".into());
        } else {
            c.commands.run = Some("mvn -q package".into());
            why.push("普通 Maven 项目：先跑 package".into());
        }
    }
    if has("build.gradle") || has("build.gradle.kts") {
        let w = if has("gradlew.bat") { "gradlew.bat" } else { "gradle" };
        c.commands.install = Some(format!("{} build", w).replace(" build", " dependencies"));
        c.commands.build = Some(format!("{} build -x test", w));
        c.commands.run = Some(format!("{} bootRun", w));
        c.commands.test = Some(format!("{} test", w));
        why.push(format!("有 build.gradle：用 {} 跑（Gradle 各版本差异大，别用全局的）", w));
    }

    if has("Cargo.toml") {
        let text = read("Cargo.toml");
        c.commands.install = Some("cargo fetch".into());
        c.commands.build = Some("cargo build".into());
        c.commands.run = Some("cargo run".into());
        c.commands.test = Some("cargo test".into());
        why.push(if text.contains("[[bin]]") || has("src/main.rs") {
            "Rust 可执行项目：cargo run".into()
        } else {
            "Rust 库：cargo test 是主要入口".into()
        });
    }
    if has("go.mod") {
        c.commands.install = Some("go mod download".into());
        c.commands.build = Some("go build ./...".into());
        c.commands.run = Some("go run .".into());
        c.commands.test = Some("go test ./...".into());
        why.push("有 go.mod：go run . 就是入口".into());
    }
    if has("CMakeLists.txt") {
        c.commands.install = Some("cmake -S . -B build".into());
        c.commands.build = Some("cmake --build build".into());
        c.commands.run = Some("cmake --build build".into());
        c.commands.test = Some("ctest --test-dir build".into());
        why.push("有 CMakeLists.txt：先 configure 再 build".into());
    }
    if has("composer.json") {
        c.commands.install = Some("composer install".into());
        if has("artisan") {
            c.commands.run = Some("php artisan serve".into());
            why.push("有 artisan：这是 Laravel 项目".into());
        }
        c.commands.test = Some("vendor\\bin\\phpunit".into());
    }
    if has("Gemfile") {
        c.commands.install = Some("bundle install".into());
        c.commands.test = Some("bundle exec rake test".into());
        why.push("有 Gemfile：用 bundle".into());
    }
    if let Some(csproj) = first_with_ext(root, "csproj") {
        c.commands.install = Some("dotnet restore".into());
        c.commands.build = Some("dotnet build".into());
        c.commands.run = Some("dotnet run".into());
        c.commands.test = Some("dotnet test".into());
        why.push(format!("找到 {}：这是 .NET 项目", csproj));
    }

    if why.is_empty() {
        why.push("没有认出的项目文件：契约是空的，你可以自己填命令".into());
    }
    (c, why)
}

fn first_with_ext(dir: &Path, ext: &str) -> Option<String> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.to_lowercase().ends_with(&format!(".{}", ext)) {
            return Some(n);
        }
    }
    None
}

/// 起草并落盘。已有契约时保留人改过的 name/description。
pub fn write_draft(root: &Path) -> Result<(Contract, Vec<String>)> {
    let (mut c, mut why) = draft(root);
    if let (Some(old), _) = Contract::load(root) {
        if old.name.is_some() {
            c.name = old.name.clone();
        }
        if old.description.is_some() {
            c.description = old.description.clone();
        }
        // 人改过的字段原样搬回来，并明确告诉用户"我没动你改的"
        let kept = c.keep_manual(&old);
        if !kept.is_empty() {
            why.push(format!(
                "保留了 {} 个你亲手改过的字段（{}）——自动起草不动它们",
                kept.len(),
                kept.join(" / ")
            ));
        }
    }
    c.save(root)?;
    Ok((c, why))
}

/// 语言 -> 一句「这是个什么项目」。给卡片小字用。
pub fn kind_sentence(kinds: &[String]) -> String {
    if kinds.is_empty() {
        "认不出类型".into()
    } else {
        kinds.join(" + ")
    }
}

/// 还没用上，但留着：以后要给「新建项目」直接给推荐模板。
pub fn suggest_template(kinds: &[Lang]) -> Option<&'static str> {
    kinds.first().map(|l| match l {
        Lang::Python => "script",
        Lang::Web => "web",
        _ => "app",
    })
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dd-proj2-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// **"什么算项目标记"只许有一份判据**（2026-10-17）。
    ///
    /// ## 拿什么换的
    ///
    /// 原来有两份：`projects::MARKERS` 常量，和 `project::manifest_kind`。
    /// 两份不一样——`manifest_kind` 认 `settings.gradle.kts`、`requirements-dev.txt`、
    /// `.fsproj`、`.sln`，`MARKERS` 都不认。
    ///
    /// 后果（实测）：一个**只含 `settings.gradle.kts`** 的目录——
    ///   `doctor` 找到清单 `settings.gradle.kts(gradle)`；
    ///   `guide` 说「**这里不像一个项目（没有任何项目标记文件）**」。
    /// **"没有任何标记文件"是假话**——工具自己刚找到了一个。
    ///
    /// 这个测试直接钉住那句话：**`manifest_kind` 认得的，`looks_like_project` 也必须认**。
    /// 谁要是再把清单拆成两份，这里会红。
    #[test]
    fn 认得出的清单都算项目() {
        // 尽量把 `manifest_kind` 的每一条分支都覆盖到
        let names = [
            "package.json",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts", // ← 原来漏的就是它
            "requirements.txt",
            "requirements-dev.txt", // ← 也漏
            "requirements-test.txt",
            "pyproject.toml",
            "setup.py",
            "Pipfile",
            "Cargo.toml",
            "go.mod",
            "CMakeLists.txt",
            "Makefile",
            "composer.json",
            "Gemfile",
            "App.csproj",
            "App.fsproj", // ← 也漏
            "App.sln",    // ← 也漏
        ];
        for (i, name) in names.iter().enumerate() {
            assert!(
                crate::understand::project::manifest_kind(name).is_some(),
                "{name} 应该被认成项目清单"
            );
            // 单独一个文件、别的什么都没有
            let d = scratch(&format!("marker{i}"));
            std::fs::write(d.join(name), "x").unwrap();
            assert!(
                looks_like_project(&d),
                "`manifest_kind` 认 {name}，而 `looks_like_project` 不认——\
                 两份判据分叉了。这正是「没有任何项目标记文件」那句假话的来源。"
            );
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn discovering_a_parent_finds_the_projects_below_it() {
        let d = scratch("discover");
        for name in ["alpha", "beta"] {
            let p = d.join(name);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("package.json"), "{}").unwrap();
        }
        std::fs::create_dir_all(d.join("只是些杂物")).unwrap();
        let found = discover(&d, 1);
        let names: Vec<String> = found.iter().map(|f| f.name.clone()).collect();
        assert!(names.contains(&"alpha".to_string()), "{:?}", names);
        assert!(names.contains(&"beta".to_string()));
        assert_eq!(found.len(), 2, "杂物目录不该算项目：{:?}", names);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 项目里嵌项目时只报外层：不然一张卡变三张，反而看不出有几个东西。
    #[test]
    fn a_project_inside_a_project_is_not_listed_again() {
        let d = scratch("nested");
        std::fs::create_dir_all(d.join("app").join("legacy")).unwrap();
        std::fs::write(d.join("app").join("package.json"), "{}").unwrap();
        std::fs::write(d.join("app").join("legacy").join("package.json"), "{}").unwrap();
        let found = discover(&d, 3);
        assert_eq!(found.len(), 1, "{:?}", found.iter().map(|f| f.path.clone()).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_node_project_gets_run_command_from_its_scripts() {
        let d = scratch("node");
        std::fs::write(
            d.join("package.json"),
            "{\"scripts\":{\"dev\":\"vite\"},\"dependencies\":{\"vue\":\"^3\"}}",
        )
        .unwrap();
        let (c, why) = draft(&d);
        assert_eq!(c.commands.run.as_deref(), Some("npm run dev"));
        assert_eq!(c.commands.install.as_deref(), Some("npm install"));
        assert_eq!(c.ready_when.and_then(|r| r.port), Some(5173));
        assert!(why.iter().any(|w| w.contains("scripts")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 有虚拟环境就直接绑它——「不用 activate」是这个工具最想省掉的一步。
    #[test]
    fn a_python_project_with_a_venv_gets_the_interpreter_bound() {
        let d = scratch("pyvenv");
        std::fs::write(d.join("requirements.txt"), "flask\n").unwrap();
        std::fs::write(d.join("main.py"), "print(1)").unwrap();
        std::fs::create_dir_all(d.join(".venv").join("Scripts")).unwrap();
        std::fs::write(d.join(".venv").join("pyvenv.cfg"), "home = x").unwrap();
        std::fs::write(d.join(".venv").join("Scripts").join("python.exe"), "").unwrap();
        let (c, _) = draft(&d);
        let exe = c.runtime.python_exe.expect("该绑上虚拟环境");
        assert!(exe.to_lowercase().contains(".venv"), "{}", exe);
        assert_eq!(c.commands.run.as_deref(), Some("python main.py"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn spring_boot_is_recognised_by_its_pom() {
        let d = scratch("boot");
        std::fs::write(
            d.join("pom.xml"),
            "<project><dependencies><dependency><groupId>org.springframework.boot</groupId></dependency></dependencies></project>",
        )
        .unwrap();
        let (c, _) = draft(&d);
        assert_eq!(c.commands.run.as_deref(), Some("mvn spring-boot:run"));
        assert_eq!(c.ready_when.and_then(|r| r.port), Some(8080));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 起草过一次之后，人改过的名字不能被覆盖。
    #[test]
    fn a_hand_written_name_survives_regeneration() {
        let d = scratch("keepname");
        std::fs::write(d.join("go.mod"), "module x\n").unwrap();
        let mut c = draft(&d).0;
        c.name = Some("我给它起的名字".into());
        c.save(&d).unwrap();
        let (again, _) = write_draft(&d).unwrap();
        assert_eq!(again.name.as_deref(), Some("我给它起的名字"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rust_and_cmake_projects_get_sane_commands() {
        let d = scratch("rust");
        std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src").join("main.rs"), "fn main(){}").unwrap();
        let (c, _) = draft(&d);
        assert_eq!(c.commands.run.as_deref(), Some("cargo run"));
        assert_eq!(c.commands.test.as_deref(), Some("cargo test"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
