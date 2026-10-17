//! **项目指南**：一句话回答「这个项目现在能不能跑起来、卡在哪、下一步敲什么」。
//!
//! ## 为什么要有它
//!
//! `doctor` 给的是**清单**（解释器、依赖、契约、九层排查链各报几条），
//! 而人打开一个项目时真正问的是**一个问题**：「我要怎么把它跑起来？」
//!
//! 这两件事不一样。清单是"我知道的都告诉你"，指南是"**按你现在该做的下一步排序**"。
//! 一个列了 7 条提示却不说"先做哪个"的界面，等于把排序工作推给用户——
//! 而那正是工具该替他做的部分（设计 §8.1 第 ⑥ 条「一屏 1 结论」）。
//!
//! ## 只报**挡路**的，不报"最好也做一下"的
//!
//! `blockers` 只装**现在就挡着跑不起来**的。装依赖、加 .gitignore 那类叫 `nice_to_have`，
//! 分开列——**混在一起就是没有排序**，而"没有排序"正是这一层要解决的问题。
//!
//! ## 三态在这里的落法
//!
//! 「能不能跑」有三种答案，不是两种：
//!   - `Ready`：查过，能跑
//!   - `Blocked`：查过，跑不起来，**而且知道卡在哪**
//!   - `Unknown`：**查不了**（读不到契约、探测失败）——这时不许说"能跑"，也不许说"不能跑"
//!
//! 第三条是这个项目的招牌，别在这一层丢了。

use jingwei_model::{Unknown, Verdict};
use std::path::Path;

use super::contract::Contract;
use crate::detect::lang::Lang;
use crate::facts::Facts;

/// 一条挡路的事。`fix` 是**人可以敲的命令**，不是"我们代你做"——
/// 四条线之一"不生成修复命令"在这里的落法：给命令行，不给魔法。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocker {
    /// 一句话说清卡在哪（给用户看，别写内部字段名）
    pub what: String,
    /// 为什么这会挡住运行
    pub why: String,
    /// 人可以敲的命令（空 = 这件事没法用一条命令解决，比如"去装个东西"）
    pub fix: String,
    /// 怎么验证它现在真的挡着（**每条结论都要有自查命令**，这是四条线之一）
    pub verify: String,
}

/// 指南。**字段顺序就是阅读顺序**。
///
/// **不派生 `Eq`**：`Verdict<String>` 只实现了 `PartialEq`（model 那边就这么定的），
/// 而这里没有"拿它当 map 键"的需求——不为了对称去加一个用不上的约束。
#[derive(Debug, Clone, PartialEq)]
pub struct Guide {
    /// 项目类型（人话：Python 3 / Rust / Node …）
    pub what: String,
    /// 现在能不能跑
    pub ready: Verdict<String>,
    /// 挡路的（按严重度排序，第一条就是"先做这个"）
    pub blockers: Vec<Blocker>,
    /// **建议的下一步**：一条可以直接敲的命令
    pub next_step: String,
    /// 不挡路、但迟早咬一口的
    pub nice_to_have: Vec<String>,
    /// 这个项目声明了怎么跑（来自契约）
    pub run_command: Option<String>,
    /// **"查不了"是谁造成的**（`ready == Unknown` 时才有意义）。
    ///
    /// 2026-10-16 加：`ready` 说得出"查不了"，**说不出这个查不了该怪谁**——
    /// 而用户能做的事完全不同。
    ///   - `TheirGap`：项目里没有契约 / 契约里没写 run 命令 → **补上就能判断**；
    ///   - `OurGap`：是我们的问题 → 他不用动，等我们修或换个办法。
    ///
    /// 两句话混成一句（"还查不出来能不能跑"），用户就不知道该做什么。
    ///
    /// **为什么默认 `Unclear` 而不是硬归一头**：`Guide` 可能是别处拼出来的，
    /// 那时我们真的不知道原因。**编一个归类比不归更糟**（这个系统的铁律）。
    pub unknown_why: Unknown,
    /// **"查不了"时该跟用户说的那句话**（2026-10-16 加）。
    ///
    /// 为什么不让外壳自己从 `unknown_why` 拼：**归属是类型给的，但"该说什么"要看场合**。
    /// 例：拿 Python 标准库（`Lib/`）当项目打开时，`unknown_why` 仍是 `TheirGap`
    /// （"缺契约"），可**标准库不该有契约**——照 `what_to_do()` 的原话
    /// "这一项你这边没有；补上就能判断"会让用户去给标准库塞一份契约，
    /// 换来一个"假项目能跑"的结论，**比不判断更坏**。
    ///
    /// 所以这句话由**知道场合的那一层**（这里）给，外壳只准转述（SOP §S16）。
    /// `None` = 还没想好，外壳退回 `unknown_why.what_to_do()`。
    pub what_to_do: Option<String>,
}

