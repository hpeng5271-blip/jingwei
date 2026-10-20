//! 重命名的 **import 回归钉子**（2026-10-20）。
//!
//! 起因是一次实测：`lib.py` 定义 `helper`，`app.py` 写 `from lib import helper` 再调用它，
//! `rename helper --to util --write` 只改了定义行与调用行，**import 行留在旧名**，
//! 而工具还打印"回读通过"——代码当场坏掉。这里钉住两头：
//! 同一个模块的 import 必须一起改；证明不了是同一个模块的 import 必须**整次拒绝**。

use jingwei_engine::graph::{index, store::Store};
use std::path::PathBuf;

fn project(tag: &str, import_line: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jw-renimp-{tag}-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("lib.py"), "def helper(x):\n    return x + 1\n").unwrap();
    std::fs::write(
        d.join("app.py"),
        format!("{import_line}\n\ndef main():\n    return helper(1)\n"),
    )
    .unwrap();
    d
}

#[test]
fn 同一个模块的_import_跟着一起改() {
    let root = project("ok", "from lib import helper");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let plan = s
        .plan_rename(&root, "symbol:lib.py#helper", "util")
        .expect("规划失败");

    assert!(plan.ambiguous.is_empty(), "不该有判不准：{plan:?}");
    let seen: Vec<(String, u32)> = plan.edits.iter().map(|e| (e.file.clone(), e.line)).collect();
    assert!(
        seen.contains(&("lib.py".to_string(), 1)),
        "定义行没算上：{plan:?}"
    );
    assert!(
        seen.contains(&("app.py".to_string(), 1)),
        "import 行没算上——这正是那次事故：{plan:?}"
    );

    _ = std::fs::remove_dir_all(&root);
}

#[test]
fn 带_bom_的_import_行也要一起改() {
    let root = project("bom", "from lib import helper");
    // PowerShell 的 `Set-Content -Encoding UTF8` 会写 BOM；BOM 不是空白，别让整行不像 import
    let app = root.join("app.py");
    let text = std::fs::read_to_string(&app).unwrap();
    std::fs::write(&app, format!("\u{feff}{text}")).unwrap();

    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");
    let plan = s
        .plan_rename(&root, "symbol:lib.py#helper", "util")
        .expect("规划失败");
    let seen: Vec<(String, u32)> = plan.edits.iter().map(|e| (e.file.clone(), e.line)).collect();
    assert!(
        seen.contains(&("app.py".to_string(), 1)),
        "带 BOM 的文件里 import 行没算上：{plan:?}"
    );

    _ = std::fs::remove_dir_all(&root);
}

#[test]
fn 证明不了是同一模块的_import_整次拒绝() {
    let root = project("bad", "from other import helper");
    let mut s = Store::open(&root.join("g.db")).expect("开库失败");
    index::index_source(&root, &mut s).expect("索引失败");

    let plan = s
        .plan_rename(&root, "symbol:lib.py#helper", "util")
        .expect("规划失败");

    assert!(
        plan.ambiguous
            .iter()
            .any(|a| a.file == "app.py" && a.line == 1),
        "证明不了的 import 必须进「不敢改」：{plan:?}"
    );

    _ = std::fs::remove_dir_all(&root);
}
