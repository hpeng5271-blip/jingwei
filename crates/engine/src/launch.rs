//! 启动：6 阶段 + 流式输出 + 就绪判定。
//! 事件走回调（CLI 打印 / Tauri 推前端），逻辑层不打印任何东西。

use crate::contract::{Contract, ReadyWhen};
use crate::runs;
use crate::facts::now_iso;
use crate::probe;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Stdout,
    Stderr,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    Ok,
    Note,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Stage {
        index: usize,
        total: usize,
        title: String,
        status: StageStatus,
        detail: String,
        ms: u128,
    },
    Line {
        source: Source,
        text: String,
    },
    Ready {
        how: String,
        ms: u128,
    },
    Exited {
        code: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Run,
    /// 装依赖、跑测试这类「跑完就结束」的动作，进度措辞不一样
    Task,
}

pub struct LaunchOptions {
    pub root: PathBuf,
    pub run_override: Option<String>,
    pub extra_args: Vec<String>,
    pub timeout: Duration,
    pub purpose: Purpose,
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub exit_code: i32,
    pub ready: Option<String>,
    pub lines: usize,
    pub pid: Option<u32>,
    pub started_at: String,
    /// 进程异常退出时，从它自己的话里读出来的症结。读不出来就是 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnosis: Option<Diagnosis>,
    /// 这次跑的时候内存怎么样。只在采到东西时才有。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<crate::mem::MemReport>,
    /// 和这个项目上一次跑同一条命令相比，哪里变了。空 = 没变化或没法比。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_delta: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    /// 一句话说清是什么病
    pub crux: String,
    /// 往哪儿走
    pub hint: String,
}

const TOTAL_STAGES: usize = 6;