impl Guide {
    /// 「先做这个」——第一条挡路的；没有挡路的就给下一步。
    pub fn headline(&self) -> String {
        if let Some(b) = self.blockers.first() {
            return b.what.clone();
        }
        match &self.ready {
            Verdict::Yes(cmd) => format!("能跑：{cmd}"),
            Verdict::No => "跑不起来，但没查出原因".to_string(),
            // **查不了的时候不许编一句话出来。**
            Verdict::Unknown => "还查不出来——先看下面的下一步".to_string(),
        }
    }

    /// **一份 JSON，两个出口**（CLI 与桌面界面）（2026-10-16）。
    ///
    /// ## 为什么要把它放在这儿，而不是各写各的
    ///
    /// 原来只有 `cmd_guide.rs` 里有这段转换。而桌面界面也要显示同一份指南——
    /// **两处各写一遍，迟早会算出两套说法**。这个仓库为"两份清单各写各的、
    /// 必然漂移"付过账（`--help` 漏列 4 个命令、说明书承诺 `inspect` 却不存在）。
    ///
    /// 放在 `Guide` 上还有一个好处：**三态的形状由类型的持有者决定**，
    /// 换出口的人不需要重新理解一遍"`Unknown` 还要带 `why` 与 `whatToDo`"。
    ///
    /// ## 形状
    ///
    /// `ready` 是**一个对象**，不是一个布尔：`{"state": "yes"|"no"|"unknown", …}`。
    /// `unknown` 那一支必须带 `why`（`their_gap` / `our_gap` / `unclear`）
    /// 与 `whatToDo`——**脚本拿到的不能是压扁的一句话**，那是三态在 JSON 上的落法。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "what": self.what,
            "ready": match &self.ready {
                Verdict::Yes(cmd) => serde_json::json!({"state": "yes", "command": cmd}),
                Verdict::No => serde_json::json!({"state": "no"}),
                Verdict::Unknown => serde_json::json!({
                    "state": "unknown",
                    "why": match self.unknown_why {
                        crate::Unknown::TheirGap => "their_gap",
                        crate::Unknown::OurGap => "our_gap",
                        crate::Unknown::Unclear => "unclear",
                    },
                    "whatToDo": self
                        .what_to_do
                        .clone()
                        .unwrap_or_else(|| self.unknown_why.what_to_do().to_string()),
                }),
            },
            "headline": self.headline(),
            "blockers": self.blockers.iter().map(|b| serde_json::json!({
                "what": b.what, "why": b.why, "fix": b.fix, "verify": b.verify,
            })).collect::<Vec<_>>(),
            "nextStep": self.next_step,
            "niceToHave": self.nice_to_have,
            "runCommand": self.run_command,
        })
    }
}

