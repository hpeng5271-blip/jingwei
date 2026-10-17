//! 启动：6 阶段 + 流式输出 + 就绪判定。
//! 事件走回调（CLI 打印 / Tauri 推前端），逻辑层不打印任何东西。
//!
//! ## 六个阶段里，只有前四个跟语言有关
//!
//! | 阶段 | 谁来决定 |
//! |---|---|
//! | 1–4 环境 / 契约 / 依赖 / 运行时 | **按项目语言分家**（`python.rs` / `node.rs`） |
//! | 5 拉起进程、6 等待就绪 | 完全一样——进程怎么起、怎么判「起来了」，跟语言没关系 |
//!
//! 这条边界是拿一个真事故换来的：拿 Python 那条路去开一个 Vue 项目，
//! 用户看到的第一句话是「没有可用的 Python，无法启动」——**答非所问比不回答更糟**，
//! 它会把人引到错的方向上。

mod java;
mod node;
mod python;
mod ready;
pub mod rundiag;

use ready::serving_url;
// 这两个是 `rundiag` 里的辅助（同模块树内直接用，不必 re-export——
// 对外暴露它们没有意义，它们是"怎么认诊断"的内部细节）。
use rundiag::{is_informative, truncate_line};
pub use rundiag::{collect_run_diagnostics, RunDiagnostic};

use crate::understand::contract::{Contract, ReadyWhen};
use crate::state::runs;
use crate::facts::now_iso;
use anyhow::{Context, Result};
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
    /// 跑一个「跑完就结束」的任务（测试这类），命令由调用方给（契约或 scripts）。
    Task,
    /// 装依赖。**单独一类是因为它不该被当成"跑个脚本"**：
    /// `pnpm install` 是包管理器自己的子命令，不是 `package.json` 里的 scripts。
    /// 混进 `Task` 的话，「补装依赖」这个按钮会去跑 test 脚本——
    /// 用户点的是安装，跑的是测试，这种错最难被发现。
    Install,
    /// 构建：`{包管理器} run build`。
    Build,
}

pub struct LaunchOptions {
    pub root: PathBuf,
    pub run_override: Option<String>,
    pub extra_args: Vec<String>,
    pub timeout: Duration,
    pub purpose: Purpose,
}

/// **前四个阶段的结论**——两种语言各算一份，后面那两阶段只认这个。
///
/// 把这些东西收成一个结构，是为了让「怎么起进程」只有一处实现：
/// 有了它，`Command::new(…)` 那一串、Job 收进程树、输出读取线程、就绪判定、
/// 内存采样、运行记录，全都不用为第二种语言再写一遍。
pub(crate) struct Prepared {
    /// 真正要执行的程序（Python 那条路是解释器绝对路径，Node 那条路是包管理器）。
    pub program: PathBuf,
    pub args: Vec<String>,
    /// 只属于这门语言的环境变量。
    pub env: Vec<(String, String)>,
    /// 人看得懂的那条命令（记进运行记录，也用来和上一次比）。
    pub run_cmd: String,
    /// **到点了还没判出就绪、但进程还活着**——这算失败还是「还说不准」。
    ///
    /// 脚本项目跑不完通常是卡住了（`Fail`）；常驻的前端服务本来就不退（`Note`）。
    /// 这里弄反的后果很具体：开发服务器会一直显示「启动失败」，
    /// 而它其实好好地跑着——用户会去查一个不存在的故障。
    pub alive_unready: StageStatus,
    /// 要不要在输出里盯监听地址。只有「跑起来之后一直在那儿」的服务才需要。
    pub url_watch: bool,
    /// 这次「什么算起来了」是怎么定的，一句话，给人看。
    pub ready_rule: String,
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
    /// 这次运行在图谱里留下的诊断（M5 的料）。
    ///
    /// **为什么它必须在 Outcome 上**：`record_run` 要的是"这次跑出了哪些问题"，
    /// 而只有这一层知道（它手里有输出尾部）。外壳拿不到这些，所以不能在外壳里补。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<RunDiagnostic>,
    /// 这次跑的时候内存怎么样。只在采到东西时才有。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<crate::sys::mem::MemReport>,
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
    crate::util::dlog::i(
        "2000",
        &format!(
            "root={} cmd={} task={}",
            root.display(),
            opts.run_override.clone().unwrap_or_else(|| "（按契约）".into()),
            opts.purpose == Purpose::Run
        ),
    );

    // ---- 前四阶段：按项目语言准备 ----
    //
    // **以「这个项目是什么」为准，不以「契约里写了什么」为准。**
    // 契约只决定「跑哪条命令」（那是用户写下的东西，优先级最高）；
    // 但一个 Node 项目要检查的是 node 与包管理器，不是解释器——
    // 拿错那一边去检查，用户看到的第一句话会与他的项目毫无关系。
    // 走哪条路，**由「这个项目是什么」决定**：
    //   · 有 pom.xml / build.gradle → JVM
    //   · 有 package.json → Node
    //   · 其余（含 Python）→ 现在那条路
    // 复合项目（根目录里既有 pom.xml 又有 package.json，比如前端工具链挂在
    // 一个 Spring 项目下）按 Maven/Gradle 优先：那种 package.json 往往是给
    // 前端资源打包用的，而"启动这个项目"问的是后端。
    // **顺序写下来，是为了让两难时有一致的答案**，而不是每次换个样。
    let prep = if has_jvm_build_file(&root) {
        java::prepare(&root, opts, &contract, &contract_err, cb)?
    } else if crate::detect::web::is_node_project(&root) {
        node::prepare(&root, opts, &contract, &contract_err, cb)?
    } else {
        python::prepare(&root, opts, &contract, &contract_err, cb)?
    };
    let run_cmd = prep.run_cmd.clone();

