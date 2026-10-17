//! `project.rs` 的测试。
//!
//! **为什么单独一个文件**：`project.rs` 早就超了 1000 行上限、登记在案（只许降不许升）。
//! 测试是最大的一块（三百多行），而且它**不参与编译产物**——把它搬出来，
//! 是这一册里最干净的一刀：主文件只剩"干什么"，测试在另一册里说"凭什么"。
//!
//! 隔离性没有变：`use super::*` 仍然拿得到 `project.rs` 里的私有项，
//! 因为它仍然是那个模块的子模块（由 `#[path]` 指定文件位置）。

#![allow(clippy::let_underscore_must_use)]

use super::*;

    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dd-proj-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn has(s: &ProjectScan, id: &str) -> bool {
        s.items.iter().any(|i| i.id == id)
    }

    /// **没写 `edition` 就是 2015 语义**——不是风格问题，是编译行为不同。
    ///
    /// ## 校准（466 个真 crate）
    ///
    /// 命中 **48 个（10%）**，其中 43 个是 0.x/1.x 老 crate，只有 6 个声明了 MSRV——
    /// 所以**绝大多数就是"很老"**，不是刻意兼容。报 `Info`：对被接手的人有信息量。
    ///
    /// 这个测试钉两头：**没写 → 报；写了 → 不报**。
    #[test]
    fn 没写edition就是2015要指出来() {
        let d = scratch("no-edition");
        std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/lib.rs"), "pub fn f(){}").unwrap();
        assert!(
            has(&scan(&d), "cargo-edition-missing"),
            "没写 edition 时 cargo 按 2015 编译（`array.into_iter()` 拿引用、\
             裸 trait object 合法），照现代教程改会编译不过——这条要报"
        );
        let _ = std::fs::remove_dir_all(&d);

        let d = scratch("has-edition");
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/lib.rs"), "pub fn f(){}").unwrap();
        assert!(
            !has(&scan(&d), "cargo-edition-missing"),
            "写了 edition 就不该报"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
    /// **中文路径：Rust 只提示，C/C++ 才警告**（2026-10-17 量的）。
    ///
    /// 原来 `C/C++ 或 Rust` 一律 Warn。实测：纯 Rust 项目放到中文路径下，
    /// `cargo build` 退出码 0（真的调了 rustc）、产物也能跑。
    /// 真正翻车的是**要 C 编译器**的那条路（MinGW 的 make / 部分 MSVC 组件）——
    /// 而"项目是 Rust"不等于"它走 cc"。
    ///
    /// 这个测试钉两头：**Rust → Info；C → Warn**。
    #[test]
    fn 中文路径里_rust_只是提示_c_才警告() {
        // ① 纯 Rust 放在中文目录下
        let d = scratch("zh-rust");
        let zh = d.join("中文项目");
        std::fs::create_dir_all(zh.join("src")).unwrap();
        std::fs::write(
            zh.join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(zh.join("src/lib.rs"), "pub fn f(){}").unwrap();
        let s = scan(&zh);
        let item = s
            .items
            .iter()
            .find(|i| i.id == "project-path-unsafe")
            .expect("路径里有中文，这条必须出现");
        assert_eq!(
            item.severity,
            Severity::Info,
            "纯 Rust 放中文路径下实测能编能跑，不该吓人：{:?}",
            item
        );
        let _ = std::fs::remove_dir_all(&d);

        // ② 同一个中文目录里塞一个 C 文件 → 警告
        let d = scratch("zh-c");
        let zh = d.join("中文项目");
        std::fs::create_dir_all(&zh).unwrap();
        // **要有项目标记**：只有 main.c 的目录不算项目，habit 那一步不会跑。
        std::fs::write(zh.join("main.c"), "int main(void){return 0;}").unwrap();
        std::fs::write(zh.join("Makefile"), "all:\n\tgcc main.c -o app\n").unwrap();
        let s = scan(&zh);
        let item = s
            .items
            .iter()
            .find(|i| i.id == "project-path-unsafe")
            .expect("有 C 文件的路径里也必须有这一条");
        assert_eq!(
            item.severity,
            Severity::Warn,
            "MinGW 的 make / 部分 MSVC 组件在中文路径下确实会翻车，这条要真的警告：{:?}",
            item
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **多 bin 且没有 `src/main.rs` 且没写 `default-run` → `cargo run` 会报错**。
    ///
    /// ## 拿什么换的（全量跑 466 个 registry crate 换的）
    ///
    /// 这条规则第一版只判"≥2 个 bin 且没 default-run"，全量跑出 **3 个命中**：
    ///   `brotli` ✓ 真的（没有 `src/main.rs`，两个 `[[bin]]`）
    ///   `cargo-watch` ✗ 误报（有 `src/main.rs`）
    ///   `embed-resource` ✗ 误报（有 `src/main.rs`）
    ///
    /// **cargo 的规则是：有 `src/main.rs` 就用它当默认**，哪怕还有别的 bin 目标。
    /// 所以"有 main.rs"这一条必须排除。**3 个命中看起来特异性很高，实际 2/3 是错的。**
    ///
    /// 这个测试把四种形状都钉住：
    ///   ① 两个 `[[bin]]` + 无 main.rs + 无 default-run → **报**
    ///   ② 同① 但**有** `src/main.rs` → **不报**（cargo 有默认，这是 v1 的误报）
    ///   ③ 同① 但写了 `default-run` → **不报**
    ///   ④ `autobins = false`、`src/bin/` 里有一堆 `.rs`、只声明 1 个 `[[bin]]` → **不报**
    ///      （`brotli` 就是这样：9 个文件只算 2 个目标）
    #[test]
    fn 多二进制没默认入口才报() {
        let cargo = |extra: &str| format!("[package]\nname=\"t\"\nversion=\"0.1.0\"\n{extra}\n");
        let two_bins = "[[bin]]\nname=\"a\"\npath=\"src/bin/a.rs\"\n[[bin]]\nname=\"b\"\npath=\"src/bin/b.rs\"\n";

        // ① 该报
        let d = scratch("multi-bin");
        std::fs::write(d.join("Cargo.toml"), cargo(two_bins)).unwrap();
        std::fs::create_dir_all(d.join("src/bin")).unwrap();
        std::fs::write(d.join("src/bin/a.rs"), "fn main(){}").unwrap();
        std::fs::write(d.join("src/bin/b.rs"), "fn main(){}").unwrap();
        assert!(
            has(&scan(&d), "cargo-multi-bin-no-default"),
            "两个 [[bin]]、没有 src/main.rs、没有 default-run —— `cargo run` 会报\
             「could not determine which binary to run」，这条必须报"
        );
        let _ = std::fs::remove_dir_all(&d);

        // ② 有 main.rs → 不报（cargo 拿它当默认）
        let d = scratch("multi-bin-with-main");
        std::fs::write(d.join("Cargo.toml"), cargo(two_bins)).unwrap();
        std::fs::create_dir_all(d.join("src/bin")).unwrap();
        std::fs::write(d.join("src/main.rs"), "fn main(){}").unwrap();
        std::fs::write(d.join("src/bin/a.rs"), "fn main(){}").unwrap();
        assert!(
            !has(&scan(&d), "cargo-multi-bin-no-default"),
            "有 src/main.rs 时 cargo 有默认入口，不会报错——报它就是误报\
             （实测 cargo-watch / embed-resource 就是这么被冤枉的）"
        );
        let _ = std::fs::remove_dir_all(&d);

        // ③ 写了 default-run → 不报
        let d = scratch("multi-bin-default");
        std::fs::write(
            d.join("Cargo.toml"),
            cargo(&format!("default-run = \"a\"\n{two_bins}")),
        )
        .unwrap();
        std::fs::create_dir_all(d.join("src/bin")).unwrap();
        std::fs::write(d.join("src/bin/a.rs"), "fn main(){}").unwrap();
        std::fs::write(d.join("src/bin/b.rs"), "fn main(){}").unwrap();
        assert!(
            !has(&scan(&d), "cargo-multi-bin-no-default"),
            "写了 default-run 之后 cargo 就知道跑哪个，不该报"
        );
        let _ = std::fs::remove_dir_all(&d);

        // ④ autobins = false：src/bin 里 9 个文件、只声明 2 个 → 算 2 个目标，不报
        //    （brotli 那 7 个 `.rs` 是被 include! 进去的模块，不是 bin）
        let d = scratch("autobins-off");
        std::fs::write(
            d.join("Cargo.toml"),
            cargo(&format!("autobins = false\n{two_bins}")),
        )
        .unwrap();
        std::fs::create_dir_all(d.join("src/bin")).unwrap();
        for i in 0..9 {
            std::fs::write(d.join("src/bin").join(format!("m{i}.rs")), "x").unwrap();
        }
        std::fs::write(d.join("src/bin/a.rs"), "fn main(){}").unwrap();
        std::fs::write(d.join("src/bin/b.rs"), "fn main(){}").unwrap();
        // 2 个声明 ≠ 报错条件（还要 ≥2 且无 main/default-run）→ 这条其实会报，
        // 所以用 default-run 版本来验"autobins 不把 9 个文件数成 9 个"。
        std::fs::write(
            d.join("Cargo.toml"),
            cargo(&format!("autobins = false\ndefault-run = \"a\"\n{two_bins}")),
        )
        .unwrap();
        assert!(
            !has(&scan(&d), "cargo-multi-bin-no-default"),
            "autobins = false + default-run 时不该报（brotli 的形状）"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **装项目的目录不是项目。**
    ///
    /// 这条是拿真事故换的：打开 `jingwei-demo`（三个演示项目的父目录）时，
    /// 界面上写着「项目类型 Python」，而依赖清单里并排躺着
    /// `hello_python/requirements.txt` 和 `hello_rust/Cargo.toml`——
    /// 一个"项目"的体检里混着两个别的项目，而那些结论一条都不该算在这一层头上。
    #[test]
    fn 装项目的目录会被指出来() {
        let d = scratch("container");
        for (name, marker) in [
            ("proj_a", "requirements.txt"),
            ("proj_b", "Cargo.toml"),
            ("proj_c", "package.json"),
        ] {
            let p = d.join(name);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join(marker), "x").unwrap();
        }
        let s = scan(&d);
        assert!(
            has(&s, "projects-dir-not-a-project"),
            "三个子项目该被指出来：{:?}",
            s.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>()
        );
        let item = s.items.iter().find(|i| i.id == "projects-dir-not-a-project").unwrap();
        assert_eq!(item.severity, Severity::Info, "这个目录本身没毛病，报成错误就是误报");
        assert!(item.title.contains('3'), "标题要说清有几个：{}", item.title);
        assert!(!item.verify.is_empty(), "四要素里的「自查」不能空");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **一个子项目不算容器**：那种情况多半是一个项目里带个示例，
    /// 硬报就成了误报——而这个项目最不许犯的就是误报。
    #[test]
    fn 只有一个子项目时不报() {
        let d = scratch("onechild");
        let p = d.join("only");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("Cargo.toml"), "x").unwrap();
        let s = scan(&d);
        assert!(!has(&s, "projects-dir-not-a-project"), "只有一个子项目，不该报");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **工具自己的数据目录不算项目内容。**
    ///
    /// 这条也是拿真事故换的：对演示项目跑 `audit --json` 时，`longest_file` 报的是
    /// `.jingwei/notes/…md`——**我们自己的笔记成了「这个项目最长的路径」**，三个项目个个如此，
    /// `files_scanned` 也跟着虚高。
    ///
    /// 它的性质比"漏了一条忽略规则"更重：**我们把自己算进了被观察的对象**——
    /// 打开过一次，对它的描述就变了。所以它不是建议，是错。
    #[test]
    fn the_tools_own_data_dir_is_not_project_content() {
        let d = scratch("own-data");
        std::fs::write(d.join("main.rs"), "fn main(){}").unwrap();
        std::fs::create_dir_all(d.join(".jingwei").join("notes")).unwrap();
        std::fs::write(
            d.join(".jingwei").join("notes").join("a-very-long-note-name.md"),
            "x",
        )
        .unwrap();
        std::fs::write(d.join(".jingwei").join("graph.db"), "x").unwrap();
        // 也要挡住更深的那一层（Tauri 那种 `src-tauri/.jingwei`）
        std::fs::create_dir_all(d.join("src-tauri").join(".jingwei")).unwrap();
        std::fs::write(d.join("src-tauri").join(".jingwei").join("graph.db"), "x").unwrap();

        let s = scan(&d);
        let leaked: Vec<&String> = s
            .all_files
            .iter()
            .filter(|f| f.contains(".jingwei"))
            .collect();
        assert!(leaked.is_empty(), "自己的数据目录漏进结论了：{leaked:?}");
        assert!(
            !s.longest_file.contains(".jingwei"),
            "最长路径不该是我们的笔记：{}",
            s.longest_file
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unignored_artifacts_are_named() {
        let d = scratch("basic");
        // 真项目有标记文件——夹具要照真东西写
        std::fs::write(d.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(d.join("node_modules").join("x")).unwrap();
        std::fs::write(d.join("node_modules").join("x").join("a.js"), "1").unwrap();
        std::fs::create_dir_all(d.join("__pycache__")).unwrap();
        std::fs::write(d.join("__pycache__").join("a.pyc"), "1").unwrap();
        std::fs::write(d.join(".gitignore"), "node_modules/\n").unwrap();
        let s = scan(&d);
        assert!(s.ignored.iter().any(|x| x == "node_modules"));
        assert!(s.unignored.iter().any(|x| x == "__pycache__"));
        assert!(has(&s, "gitignore-missing-entry"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_plain_directory_named_env_is_not_an_artifact() {
        let d = scratch("envdir");
        std::fs::create_dir_all(d.join("env")).unwrap();
        std::fs::write(d.join("env").join("config.json"), "{}").unwrap();
        let s = scan(&d);
        assert!(s.artifacts.is_empty(), "普通 env 目录不该被判成产物");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_real_venv_is_an_artifact() {
        let d = scratch("venv");
        std::fs::create_dir_all(d.join(".venv").join("Scripts")).unwrap();
        std::fs::write(d.join(".venv").join("pyvenv.cfg"), "home = x").unwrap();
        let s = scan(&d);
        assert!(s.artifacts.iter().any(|a| a.path == ".venv"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn without_gitignore_we_do_not_report_missing_entries() {
        let d = scratch("nogitignore");
        std::fs::write(d.join("Cargo.toml"), "").unwrap();
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join("target").join("a.class"), "1").unwrap();
        let s = scan(&d);
        assert!(!s.has_gitignore);
        assert!(!has(&s, "gitignore-missing-entry"));
        assert!(has(&s, "gitignore-missing"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_package_json_is_an_error_with_a_verify_command() {
        let d = scratch("badjson");
        std::fs::write(d.join("package.json"), "{ \"name\": \"x\", }").unwrap();
        let s = scan(&d);
        let item = s
            .items
            .iter()
            .find(|i| i.id == "package-json-broken")
            .expect("应该报出来");
        assert_eq!(item.severity, Severity::Error);
        assert!(!item.verify.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env_with_secrets_in_a_git_repo_is_flagged() {
        let d = scratch("env");
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join(".env"), "API_KEY=abc\nPORT=8080\n").unwrap();
        let s = scan(&d);
        let item = s
            .items
            .iter()
            .find(|i| i.id == "env-file-exposed")
            .expect("应该报出来");
        assert_eq!(item.severity, Severity::Warn);
        assert!(item.evidence.iter().any(|e| e.contains("API_KEY")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env_without_secrets_stays_info() {
        let d = scratch("envplain");
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join(".env"), "PORT=8080\n").unwrap();
        let s = scan(&d);
        let item = s.items.iter().find(|i| i.id == "env-file-exposed").unwrap();
        assert_eq!(item.severity, Severity::Info);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pom_without_source_encoding_is_flagged() {
        let d = scratch("pom");
        std::fs::write(
            d.join("pom.xml"),
            "<project><dependencies><dependency><artifactId>okhttp</artifactId><version>4.9.3</version></dependency></dependencies></project>",
        )
        .unwrap();
        let s = scan(&d);
        assert!(has(&s, "maven-encoding-unset"));
        assert!(!has(&s, "maven-version-conflict"));
        assert!(!has(&s, "maven-snapshot-dep"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn same_artifact_with_two_versions_is_flagged() {
        let d = scratch("dupver");
        std::fs::write(
            d.join("pom.xml"),
            "<project>\n<dependencies>\n\
             <dependency><artifactId>guava</artifactId><version>30.0-jre</version></dependency>\n\
             <dependency><artifactId>guava</artifactId><version>32.0-jre</version></dependency>\n\
             </dependencies>\n</project>\n",
        )
        .unwrap();
        let s = scan(&d);
        assert!(has(&s, "maven-version-conflict"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 管理段里写版本是正常写法，不许被说成冲突。
    #[test]
    fn dependency_management_is_not_a_version_conflict() {
        let d = scratch("mgmt");
        std::fs::write(
            d.join("pom.xml"),
            "<project>\n<dependencyManagement>\n<dependencies>\n\
             <dependency><artifactId>guava</artifactId><version>32.0-jre</version></dependency>\n\
             </dependencies>\n</dependencyManagement>\n\
             <dependencies>\n\
             <dependency><artifactId>guava</artifactId><version>32.0-jre</version></dependency>\n\
             </dependencies>\n</project>\n",
        )
        .unwrap();
        let s = scan(&d);
        assert!(!has(&s, "maven-version-conflict"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn requirements_that_are_not_pinned_are_flagged() {
        let d = scratch("reqs");
        std::fs::create_dir_all(d.join("src")).unwrap();
        for n in ["a", "b", "c"] {
            std::fs::write(d.join("src").join(format!("{}.py", n)), "x = 1\n").unwrap();
        }
        std::fs::write(d.join("requirements.txt"), "requests>=2.0\nflask\nnumpy\n").unwrap();
        let s = scan(&d);
        assert!(has(&s, "py-unpinned"));
        assert!(has(&s, "py-lock-missing"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn node_without_lock_is_a_warning() {
        let d = scratch("nodelock");
        std::fs::write(d.join("package.json"), "{\"name\":\"x\",\"version\":\"1.0.0\"}").unwrap();
        let s = scan(&d);
        assert!(has(&s, "node-lock-missing"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **走不完就要说走不完，而且计数要跟着改口径**（2026-10-16）。
    ///
    /// ## 这条是拿什么换的
    ///
    /// 对 **Python 3.15 的标准库**（`Lib\`，1677 个 .py）跑 `doctor` 时抓到的：
    /// 它报「有 **1610** 个 .py 文件」，而 `Lib` 下真实的 .py 是 **1677**。
    /// 把深度分布一算——**深度 0..4 累计正好 1610**。同一个上限还影响了
    /// 「77 个产物目录」（真 90）与 `audit.all_files`（最深 5 层，真 6 层，
    /// `sys/machine/*` 整个模块不在表里）。
    ///
    /// **最狠的一点**：那条结论自己给用户的自查命令
    /// （`Get-ChildItem -Recurse -Filter *.py | Measure-Object`）数出来是 1677——
    /// **自查命令当场否证了它自己**。而「无自查不报错」这条线本来就是为了防这个。
    ///
    /// ## 三个断言分开钉
    ///
    ///   ① 走不完时必须有一条 `scan-truncated`（**浅扫也要有**——原来它只长在
    ///      深扫的体积项里，所以浅扫时一声不响）；
    ///   ② `.py` 计数必须报成「**至少** N」——**只加一句"下面的计数要读成至少"
    ///      是不够的**（第一版就是这么半修的，实测标题仍写着"有 1610 个"）；
    ///   ③ **没截断时不许说"至少"**——那是把确定值说成不确定值，同一条线的另一边。
    #[test]
    fn 走不完要说走不完而且计数要改口径() {
        // 造一棵**深过 MAX_DEPTH** 的树：`a/b/c/d/e/` 里放一个 .py
        // （相对深度 5，`MAX_DEPTH = 4` 走不到）
        let d = scratch("trunc");
        // 真项目有标记文件——夹具要照真东西写（否则"不是项目"那道闸会挡住它）。
        // **用 package.json 而不是 pyproject.toml**：后者会让 `py-manifest-missing`
        // 消失（那门正是"有清单"的判据），而这条测试要测的就是那条结论。
        std::fs::write(d.join("package.json"), "{}").unwrap();
        std::fs::write(d.join("top.py"), "").unwrap();
        let deep = d.join("a").join("b").join("c").join("d").join("e");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("deep.py"), "").unwrap();

        let s = scan(&d);
        assert!(
            has(&s, "scan-truncated"),
            "走不完就必须说：深度超过上限时也要报 scan-truncated（浅扫也要）"
        );
        //  ① 计数改口径
        let item = s
            .items
            .iter()
            .find(|i| i.id == "py-manifest-missing")
            .expect("有 .py 文件却没有清单，该出这一条");
        assert!(
            item.title.contains("至少"),
            "截断时计数必须报「至少 N」，实际标题：{}",
            item.title
        );

        // ③ 反过来：**够浅的时候不许说"至少"**
        let shallow = scratch("no-trunc");
        std::fs::write(shallow.join("only.py"), "").unwrap();
        let s2 = scan(&shallow);
        if let Some(it2) = s2.items.iter().find(|i| i.id == "py-manifest-missing") {
            assert!(
                !it2.title.contains("至少"),
                "没截断时不许说「至少」——那是把确定值说成不确定值：{}",
                it2.title
            );
        }
        assert!(
            !has(&s2, "scan-truncated"),
            "浅而小的目录不该报截断"
        );
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&shallow);
    }

    /// **每一个把计数写进标题的地方都要跟着截断改口径**（2026-10-16）。
    ///
    /// ## 这条是拿什么换的
    ///
    /// 修"深度截断"那一轮，我把 `truncated` 修好了、把 `.py` 计数改了，
    /// **就以为那条缺陷修完了**。可同一个洞里还有**第二个消费点**：
    /// 「项目里没有 .gitignore，而已经攒下 N 个产物目录」——
    /// 实测 `Lib` 报 **77**，真值 **90**，而 77 正好是"深度≤4 的 `__pycache__` 之和"。
    ///
    /// **根上的缺陷修完，还要把所有消费点过一遍。** 只改一个消费点，
    /// 剩下那些会安静地继续错，而且看起来像"已经修过了"——那比没修更坏。
    ///
    /// ## 还要守住"改文案别把句子改坏"
    ///
    /// 第一版我写的是 `{py_n} .py 文件`，输出成了
    /// 「有 **至少 9 .py** 文件」——既丢了量词、又是"有 至少"这种叠词。
    /// 所以这里连**句子读得通**也一起钉：量词与"至少"不能叠着出现。
    #[test]
    fn 每个计数都要跟着截断改口径而且话要说得通() {
        let d = scratch("trunc-all");
        // 造一棵深过 MAX_DEPTH 的树：既有 .py、又有产物目录。
        //
        // ⚠️ **深树不能藏在产物目录里**（第一版就是那么干的，测试因此空跑）：
        // 产物目录（`__pycache__`/`target`/`node_modules`）是**不往里走**的，
        // 所以藏在里面的东西永远走不到，`truncated` 也就不会置位。
        // 这条本身是对的判据（别钻进 target 里数文件），
        // 错的是我的夹具——**测试要造"真的会被走到"的形状**。
        std::fs::create_dir_all(d.join("proj").join("__pycache__")).unwrap();
        // 深树：**要做到"有一个深度 5 的目录"**才触发截断。
        //
        // 这条是拿四次失败换来的，值得写清：
        //   `walk()` 的深度判据落在**目录**上（`depth > MAX_DEPTH` 时 return），
        //   所以"文件在深度 5"不算截断（目录最深才到 4）——必须有个**目录**在深度 5。
        // 而且深支路上**也要有产物目录**：`truncated` 只由"走到深度 5 的目录"置位，
        // 而产物是在**走的过程中**记的。深支路若一个产物都没有，就会得到
        // "truncated=true 但产物只有 1 个（来自另一支）"——那条断言照样测不到。
        let deep = d.join("src").join("a").join("b").join("c").join("d").join("e");
        std::fs::create_dir_all(&deep).unwrap();
        // 深支路上的产物：放在深度 3（走得到），这样 truncated 与产物数同时成立
        std::fs::create_dir_all(d.join("src").join("a").join("__pycache__")).unwrap();
        std::fs::write(deep.join("x.py"), "").unwrap();
        std::fs::write(d.join("top.py"), "").unwrap();
        // 真项目有标记文件——夹具要照真东西写（否则"不是项目"那道闸会挡住它）。
        // 用 package.json：`pyproject.toml` 会让 `py-manifest-missing` 消失。
        std::fs::write(d.join("package.json"), "{}").unwrap();

        let s = scan(&d);
        assert!(has(&s, "scan-truncated"), "走不完必须说");

        // ① 产物目录那条也要改口径
        let art = s
            .items
            .iter()
            .find(|i| i.id == "gitignore-missing")
            .expect("有产物目录却没有 .gitignore，该出这一条（夹具得造到能走到的深度）");
        assert!(
            art.title.contains("至少"),
            "截断时产物目录数必须报「至少」，实际：{}",
            art.title
        );
        assert!(!art.title.contains("攒下 至少"), "病句：{}", art.title);

        // ③ `.py` 那条：有量词、且"至少"不叠在"有"后面
        if let Some(it) = s.items.iter().find(|i| i.id == "py-manifest-missing") {
            assert!(
                it.title.contains("个 .py 文件"),
                "要带量词，实际：{}",
                it.title
            );
            assert!(
                !it.title.contains("有 至少"),
                "病句（'有 至少'），实际：{}",
                it.title
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **不是项目就别给项目建议**（2026-10-16）。
    ///
    /// ## 拿什么换的
    ///
    /// 把 5 个**别人写的** Python 库（`site-packages` 里的
    /// markdown_it / mdurl / pip / pygments / rich）当样本逐条核结论时发现：
    /// 每个库都收到同一套训话——
    ///   「项目里没有 .gitignore，而已经攒下 8 个产物目录」
    ///   「有 66 个 .py 文件，但没有任何依赖清单」
    /// 可它们是**库**：清单在上一层的 `*.dist-info` 里。**字面没错，对象错了。**
    ///
    /// ## 三组断言分开钉
    ///
    ///   ① 没有标记也没 `.git` → 出一条 `not-a-project`；
    ///   ② **不给项目建议**（不许同时出 `py-manifest-missing` / `gitignore-missing`）——
    ///      "说了但照样给"等于没做到；
    ///   ③ **README 不算标记**：第一版我把 `has_readme` 写进了判据，实测
    ///      `pip` 目录里有 `README.md` 就逃过去了——而**每个 Python 包都带 README**。
    #[test]
    fn 不是项目就别给项目建议() {
        // ① 一个只有 .py 的空目录：没有标记、没有 .git
        let d = scratch("nap");
        std::fs::write(d.join("x.py"), "").unwrap();
        let s = scan(&d);
        assert!(has(&s, "not-a-project"), "该说'这里不像项目'");
        assert!(
            !has(&s, "py-manifest-missing"),
            "不是项目就不许给'你没有依赖清单'——那是按项目口径写的"
        );
        assert!(!has(&s, "gitignore-missing"), "同上，产物那条也不许给");
        let _ = std::fs::remove_dir_all(&d);

        // ② 有 README 但没标记：README 不算数（这就是 pip 逃过去的坑）
        let d2 = scratch("nap-readme");
        std::fs::write(d2.join("x.py"), "").unwrap();
        std::fs::write(d2.join("README.md"), "# 一个库\n").unwrap();
        let s2 = scan(&d2);
        assert!(
            has(&s2, "not-a-project"),
            "README 不证明它是项目——每个 Python 包都带一个"
        );
        let _ = std::fs::remove_dir_all(&d2);

        // ③ 有标记文件 → 正常给建议（不误伤）
        let d3 = scratch("nap-real");
        std::fs::write(d3.join("x.py"), "").unwrap();
        std::fs::write(d3.join("pyproject.toml"), "[project]\nname='x'\n").unwrap();
        let s3 = scan(&d3);
        assert!(
            !has(&s3, "not-a-project"),
            "有 pyproject.toml 就是项目，不许说不是"
        );
        let _ = std::fs::remove_dir_all(&d3);
    }

    #[test]
    fn deep_scan_measures_artifact_size_and_quick_scan_does_not() {
        let d = scratch("size");
        std::fs::write(d.join("package.json"), "{}").unwrap();
        let nm = d.join("node_modules").join("pkg");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("big.js"), "x".repeat(200_000)).unwrap();
        let quick = scan(&d);
        assert!(quick.artifacts.iter().any(|a| a.path == "node_modules"));
        assert_eq!(quick.artifacts[0].size_mb, 0.0);
        let deeps = scan_deep(&d);
        assert!(deeps.artifacts[0].size_mb > 0.1);
        assert!(has(&deeps, "project-artifacts"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nested_git_is_noticed() {
        let d = scratch("nested");
        // 真项目有标记文件——夹具要照真东西写（否则"不是项目"那道闸会挡住它）
        std::fs::write(d.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(d.join("vendor").join("lib").join(".git")).unwrap();
        std::fs::write(d.join("vendor").join("lib").join("a.txt"), "1").unwrap();
        let s = scan(&d);
        assert!(has(&s, "nested-git"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn every_item_has_a_verify_command_and_a_title() {
        let d = scratch("verify");
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        std::fs::write(d.join("package.json"), "{}").unwrap();
        std::fs::write(d.join("requirements.txt"), "requests>=2\nflask\nnumpy\n").unwrap();
        std::fs::write(d.join("pom.xml"), "<project/>").unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        let s = scan_deep(&d);
        for i in &s.items {
            assert!(!i.verify.is_empty(), "条目 {} 没有自查命令", i.id);
            assert!(!i.title.is_empty(), "条目 {} 没有标题", i.id);
            assert!(
                HYGIENE_IDS.contains(&i.id.as_str()),
                "条目 {} 没登记进 HYGIENE_IDS，它会被丢进「其他」层",
                i.id
            );
            assert!(!i.group.is_empty(), "条目 {} 没有分组", i.id);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn hygiene_ids_have_no_duplicates() {
        let uniq: std::collections::BTreeSet<&str> = HYGIENE_IDS.iter().copied().collect();
        assert_eq!(uniq.len(), HYGIENE_IDS.len());
    }

    /// PHP / Ruby / Docker 这三类也要有话说，而且说的是事实。
    #[test]
    fn php_ruby_and_docker_projects_get_the_right_notes() {
        let d = scratch("polyglot");
        std::fs::write(d.join("composer.json"), "{\"require\":{\"monolog/monolog\":\"^3\"}}").unwrap();
        std::fs::write(d.join("Gemfile"), "source 'https://rubygems.org'\ngem 'rails'\n").unwrap();
        std::fs::write(d.join("Dockerfile"), "FROM node:20\nCOPY . .\n").unwrap();
        let s = scan(&d);
        assert!(has(&s, "php-lock-missing"), "composer.json 没有锁文件该报");
        assert!(has(&s, "ruby-lock-missing"), "Gemfile 没有锁文件该报");
        assert!(has(&s, "docker-no-ignore"), "Dockerfile 没有 .dockerignore 该报");
        assert_eq!(
            s.manifests.iter().find(|m| m.kind == "php").map(|m| m.note.as_str()),
            Some("require 1 条")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 有 .dockerignore 时不许再念叨。
    #[test]
    fn a_dockerignore_shuts_the_docker_note_up() {
        let d = scratch("dockerignore");
        std::fs::write(d.join("Dockerfile"), "FROM scratch\n").unwrap();
        std::fs::write(d.join(".dockerignore"), "node_modules\n").unwrap();
        let s = scan(&d);
        assert!(!has(&s, "docker-no-ignore"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **2 条不钉版本的依赖也要提**（2026-10-17 把阈值从 3 降到 2）。
    ///
    /// 为什么降：两条 `>=` 依赖，明天就可能装出不同东西——那就是不可复现。
    /// 原来 `total >= 3` 把这种情况整个放过了。
    ///
    /// **保留"过半"判据**：2 条里只 1 条不钉，不算问题（那可能是刻意的）。
    #[test]
    fn 两条依赖都不钉版本也要提() {
        let d = scratch("thr-2of2");
        std::fs::write(d.join("requirements.txt"), "a>=1\nb>=2\n").unwrap();
        std::fs::write(d.join("main.py"), "x=1\n").unwrap();
        let s = crate::project::scan(&d);
        assert!(
            s.items.iter().any(|i| i.id == "py-unpinned"),
            "2 条全不钉版本该报：{:?}",
            s.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
        );
    }

    /// 反过来：2 条里只 1 条不钉，**不算问题**。
    #[test]
    fn 两条依赖只一条不钉就不报() {
        let d = scratch("thr-1of2");
        std::fs::write(d.join("requirements.txt"), "a==1.0\nb>=2\n").unwrap();
        std::fs::write(d.join("main.py"), "x=1\n").unwrap();
        let s = crate::project::scan(&d);
        assert!(
            !s.items.iter().any(|i| i.id == "py-unpinned"),
            "2 条里只 1 条不钉不该报",
        );
    }
