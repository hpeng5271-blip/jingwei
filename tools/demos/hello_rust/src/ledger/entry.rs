//! 一条账目。单独一个文件，图谱里它是 ledger 底下的一个模块。

pub struct Entry {
    what: String,
    cents: i64,
}

impl Entry {
    pub fn new(what: &str, cents: i64) -> Entry {
        Entry {
            what: what.to_string(),
            cents,
        }
    }

    /// 金额。`ledger.rs` 里的 `Book::total` 调的就是它。
    pub fn amount(&self) -> i64 {
        self.cents
    }

    pub fn what(&self) -> &str {
        &self.what
    }
}
