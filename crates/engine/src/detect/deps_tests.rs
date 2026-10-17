// **`deps.rs` 的测试**——2026-10-16 从这个文件分出来。
//
// 为什么分：`deps.rs` 长到 1056 行，超了门禁的 1000 行上限（**这是真的被拦住**，
// 不是走登记表绕过去）。而其中 432 行是测试——非测试部分只有 623 行。
// 按 `rules_tests.rs` / `project_tests.rs` 的先例用 `#[path]` 分出来，
// **各回各的边界**：deps.rs 管"怎么查"，这里管"查得对不对"。

// 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
// 而不是让 lint 在整个 crate 里静默失声。
#![allow(clippy::let_underscore_must_use)]
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jingwei-deps-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // ── Node 那三种布局：**这两种原来是误报** ──────────────────────

    /// **PnP 项目：没有 node_modules 是正常的，绝不能报成"未安装"。**
    ///
    /// 改之前这里会返回 `No`，界面上一整列依赖全是✗——而它们其实都装着。
    /// 这类误报比"查不到"糟得多：用户会去重装依赖，然后发现问题依旧。
    #[test]
    fn pnp_项目不报未安装() {
        let d = tmp("pnp");
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"x","dependencies":{"vue":"^3"}}"#,
        )
        .unwrap();
        std::fs::write(d.join("yarn.lock"), "").unwrap();
        std::fs::write(d.join(".pnp.cjs"), "// pnp\n").unwrap();
        let v = installed(&d, Lang::Web, "vue", &[]);
        assert_eq!(
            v,
            Verdict::Unknown,
            "确认不了就说确认不了——不许说成没装"
        );
        assert_ne!(v, Verdict::No, "Unknown 绝不能被当成缺失");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **"确认不了"要说出具体原因，而且要说成"我们没做"**（2026-10-16）。
    ///
    /// 为什么这条值得钉：这两个函数是给**用户看的提示**用的。原来界面只到
    /// "N 项确认不了"——用户分不出该他补点什么、还是等我们修。
    /// 而 `rules.rs` 那边的做法说明了更好的答案：
    /// **能说出具体原因的地方，比一个笼统的归属标签更好。**
    ///
    /// 两条分开钉，因为它们是**不同语言、不同原因**，会各自坏：
    ///   ① Web/PnP：原因是"解析 `.pnp.cjs` 要跑 JS，我们还没做"；
    ///   ② Java：原因是"读不到 USERPROFILE"或"没跑过 Maven"。
    /// ③ 而且要**不是**那句笼统的 `what_to_do()`——否则就等于没说出原因。
    #[test]
    fn 确认不了的原因要说具体而不是笼统() {
        // ① PnP 项目
        let d = tmp("why-web");
        std::fs::write(d.join("package.json"), r#"{"name":"x"}"#).unwrap();
        std::fs::write(d.join(".pnp.cjs"), "// pnp\n").unwrap();
        let web = why_unknown_web(&d);
        assert!(web.contains("PnP"), "要说清是 PnP 项目：{web}");
        assert!(web.contains("我们"), "要说清是我们没做：{web}");
        assert_ne!(
            web,
            crate::Unknown::OurGap.what_to_do(),
            "知道具体原因就别用笼统那句（那就等于没说）"
        );
        // 非 PnP 的项目：原因是别处，**不许硬编 PnP 那句**
        let plain = tmp("why-web-plain");
        let fallback = why_unknown_web(&plain);
        assert!(
            !fallback.contains("PnP"),
            "不是 PnP 就不许说成 PnP（编一个原因比不编更糟）：{fallback}"
        );

        // ② Java
        let java = why_unknown_java();
        assert!(
            java.contains("USERPROFILE") || java.contains("Maven"),
            "要说清是哪一个原因：{java}"
        );
        assert_ne!(
            java,
            crate::Unknown::OurGap.what_to_do(),
            "Java 这处知道原因，不该退回笼统那句"
        );

        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&plain);
    }

    /// **每条"装没装"都要带上是从哪儿查的**（2026-10-16）。
    ///
    /// `AGENTS.md` §2 要求"每条结论可反驳"——用户该能自己去核对。
    /// 而依赖这一栏原来只有结论没有来源。四种语言的查法各不相同，
    /// 所以这个字段不是套话：说出来他就知道去哪儿看。
    ///
    /// 三个断言分开钉：
    ///   ① Rust 项目要有 `Cargo.lock` 这个来源；
    ///   ② Python **没拿到 pip list** 时，来源必须是"没有可查的地方"——
    ///      **"没查"与"没装"在文字上必须长得不一样**（三态在措辞上的落法）；
    ///   ③ 拿到 pip list 之后，来源要变成 `pip list`（不能停在"没查"）。
    #[test]
    fn 每条依赖都要带上是从哪儿查的() {
        // ① Rust
        let r = tmp("ev-rs");
        std::fs::write(r.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(r.join("Cargo.lock"), "version = 4\n").unwrap();
        let rd = collect(&r, &[Lang::Rust], None, &[]);
        if let Some(x) = rd.first() {
            assert_eq!(x.evidence, "Cargo.lock", "Rust 的证据是 Cargo.lock");
        }

        // ②③ Python：没查过 vs 查过了
        let p = tmp("ev-py");
        std::fs::write(p.join("requirements.txt"), "rich>=13.0.0\n").unwrap();
        let no_list = collect(&p, &[Lang::Python], None, &[]);
        assert!(
            no_list[0].evidence.contains("没有可查的地方"),
            "没拿到 pip list 时必须说'没有可查的地方'，实际：{}",
            no_list[0].evidence
        );
        assert!(
            !no_list[0].evidence.contains("pip list\""),
            "别把'没查'写成'查过了'"
        );
        let with_list = collect(&p, &[Lang::Python], None, &["rich==13.7.1".to_string()]);
        assert_eq!(with_list[0].evidence, "pip list", "查过了就说 pip list");
        assert!(
            with_list[0].ok,
            "有清单又装着，就是确认装好了——证据与结论要一致"
        );

        let _ = std::fs::remove_dir_all(&r);
        let _ = std::fs::remove_dir_all(&p);
    }

    /// **依赖被提升到仓库根**（npm / yarn / pnpm 的 workspace 都这么干）：
    /// 成员项目自己目录里没有 node_modules，得往上找。
    #[test]
    fn 依赖在仓库根的_node_modules_里也算装了() {
        let d = tmp("hoist");
        let member = d.join("apps").join("web");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(member.join("package.json"), r#"{"name":"web"}"#).unwrap();
        let pkg = d.join("node_modules").join("vue");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("package.json"), r#"{"version":"3.4.21"}"#).unwrap();
        assert_eq!(
            installed(&member, Lang::Web, "vue", &[]),
            Verdict::Yes(Some("3.4.21".to_string())),
            "版本要就地读出来，不是猜的"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 一个 node_modules 都没有、也不是 PnP —— 那是真的还没装。
    #[test]
    fn 真的没装才说没装() {
        let d = tmp("none");
        std::fs::write(d.join("package.json"), r#"{"name":"x"}"#).unwrap();
        assert_eq!(installed(&d, Lang::Web, "vue", &[]), Verdict::No);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rust_deps_come_from_all_three_sections() {
        let d = tmp("rust");
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"x\"\n\n[dependencies]\nserde = { version = \"1.0\", features = [\"derive\"] }\nanyhow = \"1\"\n\n[dev-dependencies]\npretty_assertions = \"1.4\"\n",
        )
        .unwrap();
        let got = declared(&d, Lang::Unknown);
        let names: Vec<&str> = got.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["anyhow", "pretty_assertions", "serde"]);
        assert_eq!(got[2].want, "1.0");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rust_lock_decides_installed() {
        let d = tmp("lock");
        std::fs::write(d.join("Cargo.toml"), "[dependencies]\nserde = \"1\"\n").unwrap();
        assert_eq!(installed(&d, Lang::Unknown, "serde", &[]), Verdict::No);
        std::fs::write(
            d.join("Cargo.lock"),
            "[[package]]\nname = \"serde\"\nversion = \"1.0.197\"\n",
        )
        .unwrap();
        assert_eq!(
            installed(&d, Lang::Unknown, "serde", &[]),
            Verdict::Yes(Some("1.0.197".into()))
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 成员目录自己不带锁文件，锁在工作区根上——向上找不到就会误报「未安装」。
    #[test]
    fn rust_lock_is_looked_up_in_parent_dirs() {
        let root = tmp("lockup");
        std::fs::write(root.join("Cargo.lock"), "[[package]]\nname = \"serde\"\nversion = \"1.0.197\"\n").unwrap();
        let member = root.join("crates").join("engine");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(member.join("Cargo.toml"), "[dependencies]\nserde = \"1\"\n").unwrap();
        assert_eq!(
            installed(&member, Lang::Unknown, "serde", &[]),
            Verdict::Yes(Some("1.0.197".into()))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 复合项目：一个 Tauri 仓库同时有 Cargo.toml 和 package.json，
    /// 只报一边就等于告诉用户「另一边不存在」。
    #[test]
    fn composite_projects_merge_every_manifest() {
        let d = tmp("composite");
        std::fs::write(d.join("Cargo.toml"), "[dependencies]\nserde = \"1\"\n").unwrap();
        std::fs::write(d.join("package.json"), r#"{"devDependencies":{"vite":"^5"}}"#).unwrap();
        let got = declared_all(&d, &[Lang::Web]);
        let names: Vec<&str> = got.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["serde", "vite"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn node_deps_read_both_groups() {
        let d = tmp("node");
        std::fs::write(
            d.join("package.json"),
            r#"{"dependencies":{"vue":"^3.4.0"},"devDependencies":{"vite":"^5.0.0"}}"#,
        )
        .unwrap();
        let got = declared(&d, Lang::Web);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "vite");
        assert_eq!(got[0].from, "package.json · devDependencies");
        // 没装（没有 node_modules）
        assert_eq!(installed(&d, Lang::Web, "vue", &[]), Verdict::No);
        std::fs::create_dir_all(d.join("node_modules").join("vue")).unwrap();
        assert_eq!(installed(&d, Lang::Web, "vue", &[]), Verdict::Yes(None));
        let _ = std::fs::remove_dir_all(&d);
    }


    /// 传递依赖必须读得出来，而且不许猜：Cargo.lock 里写着什么就是什么。
    #[test]
    fn rust_tree_reads_the_lock_graph() {
        let d = tmp("tree");
        std::fs::write(
            d.join("Cargo.lock"),
            "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\n \"serde\",\n \"walkdir 2.5.0\",\n]\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.229\"\ndependencies = [\n \"serde_derive\",\n]\n\n[[package]]\nname = \"serde_derive\"\nversion = \"1.0.229\"\n",
        )
        .unwrap();
        let (edges, total) = rust_tree(&d);
        assert_eq!(total, 3);
        assert!(edges.contains(&TreeEdge { from: "app".into(), to: "serde".into() }));
        assert!(edges.contains(&TreeEdge { from: "app".into(), to: "walkdir".into() }));
        assert!(edges.contains(&TreeEdge { from: "serde".into(), to: "serde_derive".into() }));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 没有锁文件时不许编：返回空，界面就什么都不显示。
    #[test]
    fn rust_tree_without_lock_is_empty() {
        let d = tmp("tree-none");
        let (edges, total) = rust_tree(&d);
        assert!(edges.is_empty());
        assert_eq!(total, 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **依赖的归属由 `lang` 决定，不是"这个目录里有什么文件"**（2026-10-16）。
    ///
    /// 这条守护的是一个**实测抓到的假 Error**：
    /// `jingwei guide --root D:\projects\jingwei` 报「1 个依赖没装：@tauri-apps/cli」，
    /// 而 `package.json` 声明了它、`node_modules/@tauri-apps/cli/package.json`
    /// **实实在在在磁盘上（2454 字节）**。
    ///
    /// 根因：`installed()` 的第一个分支写的是
    /// `root.join("Cargo.toml").is_file() && lang != Lang::Python`——
    /// **一个 Rust + 前端的项目（Tauri 就是）里，每条 npm 依赖都被拿去查 `Cargo.lock`**。
    /// 用户会去重装一个已经装好的包，然后发现问题依旧——**误报比空着糟得多**。
    ///
    /// 三个断言分开钉：
    ///   ① 有 `Cargo.toml` 时，**Web 依赖要去 `node_modules` 查**（不是 Cargo.lock）；
    ///   ② 同一个目录下，**Rust 依赖仍然走 `Cargo.lock`**（别修坏了另一头）；
    ///   ③ 查得到就要说"装了"，不能返回 `No`。
    #[test]
    fn 依赖归属由语言决定而不是目录里有什么文件() {
        let d = tmp("lang-decides");
        // 这是一个**混合项目**：Rust + 前端，两个清单都在
        std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(
            d.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"x","dependencies":{"vue":"^3"}}"#,
        )
        .unwrap();
        // 前端依赖装在 node_modules 里
        let nm = d.join("node_modules").join("vue");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("package.json"), r#"{"version":"3.4.0"}"#).unwrap();

        // ① Web 依赖：必须查 node_modules，不能因为 Cargo.toml 在就去查 Cargo.lock
        let web = installed(&d, Lang::Web, "vue", &[]);
        assert_eq!(
            web,
            Verdict::Yes(Some("3.4.0".to_string())),
            "Web 依赖要查 node_modules——有 Cargo.toml 不代表它是 Rust 依赖"
        );
        assert_ne!(web, Verdict::No, "它装着，不许说成没装（那是假 Error）");

        // ② Rust 依赖仍然走 Cargo.lock
        let rs = installed(&d, Lang::Rust, "serde", &[]);
        assert_eq!(
            rs,
            Verdict::Yes(Some("1.0.0".to_string())),
            "Rust 依赖还是要查 Cargo.lock"
        );

        // ③ 交叉验证：**同名的东西在另一门语言里查不到，那才该是 No**
        //    （`vue` 不在 Cargo.lock 里 → Rust 视角下确实是"没装"）
        assert_eq!(
            installed(&d, Lang::Rust, "vue", &[]),
            Verdict::No,
            "Rust 视角查 vue 确实该是 No——这不是误报，是它真不在 Cargo.lock 里"
        );

        let _ = std::fs::remove_dir_all(&d);
    }

    /// 快速路径拿不到 pip 清单时，Python 依赖必须标「无法确认」。
    /// 标成「未安装」就是误报——误报比空着更糟，它会让人不信这一栏。
    #[test]
    fn python_without_pip_list_is_unknown_not_missing() {
        let d = tmp("py-unknown");
        std::fs::write(d.join("requirements.txt"), "rich>=13.0.0\n").unwrap();
        let detail = collect(&d, &[Lang::Python], None, &[]);
        assert_eq!(detail.len(), 1);
        assert!(detail[0].unknown, "没有 pip 清单时必须标成无法确认");
        assert!(!detail[0].ok);
        // **这条测试原来是三条互相矛盾的说法里"断言赢"的那一条**，值得把历史留着：
        // 它叫 `…_is_unknown_not_missing`、注释写着"标成「未安装」就是误报"，
        // 而断言是 `status.get("rich") == Some(&false)`——**把那个误报固化成了期望**。
        // 这就是假 Error 能被固化下来的机制：**判据被写进断言，就成了"期望行为"**。
        //
        // 现在没有布尔图了（`deps_status` 这个类型 2026-10-16 删了），
        // 所以**类型层面就不存在"把未知写成 false"的地方**。
        // 三态只有 `detail` 一处表示：`ok=false` + `unknown=true`，
        // 而下游（规则层 / 快照 / 界面）都按 `unknown` 分档。
        assert!(
            detail[0].have.is_empty(),
            "无法确认时不该编出一个版本号"
        );
        let d2 = collect(&d, &[Lang::Python], None, &["rich==13.7.1".to_string()]);
        assert!(d2[0].ok);
        assert_eq!(d2[0].have, "13.7.1");
        assert!(!d2[0].unknown);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **"无法确认"要说清是谁的**（2026-10-16）。
    ///
    /// `unknown: bool` 说得出"不知道"，说不出"这个不知道该怪谁"——
    /// 而用户能做的事完全不同：我们的失败 → 换个办法；他的空白 → 补个文件。
    ///
    /// 三个断言**分开钉**（它们会各自坏）：
    ///   ① 没有 pip 清单 → **我们的失败**（问工具那一步没做）；
    ///   ② C++ → **我们的失败**（那个语言的检测我们没实现）；
    ///   ③ 已确认装好的**不许被归成失败**（那时这个问题不适用，要 `Unclear`）。
    #[test]
    fn 无法确认的依赖要说清是我们没查到() {
        // ① 没有 pip 清单
        let d = tmp("why-py");
        std::fs::write(d.join("requirements.txt"), "rich>=13.0.0\n").unwrap();
        let a = collect(&d, &[Lang::Python], None, &[]);
        assert!(a[0].unknown);
        assert_eq!(
            a[0].unknown_why(),
            crate::Unknown::OurGap,
            "没有 pip 清单是我们没问工具，不是用户没装"
        );
        // ② C++：那个语言我们没实现依赖检测
        let c = tmp("why-cpp");
        std::fs::write(c.join("vcpkg.json"), "{\"dependencies\":[\"opencv\"]}\n").unwrap();
        let b = collect(&c, &[Lang::Cpp], None, &[]);
        if let Some(x) = b.iter().find(|x| x.unknown) {
            assert_eq!(x.unknown_why(), crate::Unknown::OurGap, "C++ 是我们没实现");
        }
        // ③ 已确认装好的：这个问题不适用 → `Unclear`，**不许硬给归属**
        let e = collect(&d, &[Lang::Python], None, &["rich==13.7.1".to_string()]);
        assert!(e[0].ok && !e[0].unknown);
        assert_eq!(
            e[0].unknown_why(),
            crate::Unknown::Unclear,
            "已确认装好时不该有'谁造成的未知'——硬归一头就是编"
        );
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&c);
    }

    #[test]
    fn java_deps_merge_group_and_artifact() {
        let d = tmp("java");
        std::fs::write(
            d.join("pom.xml"),
            "<project><dependencies><dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.9</version></dependency></dependencies></project>",
        )
        .unwrap();
        let got = declared(&d, Lang::Java);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "org.slf4j:slf4j-api");
        assert_eq!(got[0].want, "2.0.9");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// C++ 的包可能来自系统、vcpkg、Conan 或源码树——查不到就必须说「无法确认」，
    /// 报成「未安装」就是误报，而误报会让人不再相信这个工具。
    #[test]
    fn cpp_is_never_reported_as_missing() {
        let d = tmp("cpp");
        std::fs::write(d.join("CMakeLists.txt"), "find_package(OpenCV REQUIRED)\n").unwrap();
        let got = declared(&d, Lang::Cpp);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "OpenCV");
        assert_eq!(installed(&d, Lang::Cpp, "OpenCV", &[]), Verdict::Unknown);
        let _ = std::fs::remove_dir_all(&d);
    }
