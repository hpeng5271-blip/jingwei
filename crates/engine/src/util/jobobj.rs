//! 进程树管控：给子进程套一个 Job Object，收工时连同它的子孙一起收掉。
//!
//! 为什么必须有：`cargo run` / `npm run dev` / `mvn spring-boot:run` 拉起来的
//! 都是"一棵树"（构建器再拉编译器、编译再拉链接器）。只 kill 父进程，
//! 子进程会变成孤儿继续占端口和内存——用户看到的现象是"我明明关了，
//! 端口还占着"。Windows 上正确的做法就是 Job Object + KILL_ON_JOB_CLOSE。
//!
//! 边界：进程在创建后、加入 Job 前的那一瞬间是自由的（理论窗口）。
//! 要彻底堵这个缝需要 CREATE_SUSPENDED + 手工 Assign + ResumeThread，
//! 代价是不能再走 std 的 Command。对"跑一个项目"这个场景，先到手即用，
//! 并在文档里写清这个边界，而不是假装它不存在。
use anyhow::{bail, Result};
use std::os::windows::io::AsRawHandle;
use std::process::Child;

#[cfg(windows)]
mod ffi {
    use std::os::windows::io::RawHandle;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateJobObjectW(attrs: *mut core::ffi::c_void, name: *const u16) -> RawHandle;
        pub fn SetInformationJobObject(
            job: RawHandle,
            class: u32,
            info: *mut core::ffi::c_void,
            len: u32,
        ) -> i32;
        pub fn AssignProcessToJobObject(job: RawHandle, process: RawHandle) -> i32;
        pub fn TerminateJobObject(job: RawHandle, code: u32) -> i32;
        pub fn CloseHandle(h: RawHandle) -> i32;
    }
}

/// 句柄 RAII：忘了关句柄会漏资源，而漏的正好是"能让整棵树活着"的东西。
#[derive(Debug)]
pub struct Job {
    handle: std::os::windows::io::RawHandle,
    terminated: std::sync::atomic::AtomicBool,
}

// 句柄可以在线程之间传（Windows 句柄是进程级的）。
//
// SAFETY: `Job` 的可变状态只有一个原子 bool `terminated`，其他字段是句柄本身；
// 句柄的跨线程传递是 Windows 的既定语义，所以 `Send`/`Sync` 成立。
unsafe impl Send for Job {}
// SAFETY: 同上——唯一的可变状态是原子量，多线程共享不会产生数据竞争。
unsafe impl Sync for Job {}

impl Job {
    /// 建一个 Job，并让它"最后一个句柄关掉时杀掉里面所有进程"。
    pub fn new() -> Result<Job> {
        // JOBOBJECT_EXTENDED_LIMIT_INFORMATION 里的 LimitFlags 在最前
        #[repr(C)]
        struct IoCounters {
            read_ops: u64,
            write_ops: u64,
            other_ops: u64,
            read_bytes: u64,
            write_bytes: u64,
            other_bytes: u64,
        }
        #[repr(C)]
        struct BasicLimit {
            per_process_time: i64,
            job_time: i64,
            limit_flags: u32,
            min_ws: usize,
            max_ws: usize,
            active_procs: u32,
            affinity: usize,
            priority: u32,
            scheduling: u32,
        }
        #[repr(C)]
        struct ExtendedLimit {
            basic: BasicLimit,
            io: IoCounters,
            process_memory: usize,
            job_memory: usize,
            peak_process: usize,
            peak_job: usize,
        }
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

        // SAFETY: 两个入参都是 null——默认安全属性、无名对象。
        let h = unsafe { ffi::CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if h.is_null() {
            bail!("CreateJobObject 失败");
        }
        let mut info = ExtendedLimit {
            basic: BasicLimit {
                per_process_time: 0,
                job_time: 0,
                limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                min_ws: 0,
                max_ws: 0,
                active_procs: 0,
                affinity: 0,
                priority: 0,
                scheduling: 0,
            },
            io: IoCounters {
                read_ops: 0,
                write_ops: 0,
                other_ops: 0,
                read_bytes: 0,
                write_bytes: 0,
                other_bytes: 0,
            },
            process_memory: 0,
            job_memory: 0,
            peak_process: 0,
            peak_job: 0,
        };
        // SAFETY: `h` 是上面刚创建并判过非空的 Job 句柄；`info` 是栈上按 `#[repr(C)]`
        // 布局的结构，长度按 `size_of` 给出，API 只读它。
        let ok = unsafe {
            ffi::SetInformationJobObject(
                h,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                &mut info as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<ExtendedLimit>() as u32,
            )
        };
        if ok == 0 {
            // SAFETY: `h` 是本函数创建、且这条失败路径只走一次的句柄。
            unsafe { ffi::CloseHandle(h) };
            bail!("SetInformationJobObject 失败");
        }
        Ok(Job {
            handle: h,
            terminated: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// 把一个已经在跑的进程收进这棵树。
    pub fn assign(&self, child: &Child) -> Result<()> {
        // SAFETY: `self.handle` 由 `Job::new` 建立并活到 `Drop`；`child` 的裸句柄
        // 由标准库保证在 `child` 存活期间有效。
        let ok = unsafe { ffi::AssignProcessToJobObject(self.handle, child.as_raw_handle()) };
        if ok == 0 {
            bail!("AssignProcessToJobObject 失败（可能已在别的 Job 里）");
        }
        Ok(())
    }

    /// 收掉整棵树。
    pub fn terminate(&self) -> Result<()> {
        if self.terminated.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        // SAFETY: `self.handle` 同上仍然有效；上面的原子交换保证这条路径只执行一次。
        let ok = unsafe { ffi::TerminateJobObject(self.handle, 1) };
        if ok == 0 {
            bail!("TerminateJobObject 失败");
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // 句柄一关，KILL_ON_JOB_CLOSE 会把剩下的一起带走。
        //
        // SAFETY: 这是该句柄的唯一所有者——`Job` 没有实现 `Clone`，`Drop` 只跑一次。
        unsafe { ffi::CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    /// 真跑一次：起一个会生子孙的进程，收掉 Job 之后它们必须都没了。
    #[test]
    fn terminating_the_job_takes_the_whole_tree_down() {
        let job = Job::new().expect("建 Job 失败");
        // cmd 里再拉一个 ping：这就是"一棵树"的最小模型
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 20 127.0.0.1 > nul"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("起 cmd 失败");
        let pid = child.id();
        job.assign(&child).expect("加入 Job 失败");
        assert!(is_alive(pid), "刚起来应该还活着；pid={} 的 tasklist 输出：{}", pid, tasklist_dump(pid));
        job.terminate().expect("收 Job 失败");
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(!is_alive(pid), "收掉 Job 之后子进程必须跟着走");
        let _ = child.wait();
    }

    fn is_alive(pid: u32) -> bool {
        tasklist_dump(pid).contains(&pid.to_string())
    }

    fn tasklist_dump(pid: u32) -> String {
        match std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
        {
            Ok(o) => crate::textfile::decode_output(&o.stdout),
            Err(_) => String::new(),
        }
    }
}
