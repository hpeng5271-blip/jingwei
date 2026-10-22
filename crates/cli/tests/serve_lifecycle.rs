//! `serve` 的**生命周期**测试（2026-10-22）：证明"关掉 stdin 它就该退出"。
//!
//! 为什么单写这一条：详情页那条路径靠"离开就停进程"来省资源；如果 `serve` 收到 EOF 不退出，
//! 界面再怎么调 `channel_stop` 也会留孤儿进程。这是**进程级**的事，前端断言测不出来。

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_jingwei") {
        return Some(std::path::PathBuf::from(p));
    }
    let here = std::env::current_exe().ok()?;
    let dir = here.parent()?.parent()?.parent()?;
    let exe = dir.join(if cfg!(windows) { "jingwei.exe" } else { "jingwei" });
    exe.exists().then_some(exe)
}

#[test]
fn 关掉stdin之后serve要退出() {
    let Some(bin) = bin() else { return };
    let d = std::env::temp_dir().join(format!("jw-serve-life-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let p = d.to_str().unwrap().to_string();

    let mut child = Command::new(&bin)
        .args(["serve", "--root", &p])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("serve 该能起来");

    // 先确认它是活的：问一句 ping，能回就算活着
    {
        use std::io::{BufRead, BufReader};
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, "{{\"id\":1,\"method\":\"ping\"}}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
        assert!(line.contains("pong"), "该回 pong：{line}");
    }

    // 关掉 stdin（等价于界面退进程/父进程消失），它应当自行退出
    drop(child.stdin.take());
    let mut exited = false;
    for _ in 0..40 {
        if let Ok(Some(_)) = child.try_wait() {
            exited = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !exited {
        child.kill().ok();
    }
    assert!(exited, "stdin 关了 serve 还在跑——那界面就管不住它");
    std::fs::remove_dir_all(&d).ok();
}
