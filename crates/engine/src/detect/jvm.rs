//! Java 那一摊：JDK 在哪、是不是真 JDK、和项目声明的要求对不对得上。
//!
//! 新手撞得最多的三件事，这里都要说清楚：
//!   1. 机器上只有 JRE（能跑不能编译），装完 JDK 又忘了配 JAVA_HOME
//!   2. 装了三个 JDK，PATH 上的和 JAVA_HOME 指的不是同一个
//!      ——「我明明装了 21」，编译却报 class file version 52
//!   3. pom 里写着 17，实际用的是 8

use crate::util::parallel;
use crate::util::winutil::{run_cmd, which};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Jdk {
    /// 安装根目录（bin 的上一级）
    pub home: String,
    pub java: String,
    pub javac: String,
    /// 主版本号：21 / 17 / 8
    pub major: u32,
    pub version: String,
    /// 有没有 javac——没有就只是 JRE
    pub is_jdk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JvmFacts {
    pub java_home: Option<String>,
    pub java_home_ok: bool,
    pub jdks: Vec<Jdk>,
    pub java_on_path: Option<String>,
    /// 项目声明要求的 Java 版本
    pub required: Option<u32>,
    pub required_from: Option<String>,
    pub maven: bool,
    pub gradle: bool,
    /// ~/.m2/toolchains.xml 里声明的 JDK。这是 Maven 做版本隔离的正规做法：
    /// pom 只写「我要 17」，具体哪个 17 由这份文件决定。
    #[serde(default)]
    pub toolchains: Vec<Toolchain>,
    /// gradle.properties 里的 org.gradle.java.home
    #[serde(default)]
    pub gradle_java_home: Option<String>,
    /// .java-version（jenv / sdkman 的约定）
    #[serde(default)]
    pub pinned: Option<String>,
    /// 最后算出来的结论：这个项目实际会用哪个 JDK，以及凭据是什么
    #[serde(default)]
    pub resolved: Option<Resolved>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Toolchain {
    pub version: String,
    pub home: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Resolved {
    /// 这个结论是从哪儿来的，比如「.java-version」「JAVA_HOME」「PATH」
    pub source: String,
    pub home: String,
    pub major: u32,
}

fn roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for raw in [
        r"%ProgramFiles%\Java",
        r"%ProgramFiles%\Eclipse Adoptium",
        r"%ProgramFiles%\Amazon Corretto",
        r"%ProgramFiles%\Zulu",
        r"%ProgramFiles%\BellSoft",
        r"%ProgramFiles%\Microsoft",
        r"%ProgramFiles%\Semeru",
        r"%ProgramFiles%\GraalVM",
        r"%LOCALAPPDATA%\Programs\Eclipse Adoptium",
        // IDEA 自己下载的 JDK 落在这儿，用户往往只用它，PATH 上什么都没有。
        r"%USERPROFILE%\.jdks",
        r"%USERPROFILE%\.sdkman\candidates\java",
        // 便携版 JDK 常躺在这种"开发工具总目录"里，且故意不进 PATH
        r"%SystemDrive%\SDK",
        r"%SystemDrive%\tools",
        r"D:\tools",
        r"D:\SDK",
        r"C:\tools",
        r"C:\SDK",
        r"D:\Java",
    ] {
        let p = crate::detect::editors::expand(raw);
        if p.is_dir() {
            v.push(p);
        }
    }
    v
}

/// 从 `openjdk version "21.0.1" 2023-10-17` 里抠出 21。
/// 1.8 那代要特别照顾：它是 "1.8.0_381"，主版本是 8 不是 1。
fn parse_major(s: &str) -> u32 {
    let Some(q) = s.find('"') else { return 0 };
    let rest = &s[q + 1..];
    let Some(e) = rest.find('"') else { return 0 };
    let ver = &rest[..e];
    let mut parts = ver.split(['.', '_', '-']);
    let first = parts.next().unwrap_or("");
    let num = |x: &str| -> u32 {
        x.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    };
    if first == "1" {
        num(parts.next().unwrap_or(""))
    } else {
        num(first)
    }
}

fn probe_jdk(java: &Path) -> Jdk {
    let home = java
        .parent()
        .and_then(|b| b.parent())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let javac = java
        .parent()
        .map(|b| b.join(if cfg!(windows) { "javac.exe" } else { "javac" }))
        .filter(|p| p.is_file());
    // 先用安装目录里那份 release 文件——每套 JDK 都带，里面有 JAVA_VERSION。
    // 读一个几百字节的文件是微秒级；起一次 JVM 问版本要几百毫秒。
    // 三套 JDK 摊下来，这一点就是 2.8 秒和 0.3 秒的差别。
    let (line, major) = match version_from_release(Path::new(&home)) {
        Some((line, major)) => (line, major),
        None => {
            // 没有 release 文件才退回去起进程。
            // java -version 写的是 stderr 不是 stdout——这个坑不踩一次记不住。
            let out = run_cmd(java, &["-version"], Duration::from_secs(12))
                .map(|(_, o)| o)
                .unwrap_or_default();
            let line = out.lines().next().unwrap_or("").trim().to_string();
            let major = parse_major(&line);
            (line, major)
        }
    };
    Jdk {
        home,
        java: java.to_string_lossy().to_string(),
        javac: javac
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        major,
        version: line,
        is_jdk: javac.is_some(),
    }
}

/// JDK 根目录下的 release 文件，形如 `JAVA_VERSION="17.0.20.1"`。
fn version_from_release(home: &Path) -> Option<(String, u32)> {
    let text = std::fs::read_to_string(home.join("release")).ok()?;
    let raw = text
        .lines()
        .find_map(|l| l.strip_prefix("JAVA_VERSION="))?
        .trim()
        .trim_matches('"')
        .to_string();
    let major = major_of(&raw);
    (major != 0).then(|| (format!("openjdk version \"{}\"", raw), major))
}

/// 从纯版本号里取主版本："1.8.0_504" → 8，"17.0.20.1" → 17。
fn major_of(ver: &str) -> u32 {
    let mut parts = ver.split(['.', '_', '-']);
    let first = parts.next().unwrap_or("");
    let num = |x: &str| -> u32 {
        x.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    };
    if first == "1" {
        num(parts.next().unwrap_or(""))
    } else {
        num(first)
    }
}

fn candidate_javas() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let push = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if p.is_file()
            && !out
                .iter()
                .any(|x| x.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy()))
        {
            out.push(p);
        }
    };
    // JAVA_HOME 优先：构建工具真正认的是它
    if let Ok(h) = std::env::var("JAVA_HOME") {
        push(PathBuf::from(&h).join("bin").join("java.exe"), &mut out);
    }
    if let Some(p) = which("java.exe") {
        push(p, &mut out);
    }
    for root in roots() {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            for sub in [e.path().join("bin"), e.path().join("content").join("bin")] {
                push(sub.join("java.exe"), &mut out);
            }
        }
    }
    out
}

