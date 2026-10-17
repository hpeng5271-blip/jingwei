//! 从一次运行的输出尾部里，认出**要记进图谱的那些诊断**。
//!
//! ## 为什么单独一个文件
//!
//! 它是 M5 接线时才出现的：`record_run` 要"这次跑出了哪些问题"，而只有编排那一层
//! 手里有输出尾部。写进去之后 `launch.rs` 就撞了 1000 行上限——而这次拆分正好落在
//! 一条真实接缝上：**它只跟"字符串 → 诊断"有关，不懂进程、不懂阶段、不懂就绪判定。**
//! （门禁那条行数上限第三次逼出正确的拆分，前两次是 `graph/imports.rs` 与 `launch/ready.rs`。）
//!
//! ## 一条不可退让的规矩：宁可少记，不可瞎挂
//!
//! 命令行工具失败时，最有信息量的就是它自己打印的那几行。但**认不出来时一条都不加**，
//! 由调用方退回"一条没有落点的症结"。理由和图谱那一层是同一条：
//! **图谱里一条指着错文件的边，比缺一条边糟得多**——缺了只是图不全，错了是图在骗人。

use serde::Serialize;
use std::collections::VecDeque;
use std::path::Path;

/// 这次运行**在图谱里**要留下的那一条诊断：一个标题，外加一个可选的目标文件。
///
/// 为什么单独一个类型而不是直接 `(String, Option<String>)`：那个元组在调用点
/// 完全看不出哪个是哪个（哪个是标题、哪个是路径，全靠记）——M5 一直没接上，
/// 有一半原因就是没人愿意去猜那个元组的顺序。
///
/// `file` 为 `None` 是**正常情形**：命令行工具失败时报的是程序自己打印的话，
/// 未必带文件位置（比如"缺包 requests"）。图谱那边不会瞎挂，只记一条没有落点的诊断。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunDiagnostic {
    pub title: String,
    /// 项目内相对路径（用 / 分隔），没有就是 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// 从输出尾部里认出院落明确的真诊断。只收两种**能判定**的形状：
///
/// 1. `cargo check --message-format=json` 的编译器诊断（带文件名、行列、错误码）——
///    解析器是现成的（`understand::diag::parse_cargo_line`），**同一份判断只有一处实现**；
/// 2. 纯文本 `path:line:col: error|warning: message`（pyright / mypy / gcc 一类）。
pub fn collect_run_diagnostics(root: &Path, tail: &VecDeque<String>) -> Vec<RunDiagnostic> {
    let mut out: Vec<RunDiagnostic> = Vec::new();
    // 语言专属的解析器要看**整段尾部**（traceback 与栈都是多行的），
    // 所以先把环形缓冲摊平一次（`make_contiguous` 是 O(1)，不会搬数据）。
    let mut buf = tail.clone();
    let all: &[String] = buf.make_contiguous();
    for line in all {
        if let Some(d) = crate::understand::diag::parse_cargo_line(line, root) {
            out.push(RunDiagnostic {
                title: if d.code.is_empty() {
                    d.message.clone()
                } else {
                    format!("{} {}", d.code, d.message)
                },
                file: Some(d.file),
            });
            continue;
        }
        // **各语言自己的形状排在通用形状前面。**
        //
        // 顺序有讲究：`parse_plain_diag` 认的是 `path:line:col: msg` 这种**通用**形状，
        // 而下面几个认的是**某一门语言特有**的形状（traceback 的缩进帧 / JS 的 `at` 栈帧 /
        // Maven 的 `[ERROR] path:[行,列]`）。先试特有的：它们能多给一样东西——
        // 把"出错在哪一行"说准。通用的放后面，才不会把特有的盖掉。
        //
        // 前两个一次调用就认整段尾部（各自只产出一条）；被扫几遍就产出几条重复的，
        // 函数末尾那步去重会收成一个。Maven 那条是按行的（一行一个 `[ERROR]`）。
        if let Some(d) = parse_python_traceback(all).or_else(|| parse_node_stack(all)) {
            out.push(d);
            continue;
        }
        if let Some(d) = parse_maven_error(line) {
            out.push(d);
            continue;
        }
        if let Some(d) = parse_plain_diag(line) {
            out.push(d);
        }
    }
    // 同一个位置重复报（编译器会把同一诊断派发多次）→ 去重，保持第一次出现的顺序
    let mut seen: Vec<(String, Option<String>)> = Vec::new();
    out.retain(|d| {
        let k = (d.title.clone(), d.file.clone());
        if seen.contains(&k) {
            false
        } else {
            seen.push(k);
            true
        }
    });
    // 落点归一化成**项目相对路径**。
    //
    // 为什么必须在离开这个函数之前做：图谱里存的是相对路径，
    // 而 Python / Node 的报错给的是**绝对**路径（`C:\…\app\repo.py`）。
    // 不归一化的话，`record_run` 找 `root.join(rel)` 会拿到一个不存在的路径，
    // 于是**认出来的落点白认了**——2026-10-16 端到端跑第一次就是这样：
    // 标题里已经写着 `app/repo.py:3`，而落点那栏还是"没有说是哪个文件"。
    // 那种"一半成功"最难看：它看起来像我们的解析器没做对，其实是差最后一步。
    for d in out.iter_mut() {
        if let Some(f) = d.file.take() {
            d.file = Some(to_project_relative(root, &f));
        }
    }
    // 一次运行最多记这些：图谱不是日志，几百条诊断挂上去只会把图冲淡
    out.truncate(50);
    out
}

