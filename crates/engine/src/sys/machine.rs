//! 这台机器本身的情况：CPU、内存、磁盘、系统版本、编译器。
//!
//! 数据尽量直接问系统（FFI），不起进程也不引依赖——这些数字每次打开界面都要看，
//! 慢一点都不合适。只有 CPU 型号藏得深（在注册表里），才退回 `reg query`。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiskInfo {
    pub drive: String,
    pub total_gb: f64,
    pub free_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Machine {
    pub os_name: String,
    /// 注册表 ProductName 的原文。留着是为了能解释「为什么两处说法不一样」。
    pub os_product: String,
    pub os_version: String,
    pub os_build: String,
    pub cpu: String,
    pub cores: usize,
    pub arch: String,
    pub ram_total_mb: u64,
    pub ram_free_mb: u64,
    pub disks: Vec<DiskInfo>,
}

#[cfg(windows)]
mod ffi {
    use super::DiskInfo;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct FileTime {
        pub low: u32,
        pub high: u32,
    }
    impl FileTime {
        pub fn ticks(self) -> u64 {
            ((self.high as u64) << 32) | self.low as u64
        }
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct SystemInfo {
        arch: u16,
        _r1: u16,
        page_size: u32,
        min_app: *mut u8,
        max_app: *mut u8,
        active_mask: usize,
        number_of_processors: u32,
        processor_type: u32,
        allocation_granularity: u32,
        processor_level: u16,
        processor_revision: u16,
    }

    extern "system" {
        fn GlobalMemoryStatusEx(s: *mut MemoryStatusEx) -> i32;
        fn GetSystemInfo(s: *mut SystemInfo);
        fn GetLogicalDrives() -> u32;
        fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        fn GetDiskFreeSpaceExW(
            dir: *const u16,
            free_available: *mut u64,
            total: *mut u64,
            total_free: *mut u64,
        ) -> i32;
    }

    pub fn memory_mb() -> (u64, u64) {
        let mut s = MemoryStatusEx {
            length: std::mem::size_of::<MemoryStatusEx>() as u32,
            ..Default::default()
        };
        // SAFETY: `s` 是本函数栈上的出参结构，`length` 已按 `size_of` 填好；
        // 该 API 只往这块内存里写，不会保存指针到调用之后。
        unsafe {
            if GlobalMemoryStatusEx(&mut s) == 0 {
                return (0, 0);
            }
        }
        (s.total_phys / 1048576, s.avail_phys / 1048576)
    }

    pub fn cores() -> usize {
        let mut s = SystemInfo::default();
        // SAFETY: 无入参；结果写进栈上默认初始化的 `SystemInfo`，布局与 Windows 一致。
        unsafe { GetSystemInfo(&mut s) };
        s.number_of_processors as usize
    }
    /// 屏幕分辨率。GetSystemMetrics(0/1) 是主显示器的像素尺寸，调用是常数级。
    pub fn screen() -> (i32, i32) {
        // 必须标出它在哪个库。忘了标的话，GUI 那边因为 Tauri 自己链了 user32
        // 照样能过，纯命令行版才会在链接期报「无法解析的外部符号」。
        #[link(name = "user32")]
        extern "system" {
            fn GetSystemMetrics(index: i32) -> i32;
            fn SetProcessDPIAware() -> i32;
        }
        // DPI 未感知的进程拿到的是「虚拟化之后」的尺寸——2560x1600 的屏
        // 会被报成 1707x1067。这是实打实的读错，必须先把感知等级提上去。
        static ONCE: std::sync::Once = std::sync::Once::new();
        // SAFETY: 无参、进程级一次性的显示设置调用；`Once` 保证只执行一次。
        ONCE.call_once(|| unsafe {
            SetProcessDPIAware();
        });
        // SAFETY: 两个查询都无入参，只读当前进程的桌面尺寸。
        unsafe { (GetSystemMetrics(0), GetSystemMetrics(1)) }
    }

    /// 现在是在用电源还是用电池。Some(true) = 插着电。
    ///
    /// 这件事决定了「电源计划该不该是高性能」——笔记本用电池时
    /// 本来就该是平衡省电，那不是问题。不先问这一句就报，就是误报。
    pub fn on_ac_power() -> Option<bool> {
        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct PowerStatus {
            ac_line: u8,
            battery_flag: u8,
            battery_percent: u8,
            system_status: u8,
            battery_life: u32,
            battery_full_life: u32,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetSystemPowerStatus(s: *mut PowerStatus) -> i32;
        }
        let mut s = PowerStatus::default();
        // SAFETY: `s` 是 `#[repr(C)]` 的出参结构（布局与 Windows 一致）且已默认初始化，
        // 该 API 只写不读。
        if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
            return None;
        }
        match s.ac_line {
            0 => Some(false),
            1 => Some(true),
            _ => None, // 255 = 未知（台式机、虚拟机）
        }
    }

    pub fn disks() -> Vec<DiskInfo> {
        // SAFETY: 无入参，返回的是位掩码，不涉及指针。
        let mask = unsafe { GetLogicalDrives() };
        let mut out = Vec::new();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root = format!("{}:\\", letter);
            let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            let (mut free_avail, mut total, mut total_free) = (0u64, 0u64, 0u64);
            // SAFETY: `wide` 是以 NUL 结尾的 UTF-16 串，在本行归还前一直存活；
            // 三个出参都是栈上的 u64。失败时 API 不写入它们。
            let ok = unsafe {
                GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_avail, &mut total, &mut total_free)
            };
            // 光驱、没插卡的读卡器会失败，跳过就好
            if ok == 0 || total == 0 {
                continue;
            }
            out.push(DiskInfo {
                drive: root,
                total_gb: (total as f64) / 1_073_741_824.0,
                free_gb: (total_free as f64) / 1_073_741_824.0,
            });
        }
        out
    }

    /// (idle, kernel, user) 三个累计计数。CPU 占用要靠两次采样求差。
    pub fn system_times() -> Option<(u64, u64, u64)> {
        let (mut i, mut k, mut u) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        // SAFETY: 三个 `FileTime` 出参都在栈上且已初始化。
        let ok = unsafe { GetSystemTimes(&mut i, &mut k, &mut u) };
        if ok == 0 {
            return None;
        }
        Some((i.ticks(), k.ticks(), u.ticks()))
    }
}

