//! hello_rust —— 给图谱页用的演示项目。
//!
//! 库与二进制分开：`lib.rs` 是库入口，`main.rs` 是二进制入口。
//! 这不是为了好看——**集成测试（tests/）只能 import 库**，
//! 一个只有 main.rs 的 crate，测试里 `use hello_rust::...` 编译不过。
//! 图谱上这也看得出来：两个文件各自定义了自己的符号。

pub mod ledger;
pub mod report;
