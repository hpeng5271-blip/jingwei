//! record.rs 的解析单测（**单元**，不是统计：只钉住三种写法别漏）。

use collect::record::parse_markdown;

#[test]
fn status_line_has_three_shapes() {
    // 1) 列表项写法
    let a = parse_markdown("# D-20260101-01 · 标题\n\n- 状态：**已落地**（`crates/x.rs`）\n", "D-20260101-01");
    assert_eq!(a.status_raw, "已落地（crates/x.rs）");

    // 2) blockquote 写法（D-20261022-03/04 就是这种，漏了会误判 uncertain）
    let b = parse_markdown(
        "# D-20260101-02 · 标题\n\n> **状态**：已裁定（pro，2026-10-22）\n",
        "D-20260101-02",
    );
    assert_eq!(b.status_raw, "已裁定（pro，2026-10-22）");

    // 3) 没有状态行 → 空（上层按 uncertain 处理）
    let c = parse_markdown("# D-20260101-03 · 标题\n\n正文。\n", "D-20260101-03");
    assert_eq!(c.status_raw, "");
}

#[test]
fn table_header_is_not_an_alternative() {
    // v3 修的就是这个：表头「选项|做法」曾被当成一条替代项
    let text = "# D-20260101-04 · 标题\n\n## 三个选项与取舍\n\n| 选项 | 做法 |\n|---|---|\n| 1 甲 | 做甲 |\n| 2 乙 | 做乙 |\n";
    let r = parse_markdown(text, "D-20260101-04");
    assert_eq!(r.alts.len(), 2, "alts={:?}", r.alts);
    assert_eq!(r.alts[0].index, 1);
}

#[test]
fn second_table_header_in_same_section_is_skipped() {
    // 同一章里两张表：第二张的表头也不能算替代项（标志位要重置）
    let text = "# D-20260101-06 · 标题\n\n## 三个选项与取舍\n\n| 选项 | 做法 |\n|---|---|\n| 1 甲 | 做甲 |\n\n补充一句。\n\n| 选项 | 做法 |\n|---|---|\n| 2 乙 | 做乙 |\n";
    let r = parse_markdown(text, "D-20260101-06");
    assert_eq!(r.alts.len(), 2, "alts={:?}", r.alts);
}

#[test]
fn bare_filenames_are_not_paths() {
    // 路径口径 E1 v3：只认 8 个目录前缀 + 前斜杠；裸文件名不收
    let text = "# D-20260101-05 · 标题\n\n见 `main.rs` 与 `crates/cli/src/main.rs`。\n";
    let r = parse_markdown(text, "D-20260101-05");
    assert_eq!(r.paths, vec!["crates/cli/src/main.rs".to_string()]);
    assert_eq!(r.path_mentions, 1);
}