#[cfg(not(windows))]
mod ffi {
    use super::DiskInfo;
    pub fn system_times() -> Option<(u64, u64, u64)> {
        None
    }
    pub fn memory_mb() -> (u64, u64) {
        (0, 0)
    }
    pub fn cores() -> usize {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
    }
    pub fn disks() -> Vec<DiskInfo> {
        Vec::new()
    }
}

/// 列一个注册表键的子键，顺带把每个子键的 DriverDesc 取出来。
/// 显卡就是这么找的：一个类下挂着一串 0000/0001…，每个写着它是谁。
#[cfg(windows)]
fn reg_list_subkeys(path: &str) -> Vec<(String, String)> {
    let Some(reg_exe) = std::env::var("SystemRoot")
        .map(|r| std::path::PathBuf::from(r).join("System32").join("reg.exe"))
        .ok()
    else {
        return Vec::new();
    };
    let Some((true, listing)) = crate::winutil::run_cmd(
        &reg_exe,
        &["query", path],
        std::time::Duration::from_secs(8),
    ) else {
        return Vec::new();
    };
    // reg.exe 回的是全路径（HKEY_LOCAL_MACHINE\…），而我们传进去的是 HKLM\…，
    // 按前缀截永远截不到。取每一行的最后一段就够了——不用管前缀长什么样。
    let names: Vec<String> = listing
        .lines()
        .filter_map(|l| l.trim().rsplit('\\').next())
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let mut out = Vec::new();
    for name in names {
        let sub = format!(r"{}\{}", path, name);
        // 不同厂商把显卡名放在不同字段里，挨个试过去
        let desc = ["DriverDesc", "Device Description", "ProviderName"]
            .iter()
            .find_map(|k| reg(&sub, k))
            .unwrap_or_default();
        out.push((name, desc));
    }
    out
}

#[cfg(not(windows))]
fn reg_list_subkeys(_path: &str) -> Vec<(String, String)> {
    Vec::new()
}

/// 一条内存条。这些数据 WMI 也能给，但 WMI 在这台机器上要 20 秒一次。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RamStick {
    pub size_mb: u64,
    /// MT/s
    pub speed: u32,
    /// DDR4 / DDR5 / LPDDR5…
    pub kind: String,
    pub maker: String,
    pub part: String,
}

/// SMBIOS 内存设备结构（type 17）里的字段偏移。
///
/// 这些数是拿真数据对出来的，不是凭记忆写的——我一开始把 Size 记成了 0x08，
/// 那其实是总线宽度，于是 8 GB 的条子被读成 64 MB。这种地方错一格就是错的，
/// 而且看起来还挺像回事。
mod smbios_off {
    pub const SIZE: usize = 0x0C;
    pub const MEMORY_TYPE: usize = 0x12;
    pub const SPEED: usize = 0x15;
    pub const MAKER: usize = 0x17;
    pub const PART: usize = 0x1A;
    pub const EXT_SIZE: usize = 0x1C;
    /// 表头：Used20Method + 版本 3 字节 + 表长 4 字节
    pub const TABLE_START: usize = 8;
}

fn kind_of(t: u8) -> &'static str {
    match t {
        0x13 => "DDR2",
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1E | 0x1F => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => "未知",
    }
}

fn w16(b: &[u8], i: usize) -> u32 {
    if i + 1 < b.len() {
        u16::from_le_bytes([b[i], b[i + 1]]) as u32
    } else {
        0
    }
}

fn w32(b: &[u8], i: usize) -> u32 {
    if i + 3 < b.len() {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    } else {
        0
    }
}

/// 结构体后面的字符串区：一串以 \0 结尾的 ASCII，最后多一个 \0 收尾。
fn smbios_string(b: &[u8], from: usize, to: usize, idx: u8) -> String {
    if idx == 0 {
        return String::new();
    }
    let mut n = 1u8;
    let mut i = from;
    while i < to {
        let start = i;
        while i < to && b[i] != 0 {
            i += 1;
        }
        if i == start {
            return String::new(); // 空串 = 列表结束
        }
        if n == idx {
            return String::from_utf8_lossy(&b[start..i]).trim().to_string();
        }
        n += 1;
        i += 1;
    }
    String::new()
}

pub fn parse_smbios_memory(d: &[u8]) -> Vec<RamStick> {
    let mut out = Vec::new();
    let mut i = smbios_off::TABLE_START;
    while i + 4 <= d.len() {
        let ty = d[i];
        let len = d[i + 1] as usize;
        if len < 4 || i + len > d.len() {
            break;
        }
        if ty == 0x7F {
            break; // end-of-table
        }
        // 字符串区：从结构末尾走到连续两个 0
        let mut j = i + len;
        while j + 1 < d.len() && !(d[j] == 0 && d[j + 1] == 0) {
            j += 1;
        }
        let str_end = j.min(d.len());
        if ty == 0x11 && len >= 0x1B {
            let raw = w16(d, i + smbios_off::SIZE);
            let size_mb = match raw {
                0 | 0xFFFF => 0, // 空槽位 / 未知
                0x7FFF if len >= 0x20 => w32(d, i + smbios_off::EXT_SIZE) as u64,
                v => v as u64,
            };
            if size_mb > 0 {
                out.push(RamStick {
                    size_mb,
                    speed: w16(d, i + smbios_off::SPEED),
                    kind: kind_of(d[i + smbios_off::MEMORY_TYPE]).to_string(),
                    maker: smbios_string(d, i + len, str_end, d[i + smbios_off::MAKER]),
                    part: smbios_string(d, i + len, str_end, d[i + smbios_off::PART]),
                });
            }
        }
        i = j + 2;
    }
    out
}

#[cfg(windows)]
pub fn smbios_memory() -> Vec<RamStick> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemFirmwareTable(provider: u32, id: u32, buf: *mut u8, size: u32) -> u32;
    }
    const RSMB: u32 = 0x5253_4D42; // 'RSMB'
    // SAFETY: 这一步只为问长度——不传缓冲区、大小为 0，API 不会写任何内存。
    let n = unsafe { GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0) };
    if n == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; n as usize];
    // SAFETY: `buf` 按上一步给出的 `n` 字节分配，`n` 在这两行之间没有变过；
    // API 最多写 `n` 字节。
    let got = unsafe { GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr(), n) };
    if got == 0 {
        return Vec::new();
    }
    buf.truncate(got as usize);
    parse_smbios_memory(&buf)
}

