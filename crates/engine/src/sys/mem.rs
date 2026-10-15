//! 内存：开发时最大的那个瓶颈。
//!
//! 16G 机器上编译变慢，根因不是 CPU，是**换页**：commit 一旦超过物理内存，
//! Windows 开始往 pagefile 上搬，编译这种随机读写的负载一进磁盘就慢十倍。
//!
//! 所以这里只回答三件事：
//!   峰值用掉多少、有没有真的进换页、当时还有谁占着。
//! 全部走系统调用（毫秒级），不碰 WMI、不常驻、不碰驱动。

use serde::{Deserialize, Serialize};

/// 存进环境快照的那一份内存事实。刻意只留数字，不带 tail——
/// 快照是给人翻的，把几百行程序输出塞进去只会把它淹掉。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemFacts {
    pub at: String,
    pub command: String,
    pub reliable: bool,
    pub paging: bool,
    pub peak_working_mb: u64,
    pub peak_commit_mb: u64,
    pub sys_commit_used_mb: u64,
    pub phys_total_mb: u64,
    pub phys_avail_at_peak_mb: u64,
    #[serde(default)]
    pub others: Vec<ProcMem>,
}

impl MemFacts {
    pub fn from_report(at: &str, command: &str, m: &MemReport) -> Self {
        Self {
            at: at.to_string(),
            command: command.to_string(),
            reliable: m.reliable,
            paging: m.paging,
            peak_working_mb: m.peak_working_mb,
            peak_commit_mb: m.peak_commit_mb,
            sys_commit_used_mb: m.sys_commit_used_mb,
            phys_total_mb: m.phys_total_mb,
            phys_avail_at_peak_mb: m.phys_avail_at_peak_mb,
            others: m.others.clone(),
        }
    }
}

