//! Java / JVM 项目的**前四个阶段**：用哪个 JDK、跑哪条命令、依赖、以及"凭什么"。
//!
//! ## 这一层最要紧的一件事：**不猜**
//!
//! 一个 Maven 项目能不能"跑起来"，答案不在我们的偏好里，而在 `pom.xml` 里：
//!
//! | pom 里有什么 | 怎么跑 | 有多确定 |
//! |---|---|---|
//! | `spring-boot-maven-plugin`（或 `spring-boot-starter-parent`） | `mvnw spring-boot:run` | 确定——这是那个插件自己注册的目标 |
//! | Gradle 的 `org.springframework.boot` 插件 | `gradlew bootRun` | 确定 |
//! | Gradle 的 `application` 插件 | `gradlew run` | 确定 |
//! | 都没有 | **不猜**——如实说"看不出怎么启动"，并给出怎么指定的办法 | —— |
//!
//! 最后一行是这一层的态度。硬编一条 `java -jar target/*.jar` 之类看起来很聪明，
//! 但那要假设 jar 已经构建过、名字是那个、清单里有主类——**猜错一次，
//! 用户就会以为"这个工具连启动都不会"**。

use super::{Event, LaunchOptions, Prepared, Purpose, StageStatus, TOTAL_STAGES};
use crate::detect::{deps, jvm, lang::Lang};
use crate::understand::contract::Contract;
use crate::util::winutil::which;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 构建工具。**包一层"Maven / Gradle"是不够的**——真正要执行的是
/// 项目自带的 wrapper（`mvnw` / `gradlew`）还是全局那个，这两者的版本可以不同。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tool {
    /// `mvnw` / `mvn` / `gradlew` / `gradle`
    pub exe: PathBuf,
    pub wrapper: bool,
    pub kind: ToolKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolKind {
    Maven,
    Gradle,
}

