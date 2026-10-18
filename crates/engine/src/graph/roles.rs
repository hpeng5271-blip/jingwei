//! **一张图要读得出三种角色**（`框架.md` §4.8）。
//!
//! ## 为什么不是一个"重要性"数字
//!
//! 入口（`main.rs` 这类）出多入少，公共底层（`util` / `facts`）入多出少。
//! 它们的度数可能差不多——**混成一个数字就把方向抹掉了**，
//! 读图的人会以为"连得多"就是"重要"，然后去找一个什么都不产出的中间层。
//!
//! ## 只算 `calls`，不算包含关系
//!
//! 图里有三种边：`contains`（项目→文件）、`defines`（文件→符号）、
//! `calls`（符号→符号）。前两种是**包含**，不是**使用**——
//! 项目节点"出度 95"只是因为它装着 95 个文件，那不是入口。
//! **"谁在用谁"只有 `calls` 说了算。**
//!
//! ## 聚到文件层面
//!
//! 读者想知道的是**模块之间**谁在用谁，不是哪个符号。所以每条 `calls`
//! 按两端符号所属的文件归并，**跨文件才算**——同一个文件内部的调用
//! 不说明模块结构。符号继承它所在文件的角色：同一文件里的东西属于同一层。
//!
//! ## 测试文件不参与
//!
//! `*_tests.rs` / `tests/` 下的文件调用一切，出度天然最高，
//! 会给真入口戴错帽子；而且一条"只被测试调用"的边会让被调方看起来像公共底层。
//! 所以它们标 `role: "test"`，**并且连到它们的边不计入任何一方**。
//!
//! ## 判据（写死在这里，别处只问它）
//!
//! ```text
//! entry       `src/main.rs` / `src/bin/*.rs`（**事实**）或 出>=3 且 出>=2×入 的老形状
//! entry_candidate  只靠度数像入口（**猜**，真项目准确率 25%）
//! foundation  入 >= 3 且 入 >= 2×出      （公共底层）
//! surface     `src/lib.rs`（库的出口）——**不是底层，也不是入口**
//! middle      其余，含孤立点
//! test        测试文件（不参与判断）
//! container   项目节点（它装东西，不是入口）
//! ```

use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// 一个文件是不是测试。**路径分隔符要统一**——库里两种写法都有。
fn is_test_file(f: &str) -> bool {
    let l = f.replace('\\', "/");
    l.ends_with("_tests.rs") || l.ends_with("_test.rs") || l.contains("/tests/")
}

/// **角色判据只写这一处**（出度与入度的对比）。
///
/// 文件和目录两级都问它——不然"什么算入口"会出现第二份定义，
/// 而这个仓库为"同一个事实在第二个地方判断"付过太多次账。
/// **`src/lib.rs` 是"公共面"，不是"公共底层"**（2026-10-18，3 个真应用换来的）。
///
/// `embed-resource` 的 `src/lib.rs` 被 3 个文件调用、自己不调用谁，按度数就成了"公共底层"。
/// 而它是**库的出口**——被别人 `use` 的那个面，不是被人当工具用的零件。
/// 两个名字说的是两件事，压成一个就是错判（这个仓库最熟的形状）。
///
/// 只认 **crate 根**（`src/lib.rs`）：`src/sub/lib.rs` 不是公共面，按度数走——
/// 不然"有 lib.rs 就叫公共面"会把普通目录也标错。
/// **这是不是一个"能跑的东西"**（Cargo 的 bin 目标）。
///
/// - `src/main.rs`：默认 bin
/// - `src/bin/<名字>.rs`：显式 bin
/// - 文件名叫 `main.rs`（任何深度）：单文件示例也算能跑的东西
///
/// **这是事实，不是猜**——所以由它判出来的角色叫「入口」，
/// 而按度数判出来的叫「入口候选」。真项目实测：度数那一路准确率只有 25%。
fn is_bin_target(f: &str) -> bool {
    let l = f.replace('\\', "/");
    if l == "main.rs" || l.ends_with("/main.rs") {
        return true;
    }
    // `src/bin/foo.rs`（再看一层就不算了：`src/bin/sub/mod.rs` 不是 bin）
    l.contains("/src/bin/") || l.starts_with("src/bin/")
}

fn is_library_surface(f: &str) -> bool {
    let l = f.replace('\\', "/");
    l == "src/lib.rs" || l.ends_with("/src/lib.rs")
}