/// `C:\proj\app\repo.py` + root `C:\proj` → `app/repo.py`；本来就已经是相对的，原样返回。
///
/// 按**路径组件**比较，不用字符串前缀：字符串前缀在 `C:\projX` 与 `C:\proj` 之间会认错。
/// 保留 `..` 段——那是真的往上走，不该被悄悄吃掉。
fn to_project_relative(root: &Path, path: &str) -> String {
    let p = Path::new(path);
    if !p.is_absolute() {
        return path.replace('\\', "/");
    }
    let mut a = root.components();
    let mut b = p.components();
    loop {
        match (a.clone().next(), b.clone().next()) {
            (Some(x), Some(y)) if x == y => {
                a.next();
                b.next();
            }
            _ => break,
        }
    }
    let rest: std::path::PathBuf = b.collect();
    if rest.as_os_str().is_empty() {
        // 落点就是项目根自己（少见，但别返回空串）
        return path.replace('\\', "/");
    }
    rest.to_string_lossy().replace('\\', "/")
}

/// 认 `src/main.py:12:5: error: ...` 这种形状。认不出就是 None（**不猜**）。
fn parse_plain_diag(line: &str) -> Option<RunDiagnostic> {
    let t = line.trim();
    let (head, rest) = t.split_once(": ")?;
    let level = rest.split(':').next()?.trim().to_ascii_lowercase();
    if level != "error" && level != "warning" {
        return None;
    }
    // head 必须是 `路径:行:列`：行与列都得是数字，否则可能只是"某个词: 某个词"
    let mut it = head.rsplitn(3, ':');
    let col = it.next()?.trim();
    let ln = it.next()?.trim();
    let path = it.next()?.trim();
    if col.parse::<u32>().is_err() || ln.parse::<u32>().is_err() || path.is_empty() {
        return None;
    }
    let msg = rest.split_once(": ").map(|x| x.1).unwrap_or(rest).trim();
    Some(RunDiagnostic {
        title: if msg.is_empty() {
            rest.trim().to_string()
        } else {
            msg.to_string()
        },
        file: Some(path.replace('\\', "/")),
    })
}

// ── 各语言自己的报错形状 ────────────────────────────────────────────
//
// 上面那条兜底（"引用最后一句实在话"）认得的是**内容**，不是**形状**。
// 而每种语言的报错都有自己的形状，认出来就能多给一样东西：**落点**。
// 一个诊断有没有文件位置，决定它在图谱里挂不挂得上、用户点不点得进去。
//
// 三条规矩（与上面那条一样）：
//   1. **只认能判定的形状**。像不像 Python traceback 是一回事，**它指的那个文件在不在**
//      是另一回事——后者由调用方去核（`record_run` 只在文件真的在磁盘上时才落节点）。
//   2. **宁可少认，不可认错**。认错的落点会让图谱指着错文件，那比缺一条糟得多。
//   3. **只取最后一步**。traceback 是一串调用栈，用户要的是**出错的那一行**，
//      不是"从哪进来的"——最后一步才是。

