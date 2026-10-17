//! 报表：把账本变成一句话。

use crate::ledger::Book;

/// 汇总。它调用 `Book::total`，而 `total` 又调用 `Entry::amount`——
/// 这一条链在图谱里能一路点下去。
pub fn summary(book: &Book) -> String {
    let total = book.total();
    let n = book.items.len();
    format!("{} 笔，合计 {} 元", n, yuan(total))
}

/// 分转元。单独一个函数，让图谱多一条边。
pub fn yuan(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}