fn classify(out: i64, inn: i64) -> &'static str {
    if out >= 3 && out >= 2 * inn {
        "entry"
    } else if inn >= 3 && inn >= 2 * out {
        "foundation"
    } else {
        "middle"
    }
}

/// 文件级角色 = **三态**，不是两个互相排斥的帽子。
///
/// 关键在第一条：**两头都是 0 的文件不是"中间层"**，是"还没连上"。
/// 实测本仓库 53 个"中间层"里有 17 个是这样——一条跨文件调用都没有。
/// 叫它中间层，等于**把"我们读不出关系"说成"它确实在中间"**（三态压扁）。
fn role_of(out: i64, inn: i64) -> &'static str {
    if out == 0 && inn == 0 {
        "isolated"
    } else {
        classify(out, inn)
    }
}

/// 节点 → 路径、节点 → 种类。
///
/// **文件节点的路径在 `id` 里**（`file:<路径>`），它的 `file` 字段是 null——
/// 这是库里两种节点存法不同的地方，按文件/目录聚就得把这条路走通。
fn node_paths(nodes: &[Value]) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut node_file: HashMap<String, String> = HashMap::new();
    let mut node_kind: HashMap<String, String> = HashMap::new();
    for n in nodes {
        let id = n.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let kind = n.get("kind").and_then(|x| x.as_str()).unwrap_or("");
        node_kind.insert(id.to_string(), kind.to_string());
        let path = if kind == "file" {
            id.strip_prefix("file:").map(|s| s.to_string())
        } else {
            n.get("file").and_then(|x| x.as_str()).map(|s| s.to_string())
        };
        if let Some(p) = path {
            node_file.insert(id.to_string(), p);
        }
    }
    (node_file, node_kind)
}

/// 一个文件属于哪个目录（取父目录）。**这是"模块"的自然粒度**：
/// `crates/cli/src/cmd_graph.rs` → `crates/cli/src`。
fn parent_dir(path: &str) -> &str {
    let p = path.replace('\\', "/");
    match p.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// 给 `nodes` 就地补上 `role` / `out` / `in`，返回**按文件计**的角色汇总。
///
/// 参数就是 `store::dump` 导出来的那两个数组——**不重新查库**：
/// 这个函数只做归并，数据从哪儿来是调用方的事。
pub fn annotate(nodes: &mut [Value], edges: &[Value]) -> Value {
    // 节点 → 路径；节点 → 种类。
    //
    // **文件节点的路径在 `id` 里**（`file:<路径>`），它的 `file` 字段是 null——
    // 这是库里两种节点存法不同的地方，角色按文件聚就得把这条路走通。
    let mut node_file: HashMap<String, String> = HashMap::new();
    let mut node_kind: HashMap<String, String> = HashMap::new();
    for n in nodes.iter() {
        let id = n.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let kind = n.get("kind").and_then(|x| x.as_str()).unwrap_or("");
        node_kind.insert(id.to_string(), kind.to_string());
        let path = if kind == "file" {
            id.strip_prefix("file:").map(|s| s.to_string())
        } else {
            n.get("file").and_then(|x| x.as_str()).map(|s| s.to_string())
        };
        if let Some(p) = path {
            node_file.insert(id.to_string(), p);
        }
    }

    // 文件 → (出, 入)：只由跨文件的 `calls` 累加。
    let mut f_out: HashMap<&str, i64> = HashMap::new();
    let mut f_in: HashMap<&str, i64> = HashMap::new();
    for e in edges {
        if e.get("kind").and_then(|x| x.as_str()) != Some("calls") {
            continue;
        }
        let (s, d) = match (
            e.get("src").and_then(|x| x.as_str()),
            e.get("dst").and_then(|x| x.as_str()),
        ) {
            (Some(s), Some(d)) => (s, d),
            _ => continue,
        };
        let (sf, df) = match (node_file.get(s), node_file.get(d)) {
            (Some(a), Some(b)) => (a.as_str(), b.as_str()),
            _ => continue,
        };
        if sf == df || is_test_file(sf) || is_test_file(df) {
            continue;
        }
        *f_out.entry(sf).or_insert(0) += 1;
        *f_in.entry(df).or_insert(0) += 1;
    }

    let file_role = |f: &str| -> &'static str {
        if is_test_file(f) {
            return "test";
        }
        // **crate 根的 lib.rs 先于度数**：它是公共面，不是工具也不是入口。
        if is_library_surface(f) {
            return "surface";
        }
        // **bin 目标就是入口**（事实，不看度数）：它没有跨文件调用也还是入口。
        if is_bin_target(f) {
            return "entry";
        }
        // 剩下的按度数：只有"像入口"的把握 → **候选**，不是入口。
        let r = role_of(*f_out.get(f).unwrap_or(&0), *f_in.get(f).unwrap_or(&0));
        if r == "entry" {
            return "entry_candidate";
        }
        r
    };

    let mut file_count: HashMap<&str, i64> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    for n in nodes.iter_mut() {
        let id = n.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let kind = node_kind.get(&id).map(|x| x.as_str()).unwrap_or("");
        let f = node_file.get(&id).map(|x| x.as_str()).unwrap_or("");
        let role: &'static str = if kind == "project" {
            // 项目节点是容器，不是入口——给它一个诚实的名字。
            "container"
        } else if f.is_empty() {
            "middle"
        } else {
            file_role(f)
        };
        if kind == "file" && !f.is_empty() && seen.insert(f.to_string()) {
            *file_count.entry(file_role(f)).or_insert(0) += 1;
        }
        let o = *f_out.get(f).unwrap_or(&0);
        let i = *f_in.get(f).unwrap_or(&0);
        if let Some(obj) = n.as_object_mut() {
            obj.insert("role".to_string(), serde_json::json!(role));
            obj.insert("out".to_string(), serde_json::json!(o));
            obj.insert("in".to_string(), serde_json::json!(i));
        }
    }

    // **计数按文件给**：读者关心的是"几个模块是入口"，不是几个符号。
    //
    // `isolated` **不是一种角色**，是"我们读不出它跟谁有关系"：
    // 本仓库实测 53 个"中间层"里有 **17 个出度 0、入度 0**——一条跨文件调用都没有。
    // 把它们叫"中间层"就是**把"没原料"说成"确实是中间"**，
    // 而这个仓库为这一类错误付过太多次账（三态压扁）。
    serde_json::json!({
        "entry": file_count.get("entry").copied().unwrap_or(0),
        // **候选是另一格**：度数很像入口、但没有结构证据。压进入口就是拿猜冒充事实。
        "entry_candidate": file_count.get("entry_candidate").copied().unwrap_or(0),
        "foundation": file_count.get("foundation").copied().unwrap_or(0),
        // **公共面**：库的出口（`src/lib.rs`）。它不是底层，也不是入口，
        // 所以必须单独一个数——三个格子压成一个就是今天修的那个错判。
        "surface": file_count.get("surface").copied().unwrap_or(0),
        "middle": file_count.get("middle").copied().unwrap_or(0),
        "test": file_count.get("test").copied().unwrap_or(0),
        "isolated": file_count.get("isolated").copied().unwrap_or(0),
    })
}

