//! `rules.rs` 的测试。
//!
//! **为什么单独一个文件**：`rules.rs` 早就超了 1000 行上限、登记在案（只许降不许升）。
//! 测试是能整块搬走的那一半（两百来行），而且它**不参与编译产物**。
//!
//! 隔离性没有变：`use super::*` 仍然拿得到 `rules.rs` 里的私有项，
//! 因为它仍然是那个模块的子模块（由 `#[path]` 指定文件位置）。

#![allow(clippy::let_underscore_must_use)]

// 测试用的就是父模块里的私有项（`run_all` / `Severity` / `Facts`），
// 靠这个通配导入拿到——所以它**不能删**（删了当场编译不过）。
// 文件名由 `#[path]` 指定，模块层级没变，`super` 仍然是 `rules`。
use super::*;

    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pl-rules-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn errors(facts: &Facts) -> Vec<String> {
        run_all(facts, None)
            .into_iter()
            .filter(|f| f.severity == Severity::Error)
            .map(|f| f.id)
            .collect()
    }

    /// **Rust 项目要有 Rust 规则。**
    ///
    /// 这条是 `tauri_like`（`package.json` + `src-tauri/Cargo.toml`）暴露的：
    /// 那个项目报出来的**全是 Node 的事**，Rust 那一半一句不说——
    /// 因为 Rust 有检测、有依赖解析、有诊断通道，唯独**体检规则一条都没有**。
    #[test]
    fn a_rust_project_gets_rust_rules() {
        let d = scratch("rust");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(d.join("src").join("main.rs"), "fn main(){}").unwrap();
        let mut facts = crate::detect::probe::make_facts(&d, None);
        // 这个项目必须被认成 Rust——不然下面的断言测的是别的东西
        assert!(
            facts.langs.contains(&crate::detect::lang::Lang::Rust),
            "夹具没被认成 Rust，测试本身不成立：{:?}",
            facts.langs
        );

        // ① 机器上**有** rustc 时：不许报「缺工具链」。
        //    （这条机器的 rustc 是有的；没有的话跳过，不假装通过。）
        facts.toolchain.insert(
            "rustc".into(),
            crate::facts::ToolFact { ok: true, ..Default::default() },
        );
        let ids: Vec<String> = run_all(&facts, None).into_iter().map(|f| f.id).collect();
        assert!(
            !ids.contains(&"rust-no-toolchain".to_string()),
            "rustc 在的时候不该报缺工具链，实际报的是：{ids:?}"
        );

        // ② 拿掉 rustc：**必须报，而且是「要处理」级别**——
        //    没工具链时 cargo build 直接起不来，这是唯一一道会把人挡死的墙。
        facts.toolchain.insert("rustc".into(), crate::facts::ToolFact::default());
        let e = errors(&facts);
        assert!(
            e.contains(&"rust-no-toolchain".to_string()),
            "没 rustc 时必须报「要处理」，实际：{e:?}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 反过来：**Python 项目不该收到 Rust 规则**。
    /// 规则按语言分派这件事，两个方向都要钉住——只钉一个方向，
    /// 分派写反了照样能通过。
    #[test]
    fn a_python_project_gets_no_rust_rule() {
        let d = scratch("py-not-rust");
        std::fs::write(d.join("main.py"), "print(1)\n").unwrap();
        let mut facts = crate::detect::probe::make_facts(&d, None);
        facts.toolchain.insert("rustc".into(), crate::facts::ToolFact::default());
        let ids: Vec<String> = run_all(&facts, None).into_iter().map(|f| f.id).collect();
        assert!(
            !ids.contains(&"rust-no-toolchain".to_string()),
            "Python 项目不该收到 Rust 规则，实际：{ids:?}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **"无法确认"绝不许报成「要处理」。**
    ///
    /// 这条是 2026-10-16 外部对照复核抓出的**假 Error**，也是这个项目
    /// 「三态 / 不误报」那条线在规则层的验收：
    ///
    /// - `deps_status` 是 `BTreeMap<String, bool>`——**从类型上就装不下三态**，
    ///   而原来 `Verdict::Unknown` 被插成 `false`，于是"我们没查过"在任何读它的地方
    ///   都等于"没装"，命中契约 `deps.required` 就连 `Severity::Error` 报出去，
    ///   证据还写着「当前解释器已装 0 个包，里面没有：X」——**那句话是我们没查过的事实**。
    /// - 什么时候会变成 Unknown：枚举 pip 列表时异常被吞掉、或者机器上没装 Python。
    ///   那两种都**不是"没装"**。
    ///
    /// 断言两个方向：unknown 不许报 `required-dep-missing`（别名"假 Error"），
    /// 而且**确认真没装时必须照报**（不然就把这条规则改哑了）。
    #[test]
    fn an_unknown_dependency_is_never_an_error() {
        use crate::facts::DepDetail;

        let mut facts = Facts::default();
        facts.project.contract = Some(serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "name": "x",
            "deps": { "required": ["rich"], "optional": [] }
        })).expect("契约该能解析"));
        // 无法确认：`unknown = true` 且 `ok = false`
        facts.project.deps_detail = vec![DepDetail {
            name: "rich".into(),
            want: ">=13".into(),
            have: String::new(),
            ok: false,
            unknown: true,
            from: "requirements.txt".into(),
            evidence: "pip list".into(),
        }];
        let ids: Vec<String> = run_all(&facts, None).into_iter().map(|f| f.id).collect();
        let errs = errors(&facts);
        assert!(
            !errs.contains(&"required-dep-missing".to_string()),
            "无法确认的依赖不许报「要处理」——那是拿我们没查过的事去指责用户：{errs:?}"
        );
        assert!(
            ids.contains(&"dep-status-unknown".to_string()),
            "但也不能一声不响：沉默的 Unknown 和假 Error 一样让人判断错，只是方向相反：{ids:?}"
        );

        // 反方向：**确认真没装**时，必须照报错误（别把规则改哑了）
        facts.project.deps_detail[0].unknown = false;
        let errs = errors(&facts);
        assert!(
            errs.contains(&"required-dep-missing".to_string()),
            "确认真没装时必须报「要处理」，否则这条规则就白写了：{errs:?}"
        );
    }

    /// 一个普通项目不该有任何「要修」。
    #[test]
    fn a_plain_python_project_raises_no_errors() {
        let d = scratch("healthy");
        std::fs::write(d.join("main.py"), "print('hi')\n").unwrap();
        let facts = crate::detect::probe::make_facts(&d, None);
        let e = errors(&facts);
        assert!(e.is_empty(), "一个普通项目不该有「要修」级别的问题，实际有：{:?}", e);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 刚建的 Java 项目，只因为没设 JAVA_HOME 就报警，是典型误报：
    /// 在 IDE 里点运行根本用不到它。
    #[test]
    fn missing_java_home_is_never_an_error() {
        let d = scratch("java");
        std::fs::write(d.join("pom.xml"), "<project/>").unwrap();
        let facts = crate::detect::probe::make_facts(&d, None);
        let hit = run_all(&facts, None)
            .into_iter()
            .find(|f| f.id == "java-home-missing");
        if let Some(f) = hit {
            assert_ne!(f.severity, Severity::Error, "没设 JAVA_HOME 不该是错误级");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 复合项目里两边都得照顾到：Java 那边不该报 Python 的问题。
    #[test]
    fn a_pure_java_project_gets_no_python_findings() {
        let d = scratch("pure");
        std::fs::write(d.join("pom.xml"), "<project/>").unwrap();
        let facts = crate::detect::probe::make_facts(&d, None);
        let ids: Vec<String> = run_all(&facts, None).into_iter().map(|f| f.id).collect();
        assert!(
            !ids.iter().any(|i| i.starts_with("python") || i == "not-in-venv" || i == "isolated-interpreter"),
            "纯 Java 项目里不该出现 Python 的结论：{:?}",
            ids
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    fn mem_facts(paging: bool, reliable: bool, avail_mb: u64) -> Facts {
        Facts {
            last_mem: Some(crate::sys::mem::MemFacts {
                at: "2026-01-01T00:00:00Z".into(),
                command: "python main.py".into(),
                reliable,
                paging,
                peak_working_mb: 900,
                phys_total_mb: 16 * 1024,
                phys_avail_at_peak_mb: avail_mb,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// 采不准的样本不许用来下结论。拿一次没采到的数字说「你内存不够」，
    /// 就是最没道理的那种误报。
    #[test]
    fn unreliable_memory_evidence_says_nothing() {
        assert!(r_memory(&mem_facts(true, false, 100)).is_empty());
    }

    #[test]
    fn real_paging_is_reported_at_the_bottom_layer() {
        let out = r_memory(&mem_facts(true, true, 200));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "memory-paging");
        assert_eq!(layer_of(&out[0].id).2, 1, "资源层必须排在整条链的最前面");
    }

    /// 内存用得多不是问题，用超了才是。这条区分不清，就会天天报警。
    #[test]
    fn using_a_lot_of_memory_is_not_a_problem_by_itself() {
        assert!(r_memory(&mem_facts(false, true, 4 * 1024)).is_empty());
    }

    #[test]
    fn being_nearly_out_is_a_hint_not_a_warning() {
        let out = r_memory(&mem_facts(false, true, 512));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].severity, Severity::Info, "还没换页就只该是提示");
    }

    /// 「不误报」的底线是**三条一起**：权威来源 / 非推测置信度 / 有自查命令。
    /// 缺哪条就降级，并把原因写进 `boundary`——用户能看见为什么降级。
    ///
    /// 2026-10-16：这道闸原来**只有第三条**（verify 非空），前两条长在
    /// `model` 的 `Conclusion::error_checked` 上，而 `Conclusion` 生产零调用——
    /// 也就是说设计里"估的不许报 Error"在机器上只守住了三分之一。
    /// 现在 `Finding` 带上了 `source` / `confidence`，这里把三条各钉一次。
    #[test]
    fn an_error_without_a_self_check_is_downgraded() {
        let mut out = vec![
            // ① 有权威来源、有自查命令，但**没有**自查命令 → 降级（第 3 条闸）
            f_meta(
                "no-proof",
                Severity::Error,
                "我猜你有问题",
                vec!["没有证据".into()],
                vec![],
                "",
                "去改点什么",
                vec![],
            ),
            // ② 三条都齐 → 留在 Error
            f_meta(
                "has-proof",
                Severity::Error,
                "这个有据可查",
                vec!["证据".into()],
                vec!["where python".into()],
                "",
                "照做",
                vec![],
            ),
            // ③ 有自查命令、但**来源不权威** → 降级（第 1 条闸）
            {
                let mut x = f(
                    "guess-source",
                    Severity::Error,
                    "来源不权威",
                    vec!["证据".into()],
                    vec!["where python".into()],
                    "",
                    "照做",
                    vec![],
                );
                x.confidence = Confidence::Certain; // 只让"来源"这一条不满足
                x
            },
            // ④ 有权威来源与自查命令、但**置信度是推测** → 降级（第 2 条闸）
            {
                let mut x = f_meta(
                    "guessed",
                    Severity::Error,
                    "这是估的",
                    vec!["证据".into()],
                    vec!["where python".into()],
                    "",
                    "照做",
                    vec![],
                );
                x.confidence = Confidence::Guessed;
                x
            },
        ];
        let n = enforce_evidence(&mut out);
        assert_eq!(n, 3, "该降级三条（各缺一条闸），实际 {n}");
        assert_eq!(out[0].severity, Severity::Warn, "没有自查命令的不能留在错误级");
        assert_eq!(out[1].severity, Severity::Error, "三条齐的不受影响");
        assert!(out[0].boundary.contains("没有可复制的自查命令"));
        assert_eq!(out[2].severity, Severity::Warn, "来源不权威的不能留在错误级");
        assert!(out[2].boundary.contains("不算权威"));
        assert_eq!(out[3].severity, Severity::Warn, "猜测出来的不能留在错误级");
        assert!(out[3].boundary.contains("估的"));
    }

    /// 真跑一遍全部规则：不管哪条触发，都不许有「没有自查命令的错误」漏出去。
    #[test]
    fn no_rule_can_ship_an_unverifiable_error() {
        let d = scratch("audit");
        // 空目录是最容易同时触发好几条的状态
        let facts = crate::detect::probe::make_facts(&d, None);
        let got = run_all(&facts, None);
        for x in &got {
            if x.severity == Severity::Error {
                assert!(
                    x.verify.iter().any(|v| !v.trim().is_empty()),
                    "规则 {} 报成了错误却没有自查命令，会被降级闸拦下——说明它写错了",
                    x.id
                );
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }
