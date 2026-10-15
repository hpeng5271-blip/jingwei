//! 会话日志。只在退出时写盘。
//!
//! 存在的原因很实在：这台机器上查不出来的问题，得靠**别人机器上真实使用**
//! 之后留下的记录。所以日志的第一要求不是详细，是**能被收集和分析**。
//!
//! 三条规矩：
//!
//! 一、**平时不写盘**。全部先进内存，退出时一次写完。工具是给开发用的，
//!     不能在编译的时候还去抢磁盘。缓冲区满了才落一次——否则被强杀就全丢。
//!
//! 二、**短码代替长句**。`1200` 比「正在打开项目」省得多，grep 也快，
//!     翻译成中文那步交给读日志的人（或工具）。码表在下面。
//!
//! 三、**键值对，不是散文**。`lang=java findings=3 ms=380` 能直接统计，
//!     「这个项目是 Java 的，发现了 3 个问题，花了 380 毫秒」不能。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

/// 装上崩溃记录。
///
/// 这是日志里最要紧的一条：程序**正常跑完**时你多半不需要日志，
/// 真正需要的是它**没正常跑完**的那次。所以崩溃前必须先把已经攒下的
/// 记录落盘，再把 panic 的位置和原因写进去——否则别人发回来的只有一句
/// "它闪退了"，什么也查不了。
///
/// 注意：发布版用的是 panic=abort，hook 仍然会在终止前执行。
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "?".into());
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "（读不出内容）".into());
        e("9000", &format!("panic at={} what={}", at, what));
        // 崩溃时缓冲里可能还压着几百行没写——先落盘再让程序死
        flush();
        prev(info);
    }));
}

/// 缓冲上限。约 2 MB 就落一次盘——被强杀时最多丢这么多。
const MAX_BUFFER: usize = 20_000;

static BUFFER: Mutex<Vec<String>> = Mutex::new(Vec::new());
static FILE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// 码表。写日志时只写码，读日志时来这里查。
/// 分段的用意：第一位是「哪一块」，后三位是「具体哪件事」。
pub fn code_name(code: &str) -> &'static str {
    match code {
        "1000" => "会话开始",
        "1001" => "会话结束",
        "1100" => "环境探测",
        "1200" => "打开项目",
        "1300" => "诊断完成",
        "2000" => "启动项目",
        "2001" => "启动阶段",
        "2002" => "启动结束",
        "2100" => "内存采样",
        "3000" => "清理扫描",
        "3001" => "清理执行",
        "4000" => "笔记写入",
        "5000" => "事实采集",
        "6000" => "系统设置",
        "9001" => "数据目录搬迁",
        "9002" => "变更基线写入",
        "9003" => "占用探测未还原",
        "9004" => "读子进程输出",
        "9005" => "收尾动作失败",
        "9006" => "运行记录写入",
        "9000" => "异常",
        _ => "未知",
    }
}

fn log_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let d = PathBuf::from(base).join("jingwei").join("log");
    if let Err(e) = std::fs::create_dir_all(&d) {
        // 日志目录建不起来，日志本身就没地方写——这时候只能直接吐 stderr。
        // 不静默：这正是最该被人看见的一类失败（多半是磁盘满或权限被改）。
        eprintln!("日志目录建不起来（{}）：{e}", d.display());
        return None;
    }
    Some(d)
}

fn file() -> &'static Option<PathBuf> {
    FILE.get_or_init(|| {
        let d = log_dir()?;
        // 按天一个文件，方便只收集「出问题那天」
        let day: String = crate::facts::now_iso().chars().take(10).collect();
        Some(d.join(format!("jingwei-{}.log", day)))
    })
}

/// 记一条。`code` 用上面码表里的四位数。
pub fn log(level: char, code: &str, msg: &str) {
    let line = format!("{} {} DD-{} {}", crate::facts::now_iso(), level, code, msg);
    let mut buf = match BUFFER.lock() {
        Ok(b) => b,
        Err(_) => return,
    };
    buf.push(line);
    if buf.len() >= MAX_BUFFER {
        // 缓冲满了先落一次。这里宁可多写一次盘，也不能让被强杀时把记录全带走。
        let take = std::mem::take(&mut *buf);
        drop(buf);
        append(&take);
    }
}

pub fn i(code: &str, msg: &str) {
    log('i', code, msg);
}
pub fn w(code: &str, msg: &str) {
    log('w', code, msg);
}
pub fn e(code: &str, msg: &str) {
    log('e', code, msg);
}

fn append(lines: &[String]) {
    let Some(p) = file().as_ref() else { return };
    use std::io::Write;
    let Ok(mut fh) = std::fs::OpenOptions::new().create(true).append(true).open(p) else {
        return;
    };
    let mut complained = false;
    for l in lines {
        if let Err(e) = writeln!(fh, "{}", l) {
            // 日志写不进去，多半是磁盘满了或文件被占。除了 stderr 没有别的去处，
            // 但不能整段静默：用户拿着"日志里什么都没有"来找问题时，会以为是工具没记。
            // 只抱怨一次，免得一行一句把控制台刷满。
            if !complained {
                eprintln!("日志写不进 {}：{e}", p.display());
                complained = true;
            }
        }
    }
}

/// 退出时调用一次。这是日志唯一一次正常的写盘时机。
pub fn flush() {
    let take = match BUFFER.lock() {
        Ok(mut b) => std::mem::take(&mut *b),
        Err(_) => return,
    };
    append(&take);
    if let Some(p) = file().as_ref() {
        // 顺手清旧文件：只留最近 14 天，不然日志本身会变成新的垃圾
        if let Some(d) = p.parent() {
            if let Ok(rd) = std::fs::read_dir(d) {
                let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
                    .flatten()
                    .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("log"))
                    .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                    .collect();
                files.sort_by_key(|(t, _)| *t);
                for (_, old) in files.iter().rev().skip(14) {
                    // 删旧日志失败只会让日志目录多占几 MB，不影响任何结论；
                    // 而这里正是日志系统自己，报错也没地方记。
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = std::fs::remove_file(old);
                }
            }
        }
    }
}

/// 缓冲区里积了多少行。给自己人看，确认「只在退出时写」这件事真的在生效。
pub fn buffered() -> usize {
    BUFFER.lock().map(|b| b.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    #[test]
    fn every_code_in_use_has_a_name() {
        // 码表漏一个，日志里就会出现「未知」，那条记录等于白写
        for c in [
            "1000", "1001", "1100", "1200", "1300", "2000", "2001", "2002", "2100", "3000",
            "3001", "4000", "5000", "6000", "9000",
        ] {
            assert_ne!(code_name(c), "未知", "码 {} 没在表里", c);
        }
    }

    #[test]
    fn logging_buffers_without_touching_the_disk() {
        let before = buffered();
        i("1000", "test=1");
        assert_eq!(buffered(), before + 1, "记一条只该进内存");
    }
}
