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
//! entry       出 >= 3 且 出 >= 2×入      （入口）
//! foundation  入 >= 3 且 入 >= 2×出      （公共底层）
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
        let o = *f_out.get(f).unwrap_or(&0);
        let i = *f_in.get(f).unwrap_or(&0);
        if o >= 3 && o >= 2 * i {
            "entry"
        } else if i >= 3 && i >= 2 * o {
            "foundation"
        } else {
            "middle"
        }
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
    serde_json::json!({
        "entry": file_count.get("entry").copied().unwrap_or(0),
        "foundation": file_count.get("foundation").copied().unwrap_or(0),
        "middle": file_count.get("middle").copied().unwrap_or(0),
        "test": file_count.get("test").copied().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
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
        assert_eq!(r["middle"], 1);
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
}