fn tag_value(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let i = text.find(&open)? + open.len();
    let j = text[i..].find(&close)? + i;
    Some(text[i..j].trim().to_string())
}

fn numeric(s: &str) -> Option<u32> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok().filter(|n| *n >= 6 && *n <= 99)
}

/// 只读构建文件里关于 Java 版本的那一行，不做 XML 解析——
/// 为一个数字引一个 XML 库不划算。
fn required_version(root: &Path) -> Option<(u32, String)> {
    if let Ok(t) = std::fs::read_to_string(root.join("pom.xml")) {
        for tag in ["maven.compiler.release", "maven.compiler.target", "java.version"] {
            if let Some(n) = tag_value(&t, tag).and_then(|v| numeric(&v)) {
                return Some((n, format!("pom.xml 的 <{}>", tag)));
            }
        }
    }
    for f in ["build.gradle", "build.gradle.kts"] {
        if let Ok(t) = std::fs::read_to_string(root.join(f)) {
            for tag in ["sourceCompatibility", "languageVersion"] {
                if let Some(i) = t.find(tag) {
                    let tail: String = t[i..].chars().take(48).collect();
                    if let Some(n) = numeric(&tail) {
                        return Some((n, format!("{} 里的 {}", f, tag)));
                    }
                }
            }
        }
    }
    None
}