#[cfg(not(windows))]
pub fn smbios_memory() -> Vec<RamStick> {
    Vec::new()
}

/// 一个事实，以及它是从哪几个独立来源拿到的。
///
/// 「可能会读不出来，但是不能读错」——落地就靠这个结构：
/// 单来源的值、和两个来源互相印证的值，可信度不是一回事；
/// 两个来源打架时必须把分歧摆出来，而不是挑一个顺眼的。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
    pub sources: Vec<String>,
    /// 各来源说法一致吗。false = 有分歧，那个值可能不准
    pub agree: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<String>,
    /// 值得说一句的解释。比如两个来源差一点点，但那是正常现象——
    /// 与其标「有分歧」让人以为出事了，不如直接告诉他差额去哪了。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Fact {
    fn merge(label: &str, claims: Vec<(String, String)>) -> Fact {
        let mut vals: Vec<String> = Vec::new();
        for (_, v) in &claims {
            if !v.trim().is_empty() && !vals.contains(v) {
                vals.push(v.clone());
            }
        }
        let agree = vals.len() <= 1;
        Fact {
            label: label.to_string(),
            value: vals.first().cloned().unwrap_or_default(),
            sources: claims.into_iter().map(|(s, _)| s).collect(),
            agree,
            detail: if agree { Vec::new() } else { vals },
            note: None,
        }
    }
    fn one(label: &str, value: String, source: &str) -> Fact {
        Fact {
            label: label.to_string(),
            value,
            sources: vec![source.to_string()],
            agree: true,
            detail: Vec::new(),
            note: None,
        }
    }
}