/// Python traceback：
/// ```text
/// Traceback (most recent call last):
///   File "app/service.py", line 42, in handle
///     return self.repo.get(x)
/// TypeError: 'NoneType' object is not subscriptable
/// ```
///
/// 取法：**从后往前找最后一条 `File "...", line N, in ...`**，配上它后面那句异常。
/// 前面那些帧是"从哪调进来的"——对用户没用，而最后一步才是出错的地方。
fn parse_python_traceback(tail: &[String]) -> Option<RunDiagnostic> {
    let mut last: Option<(String, usize)> = None;
    let mut exc: Option<String> = None;
    for line in tail {
        let t = line.trim();
        if let Some((path, ln)) = parse_py_frame(t) {
            last = Some((path, ln));
            // 换了一个帧，之前记的那句异常就不属于这个位置了
            exc = None;
            continue;
        }
        // 异常句长这样：`TypeError: ...` / `ModuleNotFoundError: ...`
        // 判据：`XxxError:` 或 `XxxException:` 开头。**要求冒号紧跟**，
        // 免得把正文里提到 `ValueError` 的普通句子也当成异常。
        if let Some((head, _)) = t.split_once(':') {
            let name = head.trim();
            let is_exc = (name.ends_with("Error") || name.ends_with("Exception"))
                && !name.contains(' ')
                && name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            if is_exc {
                exc = Some(t.to_string());
            }
        }
    }
    let (path, ln) = last?;
    let title = match exc {
        Some(e) => format!("{e}（{path}:{ln}）"),
        None => format!("{path}:{ln} 这里出错了"),
    };
    Some(RunDiagnostic {
        title,
        // **只放路径，不放行号。** 图谱里的节点是**文件**，行号属于诊断消息。
        // 2026-10-16 端到端跑第一次就是错的：这里放的是 `app/repo.py:3`，
        // 而 `record_run` 会拿它去 `root.join(rel)` 找文件——带行号的文件名不存在，
        // 于是"认出了落点"却一条也没挂上（界面上显示成"这条问题没有说是哪个文件"）。
        file: Some(path),
    })
}

/// 一行 `  File "app/service.py", line 42, in handle` → `("app/service.py", 42)`。
fn parse_py_frame(line: &str) -> Option<(String, usize)> {
    let rest = line.strip_prefix("File \"")?;
    let (path, rest) = rest.split_once('"')?;
    let rest = rest.trim_start_matches(',');
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("line ")?;
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() || path.is_empty() {
        return None;
    }
    Some((path.to_string(), num.parse().ok()?))
}

/// Node / JavaScript 栈：
/// ```text
/// TypeError: Cannot read properties of undefined (reading 'map')
///     at renderList (/app/src/list.js:17:23)
/// ```
///
/// 取法：找第一条 `at ... (path:line:col)`（**第一帧就是出错的地方**，
/// 与 Python 相反——JS 的栈是"从出错处往外"，Python 是"从外往出错处"）。
/// 这一条按语言分家的意义就在这儿：**同一个词（"最后一帧"）在两种语言里指不同位置。**
fn parse_node_stack(tail: &[String]) -> Option<RunDiagnostic> {
    let mut err: Option<String> = None;
    for line in tail {
        let t = line.trim();
        if let Some((path, ln)) = parse_js_frame(t) {
            let title = match &err {
                Some(e) => format!("{e}（{path}:{ln}）"),
                None => format!("{path}:{ln} 这里出错了"),
            };
            return Some(RunDiagnostic {
                title,
                // 同 `parse_python_traceback`：节点是文件，行号在标题里
                file: Some(path),
            });
        }
        if err.is_none() {
            if let Some((head, _)) = t.split_once(':') {
                let h = head.trim();
                if h.ends_with("Error") && !h.contains(' ') && h.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                {
                    err = Some(t.to_string());
                }
            }
        }
    }
    None
}

/// `    at renderList (/app/src/list.js:17:23)` → `("/app/src/list.js", 17)`
fn parse_js_frame(line: &str) -> Option<(String, usize)> {
    let rest = line.strip_prefix("at ")?.trim();
    // 两种写法：`fn (path:l:c)` 与裸 `path:l:c`
    let inside = match rest.split_once('(') {
        Some((_, after)) => after.trim_end_matches(')'),
        None => rest,
    };
    // 从右往左切冒号：Windows 路径里有 `C:`，正着切会切错
    let mut parts = inside.rsplitn(3, ':');
    let _col = parts.next()?;
    let ln: usize = parts.next()?.parse().ok()?;
    let path = parts.next()?.to_string();
    if path.is_empty() || !path.contains('.') {
        return None;
    }
    Some((path, ln))
}

