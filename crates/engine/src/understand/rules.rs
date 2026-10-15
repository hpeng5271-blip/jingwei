use crate::facts::Facts;
use crate::state::snapshot;

/// Java：撞得最多的三件事——只有 JRE、JAVA_HOME 指歪、装了多个 JDK 却用错版本。
fn r_java(x: &Facts) -> Vec<Finding> {
    let Some(j) = &x.jvm else { return vec![] };
    let mut out = Vec::new();
    let jdks: Vec<&crate::detect::jvm::Jdk> = j.jdks.iter().filter(|d| d.is_jdk).collect();

    if j.jdks.is_empty() {
        out.push(f(
            "java-no-runtime",
            Severity::Error,
            "这是个 Java 项目，但机器上没有 java",
            vec!["PATH 和常见安装位置里都没找到 java.exe".into()],
            vec!["where java".into(), "java -version".into()],
            "只在真的没装 Java 时出现",
            "装一个 JDK（不是 JRE）：Adoptium Temurin 或 Oracle 都行，装完重新体检",
            vec![],
        ));
    } else if jdks.is_empty() {
        out.push(f(
            "java-jre-only",
            Severity::Error,
            "只有 JRE，没有 JDK：能跑别人的 jar，编译不了自己的代码",
            vec![format!(
                "找到 {} 个 java.exe，但没有一个旁边有 javac.exe",
                j.jdks.len()
            )],
            vec!["where javac".into()],
            "只运行别人打好的 jar 时，JRE 确实够用",
            "装完整 JDK，并让 JAVA_HOME 指向它——Maven/Gradle 认的是 JAVA_HOME，不是 PATH",
            vec![],
        ));
    }

    // JAVA_HOME 没设，本身不是错误，只是「如果你从命令行构建，会失败」的前提。
    // 在 IDE 里点运行根本用不到它——一律报 Warning 就是每天报一次的误报。
    // 所以级别固定为提示，把「什么时候会真出事」写进证据里，让人自己判断。
    match (j.java_home.as_ref(), j.java_home_ok) {
        (None, _) => out.push(f(
            "java-home-missing",
            Severity::Info,
            "没设 JAVA_HOME",
            vec![
                "环境变量里没有 JAVA_HOME".into(),
                if j.java_on_path.is_some() {
                    "命令行里已经能直接找到 java，Maven/Gradle 也能跑".into()
                } else if j.maven || j.gradle {
                    "命令行里没有 java：从命令行跑 mvn / gradle 会说 \
                     「JAVA_HOME is not defined correctly」。在 IDE 里点运行不受影响。"
                        .into()
                } else {
                    "只从 IDE 里点运行的话，IDE 自己配了 JDK，不影响".into()
                },
            ],
            vec!["echo $env:JAVA_HOME".into()],
            "IDE 点运行、或者项目自带 mvnw/gradlew 时，确实可以不管",
            "设成 JDK 根目录（含 bin 的那一级），然后新开终端",
            vec![],
        )),
        (Some(h), false) => out.push(f(
            "java-home-broken",
            Severity::Error,
            format!("JAVA_HOME 指向的目录里没有 java.exe：{}", h),
            vec![format!("JAVA_HOME = {}", h)],
            vec![format!("dir {}\\bin", h)],
            "只在 JAVA_HOME 被改过、或那个 JDK 被卸载之后才出现",
            "改成一个真实存在的 JDK 根目录",
            vec![],
        )),
        _ => {}
    }

    // 「我明明装了 21，怎么还报 class file version 52」——就是这条
    let path_jdk = j
        .java_on_path
        .as_ref()
        .and_then(|p| j.jdks.iter().find(|d| d.java.eq_ignore_ascii_case(p)));
    if let (Some(home), Some(used)) = (j.java_home.as_ref(), path_jdk) {
        let home_java = std::path::Path::new(home).join("bin").join("java.exe");
        if !home_java.to_string_lossy().eq_ignore_ascii_case(&used.java) {
            out.push(f(
                "java-two-versions",
                Severity::Warn,
                "命令行里用的 Java，和 JAVA_HOME 指的不是同一个",
                vec![
                    format!("PATH 上第一个：{}", used.java),
                    format!("JAVA_HOME 指向：{}", home_java.display()),
                ],
                vec!["where java".into(), "java -version".into()],
                "两者主版本相同时不用管",
                "把 JAVA_HOME 的 bin 放到 PATH 前面，或者统一成同一个 JDK",
                vec![],
            ));
        }
    }
    if let (Some(req), Some(used)) = (j.required, path_jdk) {
        if used.major != 0 && req != used.major {
            out.push(f(
                "java-version-mismatch",
                Severity::Error,
                format!("项目要求 Java {}，命令行用的是 Java {}", req, used.major),
                vec![
                    format!("要求来自 {}", j.required_from.clone().unwrap_or_default()),
                    format!("实际：{}", used.version),
                ],
                vec!["java -version".into(), "mvn -v".into()],
                "只在项目声明了版本、且和实际不一致时出现",
                "换成对应版本的 JDK，或者改项目里的版本声明",
                vec![],
            ));
        }
    }
    // 版本隔离：一个项目实际会用哪个 JDK，以及它凭什么是那个
    if let Some(r) = &j.resolved {
        // 三种都不行：版本没认出来、钉的版本机器上没有（home 会是空的）、
        // 或者 home 摆在那儿但里面没有 java.exe
        let unusable = r.major == 0
            || r.home.is_empty()
            || !std::path::Path::new(&r.home).join("bin").join("java.exe").is_file();
        if unusable {
            out.push(f(
                "java-pinned-missing",
                Severity::Error,
                format!("项目钉死的 Java 版本用不了：{}", r.source),
                vec![
                    format!("来源：{}", r.source),
                    format!("期望：Java {}", if r.major == 0 { "?".to_string() } else { r.major.to_string() }),
                    format!("机器上装了：{}", j.jdks.iter().map(|d| format!("Java {}", d.major)).collect::<Vec<_>>().join("、")),
                ],
                vec!["java -version".into()],
                "只在项目用 .java-version / gradle.properties 指定了版本、而那个版本不在这台机器上时出现",
                "装那个版本，或者改掉项目里钉的版本号",
                vec![],
            ));
        }
    }
    // 声明了要求，但算出来的那个 JDK 对不上——真正会报 class file version 的场景
    if let (Some(req), Some(r)) = (j.required, &j.resolved) {
        if r.major != 0 && req > r.major {
            out.push(f(
                "java-toolchain-too-old",
                Severity::Error,
                format!("项目要 Java {}，但实际会用起来的是 Java {}", req, r.major),
                vec![
                    format!("要求来自 {}", j.required_from.clone().unwrap_or_default()),
                    format!("实际那个来自 {}", r.source),
                    format!("位置：{}", r.home),
                ],
                vec!["mvn -v".into(), "gradle -version".into()],
                "只在项目声明的版本比实际可用版本高时出现——这个一定会编译失败",
                "用 .java-version 或 JAVA_HOME 把项目指到装好的那个 JDK 上",
                vec![],
            ));
        }
    }
    out
}