/// **目录级**的角色：文件级之后再看一层，才叫"模块结构"。
///
/// 判据同 `classify`（**只写一处**），但度数按目录重新算：
/// 文件之间互相调用的量太多了，聚到目录才看得清 `cli` 与 `util` 谁在用什么。
///
/// 返回**每一层前三名**，不是全部——界面上那一行只够说这么多，
/// 而"堆满所有目录"正是 §4.8 说的那种错误。
pub fn dir_roles(nodes: &[Value], edges: &[Value]) -> Value {
    let (node_file, _) = node_paths(nodes);
    let mut d_out: HashMap<&str, i64> = HashMap::new();
    let mut d_in: HashMap<&str, i64> = HashMap::new();
    for e in edges {
        if e.get("kind").and_then(|x| x.as_str()) != Some("calls") {
            continue;
        }
        let (s, d) = match (
            e.get("src").and_then(|x| x.as_str()),
            e.get("dst").and_then(|x| x.as_str()),
        ) {
            (Some(s), Some(d)) => (s, d),
            _ => continue,
        };
        let (sf, df) = match (node_file.get(s), node_file.get(d)) {
            (Some(a), Some(b)) => (a.as_str(), b.as_str()),
            _ => continue,
        };
        if is_test_file(sf) || is_test_file(df) {
            continue;
        }
        let (sd, dd) = (parent_dir(sf), parent_dir(df));
        if sd == dd || sd.is_empty() || dd.is_empty() {
            continue; // 同一个目录内部互调，不说明模块之间谁在用谁
        }
        *d_out.entry(sd).or_insert(0) += 1;
        *d_in.entry(dd).or_insert(0) += 1;
    }

    let mut all: Vec<(&str, i64, i64)> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for dir in d_out.keys().chain(d_in.keys()) {
        if !seen.insert(dir) {
            continue;
        }
        all.push((dir, *d_out.get(dir).unwrap_or(&0), *d_in.get(dir).unwrap_or(&0)));
    }
    let mut entry: Vec<&(&str, i64, i64)> = all.iter().filter(|(_, o, i)| classify(*o, *i) == "entry").collect();
    let mut found: Vec<&(&str, i64, i64)> = all.iter().filter(|(_, o, i)| classify(*o, *i) == "foundation").collect();
    let middle: Vec<&(&str, i64, i64)> = all.iter().filter(|(_, o, i)| classify(*o, *i) == "middle").collect();
    // 排序：出度大的入口在前；入度大的底层在前。**同值时按名字**，结果才稳定可测。
    entry.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    found.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)));
    let pick = |v: &[&(&str, i64, i64)]| -> Value {
        serde_json::json!(v
            .iter()
            .take(3)
            .map(|(d, o, i)| serde_json::json!({ "dir": d, "out": o, "in": i }))
            .collect::<Vec<_>>())
    };
    // **截断必须说出来**（2026-10-17 修的）：本仓库实测入口目录有 4 个，
    // 而这里只放前三名——`total` 是"全部目录"的个数，
    // 读的人**推不出"入口还有第 4 个没显示"**。那又是把"没显示"当成"没有"。
    // 所以每一层各给一个"一共几个"，让界面能说"共 4 个，显示前 3"。
    // **中间层也要有个数**（2026-10-17 补）：界面原来只列入口与公共底层，
    // 13 个目录里只提了 5 个——**剩下 7 个中间层目录一个字都没说**，
    // 读的人会以为"目录只有两类"。这里**只给个数，不给名字**：
    // 名字那一半会撑爆那一行，而"还有 7 个"这件事必须交代。
    serde_json::json!({
        "entry": pick(&entry),
        "foundation": pick(&found),
        "entryTotal": entry.len(),
        "foundationTotal": found.len(),
        "middleTotal": middle.len(),
        "total": all.len(),
    })
}