/// 从已经采好的事实里推出一份指南。
///
/// **纯函数**：不读盘、不跑命令、不 spawn 任何东西。要跑命令的检查（`--check`）
/// 由调用方决定要不要先跑，结果以 `diagnostics` 的形式传进来。
/// 这条边界很重要：指南要便宜（毫秒级），否则人不会在每次打开项目时看它。
pub fn make_guide(facts: &Facts, diagnostics: &[crate::launch::rundiag::RunDiagnostic]) -> Guide {
    let lang = facts.lang;
    let contract = facts.project.contract.as_ref();

    let what = describe_project(facts);
    let mut blockers = Vec::new();
    let mut nice = Vec::new();

    // ── 1. 契约读不了 → 连"怎么跑"都不知道，这是最前面的一道 ──
    if let Some(err) = &facts.project.contract_error {
        blockers.push(Blocker {
            what: "读不了 `env.contract.json`".to_string(),
            why: format!("它是「这个项目怎么跑」的唯一真源，读不了就只剩猜：{err}"),
            fix: "jingwei init".to_string(),
            verify: "jingwei doctor".to_string(),
        });
    }

    // ── 2. 语言对应的工具链在不在 ──
    check_toolchain(facts, lang, &mut blockers);

    // ── 3. 依赖：**只按三态报**，Unknown 不许说成"没装" ──
    check_deps(facts, &mut blockers, &mut nice);

    // ── 4. 契约里声明的命令能不能用 ──
    let run_command = contract.and_then(|c| c.commands.run.clone());
    if contract.is_some() && run_command.is_none() {
        nice.push(
            "契约里没写 `commands.run`——`jingwei run` 不知道要启动什么（补一行就好）".to_string(),
        );
    }

    // ── 5. 上次跑失败过？那是**最直接**的一条线索 ──
    for d in diagnostics.iter().take(3) {
        blockers.push(Blocker {
            what: format!("上次运行报错：{}", d.title),
            why: match &d.file {
                Some(f) => format!("这是上一次真跑出来的错，落在 {f}——比任何推测都准"),
                None => "这是上一次真跑出来的错，比任何推测都准".to_string(),
            },
            fix: String::new(),
            // 有落点的直接把位置给出来；自查命令统一是"再跑一次看它还在不在"。
            verify: match &d.file {
                Some(f) => format!("jingwei run   （这次报错落在 {f}）"),
                None => "jingwei run".to_string(),
            },
        });
    }

    // ── 6. 结论：能不能跑 ──
    let ready = decide_ready(&blockers, run_command.as_deref(), facts, contract);
    let unknown_why = decide_unknown_why(&ready, facts, contract, run_command.as_deref());

    // ── 6.5 **"这个目录根本不像一个项目"是一条独立的话**（2026-10-16 加）────────
    //
    // 拿 Python 3.15 的标准库（`Lib\`）跑 `guide` 时抓到的：它说
    // 「**这一项你这边没有；补上就能判断**」——"这一项"指的是 `env.contract.json`。
    // 可标准库**不该**有契约；照这句话去做，换来的是一个"假项目能跑"的结论，
    // **比不判断更坏**。
    //
    // 根因（外部独立判读也指到同一处）：工具唯一管"这是不是一个项目"的判据是
    // `container.rs` 的「直接子目录里 ≥2 个带项目标记」，而 `Lib` 的子目录
    // （`idlelib`/`asyncio`/`site-packages`…）**一个项目标记都没有** → 那条不响。
    // **没有任何规则管"语言安装目录"**。
    //
    // 这里**不动 `Unknown` 那三支**（加一支要碰 32 处 `matches!`，代价与风险都不对），
    // 而是**修那句话本身**：不是"你缺了什么"，而是"**这里不是项目，我不该给项目建议**"。
    // 判据用现成的：拿不到 `run_command` 且根目录没有项目标记 → 它不像一个项目。
    let not_a_project = contract.is_none()
        && run_command.is_none()
        && !crate::understand::projects::looks_like_project(std::path::Path::new(&facts.project.root));
    let what_to_do = if not_a_project {
        "这里不像一个项目（没有任何项目标记文件）——所以「能不能跑」这类问题对它不适用"
    } else {
        unknown_why.what_to_do()
    };

    // **"不是一个项目"要排在所有分支之前**（2026-10-17）。
    //
    // 这一条是"同一个洞的第二个消费点"：上一段把 headline 与 `whatToDo` 修成了
    // 「这个问题对它不适用」，而**下一步还在说「补一份契约」**——
    // 同一个屏幕上，头说"不适用"、尾说"你去补"，仍然是自相矛盾。
    //
    // 判据：`not_a_project` 一旦成立，**后面每一条"该做什么"都是错的**，
    // 因为它们全都按"这是一个项目"的口径写。所以它必须最先分流。
    let next_step = if not_a_project {
        // 不是一个项目：**不要让他去补任何东西**。
        "不用做什么——这里不是一个项目（想管它这个目录，就把根指到真正含标记文件的那一层）"
            .to_string()
    } else if !blockers.is_empty() {
        let b = &blockers[0];
        if b.fix.is_empty() {
            b.verify.clone()
        } else {
            b.fix.clone()
        }
    } else if let Verdict::Yes(cmd) = &ready {
        cmd.clone()
    } else if let Some(cmd) = &run_command {
        // 说不出能不能跑、但**契约里写了怎么跑**——那就跑一次试试，这是最诚实的一步。
        cmd.clone()
    } else if contract.is_none() {
        // ⚠️ **没有契约时不许给 `jingwei run`**（2026-10-16 改，这条是拿真项目跑出来的）。
        //
        // 原来这里是 `run_command.clone().unwrap_or_else(|| "jingwei run")`——
        // 而 `run_command` 为 `None` **恰好就是"没有契约"这个信号**。
        // 于是同一份输出自相矛盾：前半句说"查不了 = 没有契约"，
        // 后半句让你去敲一条**因为"没有契约"而必然失败**的命令
        // （`launch/python.rs` 会报「契约里没有声明 run 命令」并 exit 1）。
        //
        // 实测目标：Python 标准库的 `Lib/`——150 个顶层 .py 里没有 `main.py`/`app.py`，
        // 连猜都猜不出来。**让用户敲一条注定失败的命令，比不给命令更坏。**
        "jingwei init   （还没有 env.contract.json；补一份才知道怎么跑）".to_string()
    } else {
        // 有契约、但契约里没写 `commands.run`。
        "在 env.contract.json 的 commands 里补一行 run".to_string()
    };

    Guide {
        what,
        ready,
        blockers,
        next_step,
        nice_to_have: nice,
        run_command,
        unknown_why,
        // 只在"这压根不是一个项目"时才由这一层给一句话；其余场合外壳退回类型的原话。
        what_to_do: if not_a_project {
            Some(what_to_do.to_string())
        } else {
            None
        },
    }
}