/// CPU 型号的第二个来源：CPUID 品牌串。
/// 注册表里那份是 Windows 抄来的，这份是 CPU 自己报的——两个都读，
/// 对得上才敢说「这就是它的型号」。
#[cfg(target_arch = "x86_64")]
fn cpuid_brand() -> String {
    use std::arch::x86_64::__cpuid;
    let mut s = [0u8; 48];
    for i in 0..3u32 {
        let r = __cpuid(0x8000_0002 + i);
        let o = (i * 16) as usize;
        s[o..o + 4].copy_from_slice(&r.eax.to_le_bytes());
        s[o + 4..o + 8].copy_from_slice(&r.ebx.to_le_bytes());
        s[o + 8..o + 12].copy_from_slice(&r.ecx.to_le_bytes());
        s[o + 12..o + 16].copy_from_slice(&r.edx.to_le_bytes());
    }
    // 注册表/系统信息里的中文也要按本机代码页认，不然机器清单全是问号
    crate::textfile::decode_output(&s)
        .replace('\0', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(not(target_arch = "x86_64"))]
fn cpuid_brand() -> String {
    String::new()
}

/// 这台机器装的是什么。每个事实都标明来源；来源对不上就摆出来，不挑顺眼的。
pub fn spec_report() -> Vec<Fact> {
    let hw = r"HKLM\HARDWARE\DESCRIPTION\System";
    let cpu_key = format!(r"{}\CentralProcessor\0", hw);
    let bios_key = format!(r"{}\BIOS", hw);
    let (ram_ffi, _) = ffi::memory_mb();
    let sticks = smbios_memory();
    let ram_sum: u64 = sticks.iter().map(|s| s.size_mb).sum();

    let mut out = vec![
        Fact::merge(
            "处理器",
            vec![
                ("注册表".into(), reg(&cpu_key, "ProcessorNameString").unwrap_or_default()),
                ("CPUID".into(), cpuid_brand()),
            ],
        ),
        Fact::merge(
            "核心",
            vec![
                (
                    "系统接口".into(),
                    if ffi::cores() > 0 {
                        format!("{} 个逻辑处理器", ffi::cores())
                    } else {
                        String::new()
                    },
                ),
                (
                    "注册表".into(),
                    reg(&cpu_key, "NumberOfCores")
                        .filter(|v| !v.trim().is_empty())
                        .map(|v| format!("{} 个物理核", v))
                        .unwrap_or_default(),
                ),
            ],
        ),
        // 内存总量单独处理：它要判断两个来源的差额是不是正常范围
    ];
    // 内存总量：一个来自系统调用，一个来自 SMBIOS 各条相加。
    // 差几百兆是正常的（集显和固件会占掉一截），差一条条子的量级才是问题。
    {
        let mut f = Fact::merge(
            "内存总量",
            vec![
                (
                    "系统接口".into(),
                    if ram_ffi > 0 { format!("{:.1} GB", ram_ffi as f64 / 1024.0) } else { String::new() },
                ),
                (
                    "SMBIOS 相加".into(),
                    if ram_sum > 0 { format!("{:.1} GB", ram_sum as f64 / 1024.0) } else { String::new() },
                ),
            ],
        );
        let gap = (ram_sum as f64 - ram_ffi as f64) / 1024.0;
        if gap > 0.0 && gap < 1.0 {
            // 差得不多：不是问题，是常识。说清楚比标红有用。
            f.agree = true;
            f.detail.clear();
            f.value = format!("{:.1} GB", ram_ffi as f64 / 1024.0);
            f.note = Some(format!(
                "插了两条共 {:.1} GB，系统只报得出 {:.1} GB——差的 {:.1} GB 是集显和固件保留的，正常",
                ram_sum as f64 / 1024.0, ram_ffi as f64 / 1024.0, gap
            ));
        }
        out.push(f);
    }
    if let Some(mhz) = reg(&cpu_key, "~MHz").and_then(|v| v.trim().parse::<u32>().ok()) {
        out.push(Fact::one("标称主频", format!("{:.2} GHz", mhz as f64 / 1000.0), "注册表"));
    }
    for (i, s) in sticks.iter().enumerate() {
        let mut v = format!(
            "{:.0} GB {} · {} MT/s",
            s.size_mb as f64 / 1024.0,
            s.kind,
            s.speed
        );
        let tail = format!("{} {}", s.maker, s.part).trim().to_string();
        if !tail.is_empty() {
            v.push_str(" · ");
            v.push_str(&tail);
        }
        out.push(Fact::one(&format!("内存条 {}", i + 1), v, "SMBIOS"));
    }
    for g in gpu_names() {
        out.push(Fact::one("显卡", g, "注册表"));
    }
    for (label, key) in [
        ("厂商", "SystemManufacturer"),
        ("整机型号", "SystemProductName"),
        ("主板", "BaseBoardProduct"),
        ("BIOS 版本", "BIOSVersion"),
    ] {
        if let Some(v) = reg(&bios_key, key).filter(|v| !v.trim().is_empty()) {
            out.push(Fact::one(label, v, "注册表"));
        }
    }
    // 硬盘：设备自报的完整型号 + 注册表那个残缺的，两个来源摆一起。
    // 注册表的 Prod_ 字段只有 16 字节，装不下 21 字符的 NVMe 型号——
    // 这不是谁读错了，是两个来源能看到的长度本来就不同。
    // 硬盘型号：两个注册表位置，一个完整一个截断，正好说明多源验证的意义。
    // Enum\SCSI\...\实例\FriendlyName 是设备自报的完整名字；
    // Services\disk\Enum\Prod_ 是 SCSI 标识，16 字节封顶。
    let full = disk_full_models();
    let short = disk_models();
    for (i, m) in full.iter().enumerate() {
        let mut f = Fact::one(&format!("硬盘 {}", i + 1), m.clone(), "注册表（设备自报）");
        if let Some(s) = short.get(i) {
            if !s.eq_ignore_ascii_case(m) {
                f.note = Some(format!(
                    "同一块盘在 Services\\disk\\Enum 里写作「{}」——那是 SCSI 产品标识，固定 16 字节，装不下完整型号",
                    s
                ));
            }
        }
        out.push(f);
    }
    for p in disk_properties() {
        if !p.firmware.is_empty() {
            out.push(Fact::one("硬盘固件", p.firmware.clone(), "设备描述符"));
        }
    }
    for d in ffi::disks() {
        out.push(Fact::one(
            &format!("{} 容量", d.drive.trim_end_matches('\\')),
            format!("剩余 {:.0} GB / 共 {:.0} GB", d.free_gb, d.total_gb),
            "系统接口",
        ));
    }
    let (sw, sh) = ffi::screen();
    if sw > 0 {
        out.push(Fact::one("屏幕", format!("{}x{}", sw, sh), "系统接口"));
    }
    // 影响开发的系统设置。它们不在硬件里，但直接改变编译快不快。
    out.extend(system_settings());
    out
}

/// 从这些事实能得出什么专业判断。只讲能证实的，不猜。
pub fn spec_judgements(facts: &[Fact]) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(f) = facts.iter().find(|f| f.label == "内存总量") {
        if !f.agree && f.detail.len() >= 2 {
            // 系统报的可用内存天生比条子之和少一点——集显要占、固件要占。
            // 差几百兆是正常现象，报出来就是误报。只有差到一条内存的量级，
            // 才说明真有东西没被认出来。
            let nums: Vec<f64> = f
                .detail
                .iter()
                .filter_map(|d| d.split_whitespace().next().and_then(|v| v.parse().ok()))
                .collect();
            if nums.len() >= 2 && (nums[0] - nums[1]).abs() >= 1.0 {
                out.push(format!(
                    "内存总量两个来源差 {:.1} GB（{}）。差到一条内存的量级，才说明有条子没被识别，或者插槽里是条坏的",
                    (nums[0] - nums[1]).abs(),
                    f.detail.join(" vs ")
                ));
            }
        }
    }
    let sticks: Vec<&Fact> = facts
        .iter()
        .filter(|f| f.label.starts_with("内存条"))
        .collect();
    if sticks.len() == 1 {
        out.push(
            "只认出一条内存条：这台机器跑单通道，带宽只有一半。编译和虚拟机最先受影响".into(),
        );
    } else if sticks.len() > 1 {
        let part = |f: &Fact, n: usize| -> String {
            f.value.split('·').nth(n).unwrap_or("").trim().to_string()
        };
        let speeds: std::collections::BTreeSet<String> =
            sticks.iter().map(|f| part(f, 1)).collect();
        let makes: std::collections::BTreeSet<String> = sticks
            .iter()
            .map(|f| {
                part(f, 2)
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        if makes.len() > 1 {
            out.push("内存条来自不同厂商：整机会按最慢的那条跑".into());
        }
        if speeds.len() > 1 {
            out.push(format!(
                "内存条速率不一致（{}）：整机会按最慢的那条跑",
                speeds.into_iter().collect::<Vec<_>>().join(" / ")
            ));
        }
    }
    let mut tight = Vec::new();
    for f in facts.iter().filter(|f| f.label.ends_with("容量")) {
        let nums: Vec<f64> = f
            .value
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() >= 2 && nums[1] > 0.0 && nums[0] / nums[1] < 0.12 {
            tight.push(format!(
                "{} 只剩 {:.0}%（{:.0}/{:.0} GB）",
                f.label,
                nums[0] / nums[1] * 100.0,
                nums[0],
                nums[1]
            ));
        }
    }
    if !tight.is_empty() {
        out.push(format!(
            "{}。固态盘写满之后会明显掉速，编译这种密集写入最先感觉到",
            tight.join("；")
        ));
    }
    out.extend(setting_judgements(&system_settings()));
    out
}

/// 影响开发的几个 Windows 设置。全部只读注册表，毫秒级。
///
/// 只挑这几项：每一项都能直接改变「编译快不快、会不会莫名失败」。
/// 刻意不读的：VC++ 运行库（缺了别的程序自己会报错，不需要提前防）、
/// Defender 排除项（那是安全决定，不是性能决定）。
pub fn system_settings() -> Vec<Fact> {
    let mut out = Vec::new();

    // 电源计划：对满载编译的影响最大的一项
    if let Some(g) = reg(
        r"HKLM\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes",
        "ActivePowerScheme",
    ) {
        // 把「现在靠什么供电」一起写进去：离开这一句，电源计划是好是坏根本没法判断
        let supply = match ffi::on_ac_power() {
            Some(true) => " · 插着电",
            Some(false) => " · 用电池",
            None => "",
        };
        out.push(Fact::one(
            "电源计划",
            format!("{}{}", power_plan_name(&g), supply),
            "注册表",
        ));
    }

    // 虚拟内存：关掉它，编译到内存峰值会被系统直接结束进程
    let pf = reg(
        r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management",
        "PagingFiles",
    )
    .unwrap_or_default();
    out.push(Fact::one(
        "虚拟内存",
        if pf.trim().is_empty() {
            "已关闭".to_string()
        } else {
            format!("已启用（{}）", pf.trim())
        },
        "注册表",
    ));

    // 长路径：不开的话深层 node_modules 会撞上 260 字符上限
    let lp = reg(
        r"HKLM\SYSTEM\CurrentControlSet\Control\FileSystem",
        "LongPathsEnabled",
    )
    .unwrap_or_default();
    out.push(Fact::one(
        "长路径支持",
        if lp.eq_ignore_ascii_case("0x1") {
            "已开启".to_string()
        } else {
            "未开启".to_string()
        },
        "注册表",
    ));

    // 内存完整性（核心隔离）：给每个内核调用加一层校验
    let hv = reg(
        r"HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity",
        "Enabled",
    )
    .unwrap_or_default();
    out.push(Fact::one(
        "内存完整性",
        if hv.eq_ignore_ascii_case("0x1") {
            "已开启".to_string()
        } else {
            "已关闭".to_string()
        },
        "注册表",
    ));
    out
}

/// 电源计划的 GUID → 名字。这几个是 Windows 自带的固定方案。
fn power_plan_name(guid: &str) -> String {
    let g = guid.trim().to_lowercase();
    let name = match g.as_str() {
        "381b4222-f694-41f0-9685-ff5bb260df2e" => "平衡",
        "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c" => "高性能",
        "a1841308-3541-4fab-bc81-f71556f20b4a" => "节能",
        "e9a42b02-d5df-448d-aa00-03f14749eb61" => "卓越性能",
        _ => "自定义方案",
    };
    format!("{}（{}）", name, g)
}

/// 从这几个设置能看出什么。仍然只讲能证实的。
pub fn setting_judgements(facts: &[Fact]) -> Vec<String> {
    let mut out = Vec::new();
    let val = |label: &str| -> String {
        facts
            .iter()
            .find(|f| f.label == label)
            .map(|f| f.value.clone())
            .unwrap_or_default()
    };
    // 只有插着电才提电源计划。用电池时本来就该省电。
    let plan = val("电源计划");
    let on_ac = ffi::on_ac_power();
    if !plan.is_empty()
        && !plan.starts_with("高性能")
        && !plan.starts_with("卓越性能")
        && on_ac != Some(false)
    {
        out.push(
            "当前电源计划不是「高性能」：插电满载时 CPU 会被限频，编译能差两三成。控制面板 → 电源选项 → 选「高性能」".into(),
        );
    }
    if val("虚拟内存").starts_with("已关闭") {
        out.push(
            "虚拟内存被关了。编译这类内存峰值高的活，峰值一超就会被系统直接结束进程，报错往往看不出原因".into(),
        );
    }
    if val("长路径支持").starts_with("未开启") {
        out.push(
            "长路径支持没开：路径超过 260 字符就写不进去，深层 node_modules 和嵌套很深的 Java 包名最先撞上".into(),
        );
    }
    if val("内存完整性").starts_with("已开启") {
        out.push(
            "内存完整性开着：它给每个内核调用加一层校验，实测拖慢一到两成，而它防的是驱动级攻击，跟开发环境无关。要关：Windows 安全中心 → 设备安全性 → 内核隔离 → 内存完整性。代价是少了那层保护".into(),
        );
    }
    out
}

/// 硬盘的完整身份：型号、固件、序列号、总线。
///
/// 为什么不继续用注册表：那里的 `Prod_` 字段是 **SCSI 产品标识，固定 16 字节**，
/// 我原来读出来的 `UMIS RPEYJ1T24MK` 正好在那儿断掉，真型号是 21 字符的
/// `UMIS RPEYJ1T24MKN2QWY`。拿 CrystalDiskInfo 一对才发现——
/// 这种「看起来完全正常」的截断，光靠自己发现不了。
///
/// 这里直接问设备：`IOCTL_STORAGE_QUERY_PROPERTY` + `StorageDeviceProperty`，
/// 拿的是设备自报的描述符，长度不受那 16 字节限制。
/// 打开卷句柄不需要管理员权限。
#[cfg(windows)]
pub fn disk_properties() -> Vec<DiskProps> {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct StoragePropertyQuery {
        property_id: u32,
        query_type: u32,
        additional: [u8; 4],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            sa: *mut core::ffi::c_void,
            disposition: u32,
            flags: u32,
            template: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
        fn DeviceIoControl(
            h: *mut core::ffi::c_void,
            code: u32,
            in_buf: *mut core::ffi::c_void,
            in_len: u32,
            out_buf: *mut core::ffi::c_void,
            out_len: u32,
            returned: *mut u32,
            overlapped: *mut core::ffi::c_void,
        ) -> i32;
        fn CloseHandle(h: *mut core::ffi::c_void) -> i32;
    }

    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x0002_D140;
    const STORAGE_DEVICE_PROPERTY: u32 = 0;
    const PROPERTY_STANDARD_QUERY: u32 = 0;
    const SHARE_READ_WRITE: u32 = 0x0000_0003;
    const OPEN_EXISTING: u32 = 3;

    let read_cstr = |buf: &[u8], off: usize| -> String {
        if off == 0 || off >= buf.len() {
            return String::new();
        }
        let end = buf[off..]
            .iter()
            .position(|c| *c == 0)
            .map(|i| off + i)
            .unwrap_or(buf.len());
        crate::textfile::decode_output(&buf[off..end]).trim().to_string()
    };
    let u32_at = |b: &[u8], o: usize| -> u32 {
        if o + 3 < b.len() {
            u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
        } else {
            0
        }
    };

    let mut out = Vec::new();
    // 走卷句柄而不是 \\.\PhysicalDriveN：后者要管理员权限，
    // 前者用 0 访问权限就能打开，同一块盘的描述符一样拿得到。
    for (n, d) in ffi::disks().into_iter().enumerate() {
        let letter = d.drive.trim_end_matches('\\').to_string();
        if letter.is_empty() {
            continue;
        }
        let path = format!(r"\\.\{}", letter);
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` 是以 NUL 结尾的 UTF-16 路径；其余参数全是值（0 访问权限、
        // 共享读写、无安全属性、无模板句柄）。返回值下面立刻判有效性。
        let h = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0,
                SHARE_READ_WRITE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        // 无效句柄 = INVALID_HANDLE_VALUE(-1)，也兼容 null
        if h.is_null() || h as isize == -1 {
            continue;
        }
        let mut q = StoragePropertyQuery {
            property_id: STORAGE_DEVICE_PROPERTY,
            query_type: PROPERTY_STANDARD_QUERY,
            additional: [0; 4],
        };
        let mut buf = vec![0u8; 1024];
        let mut returned = 0u32;
        // SAFETY: `h` 是上面刚创建并判过有效性的卷句柄；入参结构的长度按
        // `size_of` 给出，出参 `buf` 是长度 1024 的 Vec，API 最多写这个长度。
        let ok = unsafe {
            DeviceIoControl(
                h,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &mut q as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<StoragePropertyQuery>() as u32,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                buf.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        // SAFETY: `h` 是上面用完的句柄，且这段代码在本次迭代里只走一次，不会重复关闭。
        unsafe { CloseHandle(h) };
        if ok == 0 {
            continue;
        }
        // STORAGE_DEVICE_DESCRIPTOR：前 8 字节是 Version/Size，
        // 然后是 DeviceType..CommandQueueing（4 字节），
        // 从偏移 12 起是四个偏移量：VendorId / ProductId / ProductRevision / SerialNumber
        let product = read_cstr(&buf, u32_at(&buf, 16) as usize);
        if product.is_empty() {
            continue;
        }
        out.push(DiskProps {
            index: n as u32,
            vendor: read_cstr(&buf, u32_at(&buf, 12) as usize),
            model: product,
            firmware: read_cstr(&buf, u32_at(&buf, 20) as usize),
            serial: read_cstr(&buf, u32_at(&buf, 24) as usize),
            bus: bus_name(u32_at(&buf, 28)),
        });
    }
    out
}

#[cfg(not(windows))]
pub fn disk_properties() -> Vec<DiskProps> {
    Vec::new()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiskProps {
    pub index: u32,
    pub vendor: String,
    pub model: String,
    pub firmware: String,
    pub serial: String,
    pub bus: String,
}

fn bus_name(t: u32) -> String {
    match t {
        1 => "SCSI",
        2 => "ATAPI",
        3 => "ATA",
        7 => "USB",
        8 => "RAID",
        9 => "iSCSI",
        11 => "SATA",
        17 => "NVMe",
        0x0E => "SAS",
        _ => "未知",
    }
    .to_string()
}

/// 枚举一个注册表键的子键名（不带任何过滤）。
fn reg_subkeys(path: &str) -> Vec<String> {
    let Some(reg_exe) = std::env::var("SystemRoot")
        .map(|r| std::path::PathBuf::from(r).join("System32").join("reg.exe"))
        .ok()
    else {
        return Vec::new();
    };
    let Some((true, out)) = crate::winutil::run_cmd(
        &reg_exe,
        &["query", path],
        std::time::Duration::from_secs(8),
    ) else {
        return Vec::new();
    };
    out.lines()
        .filter_map(|l| l.trim().rsplit('\\').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(path))
        .collect()
}

/// 硬盘的完整型号。
///
/// 之前我从 `Services\disk\Enum` 读 `Prod_` 字段，那是 **SCSI 产品标识，
/// 固定 16 字节**，`UMIS RPEYJ1T24MKN2QWY` 被截成了 `UMIS RPEYJ1T24MK`。
/// 我当时下的结论是「注册表装不下」——**错**。同一棵树的设备实例下面
/// 就有 `FriendlyName`，那是设备自报的名字，21 个字符一个不少。
///
/// 教训：一个字段装不下，不等于这个来源装不下。别急着换更重的路子
/// （我为此去写了 DeviceIoControl，结果在那台机器上被系统拒了 err=5），
/// 先看完这棵树。
fn disk_full_models() -> Vec<String> {
    const BASE: &str = r"HKLM\SYSTEM\CurrentControlSet\Enum\SCSI";
    let mut out = Vec::new();
    for cls in reg_subkeys(BASE) {
        if !cls.to_lowercase().starts_with("disk") {
            continue;
        }
        let cls_path = format!(r"{}\{}", BASE, cls);
        for inst in reg_subkeys(&cls_path) {
            if let Some(f) = reg(&format!(r"{}\{}", cls_path, inst), "FriendlyName") {
                let f = f.trim().to_string();
                if !f.is_empty() && !out.contains(&f) {
                    out.push(f);
                }
            }
        }
    }
    out
}

fn reg(path: &str, key: &str) -> Option<String> {
    let reg_exe = std::env::var("SystemRoot")
        .map(|r| std::path::PathBuf::from(r).join("System32").join("reg.exe"))
        .ok()?;
    let (ok, out) = crate::winutil::run_cmd(
        &reg_exe,
        &["query", path, "/v", key],
        std::time::Duration::from_secs(8),
    )?;
    if !ok {
        return None;
    }
    // 输出形如：    KeyName    REG_SZ    Value
    out.lines()
        .find(|l| l.contains("REG_"))
        .and_then(|l| l.split("REG_").nth(1))
        .and_then(|l| l.split_once(char::is_whitespace).map(|x| x.1))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// 一次采样的结果。字段刻意少而准——状态栏上多一个数字就多一份噪音。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceSample {
    pub cpu_percent: f64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub disk_free_gb: f64,
    pub disk_total_gb: f64,
    /// 这台机器上此刻活着几个 python 进程
    pub python_procs: usize,
    /// 此刻活着的开发相关进程：名字 → 个数（只列数出来的那些）。
    /// 状态栏用它回答「我跑的东西还在不在」——不用去任务管理器里翻。
    pub dev_procs: Vec<(String, usize)>,
}

/// CPU 占用率靠两次累计计数求差。
///
/// 为什么要留状态：`GetSystemTimes` 给的是开机以来的累计值，
/// 单看一次只能算出「平均占用」，看不出此刻忙不忙。差分才是瞬时值。
#[derive(Default)]
pub struct Sampler {
    prev: Option<(u64, u64, u64)>,
}

impl Sampler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sample(&mut self) -> ResourceSample {
        let (mem_total_mb, mem_free_mb) = ffi::memory_mb();
        let mut cpu_percent = 0.0;
        if let Some(now) = ffi::system_times() {
            if let Some((pi, pk, pu)) = self.prev {
                let di = now.0.saturating_sub(pi) as f64;
                let dk = now.1.saturating_sub(pk) as f64;
                let du = now.2.saturating_sub(pu) as f64;
                // 注意：kernel 时间里已经把 idle 算进去了，所以总时间是 kernel+user
                let total = dk + du;
                if total > 0.0 {
                    cpu_percent = ((total - di) / total * 100.0).clamp(0.0, 100.0);
                }
            }
            self.prev = Some(now);
        }
        let disks = ffi::disks();
        let sys_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let d = disks
            .iter()
            .find(|x| x.drive.trim_end_matches('\\').eq_ignore_ascii_case(&sys_drive))
            .or_else(|| disks.first());
        // 一次进程快照把要数的都数完：python_procs 也从这里出，不再单独走一趟。
        // 状态栏每几秒问一次，走只数名字的轻路径（不查内存）
        let dev = crate::mem::count_many_fast(DEV_PROC_NAMES);
        let python_procs = dev
            .iter()
            .find(|(k, _)| k == "python")
            .map(|(_, n)| *n)
            .unwrap_or(0);
        ResourceSample {
            cpu_percent,
            mem_used_mb: mem_total_mb.saturating_sub(mem_free_mb),
            mem_total_mb,
            disk_free_gb: d.map(|x| x.free_gb).unwrap_or(0.0),
            disk_total_gb: d.map(|x| x.total_gb).unwrap_or(0.0),
            python_procs,
            dev_procs: dev.into_iter().filter(|(_, n)| *n > 0).collect(),
        }
    }
}

/// 状态栏数哪些进程。只认「跑起来就该在这儿」的那几个：
/// 解释器和构建器，都是新手会一脸茫然地跑去任务管理器找的东西。
const DEV_PROC_NAMES: &[&str] = &[
    "python", "node", "java", "cargo", "rustc", "dotnet", "gcc", "clang",
];

pub fn probe_machine() -> Machine {
    let (ram_total_mb, ram_free_mb) = ffi::memory_mb();
    let vk = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let product = reg(vk, "ProductName").unwrap_or_else(|| "Windows".into());
    let build = reg(vk, "CurrentBuild").unwrap_or_default();
    let build_num: u32 = build.parse().unwrap_or(0);
    Machine {
        os_name: os_display_name(&product, build_num),
        os_product: product,
        os_version: reg(vk, "DisplayVersion").unwrap_or_default(),
        os_build: build,
        cpu: reg(
            r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "ProcessorNameString",
        )
        .unwrap_or_else(|| "未知".into()),
        cores: ffi::cores(),
        arch: std::env::consts::ARCH.to_string(),
        ram_total_mb,
        ram_free_mb,
        disks: ffi::disks(),
    }
}

/// ProductName 从 Windows 11 起就没再改过，一直写着「Windows 10」——
/// 微软为了兼容那些按字符串判断版本的老程序，故意留着的。
/// 官方判据是版本号：22000 及以上才是 Windows 11。
/// 所以这里以版本号为准，只把 ProductName 里的版本前缀换掉，保留版本分支（IoT Enterprise LTSC 之类）。
fn os_display_name(product: &str, build: u32) -> String {
    let family = if build >= 22000 { "Windows 11" } else { "Windows 10" };
    let edition = product
        .strip_prefix("Windows 11 ")
        .or_else(|| product.strip_prefix("Windows 10 "))
        .unwrap_or("");
    if edition.is_empty() {
        if product.is_empty() || product == "Windows" {
            family.to_string()
        } else {
            // 名字不是我们认识的形状，两处都摆出来，别自作主张
            format!("{}（注册表写的：{}）", family, product)
        }
    } else {
        format!("{} {}", family, edition)
    }
}

/// 环境变量里值得给人看的那几个。不是全量转储——
/// 全量转储会顺手把你的隐私一起摆在屏幕上。
pub fn env_snapshot() -> BTreeMap<String, String> {
    const KEYS: &[&str] = &[
        "PATH",
        "PATHEXT",
        "TEMP",
        "TMP",
        "PYTHONPATH",
        "PYTHONHOME",
        "PYTHONUTF8",
        "PYTHONIOENCODING",
        "VIRTUAL_ENV",
        "CONDA_PREFIX",
        "PIP_INDEX_URL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
    ];
    let mut m = BTreeMap::new();
    for k in KEYS {
        if let Ok(v) = std::env::var(k) {
            m.insert(k.to_string(), v);
        }
    }
    m
}

/// 快路径：只读注册表和系统接口，不碰 WMI。
///
/// 为什么它该当主路径：这台机器上单次 CIM 查询要 20 秒左右，
/// 全套下来 40 秒起——那不是"慢一点"，是没法用。而下面这些数据
/// 全都在注册表和几个系统调用里躺着，合起来只要几十毫秒。
///
/// 拿不到的只有内存条型号/频率和硬盘型号——那两样才需要 WMI，留给「再深一层」。
pub fn probe_hardware_fast() -> serde_json::Value {
    let hw = r"HKLM\HARDWARE\DESCRIPTION\System";
    let cpu_key = format!(r"{}\CentralProcessor\0", hw);
    let cpu_name = reg(&cpu_key, "ProcessorNameString");
    let cpu_mhz = reg(&cpu_key, "~MHz");
    let (ram_total_mb, _) = ffi::memory_mb();
    let disks: Vec<serde_json::Value> = ffi::disks()
        .into_iter()
        .map(|d| serde_json::json!({ "drive": d.drive, "total_gb": d.total_gb, "free_gb": d.free_gb }))
        .collect();
    let (sw, sh) = ffi::screen();
    serde_json::json!({
        "source": "注册表 + 系统接口（快路径）",
        "cpu": cpu_name,
        "cpu_mhz": cpu_mhz,
        "cores": ffi::cores(),
        "gpus": gpu_names(),
        "ram_total_gb": (ram_total_mb as f64) / 1024.0,
        "disks": disks,
        "screen": if sw > 0 { format!("{}x{}", sw, sh) } else { String::new() },
        "baseboard": format!(
            "{} {}",
            reg(&format!(r"{}\BIOS", hw), "BaseBoardManufacturer").unwrap_or_default(),
            reg(&format!(r"{}\BIOS", hw), "BaseBoardProduct").unwrap_or_default()
        ).trim().to_string(),
        "system_model": format!(
            "{} {}",
            reg(&format!(r"{}\BIOS", hw), "SystemManufacturer").unwrap_or_default(),
            reg(&format!(r"{}\BIOS", hw), "SystemProductName").unwrap_or_default()
        ).trim().to_string(),
        "bios": format!(
            "{} {}",
            reg(&format!(r"{}\BIOS", hw), "BIOSVendor").unwrap_or_default(),
            reg(&format!(r"{}\BIOS", hw), "BIOSVersion").unwrap_or_default()
        ).trim().to_string(),
        // 内存条：读 SMBIOS 表，毫秒级。原来靠 WMI，一次要 20 秒。
        "ram": smbios_memory(),
        // 硬盘型号：设备实例路径里带着，解析一下就行
        "disks_model": disk_models(),
    })
}

/// 硬盘型号藏在设备实例路径里：`SCSI\Disk&Ven_NVMe&Prod_UMIS_RPEYJ1T24MK\...`
/// 把下划线还原成空格就是人读的样子。
fn disk_models() -> Vec<String> {
    let mut out = Vec::new();
    for v in reg_values(r"HKLM\SYSTEM\CurrentControlSet\Services\disk\Enum") {
        let Some(i) = v.find("Prod_") else { continue };
        let tail: String = v[i + 5..]
            .chars()
            .take_while(|c| *c != '\\')
            .collect();
        let name = tail.replace('_', " ").trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// 取一个注册表键下所有字符串值。`reg query` 的输出每行形如
/// `    名字    REG_SZ    值`，取 REG_SZ 后面那段。
fn reg_values(path: &str) -> Vec<String> {
    let Some(reg_exe) = std::env::var("SystemRoot")
        .map(|r| std::path::PathBuf::from(r).join("System32").join("reg.exe"))
        .ok()
    else {
        return Vec::new();
    };
    let Some((true, out)) = crate::winutil::run_cmd(
        &reg_exe,
        &["query", path],
        std::time::Duration::from_secs(8),
    ) else {
        return Vec::new();
    };
    out.lines()
        .filter_map(|l| l.split("REG_SZ").nth(1))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

/// 显卡名字：显示适配器的驱动描述就写在注册表里，
/// 比问 WMI 快三个数量级。一块机器上可能有好几个（独显 + 核显 + 虚拟机显卡）。
fn gpu_names() -> Vec<String> {
    const CLASS: &str =
        r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let mut out = Vec::new();
    let subkeys = reg_list_subkeys(CLASS);
    for (k, desc) in subkeys {
        // 只认形如 0000 / 0001 的子键，别的都是杂项
        if !k.chars().all(|c| c.is_ascii_digit()) || k.len() != 4 {
            continue;
        }
        if !desc.is_empty() && !out.contains(&desc) {
            out.push(desc);
        }
    }
    out
}


#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    #[test]
    fn memory_and_cores_are_plausible() {
        let m = probe_machine();
        assert!(m.cores >= 1, "核数不该是 0");
        if cfg!(windows) {
            assert!(m.ram_total_mb > 512, "物理内存读出来是 {}", m.ram_total_mb);
        }
    }

    #[test]
    fn env_snapshot_never_dumps_everything() {
        let m = env_snapshot();
        // 只放白名单里那几个，绝不能变成全量转储
        assert!(m.len() <= 14, "抓了 {} 个环境变量，太多了", m.len());
    }

    /// 最要紧的一条交叉验证：SMBIOS 里各条内存加起来，应该和系统报告的
    /// 物理内存对得上。偏移量写错一格，这个和就对不上——
    /// 光看「有两条 8GB」是发现不了 64MB 那种错的。
    #[test]
    fn smbios_memory_adds_up_to_the_physical_total() {
        if !cfg!(windows) {
            return;
        }
        let sticks = smbios_memory();
        if sticks.is_empty() {
            return; // 虚拟机或固件不给这张表
        }
        let sum: u64 = sticks.iter().map(|s| s.size_mb).sum();
        let (total, _) = ffi::memory_mb();
        let diff = (sum as i64 - total as i64).abs();
        assert!(
            diff < total as i64 / 10,
            "SMBIOS 加起来 {} MB，系统说 {} MB——差太多，说明字段偏移读错了",
            sum,
            total
        );
        for s in &sticks {
            assert!(s.speed > 0, "内存条没有速率：{:?}", s);
        }
    }

    /// 空输入、半截输入都不该崩。真实机器上拿到的表不保证完整。
    #[test]
    fn a_truncated_smbios_table_does_not_panic() {
        assert!(parse_smbios_memory(&[]).is_empty());
        assert!(parse_smbios_memory(&[0u8; 4]).is_empty());
        assert!(parse_smbios_memory(&[0x11, 0x5C, 0, 0, 0, 0, 0, 0]).is_empty());
    }

    #[test]
    fn windows_11_is_told_apart_by_build_not_by_name() {
        // 本机的真实值：注册表说 Windows 10，版本号说 26100
        assert_eq!(
            os_display_name("Windows 10 IoT Enterprise LTSC 2024", 26100),
            "Windows 11 IoT Enterprise LTSC 2024"
        );
        assert_eq!(os_display_name("Windows 10 Pro", 19045), "Windows 10 Pro");
        assert_eq!(os_display_name("Windows 11 Pro", 22631), "Windows 11 Pro");
        // 名字不认识时不许瞎猜，两个都写出来
        assert!(os_display_name("Some Future OS", 26100).contains("Some Future OS"));
    }
}
