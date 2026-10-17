use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 稳定哈希（FNV-1a）：用于"变没变"的比较，不需要密码学强度。
pub fn stable_hash<I, S>(items: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut h: u64 = 0xcbf29ce484222325;
    for it in items {
        for b in it.as_ref().to_lowercase().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= b'\n' as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:012x}", h & 0xffff_ffff_ffff)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PathFacts {
    pub count: usize,
    pub hash: String,
    #[serde(default)]
    pub sample: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemFacts {
    pub os: String,
    pub machine: String,
    pub cwd: String,
    pub user: String,
    pub stdout_encoding: String,
    pub path: PathFacts,
    /// 只记白名单（可能影响行为的那几个），其余不落盘
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PythonFacts {
    pub found: bool,
    pub exe: String,
    pub version: String,
    pub isolated: bool,
    pub pth_files: Vec<String>,
    pub in_venv: bool,
    pub prefix: String,
    pub base_prefix: String,
    pub site_packages: Vec<String>,
    pub sys_path0: String,
    pub installed: Vec<String>,
    pub pip: bool,
    /// PATH 里第一个叫 python.exe 的东西。它经常不是真解释器，而是应用商店/
    /// 安装管理器的转发壳——于是「你在终端里敲 python」和「工具用哪个」是两回事。
    #[serde(default)]
    pub path_on_path: Option<String>,
    /// 位数：64 / 32。原生扩展对不上就一定加载失败。
    #[serde(default)]
    pub bits: u32,
    /// x86_64 / AMD64 / ARM64
    #[serde(default)]
    pub arch: String,
    /// 解释器自报的 ABI 标签，例如 cpython-315-x86_64
    #[serde(default)]
    pub abi: String,
    /// site-packages 的绝对路径们
    #[serde(default)]
    pub site_dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolFact {
    pub ok: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DepDetail {
    pub name: String,
    /// 清单里写的版本约束，例如 ">=2.0"、"==1.4.2"。空 = 清单里没写
    #[serde(default)]
    pub want: String,
    /// 机器上实际装着的版本。空 = 没装
    #[serde(default)]
    pub have: String,
    pub ok: bool,
    /// 这个语言没有可靠的确认手段（例如 C++：包可能来自系统 / vcpkg / Conan）。
    /// 这一档**不算未安装**——确认不了就说确认不了，不能报成缺失。
    #[serde(default)]
    pub unknown: bool,
    /// 这条是从哪个文件看出来的
    #[serde(default)]
    pub from: String,
    /// **"装没装"是从哪儿查的**——这条结论的证据（2026-10-16）。
    ///
    /// `AGENTS.md` §2 那条线要求"每条结论可反驳"：用户该能自己去核对。
    /// 而依赖这一栏原来**只有结论没有来源**——他说不出"我凭什么信你"，
    /// 也说不出"我去哪儿看才能自己确认"。
    ///
    /// 依赖的查法**四种语言各不相同**，所以这个字段不是套话：
    /// Rust 看 `Cargo.lock`、Web 看 `node_modules`、Java 看 `~/.m2/repository`、
    /// Python 看 `pip list` 的输出。**说出来用户就知道该去核实哪儿。**
    ///
    /// 没查过时是"没有可查的地方"——那是三态在文字上的落法：
    /// **"没查"与"没装"必须长得不一样。**
    #[serde(default)]
    pub evidence: String,
}

impl DepDetail {
    /// **`unknown` 是谁造成的**——`AGENTS.md` §2 那句判据的类型化。
    ///
    /// 原来这里只有一个 `unknown: bool`：它说得出"不知道"，
    /// **说不出"这个不知道该怪谁"**。而用户能做的事完全不同：
    ///   - 我们的失败 → 他可以换个办法，或者等我们修；
    ///   - 用户的空白 → 他补一个文件就好。
    ///
    /// 两句话混成一句，用户就不知道该做什么（这正是三态压扁的代价）。
    ///
    /// ## 分类的依据（逐个出口查过，不是猜的）
    ///
    /// `detect/deps.rs` 里能产出 `Unknown` 的**只有这四条路**，而**四条全在"我们"这边**：
    ///   1. `py_installed` 为空（**问工具的那一步没做**——有意跳过的快速路径）；
    ///   2. `Cargo.lock` **读不出来**（文件在，读失败）；
    ///   3. `Lang::Web` 且项目是 **PnP**（依赖可见性在 `.pnp.cjs`，
    ///      **解析它要跑 JS，不在这条路径上**）；
    ///   4. `Lang::Cpp`（包可能来自系统 / vcpkg / Conan，**我们没实现**）。
    ///
    /// 所以返回 `OurGap` **不是默认值，是这四条路的共同结论**。
    /// 哪天真出现"用户的空白"那一类未知（例如某个我们依赖的清单文件用户没提供），
    /// 那时要在**产出点**分化，**不是回来改这里猜**。
    ///
    /// ## 为什么 `!self.unknown` 返回 `Unclear` 而不是 `TheirGap`
    ///
    /// 不是未知的时候这个问题**没有意义**。返回一个"看起来像答案"的值会让调用方误用
    /// （把"已装"读成"用户的空白"）。用 `Unclear` 明确表示"这个问题不适用"。
    pub fn unknown_why(&self) -> crate::Unknown {
        if !self.unknown {
            return crate::Unknown::Unclear;
        }
        crate::Unknown::OurGap
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectFacts {
    pub root: String,
    pub exists: bool,
    pub contract: Option<super::contract::Contract>,
    pub contract_error: Option<String>,
    pub contract_hash: Option<String>,
    pub declared_deps: Vec<String>,
    /// 依赖的「声明 vs 实际」两列。依赖管理工具的价值就在把这两列并排摆出来。
    ///
    /// **这是依赖状态的唯一表示。** 这里原来还有一个 `deps_status:
    /// BTreeMap<String, bool>` 的布尔图，2026-10-16 删了——它的注释自己写着
    /// "⚠️ 会丢三态：禁止新代码使用（P3 复审发现）……清理计划：M2 接上 store 时……
    /// 然后删掉本字段"，而 **M2 早就接上了，这句计划过期了很久**。
    ///
    /// 它造成的真实损失记两次：
    ///   - 规则层：`Verdict::Unknown` 被压成 `false` → 命中契约 required 就连
    ///     `Severity::Error` 报出去，证据还写着"当前解释器已装 0 个包"（第 16 轮修）；
    ///   - `snapshot.rs` 的概览汇总行："一部分装上 + 一部分无法确认" → "依赖 都装了"（第 24 轮修）。
    ///
    /// **两次都是修消费者。第三次直接掐掉来源**：类型装不下三态，就不该存在。
    #[serde(default)]
    pub deps_detail: Vec<DepDetail>,
    pub venvs: Vec<String>,
    pub recent_files: Vec<String>,
    /// 项目体检：结构、依赖清单、忽略规则。快扫，毫秒级。
    #[serde(default)]
    pub hygiene: Option<super::project::ProjectScan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Facts {
    pub schema_version: u32,
    pub tool_version: String,
    pub taken_at: String,
    pub system: SystemFacts,
    /// 这个项目是哪门语言的。探测和规则都按它分派。
    #[serde(default = "unknown_lang")]
    pub lang: super::lang::Lang,
    /// 复合项目会不止一个：Java 后端 + Vue 前端就是一个仓库两门语言
    #[serde(default)]
    pub langs: Vec<super::lang::Lang>,
    /// 只有 Java 项目才会去填——打开 Python 项目不该 spawn java
    #[serde(default)]
    pub jvm: Option<super::jvm::JvmFacts>,
    #[serde(default)]
    pub cc: Option<super::ccpp::CcFacts>,
    /// 上一次跑这个项目时的内存情况。诊断规则靠它回答「为什么慢」。
    #[serde(default)]
    pub last_mem: Option<super::mem::MemFacts>,
    pub python: PythonFacts,
    pub toolchain: BTreeMap<String, ToolFact>,
    pub project: ProjectFacts,
}

fn unknown_lang() -> super::lang::Lang {
    super::lang::Lang::Unknown
}

impl Facts {
    pub fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.schema_version != SCHEMA_VERSION {
            p.push(format!(
                "快照 schema_version={} 与工具要求 {} 不一致",
                self.schema_version, SCHEMA_VERSION
            ));
        }
        if self.taken_at.is_empty() {
            p.push("缺 taken_at".into());
        }
        p
    }
}

/// 极简时间戳（本地时区偏移由系统给出），避免引入额外日期库。
pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    // 由 UTC 秒推出年月日（Howard Hinnant 的 civil_from_days 算法）
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mth <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, mth, d, h, m, s
    )
}