#[cfg(test)]
mod tests {

    /// **`src/lib.rs` 是"公共面"，不是"公共底层"**（2026-10-18，真项目打出来的）。
    ///
    /// 判据从 3 个真应用换来：`embed-resource` 的 `lib.rs` 被 3 个文件调用、自己不调用谁，
    /// 按度数就被判成"公共底层"。而它是**库的出口**（被别人 `use` 的那个面），
    /// 不是"被调用的工具"。两个名字说的是两件事，压成一个就是错判。
    ///
    /// 这条测试同时钉住：**`src/main.rs` 仍然是入口**（别为了修 lib 把 main 也挪走）。
    #[test]
    fn 库的根文件是公共面不是公共底层() {
        let mut nodes = vec![
            node("file:src/lib.rs", "file", None),
            node("file:src/main.rs", "file", None),
        ];
        for i in 0..4 {
            nodes.push(node(&format!("file:src/m{i}.rs"), "file", None));
            nodes.push(node(&format!("symbol:src/m{i}.rs#f"), "symbol", Some(&format!("src/m{i}.rs"))));
        }
        nodes.push(node("symbol:src/lib.rs#go", "symbol", Some("src/lib.rs")));
        nodes.push(node("symbol:src/main.rs#run", "symbol", Some("src/main.rs")));

        let mut edges = Vec::new();
        // 4 个模块文件都调用 lib.rs 里的 go → lib.rs 入度 4、出度 0
        for i in 0..4 {
            edges.push(json!({
                "src": format!("symbol:src/m{i}.rs#f"),
                "dst": "symbol:src/lib.rs#go",
                "kind": "calls"
            }));
        }
        // main.rs 调用 3 个模块（跨文件）→ 出度 3、入度 0 → 仍是入口
        for i in 0..3 {
            edges.push(json!({
                "src": "symbol:src/main.rs#run",
                "dst": format!("symbol:src/m{i}.rs#f"),
                "kind": "calls"
            }));
        }
        let r = annotate(&mut nodes, &edges);
        assert_eq!(r["surface"], 1, "`src/lib.rs` 该是公共面：{r}");
        assert_eq!(r["foundation"], 0, "`src/lib.rs` 不许再算公共底层：{r}");
        assert_eq!(r["entry"], 1, "`src/main.rs` 仍是入口：{r}");
    }

