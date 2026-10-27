#![cfg_attr(windows, windows_subsystem = "windows")]

//! 经纬桌面壳：wry(WebView2) 窗口 + 内嵌 HTTP（复用 jw web 的 serve_web）。
//! 不依赖 node、不 spawn jw 子进程；关窗即进程退出。

#[path = "../main.rs"]
mod jw;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // 单实例：固定回环端口做互斥令牌；第二个实例直接退出，避免多窗口。
    let _singleton = match std::net::TcpListener::bind(("127.0.0.1", 7960)) {
        Ok(l) => l,
        Err(_) => return,
    };

    let (tx, rx) = mpsc::channel::<u16>();
    let root_for_server = root.clone();
    std::thread::spawn(move || {
        let _ = jw::serve_web(&root_for_server, 0, false, Some(tx));
    });

    let Ok(port) = rx.recv_timeout(Duration::from_secs(15)) else {
        return;
    };
    let url = format!("http://127.0.0.1:{port}/ui/index.html");

    let event_loop = tao::event_loop::EventLoop::new();
    let window = match tao::window::WindowBuilder::new()
        .with_title("经纬 · 工作台")
        .with_inner_size(tao::dpi::LogicalSize::new(1440.0, 900.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(960.0, 600.0))
        .build(&event_loop)
    {
        Ok(w) => w,
        Err(_) => return,
    };

    let _webview = wry::WebViewBuilder::new()
        .with_url(&url)
        .build(&window);

    // 窗口数断言（证据落盘，不阻塞）：3 秒后数自己进程的可见顶层窗口。
    #[cfg(windows)]
    {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(3));
            let n = count_own_windows();
            let path = std::env::temp_dir().join("jingwei-gui-windows.txt");
            let _ = std::fs::write(&path, format!("visible_windows={n}\n"));
        });
    }

    event_loop.run(|_, _, control_flow| {
        *control_flow = tao::event_loop::ControlFlow::Wait;
    });

    // 关窗即回收：自己的进程树（serve + RA）一起杀，避免残留
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &std::process::id().to_string(), "/T", "/F"])
            .output();
    }
}

#[cfg(windows)]
fn count_own_windows() -> usize {
    use std::ffi::c_void;
    type Hwnd = *mut c_void;
    extern "system" {
        fn EnumWindows(cb: extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
        fn GetWindowThreadProcessId(h: Hwnd, pid: *mut u32) -> u32;
        fn IsWindowVisible(h: Hwnd) -> i32;
        fn GetWindowTextLengthW(h: Hwnd) -> i32;
    }
    static mut COUNT: usize = 0;
    static mut PID: u32 = 0;
    extern "system" fn cb(h: Hwnd, _l: isize) -> i32 {
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, &mut pid);
            if pid == PID && IsWindowVisible(h) != 0 && GetWindowTextLengthW(h) > 0 {
                COUNT += 1;
            }
        }
        1
    }
    unsafe {
        PID = std::process::id();
        COUNT = 0;
        EnumWindows(cb, 0);
        COUNT
    }
}
