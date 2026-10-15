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
    #[serde(default)]
    pub deps_detail: Vec<DepDetail>,
    /// ⚠️ **会丢三态：禁止新代码使用**（P3 复审发现）。
    ///
    /// 它是 `bool`，所以"无法确认"（例如 C++ 的包可能来自系统/vcpkg/Conan）
    /// 到这儿就被压成 `false`，在规则判断与界面里等价于"缺失"——**这就是安静地错**。
    /// 正确的入口是 `deps_detail`（三态：ok / unknown / 缺失，带来源）。
    /// 清理计划：M2 接上 store 时，规则引擎与 Tauri 输出一起改走 `deps_detail`，然后删掉本字段。
    pub deps_status: BTreeMap<String, bool>,
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