    /// 非 crate 根的 `lib.rs`（例如 `src/sub/lib.rs`）**不许被特殊对待**——
    /// 它不是公共面，按度数走。（不然"有 lib.rs 就叫公共面"会把普通目录也标错。）
    #[test]
    fn 非根目录里的lib不算公共面() {
        let mut nodes = vec![node("file:src/sub/lib.rs", "file", None)];
        for i in 0..3 {
            nodes.push(node(&format!("file:src/m{i}.rs"), "file", None));
            nodes.push(node(&format!("symbol:src/m{i}.rs#f"), "symbol", Some(&format!("src/m{i}.rs"))));
        }
        nodes.push(node("symbol:src/sub/lib.rs#go", "symbol", Some("src/sub/lib.rs")));
        let mut edges = Vec::new();
        for i in 0..3 {
            edges.push(json!({
                "src": format!("symbol:src/m{i}.rs#f"),
                "dst": "symbol:src/sub/lib.rs#go",
                "kind": "calls"
            }));
        }
        let r = annotate(&mut nodes, &edges);
        assert_eq!(r["surface"], 0, "子目录里的 lib.rs 不是公共面：{r}");
        assert_eq!(r["foundation"], 1, "它按度数就是公共底层：{r}");
    }
    use super::*;
    use serde_json::json;

    fn node(id: &str, kind: &str, file: Option<&str>) -> Value {
        json!({ "id": id, "kind": kind, "name": id, "file": file })
    }

    /// **跨文件的调用才说明模块结构**：同一文件内部的调用不算度数。
    #[test]
    fn 同文件内部的调用不算谁在用谁() {
        let mut nodes = vec![
            node("file:a.rs", "file", None),
            node("symbol:a.rs#f", "symbol", Some("a.rs")),
            node("symbol:a.rs#g", "symbol", Some("a.rs")),
        ];
        let edges = vec![
            json!({ "src": "symbol:a.rs#f", "dst": "symbol:a.rs#g", "kind": "calls" }),
        ];
        let r = annotate(&mut nodes, &edges);
        assert_eq!(r["entry"], 0, "只有一个文件，没有跨文件调用：{r}");
        // **不是 middle，是 isolated**（2026-10-17 改的语义）：
        // 同文件互调之后这个文件的出/入都是 0 —— 咱们**读不出**它跟谁有关系，
        // 那不是"它在中间"，是"还没连上"。原来这条断言写的是 middle，
        // 等于把"没有原料"当成一种角色。
        assert_eq!(r["isolated"], 1, "没有跨文件调用就是「还没连上」：{r}");
        assert_eq!(r["middle"], 0);
    }

    /// **包含边不算**：项目节点装着文件，那不是"入口"。
    #[test]
    fn 包含边不给项目节点戴入口的帽子() {
        let mut nodes = vec![
            node("project:p", "project", None),
            node("file:a.rs", "file", None),
            node("file:b.rs", "file", None),
        ];
        let mut edges = Vec::new();
        for i in 0..20 {
            edges.push(json!({ "src": "project:p", "dst": format!("file:a{i}.rs"), "kind": "contains" }));
        }
        let r = annotate(&mut nodes, &edges);
        assert_eq!(nodes[0]["role"], "container", "项目节点是容器");
        assert_eq!(r["entry"], 0, "contains 不该让谁变成入口：{r}");
    }

    /// **测试文件不参与**：它们调用一切，出度天然最高。
    #[test]
    fn 测试文件不参与角色判断() {
        let mut nodes = vec![
            node("file:project_tests.rs", "file", None),
            node("file:real.rs", "file", None),
            node("symbol:project_tests.rs#t", "symbol", Some("project_tests.rs")),
            node("symbol:real.rs#f", "symbol", Some("real.rs")),
        ];
        let mut edges = Vec::new();
        for i in 0..10 {
            edges.push(json!({
                "src": "symbol:project_tests.rs#t",
                "dst": format!("symbol:other{i}.rs#x"),
                "kind": "calls"
            }));
        }
        edges.push(json!({ "src": "symbol:project_tests.rs#t", "dst": "symbol:real.rs#f", "kind": "calls" }));
        let r = annotate(&mut nodes, &edges);
        assert_eq!(nodes[0]["role"], "test", "测试文件要单独标");
        assert_eq!(r["entry"], 0, "测试的出度不该给任何文件戴入口帽子：{r}");
        assert_eq!(r["foundation"], 0, "只被测试调用也不算公共底层：{r}");
    }

