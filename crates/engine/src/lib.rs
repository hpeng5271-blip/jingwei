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

/// L0 · 纯工具：不涉及任何业务概念，谁都该能用，而它谁都不依赖。
pub mod util {
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
    pub mod contract;
    pub mod diag;
    pub mod fix;
    pub mod project;
    pub mod projects;
    pub mod rules;
}

/// L4 · 图谱：存与查。`store` 是库、`parse` 是解析、`imports` 是 `use` 的字符串展开、
/// `resolve` 是把引用变关系、`index` 是把它们接起来、`envelope` 是对外协议。
pub mod graph {
    pub mod envelope;
    pub mod imports;
    pub mod index;
    pub mod parse;
    pub mod resolve;
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

pub use contract::Contract;
pub use facts::{Facts, SCHEMA_VERSION};
// 外壳（cli / src-tauri）**只该从引擎拿类型**，不该各自去依赖领域层——
// 那会把"依赖方向单向"这条规矩变成"三条边都通"。
pub use jingwei_model::{Confidence, Edge, EdgeKind, Node, NodeKind, Source, Verdict};
pub use rules::{run_all, Finding, Severity};
