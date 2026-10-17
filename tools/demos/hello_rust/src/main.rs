//! 二进制入口。它调用库里的东西，所以 `main` 这个符号会连到 ledger / report。
//!
//! 这里故意写成"模块树 + 跨文件调用"的形状：如果只有一个 main.rs，
//! 图谱页只能画出三四个点，看不出它是干什么的。
//!
//! 看图谱时值得指的三条链：
//!   main.rs#main           --calls-->  ledger.rs#post
//!   report.rs#summary      --calls-->  ledger.rs#Book::total
//!   ledger.rs#Book::total  --calls-->  ledger/entry.rs#Entry::amount

use hello_rust::ledger::{ledger_post, Book};
use hello_rust::report::summary;

fn main() {
    let mut book = Book::new();
    ledger_post(&mut book, "早餐", 1200);
    ledger_post(&mut book, "地铁", 400);
    println!("{}", summary(&book));
}