    /// **目录级**：聚到目录才叫"模块结构"。
    /// 文件之间互调的量太多，看单文件只看到一堆 middle。
    #[test]
    fn 目录级也分得出入口与公共底层() {
        let mut nodes = vec![
            node("file:cli/main.rs", "file", None),
            node("file:util/a.rs", "file", None),
            node("symbol:cli/main.rs#m", "symbol", Some("cli/main.rs")),
            node("symbol:util/a.rs#f", "symbol", Some("util/a.rs")),
        ];
        let mut edges = Vec::new();
        for _ in 0..4 {
            edges.push(json!({
                "src": "symbol:cli/main.rs#m",
                "dst": "symbol:util/a.rs#f",
                "kind": "calls"
            }));
        }
        // 不动节点（这里只验目录聚合）
        let r = dir_roles(&nodes, &edges);
        assert_eq!(r["total"], 2, "两个目录：{r}");
        assert_eq!(r["entry"][0]["dir"], "cli", "cli 是入口：{r}");
        assert_eq!(r["entry"][0]["out"], 4);
        assert_eq!(r["foundation"][0]["dir"], "util", "util 是公共底层：{r}");
        assert_eq!(r["foundation"][0]["in"], 4);
        let _ = &mut nodes;
    }

    /// **目录级只显示前三名，但必须说出"一共几个"**（2026-10-17 加）。
    ///
    /// 本仓库实测：入口目录有 4 个，界面上只画了 3 个——
    /// 而 `total` 是"所有目录"的个数，读的人推不出"入口还剩一个没显示"。
    /// 那又是把"没显示"当成"没有"。
    #[test]
    fn 目录级截断要说出来() {
        // 四个"只出不入"的目录 + 一个共同汇聚点，这样前四个都算入口。
        //
        // **第一版写错了**：让 4 个目录互相调用，于是每个是 出3/入3 —— 都是中间层，
        // `entryTotal` 是 0，断言当场红。夹具的形状要**照判据设计**，不能随手造。
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        nodes.push(node("file:sink/x.rs", "file", None));
        nodes.push(node("symbol:sink/x.rs#g", "symbol", Some("sink/x.rs")));
        for k in 0..4 {
            let d = format!("d{k}");
            nodes.push(node(&format!("file:{d}/a.rs"), "file", None));
            nodes.push(node(&format!("symbol:{d}/a.rs#f"), "symbol", Some(&format!("{d}/a.rs"))));
            // 每个 d{k}/a.rs 往 sink 调 3 次：自己出 3、入 0 → 入口
            for _ in 0..3 {
                edges.push(json!({
                    "src": format!("symbol:{d}/a.rs#f"),
                    "dst": "symbol:sink/x.rs#g",
                    "kind": "calls"
                }));
            }
        }
        let r = dir_roles(&nodes, &edges);
        let shown = r["entry"].as_array().map(|a| a.len()).unwrap_or(0);
        assert_eq!(shown, 3, "只显示前三名：{r}");
        assert_eq!(r["entryTotal"], 4, "但要说出「一共 4 个」：{r}");
    }

    /// **中间层目录也要有个数**（2026-10-17 加）：界面只列入口与底层，
    /// 那 13 个目录里有 7 个中间层——**一个字都不说，读的人会以为目录只有两类**。
    #[test]
    fn 中间层目录也要有个数() {
        let nodes = vec![
            node("file:a/x.rs", "file", None),
            node("file:b/y.rs", "file", None),
            node("symbol:a/x.rs#f", "symbol", Some("a/x.rs")),
            node("symbol:b/y.rs#g", "symbol", Some("b/y.rs")),
        ];
        // 各 1 条，两个方向都不极端 → 两个都是中间层
        let edges = vec![json!({
            "src": "symbol:a/x.rs#f", "dst": "symbol:b/y.rs#g", "kind": "calls"
        })];
        let j = dir_roles(&nodes, &edges);
        assert_eq!(j["middleTotal"], 2, "两个目录都是中间层：{j}");
        assert_eq!(j["entryTotal"], 0);
        assert_eq!(j["foundationTotal"], 0);
    }