/// Maven 的报错：
/// ```text
/// [ERROR] /app/src/main/java/Main.java:[12,34] ';' expected
/// ```
///
/// 取法：`[ERROR] <path>:[行,列] <消息>`。**行号在方括号里、用逗号分列**——
/// 这是 Maven 独有的形状，所以别的语言的项目不会被它误伤。
fn parse_maven_error(line: &str) -> Option<RunDiagnostic> {
    let rest = line.trim().strip_prefix("[ERROR]")?.trim();
    let (path, after) = rest.split_once(":[")?;
    let (nums, msg) = after.split_once(']')?;
    let ln = nums.split(',').next()?.trim();
    if ln.is_empty() || !ln.chars().all(|c| c.is_ascii_digit()) || path.is_empty() {
        return None;
    }
    let msg = msg.trim();
    if msg.is_empty() {
        return None;
    }
    Some(RunDiagnostic {
        title: format!("{msg}（{path}:{ln}）"),
        file: Some(path.to_string()),
    })
}

/// 这一行值不值得当"进程自己说的那句话"引用（兜底诊断要用）。///
/// 判据很土但很实用：**有足够多的字母数字、不是纯分隔线**。
/// 这样能挑掉进度点、空行、`----` 这类噪声，留下真正的报错句。
///
/// 为什么它属于这个文件：它和 `parse_plain_diag` 回答的是同一类问题——
/// "这一行里有没有值得记进图谱的东西"。
pub(crate) fn is_informative(line: &str) -> bool {
    let n = line.chars().filter(|c| c.is_alphanumeric()).count();
    n >= 8 && !line.chars().all(|c| "-=_*·.".contains(c) || c.is_whitespace())
}

