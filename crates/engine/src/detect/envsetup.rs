//! 环境配置：造虚拟环境、把它绑进契约、装依赖。
//!
//! 关键点：绑进契约之后，`run` / `doctor` 用的是绝对路径的解释器，
//! 不需要 activate、不依赖 PATH。这对新手是决定性的——
//! 「激活没生效」这类问题从此不存在。

use crate::contract::Contract;
use crate::probe;
use crate::winutil::run_cmd;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// 有 uv 就用 uv：快，而且它装的解释器在它自己的目录里
    Uv,
    /// 标准库的 venv 模块
    Stdlib,
    /// 两条路都走不通
    None,
}

impl Method {
    pub fn id(&self) -> &'static str {
        match self {
            Method::Uv => "uv",
            Method::Stdlib => "python -m venv",
            Method::None => "不可用",
        }
    }
}

/// 这台解释器能怎么造虚拟环境——问它自己，不猜。
pub fn method_for(python: &Path) -> Method {
    if crate::winutil::which("uv.exe").is_some() {
        return Method::Uv;
    }
    match run_cmd(
        python,
        &["-c", "import venv, ensurepip"],
        Duration::from_secs(20),
    ) {
        Some((true, _)) => Method::Stdlib,
        _ => Method::None,
    }
}

/// 造不出来时给出的解释。嵌入式/便携版发行版天生没有 venv 和 ensurepip，
/// 这是本机最容易撞上的一种情况，必须说清楚而不是甩一句「失败了」。
pub fn why_unavailable(python: &Path, f: &crate::facts::PythonFacts) -> String {
    let embedded = f.isolated || !f.pth_files.is_empty();
    let mut s = format!("{} 不能建虚拟环境", python.display());
    if embedded {
        s.push_str("：它是嵌入式/便携发行版，按设计就不带 venv 和 ensurepip 模块。");
        s.push_str("\n    两条出路：① 装 uv（uv python install 3.12），让工具用它建环境；");
        s.push_str("\n              ② 从 python.org 装一个标准版 CPython，再用 jingwei env init --python 指过去。");
    } else {
        s.push_str("：解释器里找不到 venv / ensurepip 模块。");
        s.push_str("\n    先跑 python -c \"import venv, ensurepip\" 看它自己怎么说。");
    }
    s
}

pub struct Created {
    pub venv_dir: PathBuf,
    pub python_exe: PathBuf,
    pub method: Method,
    pub log: String,
}

pub fn create_venv(root: &Path, python: &Path, name: &str) -> Result<Created> {
    let dir = if Path::new(name).is_absolute() {
        PathBuf::from(name)
    } else {
        root.join(name)
    };
    if dir.join("pyvenv.cfg").is_file() {
        let exe = venv_exe(&dir);
        if exe.is_file() {
            return Ok(Created {
                venv_dir: dir,
                python_exe: exe,
                method: Method::Stdlib,
                log: "已经存在，直接复用".into(),
            });
        }
    }

    let method = method_for(python);
    let (log, ok) = match method {
        Method::Uv => {
            let Some(uv) = crate::winutil::which("uv.exe") else {
                bail!("uv 在「查到」和「调用」之间不见了，再试一次");
            };
            let r = run_cmd(
                &uv,
                &[
                    "venv",
                    &dir.to_string_lossy(),
                    "--python",
                    &python.to_string_lossy(),
                ],
                Duration::from_secs(300),
            );
            ("uv venv".to_string(), r.map(|(ok, _)| ok).unwrap_or(false))
        }
        Method::Stdlib => {
            let r = run_cmd(
                python,
                &["-m", "venv", &dir.to_string_lossy()],
                Duration::from_secs(300),
            );
            ("python -m venv".to_string(), r.map(|(ok, _)| ok).unwrap_or(false))
        }
        Method::None => {
            let f = probe::probe_python(python);
            bail!("{}", why_unavailable(python, &f))
        }
    };

    let exe = venv_exe(&dir);
    if !ok || !exe.is_file() {
        bail!(
            "{} 没成功。手跑一次看完整报错：{} venv {}",
            log,
            method.id(),
            dir.display()
        );
    }
    Ok(Created {
        venv_dir: dir,
        python_exe: exe,
        method,
        log,
    })
}

pub fn venv_exe(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("Scripts").join("python.exe")
    } else {
        dir.join("bin").join("python")
    }
}

/// 把解释器绝对路径写进契约——这一步做完，PATH 和 activate 就与这个项目无关了。
pub fn bind(root: &Path, exe: &Path) -> Result<()> {
    if !exe.is_file() {
        bail!("{} 不是个可执行文件", exe.display());
    }
    let (mut ct, err) = Contract::load(root);
    if let Some(e) = err {
        bail!("契约读不了，先修好它：{}", e);
    }
    let mut ct = ct.take().unwrap_or(Contract {
        schema_version: Some(1),
        name: root
            .file_name()
            .map(|s| s.to_string_lossy().to_string()),
        ..Default::default()
    });
    ct.runtime.python_exe = Some(exe.to_string_lossy().to_string());
    ct.save(root).context("写回契约失败")?;
    Ok(())
}

#[derive(Debug, Default, Clone)]
pub struct PyStatus {
    pub path: String,
    pub version: String,
    pub isolated: bool,
    pub in_venv: bool,
    pub has_pip: bool,
}

pub fn describe(exe: &Path) -> PyStatus {
    let f = probe::probe_python(exe);
    PyStatus {
        path: exe.to_string_lossy().to_string(),
        version: f.version,
        isolated: f.isolated || !f.pth_files.is_empty(),
        in_venv: f.in_venv,
        has_pip: f.pip,
    }
}