    /// **五个格子的和必须等于文件总数**（2026-10-17 加）。
    ///
    /// 今天同一个形状的错误出现了 **5 次**：一个格子装着两种东西——
    /// 「中间层」里混着孤立、「还没连上」里混着我们没解析、目录级截断不说、
    /// 中间层目录不说……每次都是"数一遍"才发现的。
    ///
    /// 这条守恒判据能挡住其中最要命的一种：**某个文件既不在入口、也不在底层、
    /// 也不在中间层、也不在测试、也不在孤立**——**它掉出了所有格子，而且没人会红**。
    #[test]
    fn 五个格子的和必须等于文件总数() {
        let mut nodes = vec![
            // 入口：只出不入
            node("file:cli/main.rs", "file", None),
            node("symbol:cli/main.rs#f", "symbol", Some("cli/main.rs")),
            // 底层：只入不出
            node("file:sys/mem.rs", "file", None),
            node("symbol:sys/mem.rs#g", "symbol", Some("sys/mem.rs")),
            // 中间层：一来一往
            node("file:mid/a.rs", "file", None),
            node("symbol:mid/a.rs#h", "symbol", Some("mid/a.rs")),
            node("file:mid/b.rs", "file", None),
            node("symbol:mid/b.rs#i", "symbol", Some("mid/b.rs")),
            // 测试：有边也不参与判断
            node("file:mid/a_tests.rs", "file", None),
            node("symbol:mid/a_tests.rs#j", "symbol", Some("mid/a_tests.rs")),
            // 孤立：一条边都没有
            node("file:lonely/x.rs", "file", None),
            node("symbol:lonely/x.rs#k", "symbol", Some("lonely/x.rs")),
        ];
        let mut edges = Vec::new();
        for _ in 0..3 {
            // 入口 → 底层：让 cli 出 3、入 0；mem 入 3、出 0
            edges.push(json!({
                "src": "symbol:cli/main.rs#f", "dst": "symbol:sys/mem.rs#g", "kind": "calls"
            }));
        }
        edges.push(json!({
            "src": "symbol:mid/a.rs#h", "dst": "symbol:mid/b.rs#i", "kind": "calls"
        }));
        edges.push(json!({
            "src": "symbol:mid/b.rs#i", "dst": "symbol:mid/a.rs#h", "kind": "calls"
        }));
        edges.push(json!({
            "src": "symbol:mid/a_tests.rs#j", "dst": "symbol:cli/main.rs#f", "kind": "calls"
        }));

        // 先数一遍"到底有几个文件"——**这是唯一真源**
        let files = nodes
            .iter()
            .filter(|n| n.get("kind").and_then(|k| k.as_str()) == Some("file"))
            .count() as i64;
        let r = annotate(&mut nodes, &edges);
        let sum = ["entry", "foundation", "middle", "test", "isolated"]
            .iter()
            .map(|k| r[*k].as_i64().unwrap_or(0))
            .sum::<i64>();
        assert_eq!(sum, files, "五个格子必须正好盖住所有文件，一个不漏：{r}");
    }

    /// **同一个目录内部互调不算**：它不说明模块之间谁在用谁。
    #[test]
    fn 同一目录内部互调不进目录级() {
        let nodes = vec![
            node("file:util/a.rs", "file", None),
            node("file:util/b.rs", "file", None),
            node("symbol:util/a.rs#x", "symbol", Some("util/a.rs")),
            node("symbol:util/b.rs#y", "symbol", Some("util/b.rs")),
        ];
        let edges = vec![json!({
            "src": "symbol:util/a.rs#x",
            "dst": "symbol:util/b.rs#y",
            "kind": "calls"
        })];
        let r = dir_roles(&nodes, &edges);
        assert_eq!(r["total"], 0, "同一目录内部互调不该出现任何目录：{r}");
    }

    /// **一条边都没有的文件，不是"中间层"**（2026-10-17 加）。
    ///
    /// 本仓库实测：53 个"中间层"里有 17 个出 0 / 入 0。
    /// 叫它中间层，等于把"我们读不出关系"说成"它确实在中间"。
    #[test]
    fn 孤单的文件不是中间层() {
        let mut nodes = vec![
            node("file:a.rs", "file", None),
            node("file:b.rs", "file", None),
            node("symbol:a.rs#f", "symbol", Some("a.rs")),
            node("symbol:b.rs#g", "symbol", Some("b.rs")),
        ];
        // 一条 calls 都没有：两个文件都读不出关系
        let r = annotate(&mut nodes, &[]);
        assert_eq!(r["isolated"], 2, "两个文件都没连上：{r}");
        assert_eq!(r["middle"], 0, "不许把它算成中间层：{r}");

        // 连上一条之后，两个文件各有方向 —— 才轮到 classify 说话
        let edges = vec![json!({
            "src": "symbol:a.rs#f", "dst": "symbol:b.rs#g", "kind": "calls"
        })];
        let r2 = annotate(&mut nodes, &edges);
        assert_eq!(r2["isolated"], 0, "连上之后就不该再有孤立：{r2}");
        assert_eq!(r2["middle"], 2, "出入各 1，是中间层：{r2}");
    }