pub(crate) fn prepare(
    root: &Path,
    opts: &LaunchOptions,
    contract: &Option<Contract>,
    contract_err: &Option<String>,
    cb: &mut dyn FnMut(Event),
) -> Result<Prepared> {
    // ---- 1 检查运行环境 ----
    let t = Instant::now();
    let facts = jvm::probe_jvm(root);
    let (s1, d1) = describe_env(&facts);
    cb(Event::Stage {
        index: 1,
        total: TOTAL_STAGES,
        title: "检查运行环境".into(),
        status: s1,
        detail: d1,
        ms: t.elapsed().as_millis(),
    });

    // ---- 2 校验项目契约 ----
    let t = Instant::now();
    let tool = pick_tool(root);
    let spring = is_spring_boot(root);
    let (s2, d2) = match (contract, contract_err) {
        (None, Some(e)) => (StageStatus::Fail, format!("契约解析失败：{e}")),
        (Some(c), _) if opts.purpose == Purpose::Run && c.run_command().is_some() => (
            StageStatus::Ok,
            format!(
                "{} · run: {}",
                c.name.clone().unwrap_or_else(|| "（未命名）".into()),
                c.run_command().unwrap_or("")
            ),
        ),
        _ => {
            let mut d = match &tool {
                Some(t) if t.wrapper => format!(
                    "用项目自带的 wrapper：{}（它是这个项目钉住的构建工具版本）",
                    t.exe.display()
                ),
                Some(t) => format!(
                    "这个项目没有 wrapper，用 PATH 上的 {}（{}）",
                    t.exe.display(),
                    if t.kind == ToolKind::Maven { "Maven" } else { "Gradle" }
                ),
                None => "没找到构建工具（pom.xml / build.gradle 都没有，或 PATH 上没有 mvn / gradle）".to_string(),
            };
            if spring {
                d.push_str(" · 认出来是 Spring Boot 项目");
            }
            (StageStatus::Note, d)
        }
    };
    cb(Event::Stage {
        index: 2,
        total: TOTAL_STAGES,
        title: "校验项目契约".into(),
        status: s2,
        detail: d2,
        ms: t.elapsed().as_millis(),
    });

    // ---- 3 解析依赖声明 ----
    let t = Instant::now();
    let declared = deps::declared(root, Lang::Java);
    let mut missing = Vec::new();
    let mut unknown = Vec::new();
    for d in &declared {
        match deps::installed(root, Lang::Java, &d.name, &[]) {
            deps::Verdict::No => missing.push(d.name.clone()),
            deps::Verdict::Unknown => unknown.push(d.name.clone()),
            deps::Verdict::Yes(_) => {}
        }
    }
    let (s3, d3) = if declared.is_empty() {
        (StageStatus::Note, "pom / build.gradle 里没有解析出依赖".to_string())
    } else if !missing.is_empty() {
        (
            StageStatus::Note,
            format!(
                "{} 项里本地仓库缺 {} 项（{}）——构建时 Maven 会自己去下，第一次会慢",
                declared.len(),
                missing.len(),
                head(&missing, 5)
            ),
        )
    } else if !unknown.is_empty() {
        (
            StageStatus::Note,
            format!("{} 项都在本地仓库；{} 项确认不了", declared.len(), unknown.len()),
        )
    } else {
        (StageStatus::Ok, format!("{} 项都在本地仓库", declared.len()))
    };
    cb(Event::Stage {
        index: 3,
        total: TOTAL_STAGES,
        title: format!("解析依赖声明（{} 项）", declared.len()),
        status: s3,
        detail: d3,
        ms: t.elapsed().as_millis(),
    });

    // ---- 4 准备运行时 ----
    let t = Instant::now();
    let plan = match plan_command(&tool, spring, opts.purpose) {
        Ok(p) => p,
        Err(e) => {
            cb(Event::Stage {
                index: 4,
                total: TOTAL_STAGES,
                title: "准备运行时".into(),
                status: StageStatus::Fail,
                detail: e.to_string(),
                ms: t.elapsed().as_millis(),
            });
            return Err(e);
        }
    };
    cb(Event::Stage {
        index: 4,
        total: TOTAL_STAGES,
        title: "准备运行时".into(),
        status: StageStatus::Ok,
        detail: format!("{} {}（{}）", plan.exe.display(), plan.args.join(" "), plan.why),
        ms: t.elapsed().as_millis(),
    });

    let live = plan.live;
    Ok(Prepared {
        program: plan.exe,
        args: plan.args,
        // **JAVA_HOME 要按我们查出来的结论设**：机器上装了三个 JDK、
        // PATH 上那个和项目要求的不一样，是新手最常撞的坑之一
        // （《复审记录》里那一句"我明明装了 21，编译却报 class file version 52"）。
        // Maven / Gradle 都认 JAVA_HOME，设它比改 PATH 更精确。
        env: facts
            .resolved
            .as_ref()
            .map(|r| vec![("JAVA_HOME".to_string(), r.home.clone())])
            .unwrap_or_default(),
        run_cmd: plan.run_cmd,
        alive_unready: if live { StageStatus::Note } else { StageStatus::Fail },
        url_watch: live && opts.purpose == Purpose::Run,
        ready_rule: if live && opts.purpose == Purpose::Run {
            // 两种都算：服务把地址报出来了，或者它正常跑完退出了。
            // 只写前者的话，"已就绪 · 退出码 0（判定依据：输出里出现监听地址）"
            // 会自相矛盾——依据和证据对不上，看的人立刻就不信这一栏了。
            "监听地址（localhost / 127.0.0.1 / port 8080 这类）或正常退出".to_string()
        } else {
            "退出码 0".to_string()
        },
    })
}

#[derive(Debug)]
struct Plan {
    exe: PathBuf,
    args: Vec<String>,
    run_cmd: String,
    why: String,
    /// 跑起来之后会一直占着端口（`spring-boot:run` 这种）
    live: bool,
}

/// 第 1 阶段那句话。**有 JDK 说清是哪个、凭据是什么**——
/// 一台机器上有三个 JDK 的时候，"用的是哪个"比"有没有"重要得多。
fn describe_env(f: &jvm::JvmFacts) -> (StageStatus, String) {
    if f.jdks.is_empty() && f.java_on_path.is_none() {
        return (
            StageStatus::Fail,
            "没找到 JDK。装一个（Adoptium / Zulu / IDEA 里下都行），或者把 JAVA_HOME 指对".to_string(),
        );
    }
    let mut d = match &f.resolved {
        Some(r) => format!("JDK {} · {}（凭据：{}）", r.major, r.home, r.source),
        // **找得到 JDK、却说不清用哪个**——这本身就是一条结论，而且是最值钱的那条。
        // 「我明明装了 21，编译却报 class file version 52」就是从这儿来的。
        // 三件事一起给：装了哪些、为什么说不清、怎么定下来。
        None => {
            let list: Vec<String> = f
                .jdks
                .iter()
                .map(|d| format!("{}（{}）", d.major, d.home))
                .collect();
            format!(
                "装了 {} 个 JDK：{}。但**说不清这次会用哪一个**——JAVA_HOME 没设、PATH 上没有 java。想让结果确定，就把 JAVA_HOME 指到你要的那个",
                f.jdks.len(),
                list.join(" · ")
            )
        }
    };
    if let (Some(req), Some(from)) = (&f.required, &f.required_from) {
        d.push_str(&format!(" · 项目要求 {req}（{from}）"));
        if let Some(r) = &f.resolved {
            if *req != r.major {
                // 只报事实，不编建议：可能是 pom 写得太死，也可能是真装错了
                d.push_str(&format!("——实际是 {}，对不上", r.major));
            }
        }
    }
    // 这里不再另说"装了几个 JDK"——上面那条结论已经逐个列出来了，
    // 同一件事说两遍只会把这一行撑长。
    let s = if f.resolved.is_none() {
        StageStatus::Note
    } else {
        StageStatus::Ok
    };
    (s, d)
}

