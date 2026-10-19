//! **后台扫盘**：状态机 + 线程 + 四个命令（2026-10-19 从 `main.rs` 抽出来）。
//!
//! 为什么单独一个文件：`main.rs` 已经顶到 1000 行上限，而这一块**自成一件事**——
//! 扫描的进度/终态/取消只在这里改，外壳只读 `scan_status`。
//!
//! 纪律：**终态只由这里写**（running/done/cancelled），前端不许靠"多久没收到"去猜。

use jingwei_engine::understand::{dirs_index, fingerprint, projects, scan_cache};
use serde_json::{json, Value};
use std::path::PathBuf;
/// **后台扫盘的共享状态**（2026-10-19）。
///
/// 为什么状态机放在这里而不是前端：前端只能看到"上一次拿到的东西"，
/// **猜不出"还在扫"还是"扫完了"**。所以 running/done/visited 只在这里改，
/// 前端每 300ms 读一次，读到 done=true 才算完。
#[derive(Default, Clone)]
struct ScanState {
    running: bool,
    done: bool,
    cancelled: bool,
    visited: usize,
    skipped: usize,
    found: Vec<serde_json::Value>,
    current: String,
    seconds: f64,
    /// 这次结果是上次落盘的（还没重新扫）
    cached: bool,
    /// 缓存写于何时（秒级）
    cached_at: u64,
}

static SCAN: std::sync::OnceLock<std::sync::Mutex<ScanState>> = std::sync::OnceLock::new();
static STOP: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> =
    std::sync::OnceLock::new();

fn scan_slot() -> &'static std::sync::Mutex<ScanState> {
    SCAN.get_or_init(|| std::sync::Mutex::new(ScanState::default()))
}

fn stop_flag() -> &'static std::sync::Arc<std::sync::atomic::AtomicBool> {
    STOP.get_or_init(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
}

/// **开始后台扫**：立刻返回；结果靠 `scan_status` 轮询。
///
/// roots 为空时用 `projects::default_scan_roots()`（D:\\projects、D:\\research、
/// `~/source/repos` 等）。想扫整盘就显式传 `["C:\\", "D:\\"]`。
#[tauri::command]
pub fn scan_start(roots: Vec<String>, depth: usize) -> Value {
    let roots: Vec<PathBuf> = if roots.is_empty() {
        projects::default_scan_roots()
    } else {
        roots.into_iter().map(PathBuf::from).collect()
    };
    if roots.is_empty() {
        return json!({ "ok": false, "why": "没有可扫的根目录" });
    }
    let depth = depth.clamp(1, 12);
    {
        let mut st = scan_slot().lock().unwrap();
        if st.running {
            return json!({ "ok": false, "why": "已经在扫了" });
        }
        *st = ScanState {
            running: true,
            ..Default::default()
        };
    }
    if let Ok(mut st) = scan_slot().lock() {
        st.cached = false;
        st.cached_at = 0;
    }
    stop_flag().store(false, std::sync::atomic::Ordering::Relaxed);
    let stop = stop_flag().clone();
    let roots_for_json: Vec<String> = roots.iter().map(|p| p.display().to_string()).collect();
    let roots_for_json_for_cache = roots_for_json.clone();
    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let mut all: Vec<serde_json::Value> = Vec::new();
        let mut seen_paths: std::collections::BTreeSet<String> = Default::default();
        // 跨根累加（闭包里的 v 是"当前根"的计数）
        let mut total_visited = 0u64;
        let mut total_skipped = 0u64;
        for root in &roots {
            if !root.is_dir() {
                continue;
            }
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
            let idx = dirs_index::build_parallel(root, depth, threads, &stop, |v, s, cur| {
                if let Ok(mut st) = scan_slot().lock() {
                    st.visited = v;
                    st.skipped = s;
                    st.current = cur.display().to_string();
                }
            });
            total_visited += idx.visited as u64;
            total_skipped += idx.skipped as u64;
            for p in idx.all_projects() {
                let path = p.display().to_string();
                if !seen_paths.insert(path.clone()) {
                    continue;
                }
                let f = projects::describe_public(&p);
                let (files, bytes, cut) = projects::measure(&p);
                let fp = fingerprint::of(&p);
                all.push(json!({
                    "path": f.path, "name": f.name, "kinds": f.kinds, "markers": f.markers,
                    "has_git": f.has_git, "has_contract": f.has_contract,
                    "age_days": f.age_days, "files": files, "bytes": bytes, "size_cut": cut,
                    "fp": fp.hash, "fp_truncated": fp.truncated
                }));
            }
            // 每个根扫完就先把结果交出去：**先出结果**，不等整轮
            if let Ok(mut st) = scan_slot().lock() {
                st.found = all.clone();
            }
        }
        let cancelled = stop.load(std::sync::atomic::Ordering::Relaxed);
        // **落盘**：下次打开界面不必从头扫（用户明确要求的"结果持久化"）
        // 目录数/跳过数要在清空状态前取出来（否则缓存里记成"项目数"，口径就错了）
        let final_stats = (total_visited as usize, total_skipped as usize);
        let saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cache = scan_cache::ScanCache {
            saved_at,
            roots: roots_for_json_for_cache.clone(),
            depth,
            visited: final_stats.0 as u64,
            skipped: final_stats.1 as u64,
            found: all.clone(),
        };
        // 落盘失败不影响这次扫描的结论（下次重扫就是了）
        if let Err(e) = scan_cache::save(&scan_cache::cache_path(), &cache) {
            eprintln!("扫盘缓存没写成：{e}");
        }
        if let Ok(mut st) = scan_slot().lock() {
            st.found = all;
            st.running = false;
            st.done = true;
            st.cancelled = cancelled;
            st.seconds = t0.elapsed().as_secs_f64();
        }
    });
    json!({ "ok": true, "roots": roots_for_json })
}