pub fn launch(opts: &LaunchOptions, cb: &mut dyn FnMut(Event)) -> Result<Outcome> {
    let root = opts.root.clone();
    let began = Instant::now();
    let (contract, contract_err) = Contract::load(&root);
    crate::dlog::i(
        "2000",
        &format!(
            "root={} cmd={} task={}",
            root.display(),
            opts.run_override.clone().unwrap_or_else(|| "（按契约）".into()),
            opts.purpose == Purpose::Task
        ),
    );

    // ---- 1 检查运行环境 ----
    let t = Instant::now();
    let picked = probe::first_working(&root, contract.as_ref(), 5);
    let py_path = picked.as_ref().map(|(p, _)| p.clone());
    let pyf = picked.map(|(_, f)| f).unwrap_or_default();
    // 契约明确绑了一个解释器，而它不在了——工具必须说出来。
    // 悄悄换一个更省事，但那等于把用户写下的决定吞掉：
    // 他以为自己在用 A，实际跑的是 B，这种误会最难查。
    let bound_gone = contract
        .as_ref()
        .and_then(|c| c.runtime.python_exe.as_ref())
        .filter(|b| !Path::new(b).is_file())
        .cloned();
    let isolated = pyf.isolated || !pyf.pth_files.is_empty();
    let (s1, d1) = if !pyf.found {
        (StageStatus::Fail, "没找到可用的 Python 解释器".to_string())
    } else if let Some(gone) = &bound_gone {
        (
            StageStatus::Note,
            format!(
                "契约绑定的解释器不在了（{}），临时改用 {}",
                gone, pyf.exe
            ),
        )
    } else if isolated {
        (
            StageStatus::Note,
            format!("Python {} · {} · 嵌入式（隔离模式）", pyf.version, pyf.exe),
        )
    } else {
        (
            StageStatus::Ok,
            format!("Python {} · {}", pyf.version, pyf.exe),
        )
    };
    cb(Event::Stage {
        index: 1,
        total: TOTAL_STAGES,
        title: "检查运行环境".into(),
        status: s1,
        detail: d1,
        ms: t.elapsed().as_millis(),
    });

    // ---- 2 校验项目契约 ----
    let t = Instant::now();
    let (s2, d2) = match (&contract, &contract_err) {
        (Some(c), _) => (
            StageStatus::Ok,
            format!(
                "{} · run: {}",
                c.name.clone().unwrap_or_else(|| "（未命名）".into()),
                c.run_command().unwrap_or("（未声明）")
            ),
        ),
        (None, Some(e)) => (StageStatus::Fail, format!("解析失败：{}", e)),
        (None, None) => (
            StageStatus::Note,
            "没有 env.contract.json（用 jingwei init 生成）".into(),
        ),
    };
    cb(Event::Stage {
        index: 2,
        total: TOTAL_STAGES,
        title: "校验项目契约".into(),
        status: s2,
        detail: d2,
        ms: t.elapsed().as_millis(),
    });

    // ---- 3 解析依赖声明 ----
    let t = Instant::now();
    let wanted = Contract::wanted_deps(&root, contract.as_ref());
    let installed_norm: Vec<String> = pyf
        .installed
        .iter()
        .map(|s| crate::contract::norm_name(s.split("==").next().unwrap_or("")))
        .collect();
    let missing: Vec<String> = wanted
        .iter()
        .filter(|d| !installed_norm.contains(&crate::contract::norm_name(d)))
        .cloned()
        .collect();
    cb(Event::Stage {
        index: 3,
        total: TOTAL_STAGES,
        title: format!("解析依赖声明（{} 项）", wanted.len()),
        status: if missing.is_empty() {
            StageStatus::Ok
        } else {
            StageStatus::Note
        },
        detail: if missing.is_empty() {
            "都已安装".into()
        } else {
            format!("缺 {} 项：{}", missing.len(), missing.join(", "))
        },
        ms: t.elapsed().as_millis(),
    });

    // ---- 4 准备解释器 ----
    let t = Instant::now();
    let exe = py_path.clone().ok_or_else(|| anyhow!("没有可用的 Python，无法启动"))?;
    cb(Event::Stage {
        index: 4,
        total: TOTAL_STAGES,
        title: "准备解释器".into(),
        status: StageStatus::Ok,
        detail: format!("{}（绝对路径，不需要 activate）", exe.display()),
        ms: t.elapsed().as_millis(),
    });

    // ---- 5 启动进程 ----
    let run_cmd = opts
        .run_override
        .clone()
        .or_else(|| contract.as_ref().and_then(|c| c.run_command().map(String::from)))
        .ok_or_else(|| anyhow!("契约里没有声明 run 命令（用 jingwei init 生成一份）"))?;
    let (program, args) = split_command(&run_cmd, &exe);
    let t = Instant::now();
    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .args(&opts.extra_args)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONPATH", root.to_string_lossy().to_string());
    // 桌面外壳拉起 python 时也会弹一个黑框；工具自己负责跑它，不需要窗口
    crate::winutil::hide_console(&mut cmd);
    let mut child = cmd.spawn().with_context(|| format!("拉起失败：{}", run_cmd))?;
    let pid = child.id();
    // 把子进程收进一个 Job：跑完/超时/用户停止时，连它的子孙一起收掉。
    // 不做这件事，`cargo run` 拉起的编译器/链接器会成为孤儿继续占端口和内存。
    let job = match crate::jobobj::Job::new() {
        Ok(j) => {
            if let Err(e) = j.assign(&child) {
                cb(Event::Stage {
                    index: 5,
                    total: TOTAL_STAGES,
                    title: "启动进程".into(),
                    status: StageStatus::Note,
                    detail: format!("进程已拉起，但没能收进 Job（{}）：结束时会尽力杀，可能留下子进程", e),
                    ms: 0,
                });
            }
            Some(j)
        }
        Err(_) => None,
    };
    // 内存只在跑东西的时候采：200ms 一次，跑完就停。
    // 常驻监控看着专业，实际没人在编译时盯着它，却要一直付资源。
    let mut mem = crate::mem::Sampler::new(pid);
    let mut next_mem_sample = Instant::now();

    let (tx, rx) = mpsc::channel::<(Source, String)>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(spawn_reader(out, Source::Stdout, tx.clone()));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(spawn_reader(err, Source::Stderr, tx.clone()));
    }
    drop(tx);

    cb(Event::Stage {
        index: 5,
        total: TOTAL_STAGES,
        title: if opts.purpose == Purpose::Task {
            "执行命令".into()
        } else {
            "启动进程".into()
        },
        status: StageStatus::Ok,
        detail: format!("{} {}", program.display(), args.join(" ")),
        ms: t.elapsed().as_millis(),
    });

    // ---- 6 等待就绪 ----
    let declared = contract.as_ref().and_then(|c| c.ready_when.clone());
    let default_ready = declared.is_none();
    // 契约没声明「什么算成功」时，给一个最保守的默认：进程正常退出就是成功。
    // 常驻服务不会被误判成成功——它压根不退，所以到点还是会报「没等到就绪」。
    let ready = declared.or(Some(ReadyWhen {
        exit_code: Some(0),
        timeout_s: Some(30),
        ..Default::default()
    }));
    let timeout_s = ready
        .as_ref()
        .and_then(|r| r.timeout_s)
        .unwrap_or(opts.timeout.as_secs());
    let deadline = Instant::now() + Duration::from_secs(timeout_s);
    let mut log_state: Vec<(PathBuf, u64)> = contract
        .as_ref()
        .map(|c| c.log_files.iter().map(|f| (root.join(f), 0u64)).collect())
        .unwrap_or_default();

    let t = Instant::now();
    let mut lines = 0usize;
    let mut ready_how: Option<String> = None;
    let mut exit_code: Option<i32> = None;

    // 留一截最近的输出（只要尾部）：进程挂了之后，症结就写在这一截里。
    // 400 行够装下任何一段 Python traceback，也不会把内存拖起来。
    let mut tail: VecDeque<String> = VecDeque::new();
    let mut stage_recs: Vec<runs::StageRec> = Vec::new();
    let mut emit = |cb: &mut dyn FnMut(Event), ev: Event| {
        if let Event::Line { text, .. } = &ev {
            tail.push_back(text.clone());
            while tail.len() > 400 {
                tail.pop_front();
            }
        }
        if let Event::Stage {
            index,
            title,
            status,
            detail,
            ms,
            ..
        } = &ev
        {
            stage_recs.push(runs::StageRec {
                index: *index,
                title: title.clone(),
                status: format!("{:?}", status).to_lowercase(),
                detail: detail.clone(),
                ms: *ms,
            });
        }
        cb(ev);
    };

    // 用 recv_timeout 而不是「睡一会儿再看一眼」：
    // 读线程一拿到行就推过来，输出延迟从最多 100ms 降到接近 0，
    // 空闲时也只是在等定时器，不做无谓的旋转。
    const TICK: Duration = Duration::from_millis(40);
    loop {
        match rx.recv_timeout(TICK) {
            Ok((src, text)) => {
                lines += 1;
                if ready_how.is_none() {
                    if let Some(m) = ready.as_ref().and_then(|r| r.log_match.clone()) {
                        if text.contains(&m) {
                            ready_how = Some(format!("输出里出现「{}」", m));
                        }
                    }
                }
                emit(cb, Event::Line { source: src, text });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {}
        }
        while let Ok((src, text)) = rx.try_recv() {
            lines += 1;
            if ready_how.is_none() {
                if let Some(m) = ready.as_ref().and_then(|r| r.log_match.clone()) {
                    if text.contains(&m) {
                        ready_how = Some(format!("输出里出现「{}」", m));
                    }
                }
            }
            emit(cb, Event::Line { source: src, text });
        }

        for (path, off) in log_state.iter_mut() {
            if let Ok(mut fh) = std::fs::File::open(&path) {
                let len = fh.metadata().map(|m| m.len()).unwrap_or(0);
                if len > *off {
                    // 定位失败会让这一轮从文件开头读——日志行会重复出现，看的人
                    // 会以为程序反复启动了。不能静默。
                    if let Err(e) = fh.seek(SeekFrom::Start(*off)) {
                        crate::dlog::w("9005", &format!("日志定位失败（{}）：{e}", path.display()));
                    }
                    let mut buf = String::new();
                    if fh.read_to_string(&mut buf).is_ok() {
                        for l in buf.lines() {
                            lines += 1;
                            if ready_how.is_none() {
                                if let Some(m) = ready.as_ref().and_then(|r| r.log_match.clone()) {
                                    if l.contains(&m) {
                                        ready_how = Some(format!("日志「{}」里出现该文字", m));
                                    }
                                }
                            }
                            emit(cb, Event::Line {
                                source: Source::Log,
                                text: l.to_string(),
                            });
                        }
                    }
                    *off = len;
                }
            }
        }

        if ready_how.is_none() {
            if let Some(p) = ready.as_ref().and_then(|r| r.port) {
                if std::net::TcpStream::connect(("127.0.0.1", p)).is_ok() {
                    ready_how = Some(format!("端口 {} 已监听", p));
                }
            }
        }

        if let Some(st) = child.try_wait()? {
            exit_code = Some(st.code().unwrap_or(-1));
            break;
        }
        if ready_how.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        if Instant::now() >= next_mem_sample {
            mem.tick();
            next_mem_sample = Instant::now() + Duration::from_millis(40);
        }
    }

    // 判定要立刻给，不能等子进程退出——常驻服务可能永远不退出，
    // 那样用户就只能在「什么都没说」的状态下干等。输出照旧继续往下流。
    if ready_how.is_none() {
        if let (Some(code_now), Some(want)) = (
            exit_code,
            ready.as_ref().and_then(|r| r.exit_code),
        ) {
            if code_now == want {
                ready_how = Some(format!("退出码 {}", code_now));
            }
        }
    }

    let (s6, d6) = match (&ready_how, exit_code) {
        (Some(how), _) => (
            StageStatus::Ok,
            if default_ready {
                format!("已就绪 · {}（契约没写 ready_when，按默认判定）", how)
            } else {
                format!("已就绪 · {}", how)
            },
        ),
        (None, None) => (
            StageStatus::Fail,
            format!(
                "{} 秒内没等到就绪条件（进程还在跑，输出继续跟着；契约里的 ready_when 要写清楚「什么算成功」）",
                timeout_s
            ),
        ),
        // 非零退出码是硬失败，不该用「注意」轻轻带过
        (None, Some(c)) if c != 0 => (
            StageStatus::Fail,
            format!("进程以退出码 {} 结束了", c),
        ),
        (None, Some(c)) => (
            StageStatus::Note,
            format!("进程已退出（退出码 {}），但没匹配到就绪条件", c),
        ),
    };
    cb(Event::Stage {
        index: 6,
        total: TOTAL_STAGES,
        title: if opts.purpose == Purpose::Task {
            "等待完成".into()
        } else {
            "等待就绪".into()
        },
        status: s6,
        detail: d6,
        ms: t.elapsed().as_millis(),
    });
    if let Some(how) = ready_how.clone() {
        cb(Event::Ready {
            how,
            ms: t.elapsed().as_millis(),
        });
    }

    // 边等边流：把剩余输出实时喂出去，直到子进程退出。
    // 子进程一退出，两个读线程就碰到 EOF、丢掉发送端，recv 转成 Disconnected——
    // 这本身就是「输出流完了」的信号，不需要另外轮询。
    while exit_code.is_none() {
        if let Ok((src, text)) = rx.recv_timeout(Duration::from_millis(150)) {
            lines += 1;
            emit(cb, Event::Line { source: src, text });
        }
        while let Ok((src, text)) = rx.try_recv() {
            lines += 1;
            emit(cb, Event::Line { source: src, text });
        }
        if let Some(st) = child.try_wait()? {
            exit_code = Some(st.code().unwrap_or(-1));
        }
        if Instant::now() >= next_mem_sample {
            mem.tick();
            next_mem_sample = Instant::now() + Duration::from_millis(40);
        }
    }
    for h in readers {
        // 输出读取线程 panic 才会有 Err；那多半是管道被提前关了，属收尾噪声。
        if h.join().is_err() {
            crate::dlog::w("9005", "输出读取线程异常结束");
        }
    }
    // 收尾：把整棵树带走（服务型项目里，父进程退了子进程还在占端口的坑）
    if let Some(j) = &job {
        if let Err(e) = j.terminate() {
            crate::dlog::w("9005", &format!("收掉进程树失败（子进程可能还占着端口）：{e}"));
        }
    }
    if let Err(e) = child.kill() {
        crate::dlog::w("9005", &format!("杀主进程失败：{e}"));
    }
    while let Ok((src, text)) = rx.try_recv() {
        lines += 1;
        emit(cb, Event::Line { source: src, text });
    }
    let code = exit_code.unwrap_or(-1);
    cb(Event::Exited { code });

    let diagnosis = if code != 0 && ready_how.is_none() {
        diagnose(&tail, &root)
    } else {
        None
    };

    let memory = mem.finish();
    if let Some(m) = &memory {
        crate::dlog::i(
            "2100",
            &format!(
                "peak_mb={} commit_mb={} faults={} paging={} reliable={} samples={} ms={}",
                m.peak_working_mb,
                m.peak_commit_mb,
                m.page_faults,
                m.paging,
                m.reliable,
                m.samples,
                m.duration_ms
            ),
        );
    }
    // 失败时把症结也记上——别人发回来的日志里，这一行往往就是答案
    crate::dlog::i(
        "2002",
        &format!(
            "exit={} ready={} lines={} crux={} ms={}",
            code,
            ready_how.is_some(),
            lines,
            diagnosis.as_ref().map(|d| d.crux.clone()).unwrap_or_default(),
            began.elapsed().as_millis()
        ),
    );
    // 跟这个项目上一次跑同一条命令比一比。比不出来就老实空着，
    // 不硬凑一个「一切正常」。
    let prev = crate::runs::list(&root, 8)
        .into_iter()
        .find(|r| r.command == run_cmd && r.memory.is_some());
    let memory_delta = match (&memory, prev.as_ref().and_then(|r| r.memory.as_ref())) {
        (Some(cur), Some(old)) => crate::mem::compare(old, cur),
        _ => Vec::new(),
    };
    // 记一笔：卡片的用处不在当次，而在下次回来能比。
    // 这一笔写不进去，下次回来就"没有上一版可比"——卡片的价值正好在这里。
    if let Err(e) = crate::runs::append(
        &root,
        &crate::runs::RunRecord {
            at: now_iso(),
            kind: if opts.purpose == Purpose::Task {
                "task".into()
            } else {
                "run".into()
            },
            command: run_cmd.clone(),
            exit_code: code,
            ready: ready_how.clone(),
            crux: diagnosis.as_ref().map(|d| d.crux.clone()),
            stages: stage_recs,
            tail: tail.iter().cloned().collect(),
            memory: memory.clone(),
        },
    ) {
        crate::dlog::w("9006", &format!("运行记录写入失败：{e}"));
    }
    Ok(Outcome {
        exit_code: code,
        ready: ready_how,
        lines,
        pid: Some(pid),
        started_at: now_iso(),
        diagnosis,
        memory,
        memory_delta,
    })
}