    let t = Instant::now();
    let mut cmd = Command::new(&prep.program);
    cmd.args(&prep.args)
        .args(&opts.extra_args)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &prep.env {
        cmd.env(k, v);
    }
    // 桌面外壳拉起 python 时也会弹一个黑框；工具自己负责跑它，不需要窗口
    crate::util::winutil::hide_console(&mut cmd);
    let mut child = cmd.spawn().with_context(|| format!("拉起失败：{}", run_cmd))?;
    let pid = child.id();
    // 把子进程收进一个 Job：跑完/超时/用户停止时，连它的子孙一起收掉。
    // 不做这件事，`cargo run` 拉起的编译器/链接器会成为孤儿继续占端口和内存。
    let job = match crate::util::jobobj::Job::new() {
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
    let mut mem = crate::sys::mem::Sampler::new(pid);
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
        title: if opts.purpose == Purpose::Run {
            "启动进程".into()
        } else {
            "执行命令".into()
        },
        status: StageStatus::Ok,
        detail: format!("{} {}", prep.program.display(), prep.args.join(" ")),
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
    // 常驻服务（`pnpm dev` 这类）在输出里报出来的地址。**看到地址不等于起来了**：
    // 要真的连上那个端口才算——见下面那处 `TcpStream::connect`。
    let mut seen_addr: Option<(String, u16)> = None;
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
                if prep.url_watch && seen_addr.is_none() {
                    seen_addr = serving_url(&text);
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
            if prep.url_watch && seen_addr.is_none() {
                seen_addr = serving_url(&text);
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
                        crate::util::dlog::w("9005", &format!("日志定位失败（{}）：{e}", path.display()));
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
        // 常驻服务那条路：输出里报了地址**还不够**，端口真的通了才算。
        //
        // 为什么加这一道：`127.0.0.1:3000` 这种字样也会出现在**连不上**的报错里
        // （`connect ECONNREFUSED 127.0.0.1:3000`）。只按「出现过地址」判，
        // 那种失败会被当成启动成功——而把失败报成成功，比报成失败糟得多。
        if ready_how.is_none() && prep.url_watch {
            if let Some((label, p)) = seen_addr.clone() {
                if std::net::TcpStream::connect(("127.0.0.1", p)).is_ok() {
                    ready_how = Some(format!("端口 {p} 已监听（输出里出现过 {label}）"));
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
                format!("已就绪 · {}（判定依据：{}）", how, prep.ready_rule)
            } else {
                format!("已就绪 · {}", how)
            },
        ),
        // **进程还活着、只是没判出就绪**——这一格的措辞按语言不同：
        // 脚本跑不完通常是卡住了（Fail）；前端 dev server 本来就不退，
        // 这时候说「失败」会让人去查一个不存在的故障（Note）。
        (None, None) => (
            prep.alive_unready,
            if prep.alive_unready == StageStatus::Note {
                // 两种处境说法完全不同：**看到了地址、但那个端口连不上**
                // （多半还在编译，或者它把服务绑到了别的地址上）、
                // 以及**压根没报地址**。混成一句会把人往错的方向引。
                match &seen_addr {
                    Some((label, p)) => format!(
                        "输出里出现过 {label}，但 {p} 这个端口连不上——可能还在编译，也可能它绑到了别的地址。进程还在跑，输出继续跟着"
                    ),
                    None => format!(
                        "进程还在跑，但 {timeout_s} 秒内没在输出里看到监听地址——它可能已经起来了，也可能还在编译。输出继续跟着，你可以直接开浏览器试"
                    ),
                }
            } else {
                format!(
                    "{} 秒内没等到就绪条件（进程还在跑，输出继续跟着；契约里的 ready_when 要写清楚「什么算成功」）",
                    timeout_s
                )
            },
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
        title: if opts.purpose == Purpose::Run {
            "等待就绪".into()
        } else {
            "等待完成".into()
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
            crate::util::dlog::w("9005", "输出读取线程异常结束");
        }
    }
    // 收尾：把整棵树带走（服务型项目里，父进程退了子进程还在占端口的坑）
    if let Some(j) = &job {
        if let Err(e) = j.terminate() {
            crate::util::dlog::w("9005", &format!("收掉进程树失败（子进程可能还占着端口）：{e}"));
        }
    }
    if let Err(e) = child.kill() {
        crate::util::dlog::w("9005", &format!("杀主进程失败：{e}"));
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
        crate::util::dlog::i(
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
    crate::util::dlog::i(
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
    let prev = crate::state::runs::list(&root, 8)
        .into_iter()
        .find(|r| r.command == run_cmd && r.memory.is_some());
    let memory_delta = match (&memory, prev.as_ref().and_then(|r| r.memory.as_ref())) {
        (Some(cur), Some(old)) => crate::sys::mem::compare(old, cur),
        _ => Vec::new(),
    };
    // 记一笔：卡片的用处不在当次，而在下次回来能比。
    // 这一笔写不进去，下次回来就"没有上一版可比"——卡片的价值正好在这里。
    if let Err(e) = crate::state::runs::append(
        &root,
        &crate::state::runs::RunRecord {
            at: now_iso(),
            kind: if opts.purpose == Purpose::Run {
                "run".into()
            } else {
                "task".into()
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
        crate::util::dlog::w("9006", &format!("运行记录写入失败：{e}"));
    }

    // ── M5：把这次运行记进图谱 ─────────────────────────────────────
    //
    // 为什么**必须在这里**，不能挪到外壳里：诊断清单只有这一层拿得到（它手里有输出尾部）。
    // 外壳只有 Outcome，拿不到"哪一行是哪条诊断"——把它挪上去，图谱里就只会有一条
    // 没有落点的症结，"改了这里会影响谁"仍然答不出来。
    //
    // 写不进去只记日志、**不影响这次运行的结论**：图谱是可重算的派生数据
    // （`schema.sql` 开头那句），丢了下一轮补回来；而"这次跑成没成"是当次的事实，
    // 不能因为一个副产物写失败就把它标成失败。
    let diagnostics = {
        let mut v = collect_run_diagnostics(&root, &tail);
        // 一条都没认出来时的兜底，按"我们知道多少"分三档，**从不假装知道更多**：
        //   ① 读出了症结（`diagnose` 认得那种失败）→ 用症结，没有落点；
        //   ② 只是失败了（退出码非 0）→ 记「退出码 N」这一条事实；
        //   ③ 正常结束 → 什么都不记。
        //
        // 为什么要 ②：原来只有 ①，于是"退出码非 0 但症结认不出来"的运行
        // **在图谱里一条痕迹都没有**（只剩一个 Run 节点）。那等于图谱把"这次失败了"
        // 这件事悄悄咽掉了——而它恰恰是最该留下的那一条。
        // 这是 M5 闭环测试当场抓出来的（`没有落点的诊断如实说没有`）。
        if v.is_empty() {
            if let Some(d) = &diagnosis {
                v.push(RunDiagnostic {
                    title: d.crux.clone(),
                    file: None,
                });
            } else if code != 0 {
                // 认不出形状时，**把进程自己说的最后一句有信息量的话留下**。
                //
                // 原来这里只写「退出码 N（工具没从输出里认出具体原因）」——而实测
                // （用 `jingwei graph` 看 messy_service 那次失败）那条诊断什么用都没有：
                // 输出里明明有 python 的报错行，图谱里却只有一句"我不知道"。
                // 图谱记不下原因的诊断，等于没记。所以退到"原样引用最后那句实在话"。
                let last = tail
                    .iter()
                    .rev()
                    .map(|l| l.trim())
                    .find(|l| is_informative(l));
                v.push(RunDiagnostic {
                    title: match last {
                        Some(l) => format!("{}（退出码 {code}）", truncate_line(l, 160)),
                        None => format!("退出码 {code}（输出里没有可引用的一句话）"),
                    },
                    file: None,
                });
            }
        }
        v
    };
    match crate::graph::store::Store::open(&crate::graph_db_path(&root)) {
        Ok(mut store) => {
            let pairs: Vec<(String, Option<String>)> = diagnostics
                .iter()
                .map(|d| (d.title.clone(), d.file.clone()))
                .collect();
            match crate::graph::index::record_run(
                &mut store,
                &root,
                &run_cmd,
                code,
                &pairs,
                crate::graph::index::now_ms(),
            ) {
                Ok(rec) => crate::util::dlog::i(
                    "2101",
                    &format!(
                        "图谱已记下这次运行：run={} 诊断={} 挂不到文件的={}",
                        rec.run_id, rec.diagnostics, rec.unknown_targets
                    ),
                ),
                Err(e) => crate::util::dlog::w("2102", &format!("运行记进图谱失败：{e}")),
            }
        }
        Err(e) => crate::util::dlog::w("2102", &format!("打开图谱库失败：{e}")),
    }

    Ok(Outcome {
        exit_code: code,
        ready: ready_how,
        lines,
        pid: Some(pid),
        started_at: now_iso(),
        diagnosis,
        diagnostics,
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

/// 这个目录是不是一个 JVM 项目。
///
/// 判据只看**构建文件**：`pom.xml`（Maven）、`build.gradle` / `build.gradle.kts`（Gradle）。
/// 不看 `.java` 文件——有一堆 `.java` 却没有构建文件的那种目录，
/// 是"从别处抄来的代码片段"，不是能启动的项目。
fn has_jvm_build_file(root: &Path) -> bool {
    ["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle"]
        .iter()
        .any(|f| root.join(f).is_file())
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
                    let text = crate::util::textfile::decode_output(&buf);
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