/// C/C++：这里的坑不是「装了几个」，而是「装的是哪一家」。
fn r_cc(x: &Facts) -> Vec<Finding> {
    let Some(c) = &x.cc else { return vec![] };
    let mut out = Vec::new();
    if c.compilers.is_empty() {
        out.push(f(
            "cc-no-compiler",
            Severity::Error,
            "这是个 C/C++ 项目，但机器上没有任何编译器",
            vec!["gcc / g++ / clang / cl 都没找到".into()],
            vec!["where gcc".into(), "where cl".into()],
            "只用别人编译好的可执行文件时不需要编译器",
            "最简单是装 MSYS2 或 WinLibs 的 MinGW-w64，装完把 bin 加进 PATH",
            vec![],
        ));
    } else {
        let fams: Vec<String> = c
            .compilers
            .iter()
            .map(|c| c.family.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if fams.len() > 1 {
            out.push(f(
                // 一台机器上同时有 MSVC 和 MinGW 太正常了（装了 Visual Studio
                // 就带前者）。只有同一个项目里混着用才会出事，而那是构建设置的
                // 问题，不是这台机器的问题——所以这里是提示，不是警告。
                "cc-mixed-toolchains",
                Severity::Info,
                format!("这台机器上装着不止一家的编译器：{}", fams.join(" + ")),
                c.compilers
                    .iter()
                    .map(|c| format!("{}（{}）{}", c.id, c.family, c.exe))
                    .collect(),
                vec!["where gcc".into(), "where cl".into()],
                "只用一个构建系统、且它固定用哪一家时，不算问题",
                "同一个项目里只用一家：三家的目标文件格式和运行库互不通用，混用会报一堆看不懂的链接错误",
                vec![],
            ));
        }
    }
    if c.has_cmake_lists && c.cmake.is_none() {
        out.push(f(
            "cc-cmake-missing",
            Severity::Error,
            "项目用 CMake 组织，但机器上没有 cmake",
            vec!["项目根有 CMakeLists.txt，PATH 里没有 cmake.exe".into()],
            vec!["where cmake".into()],
            "只用 Makefile 直接编、不碰 CMake 时不会遇到",
            "装 CMake（官网的 Windows 安装包会顺手配好 PATH）",
            vec![],
        ));
    } else if c.has_makefile && !c.make && !c.ninja && c.cmake.is_none() {
        out.push(f(
            "cc-make-missing",
            Severity::Warn,
            "项目里有 Makefile，但机器上没有 make",
            vec!["PATH 里没有 make.exe，也没有 cmake / ninja 可以代劳".into()],
            vec!["where make".into()],
            "Windows 上本来就不带 make，只有装了 MSYS2/MinGW 才有",
            "装 MSYS2 或在 VS 开发者命令行里用 nmake",
            vec![],
        ));
    }
    out
}

/// 前端 / Node：主要就两件事——有没有 node，依赖装没装。
fn r_web(x: &Facts) -> Vec<Finding> {
    let ok = |name: &str| x.toolchain.get(name).map(|t| t.ok).unwrap_or(false);
    let mut out = Vec::new();
    if !ok("node") || !ok("npm") {
        out.push(f(
            "web-no-node",
            Severity::Error,
            "这是个前端项目，但机器上没有 Node.js",
            vec!["PATH 里没找到 node.exe / npm.cmd".into()],
            vec!["where node".into(), "where npm".into()],
            "只在真的没装 Node 时出现",
            "装 Node.js LTS，装完新开终端",
            vec![],
        ));
        return out;
    }
    let root = std::path::Path::new(&x.project.root);
    let has_pkg = root.join("package.json").is_file();
    let has_modules = root.join("node_modules").is_dir();
    if has_pkg && !has_modules {
        out.push(f(
            "web-deps-not-installed",
            Severity::Warn,
            "有 package.json，但 node_modules 还不存在",
            vec![format!("{} 里没有 node_modules", x.project.root)],
            vec!["npm install".into()],
            "刚克隆下来、或者刚清过缓存时，这是正常状态",
            "在项目里跑 npm install（有 package-lock.json 就用 npm ci，更快也更一致）",
            vec![],
        ));
    }
    out
}

// 这两个类型已经上移到 `model`（Q6 拆循环）：它们本来就是领域概念。
// 这里转发，**所有调用点都不用改**，而 `project → rules` 那条边从此消失。
pub use jingwei_model::{Finding, Severity};

#[allow(clippy::too_many_arguments)]
fn f(
    id: &str,
    severity: Severity,
    title: impl Into<String>,
    evidence: Vec<String>,
    verify: Vec<String>,
    boundary: &str,
    next: &str,
    excluded: Vec<String>,
) -> Finding {
    Finding {
        id: id.into(),
        severity,
        title: title.into(),
        layer: String::new(),
        layer_note: String::new(),
        group: String::new(),
        evidence,
        verify,
        boundary: boundary.into(),
        next: next.into(),
        excluded,
    }
}

/// 硬规矩：给不出「怎么证伪它」的结论，不许报成错误。
///
/// 这是「不误报」的底线。一个错误级的结论，用户要么照做、要么怀疑工具——
/// 而如果他手上没有一条能自己验证的命令，他就只剩下怀疑这一条路。
/// 所以这条不是写在文档里的约定，是写在这里、每次诊断都会过的一道闸。
fn enforce_evidence(out: &mut [Finding]) -> usize {
    let mut downgraded = 0;
    for x in out.iter_mut() {
        if x.severity == Severity::Error && x.verify.iter().all(|v| v.trim().is_empty()) {
            x.severity = Severity::Warn;
            if !x.boundary.is_empty() {
                x.boundary = format!("{}（没有可自查的命令，所以只报「注意」）", x.boundary);
            } else {
                x.boundary = "没有可自查的命令，所以只报「注意」".into();
            }
            downgraded += 1;
        }
    }
    downgraded
}

/// 纯函数：facts -> findings。没有 IO、没有打印。
pub fn run_all(facts: &Facts, prev: Option<&Facts>) -> Vec<Finding> {
    let mut out = Vec::new();
    // 按项目类型分派。打开一个 Java 项目却报「没找到 Python 解释器」，
    // 是这一轮之前真实存在的假警报——规则不分语言，等于在乱指路。
    let has = |l: crate::detect::lang::Lang| facts.langs.contains(&l);
    let pythonic = has(crate::detect::lang::Lang::Python)
        || (facts.langs.is_empty() && facts.lang == crate::detect::lang::Lang::Unknown);
    if pythonic {
        out.extend(r_isolated(facts));
        out.extend(r_python_missing(facts));
        out.extend(r_version(facts));
        out.extend(r_deps(facts));
        out.extend(r_venv(facts));
        out.extend(r_venv_unbound(facts));
        out.extend(r_encoding(facts));
    }
    if has(crate::detect::lang::Lang::Java) {
        out.extend(r_java(facts));
    }
    if has(crate::detect::lang::Lang::Cpp) {
        out.extend(r_cc(facts));
    }
    if has(crate::detect::lang::Lang::Web) {
        out.extend(r_web(facts));
    }
    out.extend(r_contract(facts));
    out.extend(r_memory(facts));
    out.extend(r_env_pollution(facts));
    out.extend(r_path_alias(facts));
    out.extend(r_snapshot(facts, prev));
    // 项目卫生排在最后：它不是「跑不起来」的原因，是「跑起来之后怎么别烂掉」。
    // 但它的条目一样带自查命令，一样不许硬猜。
    out.extend(r_hygiene(facts));
    // 先按层排，再按严重程度。层序就是排查顺序：
    // 「实际用的是哪个」永远排在「它缺什么」前面。顺序错了，
    // 人会先去修第二层的问题，而第一层根本没对。
    for x in out.iter_mut() {
        let (name, note, _) = layer_of(&x.id);
        x.layer = name.to_string();
        x.layer_note = note.to_string();
    }
    enforce_evidence(&mut out);
    out.sort_by(|a, b| {
        layer_of(&a.id)
            .2
            .cmp(&layer_of(&b.id).2)
            .then(a.severity.cmp(&b.severity))
            .then(a.id.cmp(&b.id))
    });
    out
}

/// 排查链。顺序是从资深做法里提炼的——环境类问题，答案几乎总落在
/// 「实际用的是哪个」和「谁把它设成这样的」这两层里，而不是「重装一遍试试」。
pub fn layer_of(id: &str) -> (&'static str, &'static str, u8) {
    match id {
        "memory-paging" | "memory-tight" => (
            "① 机器扛不扛得住",
            "内存、磁盘这些硬资源够不够——它比「东西装没装」更底层",
            1,
        ),
        "python-not-found" | "java-no-runtime" | "java-jre-only" | "cc-no-compiler"
        | "web-no-node" => ("② 东西有没有", "这台机器上有没有能干活的那件东西", 2),
        "isolated-interpreter" | "path-python-is-alias" | "java-two-versions"
        | "java-home-missing" | "java-home-broken" | "java-pinned-missing" => (
            "③ 用的是哪个",
            "东西有好几个，实际被用起来的是哪一个，以及为什么是它",
            3,
        ),
        "python-env-pollution" | "console-encoding" | "cc-mixed-toolchains" => (
            "④ 谁在背后改它",
            "环境变量和转发壳会在你看不见的地方改变结果",
            4,
        ),
        "contract-missing" | "contract-unparsable" | "python-version-mismatch"
        | "java-toolchain-too-old" => {
            ("⑤ 项目说要什么", "项目自己声明的要求，和实际之间的差", 5)
        }
        "required-dep-missing" | "optional-dep-missing" | "web-deps-not-installed"
        | "cc-cmake-missing" | "cc-make-missing" => ("⑥ 缺什么", "声明要的东西装没装", 6),
        "not-in-venv" | "venv-exists-but-unbound" => (
            "⑦ 隔离开了没有",
            "多个项目共用一套环境，迟早互相踩",
            7,
        ),
        "snapshot-changed" => (
            "⑧ 什么时候变的",
            "环境一直在变；只有「之前能跑、现在不能跑」时它才是证据",
            8,
        ),
        _ if crate::project::HYGIENE_IDS.contains(&id) => (
            "⑨ 项目卫生",
            "和语言无关的日常维护：哪些文件不该进仓库、依赖记在哪、这个目录有多大",
            9,
        ),
        _ => ("其他", "还没归类的提示", 99),
    }
}

/// 项目体检的条目直接来自 project::scan，这里只做两件事：
/// 1）贴上「项目卫生」这一层；
/// 2）补上只有拿到环境事实才算得出来的交叉核对（本机 Node 够不够新之类）。
/// 判断本身写在 project.rs 里，这边不重复第二套。
fn r_hygiene(x: &Facts) -> Vec<Finding> {
    let Some(s) = &x.project.hygiene else {
        return vec![];
    };
    let mut out: Vec<Finding> = s.items.clone();

    // package.json 说要 Node >= 20，而本机是 18 —— 这种错在 npm install 时
    // 才炸，而且报的是「engine 不匹配」这种拐了弯的话。
    if let (Some(req), Some(node)) = (&s.node_engines, x.toolchain.get("node")) {
        if node.ok {
            if let (Some(want), Some(got)) = (
                first_number(req),
                node.version.as_deref().and_then(first_number),
            ) {
                if got < want {
                    out.push(f(
                        "node-below-engines",
                        Severity::Warn,
                        format!(
                            "本机 Node 主版本 {} 低于 package.json 要求的 {}",
                            got, req
                        ),
                        vec![
                            format!("package.json engines.node = {}", req),
                            format!(
                                "本机 node 在 {}，版本 {}",
                                node.path.clone().unwrap_or_default(),
                                node.version.clone().unwrap_or_default()
                            ),
                        ],
                        vec!["node -v".into()],
                        "如果 engines 只是个宽松提示（比如 >=16）、而你的版本恰好低一点点，多数时候仍能跑。",
                        "换成满足条件的版本：nvm 或 fnm 都能同时装好几个 Node，按项目切。",
                        vec![],
                    ));
                }
            }
        }
    }

    // pyproject 里写了 requires-python，而工作集里的解释器太老。
    if let Some(req) = &s.py_requires {
        let (op, rest) = split_op(req);
        if matches!(op, "" | ">=" | "==" | "~=" | "^" | ">") {
            if let (Some(min), Some(got)) = (
                major_minor(rest),
                major_minor(&x.python.version),
            ) {
                if got < min && x.python.found {
                    out.push(f(
                        "py-requires-unmet",
                        Severity::Warn,
                        format!(
                            "项目要求 Python {}，当前用的是 {}",
                            req, x.python.version
                        ),
                        vec![
                            format!("pyproject.toml requires-python = {}", req),
                            format!("当前解释器：{}（{}）", x.python.exe, x.python.version),
                        ],
                        vec!["python --version".into()],
                        "如果你根本没用这套解释器跑它（比如一直用 IDE 里另一个），就不会有事。",
                        "装一个够新的解释器，然后把这个项目重新绑定到它上面。",
                        vec![],
                    ));
                }
            }
        }
    }

    // 两套锁文件并存，或者声明用 pnpm 却只有 npm 的锁文件。
    let locks_lower: Vec<String> = s.lockfiles.iter().map(|l| l.to_lowercase()).collect();
    let kinds: Vec<&str> = [
        ("package-lock.json", "npm"),
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
    ]
    .iter()
    .filter(|(f, _)| locks_lower.iter().any(|l| l.ends_with(f)))
    .map(|(_, k)| *k)
    .collect();
    if kinds.len() >= 2 {
        out.push(f(
            "node-mixed-lockfiles",
            Severity::Info,
            format!("同时存在 {} 套锁文件（{}）", kinds.len(), kinds.join(" + ")),
            vec![
                "装了两种包管理器，它们各记各的版本".into(),
                "换个人 clone 时用哪个装，结果可能不一样".into(),
            ],
            vec!["Get-ChildItem package-lock.json,pnpm-lock.yaml,yarn.lock -ErrorAction SilentlyContinue".into()],
            "你只是本地都装过、但团队里明确只用一种时，删掉多余的即可。",
            "定下用哪个包管理器，删掉另外几个锁文件（以及多余的 node_modules）。",
            vec![],
        ));
    }
    out
}

/// ">=3.11" -> (">=", "3.11")
fn split_op(s: &str) -> (&str, &str) {
    let i = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    (&s[..i], &s[i..])
}

/// 取字符串里的第一个数字串。"v24.11.0" -> 24，">=18" -> 18。
fn first_number(s: &str) -> Option<u32> {
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            break;
        }
    }
    cur.parse().ok()
}