/// 从某一行里取出被单引号包住的那个名字。
/// Python 的报错很稳定地把模块名放在单引号里，这是最省事也最可靠的抓手。
fn quoted_after<'a>(tail: &'a VecDeque<String>, needle: &str) -> Option<&'a str> {
    let line = tail.iter().find(|l| l.contains(needle))?;
    line.split('\'').nth(1).filter(|s| !s.is_empty())
}

fn shadowed_by_local_file(name: &str, root: &Path) -> Diagnosis {
    Diagnosis {
        crux: format!("你自己的 {}.py 挡住了正牌的 {}", name, name),
        hint: format!(
            "把 {} 下的 {}.py 改个名。项目根目录在 import 时排在最前面，你那个会被先找到——这叫「名字遮蔽」",
            root.display(),
            name
        ),
    }
}

/// 从程序自己吐出来的字里，认出发病原因。
///
/// 这一步是「工具」和「流程壳子」的分界：进程为什么会挂，答案一直就在它的输出里，
/// 差别只在有没有人替你读出来。这里只认最常撞的那几种，认不出就老实说认不出——
/// 硬猜一个错的病因，比不猜更坏。
fn diagnose(tail: &VecDeque<String>, root: &Path) -> Option<Diagnosis> {
    let has = |needle: &str| tail.iter().any(|l| l.contains(needle));

    // import 失败有几种完全不同的病因，混成一个会指错路。
    // 只看报错文本是不够的：得回头看看项目里到底躺着什么。
    if let Some(name) = quoted_after(tail, "ModuleNotFoundError: No module named") {
        if root.join(format!("{}.py", name)).is_file() {
            // 项目根目录在 import 时排最前面：自己写的 json.py 会盖掉标准库
            return Some(shadowed_by_local_file(name, root));
        }
        if root.join(name).join("__init__.py").is_file() {
            // 包就在项目里却 import 不到 —— 那是解释器看不见项目根，
            // 便携版/嵌入式版跑在隔离模式时就是这样。
            return Some(Diagnosis {
                crux: format!("包 {} 就在项目里，但这个解释器看不见它", name),
                hint: "多半是便携版/嵌入式版的隔离模式：入口文件里加几句把项目根插进 sys.path，或者 jingwei env init 建个标准虚拟环境".into(),
            });
        }
        return Some(Diagnosis {
            crux: format!("缺包 {}：它没有装在这次使用的解释器里", name),
            hint: "jingwei env install 装一遍；装完再看 doctor 的「依赖」那一行".into(),
        });
    }
    // 名字遮蔽最常见的报法其实是这句：模块是导进来了（导进来的是你那个），
    // 只是里面没有你要用的东西。
    if let Some(name) = quoted_after(tail, "AttributeError: module '") {
        if root.join(format!("{}.py", name)).is_file() {
            return Some(shadowed_by_local_file(name, root));
        }
    }
    if has("can't open file") {
        return Some(Diagnosis {
            crux: "契约里的 run 命令指向的文件不存在".into(),
            hint: "看一眼 env.contract.json 的 commands.run；或直接 jingwei run --run \"python 你的主文件.py\"".into(),
        });
    }
    if has("EOFError") {
        return Some(Diagnosis {
            crux: "程序在等键盘输入，而这个启动器有意没给它键盘".into(),
            hint: "v1 不支持交互式程序：把输入改成从参数或文件读，然后在真正的终端里跑它".into(),
        });
    }
    if has("TabError") || has("IndentationError") {
        return Some(Diagnosis {
            crux: "缩进不一致：空格和 Tab 混着用了".into(),
            hint: "全文统一成 4 个空格；编辑器里打开「显示空白字符」，一眼就看得见".into(),
        });
    }
    if has("SyntaxError") || has("invalid syntax") {
        return Some(Diagnosis {
            crux: "代码有语法错误。第一个嫌疑是全角标点：（ ） “ ” ， ： ；".into(),
            hint: "照上面报的行号看那一行，把中文标点换成英文的".into(),
        });
    }
    if has("FileNotFoundError") {
        return Some(Diagnosis {
            crux: "程序找不到它要读写的文件".into(),
            hint: "多半是相对路径：启动器已经把工作目录设成项目根，但你手动在别处跑就不一样".into(),
        });
    }
    if has("UnicodeDecodeError") || has("UnicodeEncodeError") {
        return Some(Diagnosis {
            crux: "文件或输出的编码对不上".into(),
            hint: "读写文件时显式写 encoding=\"utf-8\"".into(),
        });
    }
    if has("PermissionError") {
        return Some(Diagnosis {
            crux: "权限不够，动不了那个文件或目录".into(),
            hint: "换到用户自己的目录下重试，别往系统目录里写".into(),
        });
    }
    None
}
/// 契约里的 run 是「人写的命令串」；拆成 program + args，
/// 并且第一个词是 python/python3 时换成我们选定的解释器绝对路径。
/// 按引号切，不然 `python "my script.py"` 会被拆成三段。
fn split_command(cmd: &str, python: &Path) -> (PathBuf, Vec<String>) {
    let parts = split_quoted(cmd);
    if parts.is_empty() {
        return (python.to_path_buf(), vec![]);
    }
    let first = parts[0].to_lowercase();
    if first == "python" || first == "python3" || first.ends_with("python.exe") {
        (python.to_path_buf(), parts[1..].to_vec())
    } else {
        (PathBuf::from(&parts[0]), parts[1..].to_vec())
    }
}

