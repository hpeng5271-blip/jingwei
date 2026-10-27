//! expand_use_text 单元表（先红后绿）：多行/嵌套/尾逗号/self/as。
use collect::parse::expand_use_text;

fn leaves(text: &str) -> Vec<String> {
    let mut v: Vec<String> = expand_use_text(text)
        .into_iter()
        .map(|t| collect::parse::leaf_of(&t).unwrap_or_default())
        .filter(|s| !s.is_empty())
        .collect();
    v.sort();
    v
}

#[test]
fn expand_table() {
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("use serde::{A, B};", vec!["A", "B"]),
        (
            "use {\n    serde::{A, B},\n    serde_json,\n};",
            vec!["A", "B", "serde_json"],
        ),
        ("use a::{b::{C, D}, e};", vec!["C", "D", "e"]),
        ("use a::{self, B};", vec!["a", "B"]),
        ("use a::B as C;", vec!["C"]),
        ("use {a, b};", vec!["a", "b"]),
        ("use a::{B, C,};", vec!["B", "C"]),
        ("use ::std::io;", vec!["io"]),
        ("use crate::{self};", vec!["crate"]),
        (
            "use serde::{\n    Searcher, Sink, SinkContext, SinkFinish, SinkMatch},\n    serde_json\n};",
            vec!["Searcher", "Sink", "SinkContext", "SinkFinish", "SinkMatch", "serde_json"],
        ),
    ];
    let mut bad = Vec::new();
    for (input, want) in cases {
        let got = leaves(input);
        let mut w: Vec<String> = want.iter().map(|s| s.to_string()).collect();
        w.sort();
        if got != w {
            bad.push(format!("{input:?}\n  got  {got:?}\n  want {w:?}"));
        }
    }
    assert!(bad.is_empty(), "展开表有 {} 条不符：\n{}", bad.len(), bad.join("\n"));
}