/// **"查不了"是谁造成的**——把 `decide_ready` 注释里那句判断搬进类型。
///
/// `decide_ready` 的注释原来写着「契约在、但没有 run 命令：**这是"用户没写"，
/// 不是"我们的失败"**」——**判断在脑子里，类型上没有**。
/// 于是到了界面上，两种完全不同的情况都变成同一句"还查不出来能不能跑"。
///
/// 这个函数只做一件事：把那个已经在注释里做过的判断**写成返回值**。
/// 三支的归属都有依据（对着 `decide_ready` 的三个 `Unknown` 出口看）：
///   - 没有契约 → 用户没提供判据的入口 → `TheirGap`
///   - 项目不存在 → 用户这边的空白 → `TheirGap`
///   - 契约在但没写 run → **注释原话"这是用户没写"** → `TheirGap`
///
/// **`ready` 不是 `Unknown` 时返回 `Unclear`**：那时这个问题没有意义，
/// 而返回一个"看起来像答案"的值会让调用方误用。
fn decide_unknown_why(
    ready: &Verdict<String>,
    facts: &Facts,
    contract: Option<&Contract>,
    run_command: Option<&str>,
) -> Unknown {
    if !matches!(ready, Verdict::Unknown) {
        return Unknown::Unclear;
    }
    if contract.is_none() || !facts.project.exists || run_command.is_none() {
        return Unknown::TheirGap;
    }
    // 走到这里说明"未知"来自别处（例如某个我们没实现的检查）。
    // **不硬归**——如实说分不清。
    Unknown::Unclear
}

/// 项目类型说成人话。**复合项目要有复合的说法**，不能只报第一门语言
/// （第 13 轮那个"Tauri 项目只报 Node 的事"就是这么错的）。
fn describe_project(facts: &Facts) -> String {
    let names: Vec<&str> = facts.langs.iter().map(|l| l.name()).collect();
    if names.is_empty() {
        return "认不出是什么项目".to_string();
    }
    if names.len() == 1 {
        return names[0].to_string();
    }
    format!("{}（混合项目）", names.join(" + "))
}

