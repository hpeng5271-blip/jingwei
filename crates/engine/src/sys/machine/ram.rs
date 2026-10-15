//! 这台机器本身的情况：CPU、内存、磁盘、系统版本、编译器。
//!
//! 数据尽量直接问系统（FFI），不起进程也不引依赖——这些数字每次打开界面都要看，
//! 慢一点都不合适。只有 CPU 型号藏得深（在注册表里），才退回 `reg query`。

use serde::{Deserialize, Serialize};

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
