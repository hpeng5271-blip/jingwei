//! 集成测试：从包外面调用它，所以这是图谱里另一条入口。
//!
//! 注意这里 import 的是 `hello_rust::...`——它要求这个 crate 有 lib 目标。
//! 只有 `main.rs` 的 crate 编译不过这一句，那正是 `src/lib.rs` 存在的理由。

use hello_rust::ledger::{count, ledger_post, Book};
use hello_rust::report::summary;

#[test]
fn 记两笔之后总额对得上() {
    let mut b = Book::new();
    ledger_post(&mut b, "早餐", 1200);
    ledger_post(&mut b, "地铁", 400);
    assert_eq!(count(&b), 2);
    assert!(summary(&b).contains("16.00"));
}
