//! **消息与提示的判据**（2026-10-21 第四段的引擎侧）。
//!
//! 设计口径（pro 的裁决 + 人补的两条）：
//! 1. 消息是资源不是日志：同一条（同 key）**只占一个槽**，重复出现只加计数；
//! 2. 生命周期只有一条合法链：`New → Shown → Read → Archived`；**归档后不许再变未读**；
//! 3. 静音窗口按"重犯次数 × 项目变更频率"自适应，长度被夹在 30 秒 ~ 30 分钟之间；
//! 4. 跳转许可：只有用户动作才算（调用方据此决定要不要抢焦点），静音期内不重复提示。
//!
//! 这一层只管判据，不管渲染；GUI 那边只消费它的结果。

use jingwei_engine::understand::notify::{self, Level, MsgState};

/// 同一个 key 重复出现：**只留一条**，计数 +1，并记下最后一次时间。
#[test]
fn 同一个键只留一条并计数() {
    let mut v = Vec::new();
    notify::ingest(&mut v, "index.timeout", Level::Error, "深绘超时", 100);
    notify::ingest(&mut v, "index.timeout", Level::Error, "深绘超时（第 2 次）", 200);
    assert_eq!(v.len(), 1, "同一 key 不该占两个槽：{v:?}");
    assert_eq!(v[0].count, 2, "重复出现要计数");
    assert_eq!(v[0].last_at, 200, "要记最后一次时间");
    assert_eq!(v[0].state, MsgState::New, "新出现的消息是未读");
}

/// 归档之后不许变回未读——那会让"已经处理过"的东西反复跳出来。
#[test]
fn 归档之后不许再变成未读() {
    let mut v = Vec::new();
    notify::ingest(&mut v, "k", Level::Info, "x", 1);
    notify::mark(&mut v, "k", MsgState::Shown).expect("New → Shown 合法");
    notify::mark(&mut v, "k", MsgState::Read).expect("Shown → Read 合法");
    notify::mark(&mut v, "k", MsgState::Archived).expect("Read → Archived 合法");
    assert!(
        notify::mark(&mut v, "k", MsgState::New).is_err(),
        "已归档不许变回未读（要新事件才算新消息）"
    );
}

/// 静音窗口：夹在 30 秒 ~ 30 分钟；重犯越多越长，项目变得越快越短。
#[test]
fn 静音窗口随重犯变长_随变更频率变短() {
    assert_eq!(notify::silence_secs(0.0, 0), 30, "基线下限 30 秒");
    let slow = notify::silence_secs(0.0, 2);
    let fast = notify::silence_secs(1.0, 2);
    assert!(fast < slow, "项目变得快（f=1）应更短：fast={fast} slow={slow}");
    assert!(
        notify::silence_secs(0.0, 100) <= 1800,
        "上限 30 分钟，不许无限长"
    );
    assert!(notify::silence_secs(0.0, 100) >= 30, "下限 30 秒");
}

/// 静音期内不重复提示；从没提示过就一定提示。
#[test]
fn 静音期内不重复提示() {
    assert!(!notify::should_notify(Some(1_000), 1_000 + 10, 60), "10 秒内该闭嘴");
    assert!(notify::should_notify(Some(1_000), 1_000 + 61, 60), "过了 60 秒该提示");
    assert!(notify::should_notify(None, 5, 60), "从没提示过 → 提示");
}

/// **归档后再来一条 = 新事件**（D-20261022-02）：状态回到 New、计数 +1。
/// 否则"归档"就变成了"永久静音"，那是对用户的隐瞒。
#[test]
fn 归档之后再来一条要变回未读() {
    let mut v = Vec::new();
    notify::ingest(&mut v, "k", Level::Info, "x", 1);
    for st in [MsgState::Shown, MsgState::Read, MsgState::Archived] {
        notify::mark(&mut v, "k", st).expect("合法转移");
    }
    notify::ingest(&mut v, "k", Level::Info, "x 又来了", 2);
    assert_eq!(v.len(), 1, "同 key 仍只占一槽");
    assert_eq!(v[0].count, 2);
    assert_eq!(v[0].state, MsgState::New, "归档后再来必须是未读");
}

/// **保留上限**：超出上限先丢已归档的，不够再丢最旧的。
#[test]
fn 超出上限先丢已归档的() {
    let mut v = Vec::new();
    for i in 0..55 {
        notify::ingest(&mut v, &format!("k{i}"), Level::Info, "x", i);
    }
    let n = v.len();
    assert!(n <= 50, "不该无限涨：{n}");
    // 把前两条归档后再塞新的：归档的那两条应该先被挤掉
    notify::mark(&mut v, "k0", MsgState::Archived).ok();
    notify::mark(&mut v, "k1", MsgState::Archived).ok();
    for i in 55..60 {
        notify::ingest(&mut v, &format!("n{i}"), Level::Info, "x", i);
    }
    assert!(!v.iter().any(|m| m.key == "k0" || m.key == "k1"), "已归档的该先丢");
}