/// 轮询用：当前扫到哪儿、发现了什么。**done 是唯一终态判据。**
#[tauri::command]
pub fn scan_status() -> Value {
    {
        let mut st = scan_slot().lock().unwrap();
        // **持久化**（2026-10-19）：内存空 + 没在扫 → 用上次落盘的结果，
        // 先让界面有东西看；真扫描开始时会被覆盖。
        if st.found.is_empty() && !st.running {
            if let Some(c) = scan_cache::load(&scan_cache::cache_path()) {
                st.found = c.found;
                st.visited = c.visited as usize;
                st.skipped = c.skipped as usize;
                st.done = true;
                st.cached = true;
                st.cached_at = c.saved_at;
            }
        }
    }
    let st = scan_slot().lock().unwrap().clone();
    json!({
        "running": st.running, "done": st.done, "cancelled": st.cancelled,
        "visited": st.visited, "skipped": st.skipped,
        "current": st.current, "seconds": st.seconds, "cached": st.cached, "cached_at": st.cached_at,
        "found": st.found,
    })
}

/// 停止：只置位，真正的收尾由扫描线程做（**终态只会由那一处写**）。
#[tauri::command]
pub fn scan_cancel() -> Value {
    stop_flag().store(true, std::sync::atomic::Ordering::Relaxed);
    json!({ "ok": true })
}

/// **找同源**（2026-10-19）：对当前扫到的项目里**同名目录**算 Jaccard（≥0.8）。
///
/// 为什么按需算而不是扫描时算：集合要对每个项目再走一遍目录，
/// 全盘时这会把扫描变慢一倍。**用户点"找同源"才付这个钱。**
#[tauri::command]
pub fn scan_similar(min: Option<f64>) -> Value {
    let min = min.unwrap_or(0.8);
    let found = scan_slot().lock().unwrap().found.clone();
    // 候选：同名目录（跨机器/克隆最常见的那一类）
    let mut by_name: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for p in &found {
        if let (Some(path), Some(name)) = (p.get("path").and_then(|v| v.as_str()), p.get("name").and_then(|v| v.as_str())) {
            by_name.entry(name.to_string()).or_default().push(path.to_string());
        }
    }
    let mut pairs = Vec::new();
    for (_name, paths) in by_name {
        if paths.len() < 2 {
            continue;
        }
        let sets: Vec<(String, Vec<String>)> = paths
            .iter()
            .map(|p| (p.clone(), fingerprint::set_of(std::path::Path::new(p))))
            .collect();
        for i in 0..sets.len() {
            for j in (i + 1)..sets.len() {
                let s = fingerprint::jaccard(&sets[i].1, &sets[j].1);
                if s >= min {
                    pairs.push(json!({ "a": sets[i].0, "b": sets[j].0, "score": s }));
                }
            }
        }
    }
    json!({ "min": min, "pairs": pairs })
}

/// 默认扫描根（给界面显示"要扫哪儿"）。
#[tauri::command]
pub fn scan_roots() -> Value {
    json!({ "roots": projects::default_scan_roots().iter().map(|p| p.display().to_string()).collect::<Vec<_>>() })
}

