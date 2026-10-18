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
            Ok(text) => {
                // **先去掉 UTF-8 BOM**（2026-10-18 实测换的）：
                //
                // Windows 上记事本、PowerShell 5.1 的 `Set-Content -Encoding utf8`
                // 默认都写 BOM，而 `serde_json` 见到 BOM 报的是
                // 「expected value at line 1 column 1」——**这个提示跟"文件里多了三个字节"
                // 完全对不上**，用户会以为契约写错了。
                //
                // BOM 不是语义错误（UTF-8 的签名），我们自己的打包与文档也被它坑过三次。
                let text = crate::util::bom::strip_bom(&text);
                match serde_json::from_str::<Contract>(text) {
                    Ok(c) => (Some(c), None),
                    // 解析失败时把"第一行长什么样"带上——BOM/编码问题一眼能看出来。
                    Err(e) => {
                        let head: String = text.chars().take(40).collect();
                        (None, Some(format!("JSON 解析失败：{e}（开头是：{head:?}）")))
                    }
                }
            }
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
        let raw_text = std::fs::read_to_string(&p).unwrap_or_default();

        // **先去掉 BOM**（2026-10-18 实测）：带 BOM 的 requirements.txt，
        // 第一条依赖会变成 `\u{feff}flask`——**永远匹配不上已安装**，长期误报"缺 flask"。
        let text = crate::util::bom::strip_bom(&raw_text);
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
        // **再补上 `pyproject.toml` 的 `[project] dependencies`**（2026-10-18）：
        // 现代 Python 项目常常只有 pyproject、没有 requirements.txt（`flask` 就是）。
        for (name, _) in Self::pyproject_deps_spec(root) {
            out.push(name);
        }
        out.sort();
        out.dedup();
        out
    }

    /// 从 `pyproject.toml` 的 `[project] dependencies` 读声明（2026-10-18 加）。
    ///
    /// ## 为什么必须有它
    ///
    /// 拿真的 `pallets/flask` 跑 `doctor`：它 **一个依赖都不报**——因为 flask 的依赖写在
    /// `pyproject.toml` 里，而我们只读 `requirements.txt`。
    /// 那不是"这个项目没有依赖"，那是"**我们没读**"。三态不许压扁。
    ///
    /// ## 为什么自己解析而不是用 toml crate
    ///
    /// 这里只要一件事：`[project]` 段里的 `dependencies = [ "a>=1", "b" ]` 列表。
    /// 为一个键引一个依赖（含它的版本兼容面）不划算。**边界写清**：
    /// - 只认 `[project]` 段（到下一个 `[` 开头的段头为止）；
    /// - 只认 `dependencies` 这个键，**不认** `optional-dependencies` / `build-system.requires`；
    /// - 支持跨多行与单行两种写法；
    /// - 解析不出来的行**跳过**，不猜（宁可少报，不许瞎报）。
    pub fn pyproject_deps_spec(root: &Path) -> Vec<(String, String)> {
        let p = root.join("pyproject.toml");
        let Ok(raw) = std::fs::read_to_string(&p) else {
            return Vec::new();
        };
        let text = crate::util::bom::strip_bom(&raw);
        let mut out: Vec<(String, String)> = Vec::new();
        let mut in_project = false;
        let mut collecting = false;
        let mut buf = String::new();
        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.starts_with('[') {
                // 段头：`[project]` 进去，别的段一律不认。
                collecting = false;
                in_project = line.replace(' ', "") == "[project]";
                continue;
            }
            if !in_project {
                continue;
            }
            if !collecting {
                let Some(eq) = line.find('=') else { continue };
                if line[..eq].trim() != "dependencies" {
                    continue;
                }
                let rest = line[eq + 1..].trim();
                if !rest.starts_with('[') {
                    continue; // `dependencies = { … }` 这种不认，跳过
                }
                collecting = true;
                buf.push_str(rest);
            } else {
                buf.push_str(line);
            }
            if !buf.contains(']') {
                continue;
            }
            collecting = false;
            // 把这一段里所有 `"名字>=版本"` 抠出来。
            if let Some(open) = buf.find('[') {
                if let Some(close) = buf[open..].find(']') {
                    let body = &buf[open + 1..open + close];
                    for piece in body.split(',') {
                        let spec = piece.trim().trim_matches('"').trim_matches('\'').trim();
                        if spec.is_empty() || spec.starts_with('#') {
                            continue;
                        }
                        let cut = spec
                            .find(|c: char| "<>=!~[; ".contains(c))
                            .unwrap_or(spec.len());
                        let name = spec[..cut].trim().to_lowercase();
                        if name.is_empty() {
                            continue;
                        }
                        let want = spec[cut..].trim().to_string();
                        out.push((name, want));
                    }
                }
            }
            buf.clear();
        }
        out
    }

    /// requirements.txt 里每一行的「名字 + 版本约束」。
    /// 版本那一截刻意留着——依赖管理的全部意义就是「你要的是什么」和
    /// 「装上的是什么」对得上对不上，名字单独拎出来什么都说明不了。
    pub fn declared_deps_spec(root: &Path) -> Vec<(String, String)> {
        let p = root.join("requirements.txt");
        let raw_text = std::fs::read_to_string(&p).unwrap_or_default();

        // **先去掉 BOM**（2026-10-18 实测）：带 BOM 的 requirements.txt，
        // 第一条依赖会变成 `\u{feff}flask`——**永远匹配不上已安装**，长期误报"缺 flask"。
        let text = crate::util::bom::strip_bom(&raw_text);
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
        // **`pyproject.toml` 也算声明**（2026-10-18）：否则 `flask` 这种项目会"一个依赖都没有"。
        for nw in Self::pyproject_deps_spec(root) {
            out.push(nw);
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

    /// **带 UTF-8 BOM 的契约要能读**（2026-10-18 实测换的）。
    ///
    /// Windows 上记事本、PowerShell 5.1 的 `Set-Content -Encoding utf8` **默认写 BOM**。
    /// 而 `serde_json` 见到 BOM 报的是「expected value at line 1 column 1」——
    /// 用户看到这句会以为是自己契约写错了，其实只是**文件头多了三个字节**。
    ///
    /// 实测来源：我拿 PowerShell 造夹具去测"契约里的构建命令有没有被执行"，
    /// 结果产品报"读不了契约"，查了半天才发现是 BOM——**夹具的错，但它暴露了产品该容忍这件事**。
    #[test]
    fn 带bom的契约也能读() {
        let d = std::env::temp_dir().join(format!("dd-contract-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let body = r#"{"schema_version":1,"name":"b","runtime":{"python":null,"python_exe":null},"deps":{"required":[],"optional":[]},"commands":{"build":"cargo build"},"ready_when":{"exit_code":0}}"#;
        // 手写 BOM：EF BB BF
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(body.as_bytes());
        std::fs::write(d.join("env.contract.json"), &bytes).unwrap();

        let (c, err) = Contract::load(&d);
        assert!(err.is_none(), "带 BOM 不该报错：{:?}", err);
        let c = c.expect("带 BOM 的契约要能读出来");
        assert_eq!(c.commands.build.as_deref(), Some("cargo build"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **真读不了的时候，要把"开头长什么样"带上**（2026-10-18）。
    ///
    /// 光一句「expected value at line 1 column 1」，用户和我们自己都不知道发生了什么。
    #[test]
    fn 读不了的时候要说清开头() {
        let d = std::env::temp_dir().join(format!("dd-contract-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("env.contract.json"), "这不是 JSON").unwrap();

        let (c, err) = Contract::load(&d);
        assert!(c.is_none());
        let err = err.expect("该报错");
        assert!(err.contains("开头是"), "要把开头带上：{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **带 BOM 的 `requirements.txt`，第一条依赖不许带上不可见字符**（2026-10-18 实测）。
    ///
    /// 实测：带 BOM 时第一条读成 `\u{feff}flask`，**永远匹配不上已安装**，
    /// 于是工具长期误报"缺 flask"（界面上那个 BOM 是看不见的）。
    #[test]
    fn 带bom的requirements也读得出干净的依赖名() {
        let d = std::env::temp_dir().join(format!("dd-req-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"flask==2.0.0\nrequests>=2.25\n");
        std::fs::write(d.join("requirements.txt"), &bytes).unwrap();

        let deps = Contract::declared_deps(&d);
        assert_eq!(deps, vec!["flask".to_string(), "requests".to_string()], "依赖名要干净：{deps:?}");
        let spec = Contract::declared_deps_spec(&d);
        assert!(spec.iter().any(|(n, _)| n == "flask"), "名字+版本那条也要干净：{spec:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **只有 `pyproject.toml` 的项目也要读得出依赖**（2026-10-18，拿真 flask 换的）。
    ///
    /// 修前：`pallets/flask` 的 `doctor` **一个依赖都不报**——因为它的依赖在 `pyproject.toml`，
    /// 而我们只读 `requirements.txt`，**读不到就早退成空**。
    /// 那不是"没有依赖"，那是"我们没读"——三态压扁。
    #[test]
    fn pyproject里的依赖要读出来() {
        let d = std::env::temp_dir().join(format!("dd-pyproj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("pyproject.toml"),
            "[build-system]\nrequires = [\"setuptools>=61\"]\n\n[project]\nname=\"t\"\ndependencies = [\n    \"flask>=3.0\",\n    \"requests\",\n]\n\n[project.optional-dependencies]\ndev = [\"pytest\"]\n",
        )
        .unwrap();

        let got = Contract::declared_deps_spec(&d);
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["flask", "requests"], "只认 [project] dependencies：{got:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **BOM 也挡不住它**（两个坑会叠在一起：Windows 编辑器 + pyproject）。
    #[test]
    fn 带bom的pyproject也读得出依赖() {
        let d = std::env::temp_dir().join(format!("dd-pyproj-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"[project]\ndependencies = [\"click\"]\n");
        std::fs::write(d.join("pyproject.toml"), &bytes).unwrap();

        assert_eq!(Contract::declared_deps(&d), vec!["click".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }
}