/// 按字符（不是字节）截断——中文按字算，截在字中间会出乱码。
pub(crate) fn truncate_line(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tail(lines: &[&str]) -> VecDeque<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    /// **进度点、分隔线、空行都不算"实在话"**——把它们当报错引用进图谱，
    /// 用户点开看到的是一串点，那比没有还糟。
    #[test]
    fn 噪声行不算实在话() {
        for junk in ["", "   ", "...", "----------", "· · ·", "===", "ok"] {
            assert!(!is_informative(junk), "不该当成报错句：{junk:?}");
        }
    }

    /// 真正的报错行要认得出来——中英文都算。
    #[test]
    fn 报错行算实在话() {
        for good in [
            "ModuleNotFoundError: No module named 'requests'",
            "error: expected `;`, found `}`",
            "FAILED tests/test_core.py::test_greet - AssertionError",
            "ImportError: Start directory is not importable: 'tests'",
            "命令没跑成功（退出码 1）",
        ] {
            assert!(is_informative(good), "该认出来：{good:?}");
        }
    }

    /// 截断按字符算，且加省略号——不能把中文截成半个字。
    #[test]
    fn 截断按字符算() {
        assert_eq!(truncate_line("短", 10), "短");
        let long = "很".repeat(200);
        let cut = truncate_line(&long, 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.ends_with('…'));
    }

    /// 纯文本那一种：`路径:行:列: error: 消息`。
    #[test]
    fn 认得纯文本的_路径行列() {
        let got = collect_run_diagnostics(Path::new("."), &tail(&["src/main.py:3:1: error: boom"]));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].file.as_deref(), Some("src/main.py"));
        assert_eq!(got[0].title, "boom");
    }

    /// **Python traceback：取最后一步，不取"从哪进来的"。**
    ///
    /// 样本是按真实 traceback 的形状写的。注意它有**三层帧**，而只有最后一层
    /// 是出错的地方——如果取错了一头，用户点进图谱看到的是调用者的那一行，
    /// 然后去改一个没问题的文件。**认错落点比不认落点糟得多。**
    #[test]
    fn python_traceback取最后一步() {
        let got = collect_run_diagnostics(
            Path::new("."),
            &tail(&[
                "Traceback (most recent call last):",
                "  File \"main.py\", line 5, in <module>",
                "    app.run()",
                "  File \"app/core.py\", line 11, in run",
                "    self.repo.get(x)",
                "  File \"app/repo.py\", line 42, in get",
                "    return self.rows[key]",
                "KeyError: 'user_1'",
            ]),
        );
        assert_eq!(got.len(), 1, "一个 traceback 只该产出一条：{got:?}");
        assert_eq!(
            got[0].file.as_deref(),
            Some("app/repo.py"),
            "取错帧了——用户要的是出错那一行，不是从哪调进来的"
        );
        assert!(got[0].title.contains("KeyError"), "异常名要带上：{}", got[0].title);
        assert!(got[0].title.contains("repo.py:42"), "标题里要说清在哪：{}", got[0].title);
    }

    /// Windows 路径也认得（相对与绝对都见过）。
    #[test]
    fn python_traceback认windows路径() {
        let got = collect_run_diagnostics(
            Path::new("."),
            &tail(&[
                "  File \"C:\\proj\\src\\svc.py\", line 7, in handle",
                "ValueError: bad",
            ]),
        );
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].file.as_deref(), Some("C:/proj/src/svc.py"));
    }

    /// **Node 栈：取第一帧**——与 Python **正好相反**。
    /// 这条测试就是"按语言分家"这件事的意义所在：
    /// 同一个词（"出错的那一帧"）在两种语言里指的是栈的两头。
    #[test]
    fn node栈取第一帧() {
        let got = collect_run_diagnostics(
            Path::new("."),
            &tail(&[
                "TypeError: Cannot read properties of undefined (reading 'map')",
                "    at renderList (/app/src/list.js:17:23)",
                "    at render (/app/src/app.js:9:5)",
                "    at processTicksAndRejections (node:internal/process/task_queues:95:5)",
            ]),
        );
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(
            got[0].file.as_deref(),
            Some("/app/src/list.js"),
            "JS 的栈是「从出错处往外」，第一帧才是出错的地方：{got:?}"
        );
        assert!(got[0].title.contains("TypeError"));
    }

    /// Maven：`[ERROR] 路径:[行,列] 消息`——行号在方括号里、逗号分列。
    #[test]
    fn maven的行列形状() {
        let got = collect_run_diagnostics(
            Path::new("."),
            &tail(&["[ERROR] /app/src/main/java/Main.java:[12,34] ';' expected"]),
        );
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].file.as_deref(), Some("/app/src/main/java/Main.java"));
        assert!(got[0].title.contains("';' expected"), "消息要原样带上：{}", got[0].title);
    }

    /// **绝对路径要归一化成项目相对路径。**
    ///
    /// 这条是拿一次"一半成功"换的：Python 的 traceback 给的是**绝对**路径，
    /// 而图谱里存的是**相对**路径。第一版没归一化，于是标题里已经写着
    /// `app/repo.py:3`（解析器明明认对了），落点那栏却还是"没有说是哪个文件"——
    /// **认出来的落点白认了。** 差的就是这一步。
    #[test]
    fn 绝对落点归一化成项目相对路径() {
        let got = collect_run_diagnostics(
            Path::new(r"C:\proj"),
            &tail(&[
                r#"  File "C:\proj\app\repo.py", line 3, in get"#,
                "KeyError: 'x'",
            ]),
        );
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(
            got[0].file.as_deref(),
            Some("app/repo.py"),
            "必须是项目相对路径——图谱只认这个，否则挂不上"
        );
        assert!(got[0].title.contains("repo.py:3"), "行号留在标题里：{}", got[0].title);
    }

    /// 前缀相同时不能认错：`C:\projX` **不是** `C:\proj` 里面。
    #[test]
    fn 前缀相似的路径不会被误当成项目内() {
        let got = collect_run_diagnostics(
            Path::new(r"C:\proj"),
            &tail(&[
                r#"  File "C:\projX\other.py", line 1, in f"#,
                "ValueError: x",
            ]),
        );
        assert_eq!(got.len(), 1, "{got:?}");
        // 不在项目里 → 归一化后仍是一条项目外的路径。`record_run` 会因为它不是
        // 项目内文件而不落节点——那是**正确行为**：图谱只画这个项目的文件。
        let f = got[0].file.as_deref().unwrap_or("");
        assert!(f.contains("projX"), "不该把项目外的文件算成项目内的：{f}");
    }

    /// **认不出来就一条都不加**：宁可少记，不可瞎挂。
    #[test]
    fn 认不出来的一条都不加() {
        for line in [
            "Traceback (most recent call last):",
            "Error: 这东西没有行列",
            "src/main.py: abc: error: 行号不是数字",
            "build failed after 3 seconds",
            "note: 编译到一半",
        ] {
            let got = collect_run_diagnostics(Path::new("."), &tail(&[line]));
            assert!(got.is_empty(), "不该认出来：{line} → {got:?}");
        }
    }

    /// 同一条诊断重复出现时只留一条（编译器会把同一诊断派发多次）。
    #[test]
    fn 重复的只留一条() {
        let got = collect_run_diagnostics(
            Path::new("."),
            &tail(&[
                "src/a.py:1:1: error: x",
                "src/a.py:1:1: error: x",
                "src/a.py:2:1: warning: y",
            ]),
        );
        assert_eq!(got.len(), 2, "{got:?}");
    }
}
