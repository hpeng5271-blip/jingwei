//! 外壳与引擎之间的**统一信封**。
//!
//! Q7 查出来的问题：38 个命令里有 **12 个根本没有"查不了"的通道**——
//! 它们返回裸 `Value` / `Vec` / `Option`，出错时只能给空对象或空数组。
//! 于是前端看到的是 **"没有显卡"**，而真相是 **"没查到显卡"**。
//! 那正是这套系统从第一天就要消灭的那种错。
//!
//! ## 三态，与 `model::Verdict` 同构
//!
//! ```
//! { "state": "yes",     "data": { … } }
//! { "state": "no",      "reason": "这几处都看过了，确实没有" }
//! { "state": "unknown", "reason": "这几种方式都读不到（多半是权限或驱动）" }
//! ```
//!
//! 前端只按 `state` 分三支，**`unknown` 绝不允许显示成"没有"**。
//!
//! ## 错误（`Err`）与 `unknown` 是两件事
//!
//! 与《设计》§11.1 那两条对齐：
//!
//! | 情形 | 走哪条 | 界面表现 |
//! |---|---|---|
//! | 整件事没做成（目录不存在、权限不够） | `Err` | 红条 + 下一步 |
//! | 做成了，但某项查不出来 | `unknown` | 灰条 + "为什么查不了" |
//!
//! ## 一个判据：空列表是"正常状态"还是"异常状态"？
//!
//! 这是用信封最容易搞反的一处，单列出来：
//!
//! | 情形 | 空意味着 | 该用哪个 |
//! |---|---|---|
//! | **用户自己创建的东西**（配置档、笔记） | "你还没建过"——正常 | `yes` + 空数组 |
//! | **系统里本该存在的东西**（Python、编辑器、硬件） | "我没找到"——异常 | `unknown` + 为什么 |
//!
//! 判据一句话：**这是"用户的空白"，还是"我们的失败"？**
//!
//! 搞反的后果不一样，所以值得记：
//! - 把"用户的空白"报成 `unknown` → 界面上莫名其妙冒出一串警告，用户以为坏了；
//! - 把"我们的失败"报成 `yes` + 空数组 → **用户看到"没有"，而真相是"没查到"**
//!   ——这是这套系统从第一天就要消灭的那类错。

use serde::Serialize;

/// 一条结果的三种状态。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Envelope<T> {
    /// 查到了。
    Yes { data: T },
    /// **查过了，确实没有**——这个结论本身是有依据的。
    No { reason: String },
    /// **做成了，但这一项查不出来**。`reason` 必须写清楚"试过哪些方式"。
    Unknown { reason: String },
}

impl<T> Envelope<T> {
    pub fn yes(data: T) -> Self {
        Envelope::Yes { data }
    }

    /// 查不到。`reason` 要**说清试过什么**，不能只写"失败"——
    /// "我在 PATH、注册表、六种常见安装目录里都没找到"才是有用的信息。
    pub fn unknown(reason: impl Into<String>) -> Self {
        Envelope::Unknown {
            reason: reason.into(),
        }
    }

    pub fn no(reason: impl Into<String>) -> Self {
        Envelope::No {
            reason: reason.into(),
        }
    }
}

impl<T: Serialize> Envelope<T> {
    /// 转成给前端的值。
    ///
    /// **序列化失败也走 `unknown`**：那说明我们自己的类型有问题，
    /// 但对外仍然只能说"这项查不了"，不能让调用方拿到半个对象。
    pub fn to_value(&self) -> serde_json::Value {
        match serde_json::to_value(self) {
            Ok(v) => v,
            Err(e) => serde_json::json!({
                "state": "unknown",
                "reason": format!("结果序列化失败（这是缺陷，不是你的问题）：{e}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 三态的序列化形状固定() {
        let yes = Envelope::yes(vec![1, 2]).to_value();
        assert_eq!(yes["state"], "yes");
        assert_eq!(yes["data"][0], 1);

        let no = Envelope::<Vec<i32>>::no("三处都看过了，确实没有").to_value();
        assert_eq!(no["state"], "no");
        assert!(no["reason"].as_str().unwrap().contains("确实没有"));

        let unk = Envelope::<Vec<i32>>::unknown("PATH 与注册表都读不到").to_value();
        assert_eq!(unk["state"], "unknown");
        assert!(unk["data"].is_null(), "unknown 不许带 data——那会让人以为查到了");
    }

    /// **`unknown` 与 `no` 是两件事**，前端靠 `state` 分开。
    /// 写一条测试盯着它们别被谁合成一个——这正是这套系统最核心的那条线。
    #[test]
    fn 查不了与没有不是一个状态() {
        let a = Envelope::<i32>::no("查过了，没有").to_value();
        let b = Envelope::<i32>::unknown("查不了").to_value();
        assert_ne!(a["state"], b["state"]);
    }
}