/// 项目的构建工具。**wrapper 优先**：它钉着这个项目自己的版本，
/// 而 PATH 上那个全局的可能是另一代（Maven 3 与 4、Gradle 7 与 8 差别不小）。
fn pick_tool(root: &Path) -> Option<Tool> {
    let maven_project =
        root.join("pom.xml").is_file() || root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file();
    if !maven_project {
        return None;
    }
    let is_gradle =
        root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file();
    let kind = if is_gradle {
        ToolKind::Gradle
    } else {
        ToolKind::Maven
    };
    // Windows 上是 .cmd / .bat，别的平台上没有后缀
    let wrappers: &[&str] = match kind {
        ToolKind::Maven => &["mvnw.cmd", "mvnw"],
        ToolKind::Gradle => &["gradlew.bat", "gradlew"],
    };
    for w in wrappers {
        let p = root.join(w);
        if p.is_file() {
            return Some(Tool {
                exe: p,
                wrapper: true,
                kind,
            });
        }
    }
    let global = match kind {
        ToolKind::Maven => which("mvn.cmd").or_else(|| which("mvn")),
        ToolKind::Gradle => which("gradle.bat").or_else(|| which("gradle")),
    }?;
    Some(Tool {
        exe: global,
        wrapper: false,
        kind,
    })
}

/// 这个项目是不是 Spring Boot。**只认写在文件里的东西**：
/// pom 里的插件、Gradle 里的插件 id。
pub(crate) fn is_spring_boot(root: &Path) -> bool {
    for f in ["pom.xml"] {
        if let Ok(t) = std::fs::read_to_string(root.join(f)) {
            if t.contains("spring-boot-maven-plugin") || t.contains("spring-boot-starter-parent") {
                return true;
            }
        }
    }
    for f in ["build.gradle", "build.gradle.kts"] {
        if let Ok(t) = std::fs::read_to_string(root.join(f)) {
            if t.contains("org.springframework.boot") {
                return true;
            }
        }
    }
    false
}

/// 这个用途下跑什么。**查不到就报错，不硬编**。
fn plan_command(tool: &Option<Tool>, spring: bool, purpose: Purpose) -> Result<Plan> {
    let Some(tool) = tool else {
        return Err(anyhow!(
            "没找到构建工具：这个目录里没有 pom.xml / build.gradle，或者 PATH 上没有 mvn / gradle。\
             如果命令是别的，用 jingwei run --run \"<你的命令>\" 指定"
        ));
    };
    let (sub, why, live) = match (tool.kind, purpose, spring) {
        (ToolKind::Maven, Purpose::Run, true) => (
            "spring-boot:run",
            "pom 里有 spring-boot-maven-plugin，这是它注册的启动目标",
            true,
        ),
        (ToolKind::Gradle, Purpose::Run, true) => (
            "bootRun",
            "build.gradle 里引了 org.springframework.boot 插件，bootRun 是它的启动任务",
            true,
        ),
        (ToolKind::Gradle, Purpose::Run, false) => (
            "run",
            "没看到 Spring Boot 插件；`run` 来自 Gradle 的 application 插件",
            true,
        ),
        (_, Purpose::Run, false) => {
            return Err(anyhow!(
                "看不出这个 Maven 项目怎么启动：pom 里没有 spring-boot-maven-plugin。\
                 要么补上那个插件，要么直接指定：jingwei run --run \"mvn exec:java -Dexec.mainClass=你的主类\""
            ))
        }
        (ToolKind::Maven, Purpose::Build, _) => ("package", "打包（会跑测试）", false),
        (ToolKind::Gradle, Purpose::Build, _) => ("build", "打包（会跑测试）", false),
        (ToolKind::Maven, Purpose::Task, _) => ("test", "Maven 的测试目标", false),
        (ToolKind::Gradle, Purpose::Task, _) => ("test", "Gradle 的测试任务", false),
        (ToolKind::Maven, Purpose::Install, _) => (
            "dependency:go-offline",
            "把所有依赖（含插件）一次下到本地仓库，之后构建就不用再等网络",
            false,
        ),
        // **Gradle 没有"装依赖"这一步**——依赖是构建时按需解析的。
        // 硬编一条 `gradle dependencies` 只会打印依赖树，不下载任何东西，
        // 而那会让人以为"装好了"。
        (ToolKind::Gradle, Purpose::Install, _) => {
            return Err(anyhow!(
                "Gradle 没有单独的「装依赖」这一步：依赖在构建时按需下载。\
                 要现在就把它们拉下来，用 jingwei run --run \"gradlew build\""
            ))
        }
    };
    Ok(Plan {
        exe: tool.exe.clone(),
        args: vec![sub.to_string()],
        run_cmd: format!("{} {sub}", tool.exe.display()),
        why: why.to_string(),
        live,
    })
}