/// 工具链：这门语言要的东西在不在 PATH 上。
///
/// **只查"没有就完全跑不起来"的那些**——`git` 不在不算挡路（那是 nice_to_have 的事）。
fn check_toolchain(facts: &Facts, lang: Lang, blockers: &mut Vec<Blocker>) {
    let has = |name: &str| facts.toolchain.get(name).map(|t| t.ok).unwrap_or(false);
    match lang {
        Lang::Rust => {
            if !has("rustc") {
                blockers.push(Blocker {
                    what: "机器上没有 Rust 工具链".to_string(),
                    why: "没装 rustc 时 `cargo build` 直接起不来，而报错只会说\
                          「'cargo' 不是内部或外部命令」——那句话不告诉你缺的是工具链"
                        .to_string(),
                    fix: "去 https://rustup.rs 装，装完**重开一个终端**（它改的是 PATH）"
                        .to_string(),
                    verify: "(Get-Command cargo -All).Source".to_string(),
                });
            }
        }
        Lang::Python => {
            if !facts.python.found {
                blockers.push(Blocker {
                    what: "机器上没找到能用的 Python".to_string(),
                    why: "这个项目是 Python 的，没有解释器就没有任何一步能走".to_string(),
                    fix: "装一个 Python 3，或 `jingwei envs` 看这台机器上已经有哪些".to_string(),
                    verify: "jingwei envs".to_string(),
                });
            }
        }
        Lang::Web => {
            if !has("node") {
                blockers.push(Blocker {
                    what: "机器上没有 Node".to_string(),
                    why: "前端项目没有 Node 就装不了依赖、起不了服务".to_string(),
                    fix: "装 Node（建议 LTS），装完重开终端".to_string(),
                    verify: "node -v".to_string(),
                });
            }
        }
        Lang::Java => {
            if !has("java") {
                blockers.push(Blocker {
                    what: "机器上没有 Java 运行时".to_string(),
                    why: "Java 项目没有 JVM 就跑不起来".to_string(),
                    fix: "装一个 JDK（建议 17 或更新）".to_string(),
                    verify: "java -version".to_string(),
                });
            }
        }
        // C++ / 未知：编译器那几条已经由体检规则覆盖，这里不重复报
        // （**同一件事在两处各报一次，用户会以为是两个问题**——第 13 轮的教训）。
        Lang::Cpp | Lang::Unknown => {}
    }
}

/// 依赖：三态。**Unknown 不进 blockers**，它进 nice_to_have 并说清"没查到"。
fn check_deps(facts: &Facts, blockers: &mut Vec<Blocker>, nice: &mut Vec<String>) {
    let mut missing: Vec<&str> = Vec::new();
    let mut unknown: Vec<&str> = Vec::new();
    for d in &facts.project.deps_detail {
        if d.unknown {
            unknown.push(&d.name);
        } else if !d.ok {
            missing.push(&d.name);
        }
    }
    if !missing.is_empty() {
        let list = missing.join("、");
        blockers.push(Blocker {
            what: format!("{} 个依赖没装：{}", missing.len(), list),
            why: "声明了却没装，跑起来会在 import 那一行炸".to_string(),
            fix: "jingwei env install".to_string(),
            verify: "jingwei doctor".to_string(),
        });
    }
    if !unknown.is_empty() {
        // **不许说成"没装"**——那是把"我们没查过"写成"你的问题"。
        //
        // 2026-10-16：这句话的**归属**现在从类型来（`DepDetail::unknown_why`），
        // 不再在这里手写。手写的问题不是这句话错了（它本来就对），
        // 而是**同一个判断在三处各写一句**（这里、`snapshot.rs`、`rules.rs`），
        // 迟早有一处漏掉"不是没装"那半句。
        let why = facts
            .project
            .deps_detail
            .iter()
            .find(|d| d.unknown)
            .map(|d| d.unknown_why().what_to_do())
            .unwrap_or("");
        nice.push(format!(
            "{} 个依赖没能确认（**不是没装，是没查到**）：{}——{}。\
             装完之后 `jingwei doctor` 会给出确切结论",
            unknown.len(),
            unknown.join("、"),
            why
        ));
    }
}

