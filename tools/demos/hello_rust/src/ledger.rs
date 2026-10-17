//! 账本：一笔一笔地记。

pub mod entry;

pub use entry::Entry;

/// 一本账。条目按加入顺序留着。
pub struct Book {
    pub items: Vec<Entry>,
}

impl Book {
    pub fn new() -> Book {
        Book { items: Vec::new() }
    }

    /// 全部金额相加。它调用 `Entry::amount`——这条边跨了两个文件。
    pub fn total(&self) -> i64 {
        self.items.iter().map(|e| e.amount()).sum()
    }
}

/// 记一笔。被 `main.rs` 调用，所以这条边也跨文件。
///
/// 名字前面挂 `ledger_` 是演示需要：`main.rs` 里同时 import 了 ledger 和 report
/// 两个模块，同名短函数名容易让人读错，而图谱上也只是多一条边的事。
pub fn ledger_post(book: &mut Book, what: &str, cents: i64) {
    let e = Entry::new(what, cents);
    book.items.push(e);
}

/// 只有测试用的辅助函数——它也在图谱里。
/// 这本身是个演示点：图谱不区分"测试代码"和"产品代码"，那是使用者的判断。
pub fn count(book: &Book) -> usize {
    book.items.len()
}
