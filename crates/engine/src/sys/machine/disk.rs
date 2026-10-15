//! 这台机器本身的情况：CPU、内存、磁盘、系统版本、编译器。
//!
//! 数据尽量直接问系统（FFI），不起进程也不引依赖——这些数字每次打开界面都要看，
//! 慢一点都不合适。只有 CPU 型号藏得深（在注册表里），才退回 `reg query`。

use serde::{Deserialize, Serialize};

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
        crate::util::textfile::decode_output(&buf[off..end]).trim().to_string()
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
    for (n, d) in super::ffi::disks().into_iter().enumerate() {
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
    let Some((true, out)) = crate::util::winutil::run_cmd(
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
pub(super) fn disk_full_models() -> Vec<String> {
    const BASE: &str = r"HKLM\SYSTEM\CurrentControlSet\Enum\SCSI";
    let mut out = Vec::new();
    for cls in reg_subkeys(BASE) {
        if !cls.to_lowercase().starts_with("disk") {
            continue;
        }
        let cls_path = format!(r"{}\{}", BASE, cls);
        for inst in reg_subkeys(&cls_path) {
            if let Some(f) = super::reg(&format!(r"{}\{}", cls_path, inst), "FriendlyName") {
                let f = f.trim().to_string();
                if !f.is_empty() && !out.contains(&f) {
                    out.push(f);
                }
            }
        }
    }
    out
}