pub fn probe_jvm(root: &Path) -> JvmFacts {
    let toolchains = read_toolchains();
    let gradle_java_home = gradle_property(root, "org.gradle.java.home");
    let pinned = std::fs::read_to_string(root.join(".java-version"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // 每台 JDK 都要起子进程问版本，并行起来
    let jdks = parallel::map(candidate_javas(), |p| probe_jdk(&p));
    let java_home = std::env::var("JAVA_HOME")
        .ok()
        .filter(|h| !h.trim().is_empty());
    let probed_home = java_home
        .as_ref()
        .map(|h| Path::new(h).join("bin").join("java.exe").is_file())
        .unwrap_or(false);
    let (required, required_from) = match required_version(root) {
        Some((n, w)) => (Some(n), Some(w)),
        None => (None, None),
    };
    let java_on_path = which("java.exe").map(|p| p.to_string_lossy().to_string());
    let resolved = resolve(
        pinned.as_deref(),
        gradle_java_home.as_deref(),
        java_home.as_deref(),
        &java_on_path,
        &jdks,
    );
    JvmFacts {
        java_home,
        java_home_ok: probed_home,
        jdks,
        java_on_path,
        required,
        required_from,
        maven: which("mvn.cmd").is_some() || which("mvn").is_some(),
        gradle: which("gradle.bat").is_some()
            || which("gradle").is_some()
            || root.join("gradlew.bat").is_file()
            || root.join("mvnw.cmd").is_file(),
        toolchains,
        gradle_java_home,
        pinned,
        resolved,
    }
}

/// Maven 的 toolchains.xml 放在用户目录下，是「一机多 JDK」的正规做法：
/// pom 里只写版本要求，实际用哪个由这份文件决定。
fn read_toolchains() -> Vec<Toolchain> {
    let Some(home) = std::env::var_os("USERPROFILE") else {
        return Vec::new();
    };
    let p = PathBuf::from(home).join(".m2").join("toolchains.xml");
    let Ok(text) = std::fs::read_to_string(p) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for chunk in text.split("<toolchain>").skip(1) {
        let block = chunk.split("</toolchain>").next().unwrap_or("");
        if tag_value(block, "type").as_deref() != Some("jdk") {
            continue;
        }
        let version = tag_value(block, "version").unwrap_or_default();
        let home = tag_value(block, "jdkHome").unwrap_or_default();
        if !home.is_empty() {
            out.push(Toolchain { version, home });
        }
    }
    out
}

/// 从 gradle.properties 取一个属性。Gradle 的 toolchain 也可能写在
/// build.gradle 里，但那是代码不是配置，这里只读得动的部分。
fn gradle_property(root: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join("gradle.properties")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// 这个项目实际会用哪个 JDK？按真实优先级往下问，第一个说话的就是答案。
///
/// 顺序不是随便排的：越往下越隐式。`.java-version` 是特意钉死的，
/// JAVA_HOME 是老习惯，PATH 是最后的兜底。
fn resolve(
    pinned: Option<&str>,
    gradle_home: Option<&str>,
    java_home: Option<&str>,
    java_on_path: &Option<String>,
    jdks: &[Jdk],
) -> Option<Resolved> {
    let exe_of = |h: &str| Path::new(h).join("bin").join("java.exe");
    let major_at = |exe: &str| {
        jdks.iter()
            .find(|d| d.java.eq_ignore_ascii_case(exe))
            .map(|d| d.major)
            .unwrap_or(0)
    };
    if let Some(v) = pinned {
        let want = numeric(v).unwrap_or(0);
        if let Some(d) = jdks.iter().find(|d| d.major == want) {
            return Some(Resolved {
                source: ".java-version".into(),
                home: d.home.clone(),
                major: d.major,
            });
        }
        // 钉了一个机器上压根没有的版本——这本身就是最该说出来的事
        return Some(Resolved {
            source: format!(".java-version 要求 {}（机器上没装）", v),
            home: String::new(),
            major: want,
        });
    }
    if let Some(h) = gradle_home {
        return Some(Resolved {
            source: "gradle.properties 的 org.gradle.java.home".into(),
            home: h.to_string(),
            major: major_at(&exe_of(h).to_string_lossy()),
        });
    }
    if let Some(h) = java_home {
        return Some(Resolved {
            source: "JAVA_HOME".into(),
            home: h.to_string(),
            major: major_at(&exe_of(h).to_string_lossy()),
        });
    }
    if let Some(p) = java_on_path {
        let major = major_at(p);
        if major != 0 {
            return Some(Resolved {
                source: "PATH".into(),
                home: jdks
                    .iter()
                    .find(|d| d.java.eq_ignore_ascii_case(p))
                    .map(|d| d.home.clone())
                    .unwrap_or_default(),
                major,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    #[test]
    fn java_8_is_read_as_8_not_1() {
        // 1.8 那代的版本串长得不一样，最容易读错
        assert_eq!(parse_major(r#"java version "1.8.0_381""#), 8);
        assert_eq!(parse_major(r#"openjdk version "21.0.1" 2023-10-17"#), 21);
        assert_eq!(parse_major(r#"java version "17.0.9" 2023-10-17 LTS"#), 17);
        assert_eq!(parse_major("garbage"), 0);
    }

    #[test]
    fn reads_the_version_a_pom_declares() {
        let d = std::env::temp_dir().join(format!("pl-jvm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("pom.xml"),
            "<project><properties><maven.compiler.release>17</maven.compiler.release></properties></project>",
        )
        .unwrap();
        let (n, w) = required_version(&d).unwrap();
        assert_eq!(n, 17);
        assert!(w.contains("pom.xml"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