/// 和上一次跑相比，这次有什么不一样。
///
/// 单次数字看不出好坏——「峰值 400 MB」本身不说明任何事。一比才有结论：
/// 上次 220、这次 400，那才是「这次不对劲」。
pub fn compare(prev: &MemReport, cur: &MemReport) -> Vec<String> {
    let mut out = Vec::new();
    // 两边都得可信才比。拿一次没采准的数字去比，只会得出错的结论。
    if !prev.reliable || !cur.reliable {
        return out;
    }
    let (a, b) = (prev.peak_working_mb as f64, cur.peak_working_mb as f64);
    if a >= 8.0 {
        let ratio = b / a;
        if ratio >= 1.25 || ratio <= 0.8 {
            out.push(format!(
                "峰值内存 {} MB → {} MB（{}）",
                prev.peak_working_mb,
                cur.peak_working_mb,
                if ratio >= 1.0 {
                    format!("多了 {:.0}%", (ratio - 1.0) * 100.0)
                } else {
                    format!("少了 {:.0}%", (1.0 - ratio) * 100.0)
                }
            ));
        }
    }
    let rate = |m: &MemReport| m.page_faults as f64 / (m.duration_ms.max(1) as f64);
    let (ar, br) = (rate(prev), rate(cur));
    if ar > 0.0 && br / ar >= 2.0 {
        out.push(format!("缺页速率涨了 {:.1} 倍", br / ar));
    }
    if !prev.paging && cur.paging {
        out.push("上次没走到换页，这次走到了".into());
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemPoint {
    pub ms: u128,
    /// 目标进程
    pub proc_working_mb: u64,
    pub proc_commit_mb: u64,
    pub proc_faults: u64,
    /// 彼时的系统
    pub sys_commit_used_mb: u64,
    pub sys_commit_limit_mb: u64,
    pub phys_total_mb: u64,
    pub phys_avail_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemReport {
    /// 实际采到的次数。太少的结论不能信。
    pub samples: usize,
    pub duration_ms: u128,
    /// 采样够不够密。进程活得比采样间隔还短时是 false——
    /// 那时候报出来的「私有提交 0 MB」不是事实，是没采到。
    #[serde(default)]
    pub reliable: bool,
    /// 目标进程自己
    pub peak_working_mb: u64,
    pub peak_commit_mb: u64,
    pub page_faults: u64,
    /// 峰值那一刻的整个系统
    pub sys_commit_used_mb: u64,
    pub phys_total_mb: u64,
    pub phys_avail_at_peak_mb: u64,
    /// 判断：峰值时系统的 commit 有没有超过物理内存。
    /// 超了就是「在换页」——这是编译变慢最直接的证据。
    pub paging: bool,
    /// 峰值那一刻，除目标进程外占用最高的几个
    #[serde(default)]
    pub others: Vec<ProcMem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcMem {
    pub name: String,
    pub pid: u32,
    pub working_mb: u64,
}

// ---------------------------------------------------------------- FFI

#[cfg(windows)]
mod ffi {
    use super::ProcMem;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct ProcessMemoryCountersEx {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct PerformanceInformation {
        cb: u32,
        commit_total: usize,
        commit_limit: usize,
        commit_peak: usize,
        physical_total: usize,
        physical_available: usize,
        system_cache: usize,
        kernel_total: usize,
        kernel_paged: usize,
        kernel_nonpaged: usize,
        page_size: usize,
        handle_count: u32,
        process_count: u32,
        thread_count: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ProcessEntry32W {
        size: u32,
        usage: u32,
        pid: u32,
        default_heap: usize,
        module: u32,
        threads: u32,
        parent_pid: u32,
        pri_class_base: i32,
        flags: u32,
        exe: [u16; 260],
    }
    /// 手写 `Default` 而不是 derive：`Process32FirstW` 要求 `dwSize` 字段在**第一次**
    /// 调用之前就填好结构体大小，derive 出来是 0，调用会直接失败——而失败的表现是
    /// "一个进程都数不到"，看起来像机器上没跑东西，不会报错。这种错最难查。
    impl Default for ProcessEntry32W {
        fn default() -> Self {
            Self {
                size: std::mem::size_of::<ProcessEntry32W>() as u32,
                usage: 0,
                pid: 0,
                default_heap: 0,
                module: 0,
                threads: 0,
                parent_pid: 0,
                pri_class_base: 0,
                flags: 0,
                exe: [0; 260],
            }
        }
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(h: *mut core::ffi::c_void, c: *mut ProcessMemoryCountersEx, cb: u32) -> i32;
        fn GetPerformanceInfo(info: *mut PerformanceInformation, cb: u32) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
        fn CloseHandle(h: *mut core::ffi::c_void) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut core::ffi::c_void;
        fn Process32FirstW(snap: *mut core::ffi::c_void, e: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snap: *mut core::ffi::c_void, e: *mut ProcessEntry32W) -> i32;
        fn GetTickCount64() -> u64;
    }

    /// 开机以来的毫秒数。比时钟更稳，也不受系统时间改动影响。
    pub fn ticks() -> u64 {
        // SAFETY: 无入参，返回一个 u64。
        unsafe { GetTickCount64() }
    }

    fn proc_mem(h: *mut core::ffi::c_void) -> Option<(u64, u64, u64, u64)> {
        let mut c = ProcessMemoryCountersEx {
            cb: std::mem::size_of::<ProcessMemoryCountersEx>() as u32,
            ..Default::default()
        };
        // SAFETY: `h` 由调用方保证是刚成功打开的进程句柄；`c` 是栈上的出参结构，
        // `cb` 已按 `size_of` 填好，API 只写这块内存。
        let ok = unsafe { GetProcessMemoryInfo(h, &mut c, c.cb) };
        if ok == 0 {
            return None;
        }
        Some((
            (c.working_set / 1048576) as u64,
            (c.private_usage / 1048576) as u64,
            c.page_fault_count as u64,
            (c.peak_working_set / 1048576) as u64,
        ))
    }

    /// (commit 已用, commit 上限, 物理总量, 物理可用) 单位 MB
    pub fn system_mem() -> (u64, u64, u64, u64) {
        let mut p = PerformanceInformation {
            cb: std::mem::size_of::<PerformanceInformation>() as u32,
            ..Default::default()
        };
        // SAFETY: `p` 是栈上的出参结构，`cb` 已按 `size_of` 填好。
        let ok = unsafe { GetPerformanceInfo(&mut p, p.cb) };
        if ok == 0 {
            return (0, 0, 0, 0);
        }
        let m = |v: usize| (v.saturating_mul(p.page_size) / 1048576) as u64;
        (m(p.commit_total), m(p.commit_limit), m(p.physical_total), m(p.physical_available))
    }

    /// 一个进程的工作集和私有提交（MB）
    pub fn process_mem(pid: u32) -> Option<(u64, u64, u64)> {
        const QUERY_LIMITED: u32 = 0x1000;
        // SAFETY: 三个入参都是值。只要了 `QUERY_LIMITED_INFORMATION` 权限，
        // 不持有、也不修改目标进程；返回值下面立刻判空。
        let h = unsafe { OpenProcess(QUERY_LIMITED, 0, pid) };
        if h.is_null() {
            return None;
        }
        let r = proc_mem(h).map(|(w, c, f, _)| (w, c, f));
        // SAFETY: `h` 来自上面刚成功打开的句柄，这条路径上只关一次。
        unsafe { CloseHandle(h) };
        r
    }

    /// 当前进程自己（用来确认探针本身不吃内存）。
    /// 走 OpenProcess 而不是伪句柄——同一条路径只留一种走法，
    /// 两条路的差异会藏在最不该出问题的地方。
    pub fn self_mem() -> (u64, u64) {
        process_mem(std::process::id())
            .map(|(w, c, _)| (w, c))
            .unwrap_or((0, 0))
    }

    /// 此刻占用最高的几个进程。用来回答「当时还有谁占着」。
    pub fn top_consumers(limit: usize, exclude: &[u32]) -> Vec<ProcMem> {
        let mut all = snapshot_names();
        all.retain(|p| !exclude.contains(&p.pid));
        all.sort_by_key(|p| std::cmp::Reverse(p.working_mb));
        all.truncate(limit);
        all
    }

    /// 名字里含某个词（小写比较）的进程有几个。
    /// 状态栏要的"有几个 python 在跑"就走这条——枚举一次快照，
    /// 比每隔两秒 spawn 一个 tasklist.exe 便宜得多（那个真会吃 CPU）。
    pub fn count_by_name(needle: &str) -> usize {
        let n = needle.to_lowercase();
        snapshot_names()
            .into_iter()
            .filter(|p| p.name.to_lowercase().contains(&n))
            .count()
    }

    /// 只数名字，**不查内存**。
    ///
    /// 为什么要有这个"轻"版本：数名字只需要遍历一次进程列表，
    /// 而带上内存就要对全机每个进程做一次 OpenProcess + 查询工作集——
    /// 几百次系统调用。状态栏每几秒问一次，那点开销是白烧的 CPU，
    /// 也是界面"莫名变卡"的来源之一。
    /// 内存相关的功能（谁在吃内存）仍然走重量版，它问得不频繁。
    pub fn count_many_fast(needles: &[&str]) -> Vec<(String, usize)> {
        let names: Vec<String> = needles.iter().map(|n| n.to_lowercase()).collect();
        let mut out: Vec<(String, usize)> =
            needles.iter().map(|n| ((*n).to_string(), 0)).collect();
        const SNAP_PROCESS: u32 = 0x0000_0002;
        // SAFETY: 入参都是值；返回的句柄下面立刻判了 null 与 INVALID_HANDLE_VALUE。
        let snap = unsafe { CreateToolhelp32Snapshot(SNAP_PROCESS, 0) };
        if snap.is_null() || snap as isize == -1 {
            return out;
        }
        let mut e = ProcessEntry32W::default();
        // SAFETY: `snap` 是上面判过有效的快照句柄；`e` 是栈上的出参，
        // 它的 `Default` 已把 `dwSize` 填成结构体大小（这个 API 要求如此）。
        let mut ok = unsafe { Process32FirstW(snap, &mut e) } != 0;
        while ok {
            if e.pid != 0 {
                let end = e.exe.iter().position(|c| *c == 0).unwrap_or(e.exe.len());
                let lower = String::from_utf16_lossy(&e.exe[..end]).to_lowercase();
                for (i, n) in names.iter().enumerate() {
                    if lower.contains(n.as_str()) {
                        out[i].1 += 1;
                    }
                }
            }
            e.size = std::mem::size_of::<ProcessEntry32W>() as u32;
            // SAFETY: 同上；每轮都重设 `e.size`，因为这个 API 会改写入参结构。
            ok = unsafe { Process32NextW(snap, &mut e) } != 0;
        }
        // SAFETY: `snap` 是本函数创建的快照句柄，此处只关一次。
        unsafe { CloseHandle(snap) };
        out
    }

    /// 一次快照里数好几种进程，返回「名字 → 个数」。
    ///
    /// 为什么不循环调 count_by_name：那会重复做整份进程快照（每个进程还要取一次
    /// 工作集），状态栏每几秒问一次，重复的部分就是白烧的 CPU。顺手数完最省。
    pub fn count_many(needles: &[&str]) -> Vec<(String, usize)> {
        let names: Vec<String> = needles.iter().map(|n| n.to_lowercase()).collect();
        let mut out: Vec<(String, usize)> =
            needles.iter().map(|n| ((*n).to_string(), 0)).collect();
        for p in snapshot_names() {
            let lower = p.name.to_lowercase();
            for (i, n) in names.iter().enumerate() {
                if lower.contains(n.as_str()) {
                    out[i].1 += 1;
                }
            }
        }
        out
    }

    /// 一次进程快照：名字 + 工作集（MB）。所有"按名字找进程"的需求共用它。
    fn snapshot_names() -> Vec<ProcMem> {
        const SNAP_PROCESS: u32 = 0x0000_0002;
        // SAFETY: 入参都是值；返回的句柄下面立刻判了 null 与 INVALID_HANDLE_VALUE。
        let snap = unsafe { CreateToolhelp32Snapshot(SNAP_PROCESS, 0) };
        if snap.is_null() || snap as isize == -1 {
            return Vec::new();
        }
        let mut e = ProcessEntry32W::default();
        let mut found: Vec<ProcMem> = Vec::new();
        // SAFETY: `snap` 是上面判过有效的快照句柄；`e` 是栈上的出参，
        // 它的 `Default` 已把 `dwSize` 填成结构体大小（这个 API 要求如此）。
        let mut ok = unsafe { Process32FirstW(snap, &mut e) } != 0;
        while ok {
            if e.pid != 0 {
                if let Some((w, _, _)) = process_mem(e.pid) {
                    if w > 0 {
                        let end = e.exe.iter().position(|c| *c == 0).unwrap_or(e.exe.len());
                        found.push(ProcMem {
                            name: String::from_utf16_lossy(&e.exe[..end]),
                            pid: e.pid,
                            working_mb: w,
                        });
                    }
                }
            }
            e.size = std::mem::size_of::<ProcessEntry32W>() as u32;
            // SAFETY: 同上；每轮都重设 `e.size`，因为这个 API 会改写入参结构。
            ok = unsafe { Process32NextW(snap, &mut e) } != 0;
        }
        // SAFETY: `snap` 是本函数创建的快照句柄，此处只关一次。
        unsafe { CloseHandle(snap) };
        found
    }
}

#[cfg(not(windows))]
mod ffi {
    use super::ProcMem;
    pub fn ticks() -> u64 {
        0
    }
    pub fn system_mem() -> (u64, u64, u64, u64) {
        (0, 0, 0, 0)
    }
    pub fn process_mem(_pid: u32) -> Option<(u64, u64, u64)> {
        None
    }
    pub fn self_mem() -> (u64, u64) {
        (0, 0)
    }
    pub fn top_consumers(_limit: usize, _exclude: &[u32]) -> Vec<ProcMem> {
        Vec::new()
    }
    pub fn count_by_name(_needle: &str) -> usize {
        0
    }
    pub fn count_many(_needles: &[&str]) -> Vec<(String, usize)> {
        Vec::new()
    }
    pub fn count_many_fast(_needles: &[&str]) -> Vec<(String, usize)> {
        Vec::new()
    }
}

pub use ffi::{count_by_name, count_many, count_many_fast, self_mem, process_mem, system_mem, ticks, top_consumers};

/// 采样器：跑项目期间开着，跑完收工。
///
/// 不常驻是有意的——一个常驻的监控面板，用户不会在编译时盯着它看，
/// 却要一直为它付资源。采完就把结论留下，过程数据丢掉。
pub struct Sampler {
    pid: u32,
    started: u64,
    peak_working_mb: u64,
    peak_commit_mb: u64,
    faults: u64,
    at_peak: Option<MemPoint>,
    /// 最后一次采到东西的时刻。短命进程可能在峰值点还没提交内存，
    /// 收尾时用它兜底。
    last: Option<(u64, u64)>,
    samples: usize,
}

impl Sampler {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            started: ticks(),
            peak_working_mb: 0,
            peak_commit_mb: 0,
            faults: 0,
            at_peak: None,
            last: None,
            samples: 0,
        }
    }

    pub fn tick(&mut self) {
        let Some((working, commit, faults)) = process_mem(self.pid) else {
            return;
        };
        self.faults = faults;
        self.samples += 1;
        self.last = Some((working, commit));
        // 两个指标各取各的峰值，不能绑在一起。
        // 刚 CreateProcess 出来的那一瞬，工作集里大多是映射进来的镜像
        //（算工作集、不算私有提交），这时候绑着记会记下一个 0。
        if commit > self.peak_commit_mb {
            self.peak_commit_mb = commit;
        }
        if working > self.peak_working_mb {
            let (used, limit, total, avail) = system_mem();
            self.peak_working_mb = working;
            self.at_peak = Some(MemPoint {
                ms: (ticks().saturating_sub(self.started)) as u128,
                proc_working_mb: working,
                proc_commit_mb: commit,
                proc_faults: faults,
                sys_commit_used_mb: used,
                sys_commit_limit_mb: limit,
                phys_total_mb: total,
                phys_avail_mb: avail,
            });
        }
    }

    /// 收工时把结论整出来。只有在真的采到东西时才返回。
    pub fn finish(self) -> Option<MemReport> {
        // 基准点：优先用工作集峰值那一刻的系统状态；
        // 一次峰值都没采到（进程太快，还没长起来就没了）就退回最后一次采样。
        let p = match &self.at_peak {
            Some(at) => at.clone(),
            None => {
                let (lw, lc) = self.last?;
                MemPoint {
                    ms: (ticks().saturating_sub(self.started)) as u128,
                    proc_working_mb: lw,
                    proc_commit_mb: lc,
                    proc_faults: self.faults,
                    ..Default::default()
                }
            }
        };
        let (used, _limit, total, _avail) = system_mem();
        let dur = ticks().saturating_sub(self.started);
        // 判据：那一刻系统的 commit 已经压过物理内存，
        // 或者物理可用几乎见底——两者都意味着接下来要往磁盘上搬。
        let paging = p.sys_commit_used_mb > p.phys_total_mb
            || (p.phys_avail_mb < 512 && p.phys_total_mb > 0);
        Some(MemReport {
            samples: self.samples,
            reliable: self.samples >= 2 && dur >= 120,
            duration_ms: (ticks().saturating_sub(self.started)) as u128,
            peak_working_mb: self.peak_working_mb.max(p.proc_working_mb),
            peak_commit_mb: self.peak_commit_mb.max(p.proc_commit_mb),
            page_faults: self.faults,
            sys_commit_used_mb: if p.sys_commit_used_mb > used { p.sys_commit_used_mb } else { used },
            phys_total_mb: if total > 0 { total } else { p.phys_total_mb },
            phys_avail_at_peak_mb: p.phys_avail_mb,
            paging,
            others: top_consumers(6, &[self.pid]),
        })
    }
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    /// 轻路径 vs 重路径的实测对比。默认忽略；要看数字就：
    /// `cargo test -p jingwei-engine perf_snapshot -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn perf_snapshot_light_vs_heavy() {
        let n = ["python", "node", "cargo"];
        let t0 = std::time::Instant::now();
        let fast = count_many_fast(&n);
        let t1 = t0.elapsed();
        let t2 = std::time::Instant::now();
        let heavy = count_many(&n);
        let t3 = t2.elapsed();
        println!("fast={:?} heavy={:?} fast={:?} heavy={:?}", t1, t3, fast, heavy);
        assert_eq!(fast.len(), heavy.len());
    }

    use super::*;

    #[test]
    fn system_memory_numbers_are_plausible() {
        if !cfg!(windows) {
            return;
        }
        let (used, limit, total, avail) = system_mem();
        assert!(total > 1024, "物理内存读出来只有 {} MB", total);
        assert!(used > 0 && used < limit, "commit {} / {} 不合理", used, limit);
        assert!(avail <= total);
    }

    #[test]
    fn we_can_read_our_own_memory() {
        let (w, c) = self_mem();
        if cfg!(windows) {
            assert!(w > 0, "读自己的工作集是 0");
            assert!(c > 0, "读自己的私有提交是 0");
        }
    }

    #[test]
    fn top_consumers_never_returns_the_excluded_pid() {
        let me = std::process::id();
        let top = top_consumers(5, &[me]);
        assert!(!top.iter().any(|p| p.pid == me), "排除掉的进程不该出现在榜上");
    }

    /// 采样器对自己进程采一轮，必须真的采到东西。
    /// 采不到就说明 FFI 结构体或句柄有问题——那会让整个功能静默失效。
    #[test]
    fn sampler_actually_observes_something() {
        if !cfg!(windows) {
            return;
        }
        let mut s = Sampler::new(std::process::id());
        for _ in 0..3 {
            s.tick();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let r = s.finish().expect("采样器什么都没采到");
        assert!(r.peak_working_mb > 0, "峰值工作集不该是 0");
        assert!(r.phys_total_mb > 1024);
    }

    fn rep(peak: u64, faults: u64, dur: u128, paging: bool) -> MemReport {
        MemReport {
            reliable: true,
            peak_working_mb: peak,
            page_faults: faults,
            duration_ms: dur,
            paging,
            ..Default::default()
        }
    }

    #[test]
    fn a_real_jump_is_reported() {
        let d = compare(&rep(11, 3_000, 1_000, false), &rep(311, 80_000, 2_000, false));
        assert!(d.iter().any(|x| x.contains("11 MB → 311 MB")), "{:?}", d);
        assert!(d.iter().any(|x| x.contains("缺页速率")), "{:?}", d);
    }

    /// 两次差不多时什么都不说。噪声会让「和上次比」这一行迅速失去意义。
    #[test]
    fn a_small_wobble_stays_silent() {
        let d = compare(&rep(300, 10_000, 2_000, false), &rep(310, 10_500, 2_000, false));
        assert!(d.is_empty(), "5% 的波动不该报出来：{:?}", d);
    }

    /// 采不准的数字不拿来比——拿两个都不准的数比出一个结论，是最坏的情况。
    #[test]
    fn unreliable_samples_are_never_compared() {
        let mut prev = rep(11, 3_000, 1_000, false);
        prev.reliable = false;
        assert!(compare(&prev, &rep(311, 80_000, 2_000, false)).is_empty());
        let mut cur = rep(311, 80_000, 2_000, false);
        cur.reliable = false;
        assert!(compare(&rep(11, 3_000, 1_000, false), &cur).is_empty());
    }

    #[test]
    fn crossing_into_paging_is_called_out() {
        let d = compare(&rep(200, 5_000, 2_000, false), &rep(210, 5_000, 2_000, true));
        assert!(d.iter().any(|x| x.contains("换页")), "{:?}", d);
    }
}