/// 主次版本。"3.15.0" -> (3,15)；"4" -> (4,0)。
fn major_minor(s: &str) -> Option<(u32, u32)> {
    let mut nums = s.split(|c: char| !c.is_ascii_digit());
    let a = nums.find(|x| !x.is_empty())?.parse().ok()?;
    let b = nums.find(|x| !x.is_empty()).and_then(|x| x.parse().ok()).unwrap_or(0);
    Some((a, b))
}

fn r_isolated(x: &Facts) -> Vec<Finding> {
    let p = &x.python;
    if !(p.isolated || !p.pth_files.is_empty()) {
        return vec![];
    }
    vec![f(
        "isolated-interpreter",
        Severity::Warn,
        "你的 Python 是「嵌入式/便携发行版」：当前目录不会进 sys.path",
        vec![
            format!("sys.flags.isolated = {}", p.isolated),
            format!(
                "安装目录里的 ._pth：{}",
                if p.pth_files.is_empty() { "（无）".to_string() } else { p.pth_files.join("; ") }
            ),
            format!("sys.path[0] = {}", p.sys_path0),
        ],
        vec![r#"python -c "import sys; print(sys.flags.isolated, sys.path[:2])""#.into()],
        "只在便携版/嵌入式版（安装目录有 pythonXX._pth）上出现；官网 msi/exe 装的没这问题",
        "项目里放自定位入口（用 __file__ 把项目根插进 sys.path），或 python -m pip install -e .",
        vec![
            "报错写 No module named 'xxx' 时，先别怀疑拼写或漏装：这台机器上多半是它".into(),
            "PYTHONPATH 也救不了（隔离模式忽略环境变量）".into(),
        ],
    )]
}

fn r_python_missing(x: &Facts) -> Vec<Finding> {
    if x.python.found {
        return vec![];
    }
    vec![f(
        "python-not-found",
        Severity::Error,
        "没找到可用的 Python 解释器",
        vec!["在 PATH、项目 venv、常见安装位置里都没找到 python.exe".into()],
        vec!["where python".into(), "python -V".into()],
        "只在没装 Python 或 PATH 没配好时出现",
        "装一个（推荐 uv：uv python install 3.12），装完重新体检",
        vec!["换目录跑解决不了".into()],
    )]
}

fn r_contract(x: &Facts) -> Vec<Finding> {
    let p = &x.project;
    if let Some(err) = &p.contract_error {
        return vec![f(
            "contract-unparsable",
            Severity::Error,
            "env.contract.json 不是合法 JSON",
            vec![err.clone()],
            vec!["jingwei init --dry-run".into()],
            "只有你改过这个文件才会出现",
            "多半是少了逗号或引号",
            vec![],
        )];
    }
    if p.contract.is_none() {
        return vec![f(
            "contract-missing",
            Severity::Info,
            "这个项目还没有环境契约（env.contract.json）",
            vec![format!("项目根：{}", p.root)],
            vec!["jingwei init --dry-run".into()],
            "纯脚本目录不需要契约；要跑起来的项目才值得写",
            "用 jingwei init 生成一份",
            vec![],
        )];
    }
    vec![]
}

fn r_version(x: &Facts) -> Vec<Finding> {
    let Some(c) = &x.project.contract else { return vec![] };
    let Some(spec) = &c.runtime.python else { return vec![] };
    if x.python.version.is_empty() || range_ok(spec, &x.python.version) {
        return vec![];
    }
    vec![f(
        "python-version-mismatch",
        Severity::Error,
        format!("Python 版本不满足契约：需要 {}，当前 {}", spec, x.python.version),
        vec![format!("契约要求 {}", spec), format!("当前解释器 {}", x.python.exe)],
        vec!["python -V".into()],
        "只在你换了解释器或换了机器时出现",
        "要么换解释器，要么改契约",
        vec![],
    )]
}

fn range_ok(spec: &str, version: &str) -> bool {
    fn nums(s: &str) -> Vec<u32> {
        let mut v = Vec::new();
        for part in s.split(|c: char| !c.is_ascii_digit()) {
            if let Ok(n) = part.parse::<u32>() {
                v.push(n);
            }
        }
        while v.len() < 3 {
            v.push(0);
        }
        v
    }
    let cur = nums(version);
    let want = nums(spec);
    if want.iter().all(|n| *n == 0) || cur.iter().all(|n| *n == 0) {
        return true;
    }
    let spec = spec.trim();
    if spec.starts_with(">=") {
        cur >= want
    } else if spec.starts_with('>') {
        cur > want
    } else if spec.starts_with("==") {
        cur == want
    } else {
        true
    }
}

fn r_deps(x: &Facts) -> Vec<Finding> {
    let mut out = Vec::new();
    let contract = x.project.contract.as_ref();
    let req = contract.map(|c| c.deps.required.clone()).unwrap_or_default();
    let opt = contract.map(|c| c.deps.optional.clone()).unwrap_or_default();
    let missing: Vec<String> = x
        .project
        .deps_status
        .iter()
        .filter(|(_, ok)| !**ok)
        .map(|(k, _)| k.clone())
        .collect();
    if missing.is_empty() {
        return out;
    }
    let install = contract
        .and_then(|c| c.commands.install.clone())
        .unwrap_or_else(|| "python -m pip install -r requirements.txt".into());
    let hard: Vec<String> = missing
        .iter()
        .filter(|m| req.iter().any(|r| r.eq_ignore_ascii_case(m)))
        .cloned()
        .collect();
    let soft: Vec<String> = missing
        .iter()
        .filter(|m| opt.iter().any(|r| r.eq_ignore_ascii_case(m)))
        .cloned()
        .collect();
    if !hard.is_empty() {
        out.push(f(
            "required-dep-missing",
            Severity::Error,
            format!("契约要求的依赖没装：{}", hard.join(", ")),
            vec![
                format!("契约 deps.required = {:?}", req),
                format!(
                    "当前解释器已装 {} 个包，里面没有：{}",
                    x.python.installed.len(),
                    hard.join(", ")
                ),
            ],
            vec![format!("python -m pip show {}", hard[0])],
            "只有当代码真的用到它才是错；能降级的话把它挪到 deps.optional",
            &format!("装它：{}", install),
            vec!["换目录跑没用（依赖装在解释器里）".into()],
        ));
    }
    if !soft.is_empty() {
        out.push(f(
            "optional-dep-missing",
            Severity::Info,
            format!("可选依赖没装：{}（功能会自动降级）", soft.join(", ")),
            vec![format!("契约 deps.optional = {:?}", opt)],
            vec![format!("python -m pip show {}", soft[0])],
            "不影响功能，只影响体验；不想装就永远别管",
            &format!("想要就用：{}", install),
            vec![],
        ));
    }
    out
}

fn r_venv(x: &Facts) -> Vec<Finding> {
    if !x.python.found || x.python.in_venv || x.project.declared_deps.is_empty() {
        return vec![];
    }
    // 「你不在虚拟环境里」——对只有一个项目、只装它自己依赖的学习者来说，
    // 这不是问题，是正常状态。而且这条会在几乎每个没建 venv 的项目上冒出来，
    // 一律报警就是每天报一次的误报。降到提示级：话还在，不吓人。
    vec![f(
        "not-in-venv",
        Severity::Info,
        "这个项目有依赖，但当前解释器不在虚拟环境里",
        vec![
            format!("解释器 {}", x.python.exe),
            format!("prefix = base_prefix = {}", x.python.prefix),
            format!("项目声明的依赖：{}", x.project.declared_deps.join(", ")),
        ],
        vec![r#"python -c "import sys;print(sys.prefix == sys.base_prefix)""#.into()],
        "只有一个项目、也只装它的依赖时，可以不管",
        "jingwei env init 建一个并绑上（不需要 activate，也不看 PATH）",
        vec![],
    )]
}

/// 项目里明明有虚拟环境，工具却在用机器上别处的解释器。
/// 这是「在我机器上能跑」的头号来源：同一个项目，两个解释器，两套已装包。
fn r_venv_unbound(x: &Facts) -> Vec<Finding> {
    if x.project.venvs.is_empty() || !x.python.found {
        return vec![];
    }
    let used = x.python.exe.to_lowercase();
    let inside = x
        .project
        .venvs
        .iter()
        .any(|v| used.starts_with(&v.to_lowercase()));
    if inside {
        return vec![];
    }
    let bound = x
        .project
        .contract
        .as_ref()
        .and_then(|c| c.runtime.python_exe.clone());
    vec![f(
        "venv-exists-but-unbound",
        Severity::Warn,
        "项目里有虚拟环境，但工具没在用它",
        vec![
            format!("项目里的虚拟环境：{}", x.project.venvs.join("；")),
            format!("实际在用的解释器：{}", x.python.exe),
            match &bound {
                Some(b) => format!("契约里绑的是：{}（它就是那个不在项目里的）", b),
                None => "契约里没绑解释器，所以工具自己挑了一个".into(),
            },
        ],
        vec!["jingwei env status".into()],
        "虚拟环境是空的、或者你故意想用全局环境时，这不算错",
        &format!(
            "绑过去：jingwei env use {}\\Scripts\\python.exe   （或者重新 jingwei env init）",
            x.project.venvs[0]
        ),
        vec![
            "换目录跑解决不了：挑解释器跟你在哪敲命令无关".into(),
            "这个项目声明的依赖，装在了另一个解释器里".into(),
        ],
    )]
}

/// PYTHONPATH / PYTHONHOME 是全局的，会改变 import 的结果。
/// 它们最阴的地方是：只影响你这台机器，代码本身看不出来。
fn r_env_pollution(x: &Facts) -> Vec<Finding> {
    let mut hits = Vec::new();
    for k in ["PYTHONPATH", "PYTHONHOME"] {
        if let Some(v) = x.system.env.get(k) {
            hits.push(format!("{} = {}", k, v));
        }
    }
    if hits.is_empty() {
        return vec![];
    }
    vec![f(
        "python-env-pollution",
        Severity::Warn,
        "有环境变量在偷偷改变 import 的结果",
        hits,
        vec![
            r#"python -c "import sys; print(sys.path)""#.into(),
            r#"python -c "import os; print(os.environ.get('PYTHONPATH'))""#.into(),
        ],
        "只在确实需要跨目录共享代码时才该设；设了之后，同一份代码在别人机器上不一定跑得起来",
        "先确认是谁设的（多半是某个安装脚本写进了用户环境变量），不需要就删掉",
        vec!["只是设了、但指向的目录不存在，通常没有实际影响".into()],
    )]
}

/// 命令行里的 python 指向的是转发壳（应用商店别名之类），
/// 于是你在终端里看到的行为，和工具里跑出来的不一样。
fn r_path_alias(x: &Facts) -> Vec<Finding> {
    let Some(on_path) = &x.python.path_on_path else {
        return vec![];
    };
    if on_path.is_empty() || x.python.exe.eq_ignore_ascii_case(on_path) {
        return vec![];
    }
    // 只有 PATH 上那个是「转发壳」才值得说：真安装的话，两边其实是同一个，
    // 提一句也是噪音。
    if !on_path.to_lowercase().contains("windowsapps") {
        return vec![];
    }
    vec![f(
        "path-python-is-alias",
        Severity::Info,
        "你在终端里敲 python，和工具用的不是同一个",
        vec![
            format!("PATH 找到的第一个 python：{}", on_path),
            format!("工具实际用的解释器：{}", x.python.exe),
        ],
        vec!["where python".into(), "python -c \"import sys; print(sys.executable)\"".into()],
        "这是 Windows 应用商店/安装管理器的转发机制，不算坏；但两边的包是两套",
        "在终端里干活时，把这个项目的虚拟环境激活，或者用工具的 run",
        vec!["工具永远用真解释器的绝对路径，所以不受 PATH 影响".into()],
    )]
}

/// 机器资源层：它回答的是「这台机器扛不扛得住」，比「东西装没装」更底层。
/// 只在上一次真的跑到换页、或者物理内存贴着底的时候说话——
/// 内存每时每刻都在变，随便波动就报就是天天报警。
fn r_memory(x: &Facts) -> Vec<Finding> {
    let Some(m) = &x.last_mem else { return vec![] };
    // 采不准的数字不拿来下结论。这条比什么都重要：
    // 拿一次没采到的样本去说「你内存不够」，就是误报。
    if !m.reliable {
        return vec![];
    }
    let mut out = Vec::new();
    let who: Vec<String> = m
        .others
        .iter()
        .take(5)
        .map(|p| format!("{}（{} MB）", p.name, p.working_mb))
        .collect();
    if m.paging {
        out.push(f(
            "memory-paging",
            Severity::Warn,
            "上次跑的时候内存不够用了，多出来的部分被搬到了磁盘上",
            vec![
                format!("命令：{}（{}）", m.command, m.at),
                format!(
                    "那一刻系统已提交 {:.1} GB，物理内存只有 {:.1} GB",
                    m.sys_commit_used_mb as f64 / 1024.0,
                    m.phys_total_mb as f64 / 1024.0
                ),
                format!("进程自己峰值 {} MB", m.peak_working_mb),
                if who.is_empty() {
                    String::new()
                } else {
                    format!("同一时刻还占着内存的：{}", who.join(" · "))
                },
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect(),
            vec!["jingwei run".into()],
            "只在「已提交内存超过物理内存」时才算数；系统偶尔抖动不会报",
            "编译这类活最怕换页——先把上面那几个不干活却占着内存的东西关掉，再跑一次对比",
            vec![
                "内存占用高不等于有问题：程序本来就该用内存".into(),
                "这条说的是「用超了、被搬到磁盘上了」，不是「你用得多」".into(),
            ],
        ));
    } else if m.phys_total_mb > 0 && m.phys_avail_at_peak_mb < m.phys_total_mb / 10 {
        out.push(f(
            "memory-tight",
            Severity::Info,
            "上次跑的时候物理内存已经贴着底了，还没换页，但再加点东西就会",
            vec![
                format!("命令：{}（{}）", m.command, m.at),
                format!(
                    "只剩 {} MB / {:.1} GB 可用",
                    m.phys_avail_at_peak_mb,
                    m.phys_total_mb as f64 / 1024.0
                ),
            ],
            vec!["jingwei run".into()],
            "贴着底不等于会变慢；只有真的换页才慢",
            "不用现在做什么，知道这个余量就够了",
            vec![],
        ));
    }
    out
}

fn r_encoding(x: &Facts) -> Vec<Finding> {
    if x.system.env.contains_key("PYTHONUTF8") {
        return vec![];
    }
    vec![f(
        "console-encoding",
        Severity::Info,
        "没设 PYTHONUTF8：中文/emoji 可能在控制台报编码错",
        vec!["环境变量里没有 PYTHONUTF8".into()],
        vec![r#"python -c "print('中文')""#.into()],
        "只打印英文时永远不用管",
        "启动时会自动带上 PYTHONUTF8=1（工具内启动不受影响）",
        vec![],
    )]
}

fn r_snapshot(x: &Facts, prev: Option<&Facts>) -> Vec<Finding> {
    let Some(prev) = prev else { return vec![] };
    let changes = snapshot::diff(prev, x);
    if changes.is_empty() {
        return vec![];
    }
    let hard = changes.iter().any(|c| {
        c.kind.starts_with("path") || c.kind.starts_with("tool") || c.kind.starts_with("python")
    });
    vec![f(
        "snapshot-changed",
        if hard { Severity::Warn } else { Severity::Info },
        format!("与上一份快照相比，环境变了 {} 处", changes.len()),
        changes.iter().take(12).map(|c| c.describe()).collect(),
        vec!["jingwei snap --diff".into()],
        "环境一直在变；只有「之前能跑、现在不能跑」时，这份对比才是证据",
        "先看哪一条和出问题的时间点对得上",
        vec![],
    )]
}
// 学习场景下，误报比漏报更坏：一个平平无奇、什么都没配错的项目，
// 如果冒出「要修」，用户会开始怀疑自己，然后怀疑工具。
#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
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

    /// 初学者最常见的项目长这样：一个 .py，没有契约、没有依赖声明。
    /// 这种项目不该有任何「要修」。
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

    /// 「不误报」的底线是能被验证。给不出自查命令的结论，
    /// 用户只能在「照做」和「怀疑工具」之间猜——那就降级。
    #[test]
    fn an_error_without_a_self_check_is_downgraded() {
        let mut out = vec![
            f(
                "no-proof",
                Severity::Error,
                "我猜你有问题",
                vec!["没有证据".into()],
                vec![],
                "",
                "去改点什么",
                vec![],
            ),
            f(
                "has-proof",
                Severity::Error,
                "这个有据可查",
                vec!["证据".into()],
                vec!["where python".into()],
                "",
                "照做",
                vec![],
            ),
        ];
        let n = enforce_evidence(&mut out);
        assert_eq!(n, 1, "只该降级一条");
        assert_eq!(out[0].severity, Severity::Warn, "没有自查命令的不能留在错误级");
        assert_eq!(out[1].severity, Severity::Error, "有自查命令的不受影响");
        assert!(out[0].boundary.contains("没有可自查的命令"));
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
}