/// 结论。**三态，而且"查不了"不许压成任何一边。**
fn decide_ready(
    blockers: &[Blocker],
    run_command: Option<&str>,
    facts: &Facts,
    contract: Option<&Contract>,
) -> Verdict<String> {
    if !blockers.is_empty() {
        return Verdict::No;
    }
    // 没有挡路的 —— 但"没有挡路的"不等于"能跑"：
    // 连契约都没有的项目，我们**根本没查过它怎么跑**，那是 Unknown。
    let Some(c) = contract else {
        return Verdict::Unknown;
    };
    if !facts.project.exists {
        return Verdict::Unknown;
    }
    let Some(cmd) = run_command else {
        // 契约在、但没有 run 命令：这是"用户没写"，不是"我们的失败"。
        // 报 Unknown 并说清缺什么，比报 No 有用（No 会让人以为项目坏了）。
        return Verdict::Unknown;
    };
    let _ = c;
    Verdict::Yes(cmd.to_string())
}

/// 这个项目根目录像不像"能跑的东西"。给 CLI 做第一道判断用。
pub fn looks_runnable(root: &Path) -> bool {
    root.join("env.contract.json").exists()
        || root.join("Cargo.toml").exists()
        || root.join("package.json").exists()
        || root.join("pyproject.toml").exists()
        || root.join("requirements.txt").exists()
        || root.join("pom.xml").exists()
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声（与 `deps_tests.rs` 同一句）。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;
    use crate::detect::lang::Lang;
    use crate::facts::{DepDetail, Facts, ProjectFacts, PythonFacts, ToolFact};

    fn facts_with(lang: Lang, deps: Vec<DepDetail>, contract: Option<Contract>) -> Facts {
        Facts {
            schema_version: 1,
            tool_version: "test".into(),
            taken_at: String::new(),
            system: Default::default(),
            lang,
            langs: vec![lang],
            jvm: None,
            cc: None,
            last_mem: None,
            python: PythonFacts {
                found: true,
                ..Default::default()
            },
            toolchain: Default::default(),
            project: ProjectFacts {
                root: ".".into(),
                exists: true,
                contract,
                deps_detail: deps,
                ..Default::default()
            },
        }
    }

    fn dep(name: &str, ok: bool, unknown: bool) -> DepDetail {
        DepDetail {
            name: name.into(),
            want: String::new(),
            have: String::new(),
            ok,
            unknown,
            from: "requirements.txt".into(),
            evidence: "pip list".into(),
        }
    }

    fn py_contract(run: Option<&str>) -> Contract {
        let mut c: Contract = serde_json::from_str(r#"{"schema_version":1,"name":"x"}"#).unwrap();
        c.commands.run = run.map(|s| s.to_string());
        c
    }

    /// **没有挡路的事、有契约有 run 命令 → 能跑，而且把命令原样给出来。**
    #[test]
    fn 万事俱备时说能跑并把命令给出来() {
        let f = facts_with(
            Lang::Python,
            vec![dep("rich", true, false)],
            Some(py_contract(Some("python main.py"))),
        );
        let g = make_guide(&f, &[]);
        assert_eq!(g.blockers, vec![]);
        assert_eq!(g.ready, Verdict::Yes("python main.py".into()));
        assert_eq!(g.next_step, "python main.py");
    }

    /// **依赖没装 → 挡路，而且下一步就是装它。**
    #[test]
    fn 依赖没装是挡路的() {
        let f = facts_with(
            Lang::Python,
            vec![dep("rich", false, false)],
            Some(py_contract(Some("python main.py"))),
        );
        let g = make_guide(&f, &[]);
        assert_eq!(g.ready, Verdict::No);
        assert_eq!(g.blockers.len(), 1);
        assert!(g.blockers[0].what.contains("rich"), "{:?}", g.blockers[0]);
        assert!(!g.blockers[0].verify.is_empty(), "每条结论都要有自查命令");
    }

    /// **"没查到"绝不许变成"没装"**——这一条是拿一个假 Error 换来的。
    #[test]
    fn 查不到的依赖不算挡路的但要说出来() {
        let f = facts_with(
            Lang::Python,
            vec![dep("rich", false, true)],
            Some(py_contract(Some("python main.py"))),
        );
        let g = make_guide(&f, &[]);
        assert_eq!(g.blockers, vec![], "查不到不是挡路的");
        assert_eq!(g.ready, Verdict::Yes("python main.py".into()));
        assert_eq!(g.nice_to_have.len(), 1);
        assert!(
            g.nice_to_have[0].contains("没能确认") && g.nice_to_have[0].contains("不是没装"),
            "必须说清「不是没装，是没查到」：{:?}",
            g.nice_to_have
        );
    }

    /// **没有契约 → Unknown，不是 No。**
    /// 「我们根本没查过它怎么跑」与「它跑不起来」是两件事，
    /// 报成 No 会让人以为项目坏了。
    #[test]
    fn 没有契约时是查不了而不是跑不了() {
        let f = facts_with(Lang::Python, vec![], None);
        let g = make_guide(&f, &[]);
        assert_eq!(g.ready, Verdict::Unknown);
        assert_eq!(g.blockers, vec![]);
        assert!(g.headline().contains("查不出来"), "{}", g.headline());
    }

    /// **契约在、但没写 run 命令 → 也是 Unknown**，并明说缺什么。
    #[test]
    fn 契约没写run命令时是查不了并说明缺什么() {
        let f = facts_with(Lang::Python, vec![], Some(py_contract(None)));
        let g = make_guide(&f, &[]);
        assert_eq!(g.ready, Verdict::Unknown);
        assert!(
            g.nice_to_have.iter().any(|s| s.contains("commands.run")),
            "{:?}",
            g.nice_to_have
        );
    }

    /// **"查不了"要说清是谁的**（2026-10-16 加）。
    ///
    /// 这条守着 `AGENTS.md` §2 那句判据：
    /// **「这是"用户的空白"，还是"我们的失败"？」**——
    /// 它原来只活在文档里，代码里 32 处 `Verdict::Unknown` 一处都没区分。
    ///
    /// 三个断言**分开钉**，因为它们会各自坏：
    ///   ① 没有契约 → **用户的空白**（他没提供判据的入口）；
    ///   ② 契约没写 run → **用户的空白**（`decide_ready` 的注释原话："这是用户没写"）；
    ///   ③ 分不清时**不许硬归**（`Unclear`，不是随便挑一头）。
    #[test]
    fn 查不了要说清是用户的空白还是我们的失败() {
        // ① 没有契约：用户没提供判据的入口
        let f = facts_with(Lang::Python, vec![], None);
        let g = make_guide(&f, &[]);
        assert_eq!(g.ready, Verdict::Unknown);
        assert_eq!(g.unknown_why, Unknown::TheirGap, "没有契约该归用户的空白");

        // ② 契约在、没写 run：也是用户没写
        let f2 = facts_with(Lang::Python, vec![], Some(py_contract(None)));
        let g2 = make_guide(&f2, &[]);
        assert_eq!(g2.ready, Verdict::Unknown);
        assert_eq!(g2.unknown_why, Unknown::TheirGap, "没写 run 该归用户的空白");

        // ③ 能跑的时候这个问题没有意义 → 不许给一个"看起来像答案"的值
        let f3 = facts_with(Lang::Python, vec![], Some(py_contract(Some("python main.py"))));
        let g3 = make_guide(&f3, &[]);
        assert!(matches!(g3.ready, Verdict::Yes(_)));
        assert_eq!(g3.unknown_why, Unknown::Unclear, "不是未知时不该硬给归属");
    }

    /// **两种"查不了"在界面上必须是两句不同的话**——
    /// 同一句话会让用户不知道该做什么（一种是"你补上"，一种是"不怪你"）。
    #[test]
    fn 两种查不了的说法必须不同() {
        let a = Unknown::TheirGap.what_to_do();
        let b = Unknown::OurGap.what_to_do();
        assert_ne!(a, b, "两种未知不能是同一句话");
        // 措辞要指向"谁该做什么"，不是复述"查不了"
        assert!(a.contains("你"), "用户的空白要指向用户：{a}");
        assert!(b.contains("我们") || b.contains("不怪你"), "我们的失败要承担责任：{b}");
    }

    /// 缺 Rust 工具链是**挡路的**，而且 fix 要提醒"重开终端"——
    /// 那是 rustup 最常见的坑（改了 PATH，已经开着的窗口读不到）。
    #[test]
    fn 缺rust工具链是挡路的且提醒重开终端() {        let mut f = facts_with(Lang::Rust, vec![], Some(py_contract(Some("cargo run"))));
        f.toolchain.insert("rustc".into(), ToolFact::default());
        let g = make_guide(&f, &[]);
        assert_eq!(g.ready, Verdict::No);
        assert!(g.blockers[0].what.contains("Rust 工具链"));
        assert!(g.blockers[0].fix.contains("重开"), "{}", g.blockers[0].fix);
    }

    /// 混合项目要说成混合的——**不能只报第一门语言**
    /// （Tauri 项目只报 Node 的事，就是第 13 轮那个缺陷的形状）。
    #[test]
    fn 混合项目说成混合的() {
        let mut f = facts_with(Lang::Web, vec![], None);
        f.langs = vec![Lang::Web, Lang::Rust];
        let g = make_guide(&f, &[]);
        assert!(g.what.contains("混合"), "{}", g.what);
        assert!(g.what.contains("Node") && g.what.contains("Rust"), "{}", g.what);
    }

    /// **上一步真跑出来的错，比任何推测都准**——它排在最前面。
    #[test]
    fn 上次运行的报错进挡路清单() {
        let f = facts_with(Lang::Python, vec![], Some(py_contract(Some("python main.py"))));
        let d = crate::launch::rundiag::RunDiagnostic {
            title: "ModuleNotFoundError: rich".into(),
            file: Some("main.py".into()),
        };
        let g = make_guide(&f, &[d]);
        assert_eq!(g.ready, Verdict::No);
        assert!(g.blockers[0].what.contains("ModuleNotFoundError"));
        assert!(
            g.blockers[0].verify.contains("main.py"),
            "有落点就要把落点说出来：{}",
            g.blockers[0].verify
        );
    }

    /// **"这里不是一个项目"必须与"你还没写契约"分开说**（2026-10-16 加）。
    ///
    /// ## 这条是拿什么换的
    ///
    /// 对 **Python 3.15 的标准库**（`Lib\`）跑 `guide` 时抓到的：
    /// `ready.state = unknown`、`why = their_gap`、`whatToDo` =
    /// 「**这一项你这边没有；补上就能判断**」——"这一项"指 `env.contract.json`。
    ///
    /// 可**标准库不该有契约**。照那句话去做，用户会往语言安装目录里塞一份契约，
    /// 换来一个"**假项目能跑**"的结论——**比不判断更坏**。
    ///
    /// 根因（外部独立判读也指到同一处）：工具唯一管"这是不是一个项目"的判据是
    /// 「直接子目录里 ≥2 个带项目标记」，而 `Lib` 的子目录一个标记都没有 → 那条不响。
    /// **没有任何规则管"语言安装目录"。**
    ///
    /// 三个断言分开钉：
    ///   ① 没有项目标记的目录 → `what_to_do` 要**明说"这里不像一个项目"**；
    ///   ② 那句话说出来了，就**不许再说"补上就能判断"**（那是两种不同的行动）；
    ///   ③ 而**有标记的**目录仍走老话（"你缺契约"）——别把这个修法蔓延成误报。
    #[test]
    fn 不是一个项目要与还没写契约分开说() {
        // ① 一个空目录：没有任何项目标记 → 不像项目
        let d = std::env::temp_dir().join(format!("jw-guide-nap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut f = facts_with(Lang::Python, vec![], None);
        f.project.root = d.to_string_lossy().to_string();
        let g = make_guide(&f, &[]);
        assert_eq!(g.ready, Verdict::Unknown, "没契约就该是查不了");
        let said = g
            .what_to_do
            .clone()
            .unwrap_or_else(|| g.unknown_why.what_to_do().to_string());
        assert!(
            said.contains("不像一个项目"),
            "空目录要说清'这里不像项目'，实际：{said}"
        );
        assert!(
            !said.contains("补上就能判断"),
            "既然说了'不是项目'，就不许再说'你补上就行'——那是两回事：{said}"
        );

        // ③ 有项目标记（package.json）的目录 → 仍走"你缺契约"
        std::fs::write(d.join("package.json"), "{}").unwrap();
        let g3 = make_guide(&f, &[]);
        let said3 = g3
            .what_to_do
            .clone()
            .unwrap_or_else(|| g3.unknown_why.what_to_do().to_string());
        assert!(
            !said3.contains("不像一个项目"),
            "有项目标记就不许说'不是项目'（这是误报）：{said3}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