fn split_quoted(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in s.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else {
                    cur.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch.is_whitespace() {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                } else {
                    cur.push(ch);
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn spawn_reader<R: Read + Send + 'static>(
    r: R,
    src: Source,
    tx: mpsc::Sender<(Source, String)>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // 必须按字节读：子进程输出假如不是合法 UTF-8（中文环境下很常见），
        // lines() 会直接报错断流，后面的输出就全丢了。
        // 解码交给 textfile::decode_output：UTF-8 → 本机 ANSI 代码页 → 有损兜底。
        // 中文报错不再变成一串问号——这是"看不懂"里最高频的一种。
        let mut reader = BufReader::new(r);
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
                        buf.pop();
                    }
                    let text = crate::textfile::decode_output(&buf);
                    if tx.send((src, text)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    fn tail(lines: &[&str]) -> VecDeque<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jingwei-diag-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn quoted_arguments_survive_being_split() {
        // 路径里有空格时，按空白切会把一条命令切成三段
        let p = split_quoted(r#"python "my script.py" --flag"#);
        assert_eq!(p, vec!["python", "my script.py", "--flag"]);
        assert_eq!(split_quoted("python -m unittest").len(), 3);
        assert!(split_quoted("   ").is_empty());
    }

    #[test]
    fn a_local_file_with_the_same_name_is_reported_as_shadowing() {
        let d = scratch("shadow");
        std::fs::write(d.join("json.py"), "").unwrap();
        // 名字遮蔽最常见的报法是 AttributeError，不是 ModuleNotFoundError
        let t = tail(&["AttributeError: module 'json' has no attribute 'dumps'"]);
        let g = diagnose(&t, &d).unwrap();
        assert!(g.crux.contains("挡住了"), "{}", g.crux);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 包目录不是「名字遮蔽」。这个区分很重要：
    /// 一开始我把它当成遮蔽，于是给用户指了一条完全错误的路。
    #[test]
    fn a_local_package_directory_is_not_shadowing() {
        let d = scratch("pkg");
        std::fs::create_dir_all(d.join("mypkg")).unwrap();
        std::fs::write(d.join("mypkg").join("__init__.py"), "").unwrap();
        let t = tail(&["ModuleNotFoundError: No module named 'mypkg'"]);
        let g = diagnose(&t, &d).unwrap();
        assert!(g.crux.contains("看不见它"), "{}", g.crux);
        assert!(!g.crux.contains("挡住"), "包目录被误判成名字遮蔽：{}", g.crux);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_genuinely_missing_package_says_so() {
        let d = scratch("missing");
        let t = tail(&["ModuleNotFoundError: No module named 'requests'"]);
        let g = diagnose(&t, &d).unwrap();
        assert!(g.crux.contains("缺包"), "{}", g.crux);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 认不出来就闭嘴。硬猜一个错的病因，比不给病因更坏。
    #[test]
    fn an_unrecognised_failure_gets_no_diagnosis() {
        let d = scratch("unknown");
        let t = tail(&["something entirely unexpected", "process died"]);
        assert!(diagnose(&t, &d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}

