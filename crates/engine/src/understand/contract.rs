use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Runtime {
    /// 版本范围，如 ">=3.10"（人看的）
    pub python: Option<String>,
    /// 具体解释器绝对路径（工具启动时用它，**不依赖 PATH、不需要 activate**）
    pub python_exe: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Deps {
    pub required: Vec<String>,
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Commands {
    pub install: Option<String>,
    /// 编译/打包这一步。和 install 分开：装依赖 ≠ 编译。
    pub build: Option<String>,
    pub run: Option<String>,
    pub test: Option<String>,
}

/// 「什么算启动成功」——新手最缺的判断依据
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ReadyWhen {
    /// 端口开始监听
    pub port: Option<u16>,
    /// 输出/日志里出现这段文字
    pub log_match: Option<String>,
    /// 一次性脚本：退出码等于它
    pub exit_code: Option<i32>,
    pub timeout_s: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Contract {
    pub schema_version: Option<u32>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub runtime: Runtime,
    pub deps: Deps,
    pub commands: Commands,
    /// 项目自己的日志文件（工具会 tail 它们，和进程输出合并显示）
    pub log_files: Vec<String>,
    pub ready_when: Option<ReadyWhen>,
    /// 人手改过的字段名（install / build / run / test / deps / ready_when …）。
    ///
    /// 为什么要有这个：自动推断出来的命令只是**草稿**，
    /// 而人敲进去的是**决定**。没有这张名单，下一次自动起草就会把
    /// 人改了半天的东西覆盖掉——那是这个工具最不该犯的错。
    #[serde(default)]
    pub manual: Vec<String>,
}

impl Contract {
    /// 与旧版本比，把**人手改过**的字段记下来。
    /// 规则很简单：值不一样 = 有人动过 = 以后自动起草不许覆盖它。
    pub fn mark_manual_against(&mut self, old: &Contract) {
        let hit = |name: &str, changed: bool, out: &mut Vec<String>| {
            if changed && !out.iter().any(|x| x == name) {
                out.push(name.to_string());
            }
        };
        let mut names = std::mem::take(&mut self.manual);
        hit("install", self.commands.install != old.commands.install, &mut names);
        hit("build", self.commands.build != old.commands.build, &mut names);
        hit("run", self.commands.run != old.commands.run, &mut names);
        hit("test", self.commands.test != old.commands.test, &mut names);
        hit("deps", self.deps.required != old.deps.required, &mut names);
        hit("log_files", self.log_files != old.log_files, &mut names);
        hit("ready_when", self.ready_when != old.ready_when, &mut names);
        // 旧的名单也要带上：这次没改，不代表上次改的不算
        for n in &old.manual {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
        self.manual = names;
    }

    /// 自动起草时，按名单把人气过的字段原样搬回来。
    /// 返回被保住的字段名，用来告诉用户"我这次没动你改过的东西"。
    pub fn keep_manual(&mut self, old: &Contract) -> Vec<String> {
        let mut kept = Vec::new();
        for name in &old.manual {
            match name.as_str() {
                "install" => { self.commands.install = old.commands.install.clone(); kept.push(name.clone()); }
                "build" => { self.commands.build = old.commands.build.clone(); kept.push(name.clone()); }
                "run" => { self.commands.run = old.commands.run.clone(); kept.push(name.clone()); }
                "test" => { self.commands.test = old.commands.test.clone(); kept.push(name.clone()); }
                "deps" => { self.deps = old.deps.clone(); kept.push(name.clone()); }
                "log_files" => { self.log_files = old.log_files.clone(); kept.push(name.clone()); }
                "ready_when" => { self.ready_when = old.ready_when.clone(); kept.push(name.clone()); }
                _ => {}
            }
        }
        self.manual = old.manual.clone();
        kept
    }
}

pub fn contract_path(root: &Path) -> PathBuf {
    root.join("env.contract.json")
}

impl Contract {
    /// 读契约。返回 (契约, 解析错误信息)：文件不存在时两者都是 None。
    pub fn load(root: &Path) -> (Option<Contract>, Option<String>) {
        let p = contract_path(root);
        if !p.is_file() {
            return (None, None);
        }
        match std::fs::read_to_string(&p) {
            Ok(text) => match serde_json::from_str::<Contract>(&text) {
                Ok(c) => (Some(c), None),
                Err(e) => (None, Some(format!("JSON 解析失败：{}", e))),
            },
            Err(e) => (None, Some(format!("读取失败：{}", e))),
        }
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let p = contract_path(root);
        let text = serde_json::to_string_pretty(self).context("序列化契约失败")?;
        std::fs::write(&p, text + "\n").with_context(|| format!("写入 {} 失败", p.display()))?;
        Ok(())
    }

    pub fn hash(&self) -> String {
        let text = serde_json::to_string(self).unwrap_or_default();
        crate::facts::stable_hash([text])
    }

    pub fn run_command(&self) -> Option<&str> {
        self.commands.run.as_deref()
    }

    /// 从 requirements.txt 读声明（没有就是空）
    pub fn declared_deps(root: &Path) -> Vec<String> {
        let p = root.join("requirements.txt");
        let Ok(text) = std::fs::read_to_string(&p) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('-') {
                continue;
            }
            let name: String = line
                .split(|c: char| "<>=!~[; ".contains(c))
                .next()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            if !name.is_empty() {
                out.push(name);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// requirements.txt 里每一行的「名字 + 版本约束」。
    /// 版本那一截刻意留着——依赖管理的全部意义就是「你要的是什么」和
    /// 「装上的是什么」对得上对不上，名字单独拎出来什么都说明不了。
    pub fn declared_deps_spec(root: &Path) -> Vec<(String, String)> {
        let p = root.join("requirements.txt");
        let Ok(text) = std::fs::read_to_string(&p) else {
            return Vec::new();
        };
        let mut out: Vec<(String, String)> = Vec::new();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('-') {
                continue;
            }
            let cut = line
                .find(|c: char| "<>=!~[; ".contains(c))
                .unwrap_or(line.len());
            let name = line[..cut].trim().to_lowercase();
            if name.is_empty() {
                continue;
            }
            // [security] 这种是「附加项」，不是版本；分号后面是环境标记。
            // 两个都切掉，剩下的才是「你要的那个版本」。
            let mut rest = line[cut..].trim();
            if rest.starts_with('[') {
                if let Some(i) = rest.find(']') {
                    rest = rest[i + 1..].trim();
                }
            }
            let spec = rest
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            out.push((name, spec));
        }
        out.sort();
        out.dedup();
        out
    }

    /// 要检查的依赖（合并 requirements.txt 与契约里的声明），带版本与出处
    pub fn wanted_deps_detail(
        root: &Path,
        contract: Option<&Contract>,
    ) -> Vec<(String, String, String)> {
        let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
        for (name, spec) in Contract::declared_deps_spec(root) {
            map.insert(name, (spec, "requirements.txt".to_string()));
        }
        if let Some(c) = contract {
            for d in c.deps.required.iter() {
                map.entry(d.to_lowercase())
                    .or_insert((String::new(), "env.contract.json".to_string()));
            }
            for d in c.deps.optional.iter() {
                map.entry(d.to_lowercase())
                    .or_insert((String::new(), "env.contract.json（可选）".to_string()));
            }
        }
        map.into_iter().map(|(k, (s, f))| (k, s, f)).collect()
    }

    /// 需要检查的依赖（契约里声明的 + requirements.txt 里的）
    pub fn wanted_deps(root: &Path, contract: Option<&Contract>) -> Vec<String> {
        let mut set: BTreeMap<String, ()> = BTreeMap::new();
        for d in Contract::declared_deps(root) {
            set.insert(d, ());
        }
        if let Some(c) = contract {
            for d in c.deps.required.iter().chain(c.deps.optional.iter()) {
                set.insert(d.to_lowercase(), ());
            }
        }
        set.into_keys().collect()
    }
}

/// 依赖名规范化（requirements 里写 Pillow，distributions 里也是 Pillow）
pub fn norm_name(s: &str) -> String {
    s.trim().to_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "jingwei-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn declared_deps_strips_specifiers_markers_and_comments() {
        let d = tmpdir("deps");
        std::fs::write(
            d.join("requirements.txt"),
            "# 注释行\nrich>=13.0.0\nPillow==10.0.0\nrequests[security] ; python_version<'3.9'\n-r other.txt\n\n",
        )
        .unwrap();
        assert_eq!(
            Contract::declared_deps(&d),
            vec!["pillow".to_string(), "requests".to_string(), "rich".to_string()]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn declared_deps_is_empty_without_the_file() {
        let d = tmpdir("nodeps");
        assert!(Contract::declared_deps(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn contract_round_trips_and_hash_is_stable() {
        let d = tmpdir("roundtrip");
        assert!(Contract::load(&d).0.is_none());
        let c = Contract {
            schema_version: Some(1),
            name: Some("t".into()),
            ..Default::default()
        };
        c.save(&d).unwrap();
        let (back, err) = Contract::load(&d);
        assert!(err.is_none());
        let back = back.expect("应该能读回来");
        assert_eq!(back.name.as_deref(), Some("t"));
        assert_eq!(back.hash(), c.hash());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_json_is_reported_not_panicked() {
        let d = tmpdir("broken");
        std::fs::write(contract_path(&d), "{ nah").unwrap();
        let (c, err) = Contract::load(&d);
        assert!(c.is_none());
        assert!(err.is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_run_command_is_none_not_empty_string() {
        let c = Contract::default();
        assert_eq!(c.run_command(), None);
    }

    #[test]
    fn norm_name_matches_the_packaging_spec() {
        assert_eq!(norm_name("Pillow"), "pillow");
        assert_eq!(norm_name("scikit-learn"), "scikit_learn");
        assert_eq!(norm_name("  Flask "), "flask");
    }

    /// 依赖管理工具的全部意义就在这一条测试上：「声明」和「实际」要能对上号。
    /// 版本约束得留住（不然「装了个 1.0 而你要的是 2.0」看不出来），
    /// 附加项和环境标记得切掉（它们不是版本，混进来就会误判成「版本不对」）。
    /// **人改过的东西不许被自动起草覆盖。** 这条是这个工具的信用底线：
    /// 覆盖一次，用户就再也不敢改第二回。
    #[test]
    fn manual_fields_survive_a_redraft() {
        let mut old = Contract::default();
        old.commands.run = Some("python -m myapp".into());
        old.commands.test = Some("pytest -q".into());
        old.manual = vec!["run".into()];     // 只有 run 是人改的

        let mut fresh = Contract::default();
        fresh.commands.run = Some("python main.py".into());     // 推断出来的
        fresh.commands.test = Some("python main.py".into());
        let kept = fresh.keep_manual(&old);

        assert_eq!(kept, vec!["run".to_string()], "只有名单里的字段该被保住");
        assert_eq!(fresh.commands.run.as_deref(), Some("python -m myapp"));
        assert_eq!(fresh.commands.test.as_deref(), Some("python main.py"), "没人改的字段该跟着新推断走");
        assert_eq!(fresh.manual, vec!["run".to_string()], "名单要传下去");
    }

    /// 改了哪个字段，就记哪个字段而已——不要一改就全锁死。
    #[test]
    fn only_changed_fields_are_marked_manual() {
        let mut old = Contract::default();
        old.commands.run = Some("cargo run".into());
        old.commands.test = Some("cargo test".into());
        let mut now = old.clone();
        now.commands.run = Some("cargo run --release".into());
        now.mark_manual_against(&old);
        assert_eq!(now.manual, vec!["run".to_string()]);
    }

    #[test]
    fn dep_specs_keep_the_version_and_drop_the_noise() {
        let d = tmpdir("deps-spec");
        std::fs::write(
            d.join("requirements.txt"),
            "# 注释\nrich>=13.0.0\nPillow==10.0.0\nrequests[security] ; python_version<'3.9'\n-r other.txt\n\n",
        )
        .unwrap();
        assert_eq!(
            Contract::declared_deps_spec(&d),
            vec![
                ("pillow".to_string(), "==10.0.0".to_string()),
                ("requests".to_string(), "".to_string()),
                ("rich".to_string(), ">=13.0.0".to_string()),
            ]
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

