//! jingwei 引擎：全部逻辑在这里，不依赖任何 GUI。
//!
//! 对外唯一接口是 JSON（facts / findings / plan），所以壳（CLI / Tauri）可以随便换。
//!
//! # 六层（箭头只许从上层指向下层）
//!
//! ```text
//! L0 util     纯工具，不懂业务      winutil · dlog · textfile · parallel · scan · jobobj
//! L1 sys      机器与系统           machine · mem · native · clean
//! L2 probe    探测（拿事实）        probe · jvm · ccpp · deps · editors · envsetup · profiles · lang
//! L3 project  项目理解（出结论）     project · projects · contract · rules · diag · fix
//! L4 graph    图谱（存与查）        store · parse · index · envelope
//! L5 state    项目状态（可回滚）     changes · snapshot · workcopy · runs · notes · scaffold
//! L6          launch（编排）
//! ```
//!
//! **分层原来只写在《复审记录》里，没进代码结构**——那是 R2 记下的一条"没兑现"。
//! 重建时把它落成了目录：**光看文件在哪，就知道它该依赖谁。**
//!
//! 下面那一串 `pub use` 是**过渡用的兼容路径**：旧的 `crate::util::winutil::` 仍然能用，
//! 免得一次改几十处 `use`。新写的代码请直接用 `crate::util::winutil::`——
//! 兼容层是为了让这次搬迁能一步到位，不是为了长期留着。

pub mod facts;
pub mod launch;

/// **最小 LSP**：只做"跳到定义"，位置来自图谱而不是重新解析。
pub mod lsp;

/// L0 · 纯工具：不涉及任何业务概念，谁都该能用，而它谁都不依赖。
pub mod util {
    /// **读用户文件先去掉 UTF-8 BOM**（Windows 上记事本/PowerShell 默认写它）。
    pub mod bom;
    pub mod dlog;
    pub mod jobobj;
    pub mod parallel;
    pub mod scan;
    pub mod textfile;
    pub mod winutil;
}

/// L1 · 机器与系统：读这台机器（`machine` / `mem` / `native`），动这台机器（`clean`）。
pub mod sys {
    pub mod clean;
    pub mod machine;
    pub mod mem;
    pub mod native;
}

/// L2 · 探测：把外部世界的事实拿进来。
///
/// **目录叫 `detect` 而不是 `probe`**：层里那个模块本来就叫 `probe.rs`，
/// 层名与模块名撞在一起的话，`crate::detect::probe::xxx` 到底指层还是指模块就说不清了。
/// 层用"这一层在干什么"命名（detect / understand），模块用"它是什么"命名。
pub mod detect {
    pub mod ccpp;
    pub mod deps;
    pub mod editors;
    pub mod envsetup;
    pub mod jvm;
    pub mod lang;
    pub mod nodelock;
    pub mod profiles;
    pub mod probe;
    pub mod web;
}

/// L3 · 项目理解：从事实里得出结论。（同样是层名避让模块名）
pub mod understand {
    pub mod container;
    pub mod contract;
    pub mod diag;
    /// **目录级索引**：把"采集"与"判定"拆开（见 `框架.md` §4.9）。
    ///
    /// **故意还没接线**：先用回归测试证明"同样输入给同样结果"，再接上去。
    pub mod dirs_index;
    /// 项目指纹：判断"两份目录是不是同一个项目"。
    pub mod fingerprint;
    pub mod fix;
    /// **项目指南**：一句话回答"现在能不能跑、卡在哪、下一步敲什么"。
    /// 与 `rules` 的分工：那边是**清单**（我知道的都告诉你），这边是**排序**
    /// （按你现在该做的下一步）。见那个文件的头。
    pub mod guide;
    pub mod project;
    pub mod projects;
    pub mod rules;
    /// **没有项目标记、但有源码的目录要不要给建议**（2026-10-18 用户拍板「要」）。
    /// 在包管理器树里仍然沉默——理由见那个文件的头。
    pub mod unmarked;
    /// **冒烟**：主动跑一次构建，回答"这台机器到底够不够用"（见 `框架.md` §4.10）。
    pub mod smoke;
    /// **cargo 的二进制目标数与 Cargo.toml 键**——见那个文件的头。
    pub mod rustbins;
}