    /// 三种角色真的分得开：出多入少 = 入口，入多出少 = 公共底层。
    #[test]
    fn 入口与公共底层按方向分开() {
        let mut nodes = vec![
            node("file:main.rs", "file", None),
            node("file:util.rs", "file", None),
            node("symbol:main.rs#main", "symbol", Some("main.rs")),
            node("symbol:util.rs#a", "symbol", Some("util.rs")),
            node("symbol:util.rs#b", "symbol", Some("util.rs")),
            node("symbol:util.rs#c", "symbol", Some("util.rs")),
        ];
        let mut edges = Vec::new();
        // main 调用 util 三次 → main 出3/入0，util 入3/出0
        for f in ["a", "b", "c"] {
            edges.push(json!({
                "src": "symbol:main.rs#main",
                "dst": format!("symbol:util.rs#{f}"),
                "kind": "calls"
            }));
        }
        let r = annotate(&mut nodes, &edges);
        assert_eq!(r["entry"], 1, "main.rs 该是入口：{r}");
        assert_eq!(r["foundation"], 1, "util.rs 该是公共底层：{r}");
        assert_eq!(nodes[0]["role"], "entry");
        assert_eq!(nodes[1]["role"], "foundation");
    }

    /// **入口有两类，不许混**（2026-10-18，3 个真应用量出 25% 之后改的）。
    ///
    /// - **结构入口**（`src/main.rs` / `src/bin/*.rs`）：这是**事实**，不看度数。
    /// - **入口候选**（按度数出≥3 且 出≥2×入）：这是**猜**，真项目上准确率 25%。
    ///
    /// 压成同一个角色，就是拿"猜"冒充"事实"——这个仓库最熟的那个形状。
    #[test]
    fn 结构入口与度数候选分开算() {
        let mut nodes = vec![
            node("file:src/main.rs", "file", None),
            node("file:src/walk.rs", "file", None),
            node("file:src/types.rs", "file", None),
            node("symbol:src/main.rs#main", "symbol", Some("src/main.rs")),
        ];
        for i in 0..4 {
            nodes.push(node(&format!("file:src/m{i}.rs"), "file", None));
            nodes.push(node(&format!("symbol:src/m{i}.rs#f"), "symbol", Some(&format!("src/m{i}.rs"))));
        }
        let mut edges = Vec::new();
        // m0.rs 调用 m1/m2/m3 三个模块（出3/入0）→ 按度数是"入口"形状，
        // 但它不是 bin 目标——所以只能是候选。（i 从 1 开始：调自己不算跨文件）
        for i in 1..4 {
            edges.push(json!({
                "src": "symbol:src/m0.rs#f",
                "dst": format!("symbol:src/m{i}.rs#f"),
                "kind": "calls"
            }));
        }
        let r = annotate(&mut nodes, &edges);
        // main.rs 一条跨文件调用都没有，但它是 `src/main.rs` → **结构入口**
        assert_eq!(r["entry"], 1, "`src/main.rs` 该算结构入口（不看度数）：{r}");
        // m0.rs 出 3 入 0 → 但它不是 bin 目标 → **候选**
        assert!(r["entry_candidate"].as_u64().unwrap() >= 1, "度数出来的只能是候选：{r}");
    }

    /// 没有 `main.rs` 的项目（库 / 脚本）**一个结构入口都没有**，
    /// 度数出来的全进「候选」——**不许冒充入口**。
    #[test]
    fn 没有main时只有候选() {
        let mut nodes = vec![];
        for i in 0..4 {
            nodes.push(node(&format!("file:a{i}.rs"), "file", None));
            nodes.push(node(&format!("symbol:a{i}.rs#f"), "symbol", Some(&format!("a{i}.rs"))));
        }
        let mut edges = Vec::new();
        for i in 1..4 {
            edges.push(json!({
                "src": "symbol:a0.rs#f",
                "dst": format!("symbol:a{i}.rs#f"),
                "kind": "calls"
            }));
        }
        let r = annotate(&mut nodes, &edges);
        assert_eq!(r["entry"], 0, "没有 main.rs 就没有结构入口：{r}");
        assert_eq!(r["entry_candidate"], 1, "a0 该是候选（出3入0）：{r}");
    }
}