fn head(v: &[String], n: usize) -> String {
    if v.len() <= n {
        v.join(", ")
    } else {
        format!("{} 等 {} 项", v[..n].join(", "), v.len())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::let_underscore_must_use)]

    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jw-java-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造一个"有构建文件 + 自带 wrapper"的项目。
    ///
    /// **测试里必须落到 wrapper 上**：全局 `mvn` / `gradle` 在没装的机器上找不到，
    /// 那会让这几条测试的结论随"跑测试的机器"变——而测试要的是确定的答案。
    /// （真机上"没有 wrapper 也没有全局命令"那种情况，由 `plan_command` 的
    /// 那条错误分支如实说出来。）
    fn tool(root: &Path, kind: ToolKind) -> Option<Tool> {
        let (name, wrapper) = match kind {
            ToolKind::Maven => ("pom.xml", "mvnw.cmd"),
            ToolKind::Gradle => ("build.gradle", "gradlew.bat"),
        };
        std::fs::write(root.join(name), "").unwrap();
        std::fs::write(root.join(wrapper), "@echo off\n").unwrap();
        pick_tool(root)
    }

    /// **Spring Boot 认得出**，而且是从文件里认的，不是靠目录名猜的。
    #[test]
    fn 从_pom_里认出_spring_boot() {
        let d = tmp("spring");
        std::fs::write(
            d.join("pom.xml"),
            "<project><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>",
        )
        .unwrap();
        assert!(is_spring_boot(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **看不出怎么启动时不猜**——这一条是这一层的核心态度。
    #[test]
    fn 普通_maven_项目不硬编启动命令() {
        let d = tmp("plain");
        let t = tool(&d, ToolKind::Maven);
        assert!(t.is_some(), "有 pom.xml 就该认出 Maven");
        let e = plan_command(&t, false, Purpose::Run).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("看不出"), "{msg}");
        assert!(msg.contains("--run"), "要给出办法：{msg}");
        // 但测试和打包是确定的，照样给
        assert_eq!(
            plan_command(&t, false, Purpose::Task).unwrap().args,
            vec!["test".to_string()]
        );
        assert_eq!(
            plan_command(&t, false, Purpose::Build).unwrap().args,
            vec!["package".to_string()]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn spring_boot_的启动命令来自插件() {
        let d = tmp("boot");
        let t = tool(&d, ToolKind::Maven);
        let p = plan_command(&t, true, Purpose::Run).unwrap();
        assert_eq!(p.args, vec!["spring-boot:run".to_string()]);
        assert!(p.live, "spring-boot:run 是常驻的");
        assert!(
            p.why.contains("spring-boot-maven-plugin"),
            "凭据要说清是 pom 里的哪个插件给的：{}",
            p.why
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **Gradle 没有"装依赖"这一步**——硬编一条只会打印依赖树。
    #[test]
    fn gradle_的装依赖如实说不适用() {
        let d = tmp("gradle");
        let t = tool(&d, ToolKind::Gradle);
        let msg = plan_command(&t, false, Purpose::Install).unwrap_err().to_string();
        assert!(msg.contains("没有单独的"), "{msg}");
        assert_eq!(
            plan_command(&t, false, Purpose::Build).unwrap().args,
            vec!["build".to_string()]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **wrapper 优先于全局命令**：它钉着这个项目自己的构建工具版本。
    #[test]
    fn 项目自带的_wrapper_优先() {
        let d = tmp("wrapper");
        std::fs::write(d.join("pom.xml"), "<project/>").unwrap();
        std::fs::write(d.join("mvnw.cmd"), "@echo off\n").unwrap();
        let t = pick_tool(&d).unwrap();
        assert!(t.wrapper, "有 mvnw 就该用它");
        assert!(t.exe.ends_with("mvnw.cmd"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