/// L4 · 图谱：存与查。`store` 是库、`parse` 是解析、`imports` 是 `use` 的字符串展开、
/// `resolve` 是把引用变关系、`index` 是把它们接起来、`envelope` 是对外协议。
pub mod graph {
    pub mod envelope;
    pub mod imports;
    pub mod index;
    pub mod parse;
    /// Python 的语法树遍历。**第二门语言**（2026-10-16）。
    /// 与 `parse` 分开是因为"什么算符号"两门语言根本不是一回事——见那个文件的头。
    pub mod parse_python;
    pub mod query;
    pub mod resolve;
    /// 图谱的三种角色（框架.md §4.8）。从 `store::dump` 里拆出来，
    /// 因为那个文件已经一千行、而这段判据是一块独立的"读懂图"的逻辑。
    pub mod roles;
    pub mod store;
}

/// L5 · 项目状态：改动、快照、草稿、运行记录、笔记、脚手架。
pub mod state {
    pub mod changes;
    pub mod notes;
    pub mod runs;
    pub mod scaffold;
    pub mod snapshot;
    pub mod watch;
    pub mod workcopy;
}

// ── 过渡用的兼容路径（新代码请直接用上面的完整路径）────────────────
pub use graph::*;
pub use detect::*;
pub use understand::*;
pub use state::*;
pub use sys::*;
pub use util::*;

/// 工具在项目里存放自己东西的目录名。
pub const DATA_DIR: &str = ".jingwei";
/// 改名前用过的名字，从新到旧：`.pylaunch` → `.drydock` → `.jingwei`。
///
/// **这个链条只许往后加，不许删**。删掉任何一个，就等于把用户那一代名字下
/// 攒的笔记和运行记录丢在原地，而工具再也看不见它们——用户不会知道，
/// 只会觉得"我的记录没了"。
const LEGACY_DATA_DIRS: &[&str] = &[".drydock", ".pylaunch"];

/// 项目里的数据目录。第一次用到时顺手把老名字搬过来——
/// 用户的东西不该因为工具改名而消失。
pub fn data_dir(root: &std::path::Path) -> std::path::PathBuf {
    let new = root.join(DATA_DIR);
    if !new.exists() {
        for legacy in LEGACY_DATA_DIRS {
            let old = root.join(legacy);
            if old.is_dir() {
                move_dir(&old, &new);
                break;
            }
        }
    }
    new
}

/// 把上一代数据目录搬到当前名字下。
///
/// 同一个项目目录内部的搬迁必然同盘，`rename` 就够。失败只有两种可能：
/// 目标已经存在，或者目录被别的进程占着。两种都**不删老目录**——
/// 这次退回新目录继续用，下次启动再试一次。宁可多留一个空目录，
/// 也不能因为搬不动就把用户的东西弄丢。
fn move_dir(from: &std::path::Path, to: &std::path::Path) {
    if let Err(e) = std::fs::rename(from, to) {
        crate::util::dlog::w(
            "9001",
            &format!("数据目录搬迁失败 {} → {}：{e}", from.display(), to.display()),
        );
    }
}

/// 项目里的图谱库路径：`.jingwei/graph.db`。
///
/// **这里是唯一入口。** 这个路径原来在三处各拼了一遍（集成测试、`launch`、
/// 桌面外壳的 `graph_db()`）——同一事实多个副本，改一处就会有一处对不上，
/// 而症状是"图谱像是空的"，不报错。设计 §3.2「判断只有一份」。
pub fn graph_db_path(root: &std::path::Path) -> std::path::PathBuf {
    data_dir(root).join("graph.db")
}

pub use contract::Contract;
pub use facts::{Facts, SCHEMA_VERSION};
// 外壳（cli / src-tauri）**只该从引擎拿类型**，不该各自去依赖领域层——
// 那会把"依赖方向单向"这条规矩变成"三条边都通"。
pub use jingwei_model::{Confidence, Edge, EdgeKind, Node, NodeKind, Source, Unknown, Verdict};
pub use rules::{run_all, Finding, Severity};
