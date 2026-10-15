//! 底层检验：钻进原生扩展（.pyd）里看它到底要什么。
//!
//! 为什么非做不可：`ImportError: DLL load failed` 是 Python 里最难查的一类错——
//! 报错只说「加载失败」，不说缺哪个。`.pyd` 本质是 Windows PE 动态库，
//! 答案就写在它的导入表里。这里自己解析 PE，把「它要哪些 DLL、哪些找不到、
//! 位数对不对」摆出来。
//!
//! 不引第三方库：PE 头就那么几个字段，手写一遍比引一个解析库更可控，
//! 而且工具本身要保持零依赖。

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeInfo {
    /// IMAGE_FILE_HEADER.Machine
    pub machine: u16,
    pub arch: String,
    /// 导入表里的 DLL 名（原始写法，大小写不一）
    pub imports: Vec<String>,
}

fn arch_of(machine: u16) -> &'static str {
    match machine {
        0x014c => "x86",
        0x8664 => "x64",
        0x01c4 => "ARM",
        0xaa64 => "ARM64",
        0x01c0 => "ARM",
        _ => "未知",
    }
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?]))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(o)?,
        *b.get(o + 1)?,
        *b.get(o + 2)?,
        *b.get(o + 3)?,
    ]))
}

/// 读一个 PE 文件的位数和导入表
pub fn read_pe(b: &[u8]) -> Option<PeInfo> {
    if b.len() < 0x40 || &b[0..2] != b"MZ" {
        return None;
    }
    let pe_off = u32_at(b, 0x3c)? as usize;
    if b.get(pe_off..pe_off + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe_off + 4;
    let machine = u16_at(b, coff)?;
    let n_sections = u16_at(b, coff + 2)? as usize;
    let opt_size = u16_at(b, coff + 16)? as usize;
    let opt = coff + 20;
    let magic = u16_at(b, opt)?;
    // PE32 / PE32+ 的数据目录起始位置不一样——这是最容易写错的一处
    let dd = match magic {
        0x10b => opt + 96,
        0x20b => opt + 112,
        _ => return None,
    };
    let import_rva = u32_at(b, dd + 8)? as usize; // 目录[1] = 导入表
    let secs = opt + opt_size;

    // RVA → 文件偏移：找落在哪个节里
    let rva_to_off = |rva: usize| -> Option<usize> {
        for i in 0..n_sections {
            let s = secs + i * 40;
            let va = u32_at(b, s + 12)? as usize;
            let raw_size = u32_at(b, s + 16)? as usize;
            let raw = u32_at(b, s + 20)? as usize;
            if rva >= va && rva < va + raw_size.max(1) {
                return Some(raw + (rva - va));
            }
        }
        None
    };
    let cstr = |off: usize| -> String {
        let mut s = String::new();
        let mut i = off;
        while let Some(c) = b.get(i) {
            if *c == 0 {
                break;
            }
            s.push(*c as char);
            i += 1;
            if s.len() > 260 {
                break;
            }
        }
        s
    };

    let mut imports = Vec::new();
    if import_rva != 0 {
        let mut off = rva_to_off(import_rva)?;
        loop {
            // 导入描述符 20 字节；全零表示结束
            let name_rva = u32_at(b, off + 12)? as usize;
            let first = u32_at(b, off)?;
            let chain = u32_at(b, off + 16)?;
            if name_rva == 0 && first == 0 && chain == 0 {
                break;
            }
            if let Some(no) = rva_to_off(name_rva) {
                let n = cstr(no);
                if !n.is_empty() {
                    imports.push(n);
                }
            }
            off += 20;
            if imports.len() > 4096 {
                break;
            }
        }
    }
    Some(PeInfo {
        machine,
        arch: arch_of(machine).into(),
        imports,
    })
}

pub fn read_pe_file(p: &Path) -> Option<PeInfo> {
    let b = std::fs::read(p).ok()?;
    read_pe(&b)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeIssue {
    pub pyd: String,
    pub package: String,
    /// 导入表里有、但在任何搜索路径下都找不到的 DLL
    pub missing: Vec<String>,
    /// 位数和解释器对不上
    pub arch: String,
    pub arch_mismatch: bool,
}

/// 把一长串同类问题收成一张卡。
///
/// 为什么必须收：`site-packages` 里一次能扫出几十个".pyd 位数不对"，
/// 一条一个卡片会把人淹掉，而它们的**根因是同一条**。
/// 所以这里只报总数 + 涉及哪些包 + 两三个例子 + 一句结论。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeSummary {
    pub total: usize,
    pub arch_mismatch: usize,
    pub missing_dll: usize,
    /// 涉及到的包（前若干个）与各自的问题数
    pub packages: Vec<(String, usize)>,
    pub samples: Vec<NativeIssue>,
    pub arch: String,
    /// 一句人话结论
    pub verdict: String,
    /// 下一步；没有就说没有
    pub hint: String,
}

pub fn summarize(issues: &[NativeIssue], py_arch: &str) -> NativeSummary {
    let mut by: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut arch_mismatch = 0;
    let mut missing_dll = 0;
    for i in issues {
        *by.entry(i.package.clone()).or_insert(0) += 1;
        if i.arch_mismatch {
            arch_mismatch += 1;
        }
        if !i.missing.is_empty() {
            missing_dll += 1;
        }
    }
    let mut packages: Vec<(String, usize)> = by.into_iter().collect();
    packages.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let verdict = if issues.is_empty() {
        "原生扩展都对得上（位数与依赖都找得到）".to_string()
    } else if arch_mismatch == issues.len() {
        format!(
            "{} 个原生扩展的位数和当前解释器（{}）不一致——它们全都不可能加载",
            issues.len(),
            py_arch
        )
    } else if missing_dll == issues.len() {
        format!("{} 个原生扩展缺依赖 DLL", issues.len())
    } else {
        format!(
            "{} 个原生扩展有问题：{} 个位数不对、{} 个缺依赖",
            issues.len(),
            arch_mismatch,
            missing_dll
        )
    };
    let hint = if issues.is_empty() {
        String::new()
    } else if arch_mismatch > 0 {
        format!(
            "这些 .pyd 是从别处拷来的、或者当时用另一个位数的 Python 装的。\
             干净的修法是：用当前解释器重装这几个包（pip install --force-reinstall {}）。\
             加载失败时报的是「DLL load failed」，看不出真正原因——这就是它骗人的地方。",
            packages
                .iter()
                .take(3)
                .map(|(p, _)| p.clone())
                .collect::<Vec<_>>()
                .join(" ")
        )
    } else {
        "缺的多半是包自带的运行库；先按上面的清单重装那个包再看。".to_string()
    };

    NativeSummary {
        total: issues.len(),
        arch_mismatch,
        missing_dll,
        packages: packages.into_iter().take(6).collect(),
        samples: issues.iter().take(3).cloned().collect(),
        arch: py_arch.to_string(),
        verdict,
        hint,
    }
}

/// DLL 搜索路径，按 Windows 实际的优先级近似排一遍
fn search_dirs(py_prefix: &Path, base_prefix: &Path, pyd_dir: &Path) -> Vec<PathBuf> {
    let mut v = vec![pyd_dir.to_path_buf()];
    for p in [py_prefix, base_prefix] {
        v.push(p.to_path_buf());
        v.push(p.join("DLLs"));
        v.push(p.join("Library").join("bin"));
    }
    if let Ok(sysroot) = std::env::var("SystemRoot") {
        let s = PathBuf::from(sysroot);
        v.push(s.join("System32"));
        v.push(s.join("SysWOW64"));
        v.push(s.clone());
    }
    if let Ok(path) = std::env::var("PATH") {
        v.extend(std::env::split_paths(&path));
    }
    v
}

/// 只检查「看起来是本程序自己带的」那些 DLL；系统 DLL 交给 Windows。
fn is_system_dll(name: &str) -> bool {
    let n = name.to_lowercase();
    const SYSTEM: &[&str] = &[
        "kernel32.dll", "user32.dll", "advapi32.dll", "ole32.dll", "oleaut32.dll",
        "shell32.dll", "gdi32.dll", "msvcrt.dll", "ntdll.dll", "ws2_32.dll",
        "crypt32.dll", "secur32.dll", "bcrypt.dll", "comdlg32.dll", "comctl32.dll",
        "shlwapi.dll", "version.dll", "winmm.dll", "rpcrt4.dll", "iphlpapi.dll",
        "vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "ucrtbase.dll",
        "api-ms-win-crt-runtime-l1-1-0.dll", "api-ms-win-core-synch-l1-1-0.dll",
        "python3.dll", "python3t.dll",
    ];
    if SYSTEM.contains(&n.as_str()) {
        return true;
    }
    // api-ms-win-* 是 API Set，由系统按需转发，文件本身查不到很正常
    n.starts_with("api-ms-win-") || n.starts_with("ext-ms-win-")
}

fn resolves(name: &str, dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|d| d.join(name).is_file())
}

/// 扫 site-packages 里的原生扩展，看谁缺依赖、谁位数不对。
///
/// `py_arch` 是解释器的位数（"x64"/"x86"）。位数对不上的 .pyd 一定加载不了，
/// 而且报的错跟缺 DLL 长得一模一样——这是排查时最容易被带偏的地方。
pub fn scan_native(
    site_packages: &[PathBuf],
    py_prefix: &Path,
    base_prefix: &Path,
    py_arch: &str,
    limit: usize,
) -> Vec<NativeIssue> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for site in site_packages {
        // 只挖三层：包/子包/扩展。再深就是数据分析包的内脏了，收益低。
        for e in walkdir::WalkDir::new(site)
            .max_depth(3)
            .into_iter()
            .flatten()
        {
            if out.len() >= limit {
                return out;
            }
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("pyd") {
                continue;
            }
            let key = p.to_string_lossy().to_lowercase();
            if !seen.insert(key) {
                continue;
            }
            let Some(pe) = read_pe_file(p) else { continue };
            let dirs = search_dirs(py_prefix, base_prefix, p.parent().unwrap_or(Path::new(".")));
            let missing: Vec<String> = pe
                .imports
                .iter()
                .filter(|d| !is_system_dll(d) && !resolves(d, &dirs))
                .cloned()
                .collect();
            let arch_mismatch = !py_arch.is_empty() && pe.arch != py_arch && pe.arch != "未知";
            if missing.is_empty() && !arch_mismatch {
                continue;
            }
            let package = p
                .strip_prefix(site)
                .ok()
                .and_then(|r| r.components().next())
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .unwrap_or_default();
            out.push(NativeIssue {
                pyd: p.to_string_lossy().to_string(),
                package,
                missing,
                arch: pe.arch,
                arch_mismatch,
            });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildTools {
    /// 找得到 cl.exe 吗——装源码包时必须有
    pub has_cl: bool,
    pub cl_path: Option<String>,
    pub has_setuptools: bool,
    pub has_wheel: bool,
}

/// 能不能在本地编译扩展。装到只有源码包的库时，这是成败关键。
pub fn build_tools(has_setuptools: bool, has_wheel: bool) -> BuildTools {
    let mut has_cl = false;
    let mut cl_path = None;
    // PATH 里找，再退一步去 Visual Studio 的常见位置
    for name in ["cl.exe", "cl.bat"] {
        if let Ok(path) = std::env::var("PATH") {
            for d in std::env::split_paths(&path) {
                let p = d.join(name);
                if p.is_file() {
                    has_cl = true;
                    cl_path = Some(p.to_string_lossy().to_string());
                    break;
                }
            }
        }
        if has_cl {
            break;
        }
    }
    BuildTools {
        has_cl,
        cl_path,
        has_setuptools,
        has_wheel,
    }
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    #[test]
    fn rejects_non_pe_bytes() {
        assert!(read_pe(b"not a pe at all").is_none());
        assert!(read_pe(&[]).is_none());
    }

    #[test]
    fn api_set_dlls_are_treated_as_system() {
        // 这些文件在磁盘上根本不存在，但它们从不需要手动解析
        assert!(is_system_dll("api-ms-win-core-synch-l1-1-0.dll"));
        assert!(is_system_dll("ext-ms-win-ntuser-window-l1-1-0.dll"));
        assert!(!is_system_dll("libiomp5md.dll"));
        assert!(is_system_dll("VCRUNTIME140_1.dll")); // 大小写不敏感
    }

    #[test]
    fn parses_a_real_pyd_if_we_have_one() {
        // 本机装了 Python 的话顺手验一把真实文件；没有就跳过
        let candidates = [
            r"C:\Windows\System32\python3.dll",
            r"C:\Windows\System32\ucrtbase.dll",
        ];
        for c in candidates {
            let p = Path::new(c);
            if !p.is_file() {
                continue;
            }
            let pe = read_pe_file(p).expect("系统 DLL 应该是合法 PE");
            assert!(!pe.imports.is_empty(), "{} 的导入表不该是空的", c);
            return;
        }
    }

    /// 手工造一个最小 PE：64 位、一个节、导入表里只有一个不存在的 DLL。
    /// 用它把「解析 → 扫描 → 报出缺哪个」整条链路走一遍——
    /// 只测解析器等于只测了半条路，真正的价值在最后那句「缺 X.dll」。
    fn fake_pyd(import: &str) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes()); // e_lfanew
        b[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        b[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes()); // x64
        b[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes()); // 一个节
        b[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes()); // opt 头长度
        let opt = coff + 20;
        b[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes()); // PE32+
        let dd = opt + 112;
        // 数据目录[1] = 导入表，RVA 0x1000
        b[dd + 8..dd + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        b[dd + 12..dd + 16].copy_from_slice(&40u32.to_le_bytes());
        let sec = opt + 240;
        b[sec..sec + 8].copy_from_slice(b".rdata\0\0");
        b[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // VirtualAddress
        b[sec + 16..sec + 20].copy_from_slice(&0x400u32.to_le_bytes()); // SizeOfRawData
        b[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes()); // PointerToRawData
        // 导入描述符 @ 偏移 0x200（RVA 0x1000）：Name 指向 RVA 0x1060
        b[0x20c..0x210].copy_from_slice(&0x1060u32.to_le_bytes());
        // 后面 20 字节全零 = 描述符列表结束；DLL 名放在 RVA 0x1060 → 偏移 0x260
        let name = import.as_bytes();
        b[0x260..0x260 + name.len()].copy_from_slice(name);
        b
    }

    #[test]
    fn a_pyd_missing_a_dll_is_named_precisely() {
        let dir = std::env::temp_dir().join(format!("pl-native-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pkg = dir.join("fakepkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("ext.pyd"), fake_pyd("NOTEXIST_1234.dll")).unwrap();

        let issues = scan_native(std::slice::from_ref(&dir), &dir, &dir, "x64", 10);
        assert_eq!(issues.len(), 1, "应该恰好报出一个有问题的扩展");
        let i = &issues[0];
        assert_eq!(i.package, "fakepkg");
        assert!(
            i.missing.iter().any(|d| d.eq_ignore_ascii_case("NOTEXIST_1234.dll")),
            "必须点名缺的是哪个 DLL，实际报的是 {:?}",
            i.missing
        );
        assert!(!i.arch_mismatch, "位数是对的，不该误报位数问题");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pyd_for_the_wrong_bit_width_is_flagged() {
        let dir = std::env::temp_dir().join(format!("pl-native32-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut pe = fake_pyd("kernel32.dll");
        pe[0x44..0x46].copy_from_slice(&0x014cu16.to_le_bytes()); // 改成 32 位
        std::fs::write(dir.join("ext.pyd"), pe).unwrap();
        let issues = scan_native(std::slice::from_ref(&dir), &dir, &dir, "x64", 10);
        assert!(
            issues.iter().any(|i| i.arch_mismatch),
            "32 位扩展配 64 位解释器必须被指出来"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 一堆同类问题必须收成一张卡：总数、涉及哪些包、几个例子。
    #[test]
    fn many_same_kind_issues_collapse_into_one_summary() {
        let mk = |pkg: &str, name: &str| NativeIssue {
            pyd: format!("C:/sp/{}/{}.pyd", pkg, name),
            package: pkg.to_string(),
            missing: vec![],
            arch: "x86".into(),
            arch_mismatch: true,
        };
        let issues = vec![
            mk("charset_normalizer", "md"),
            mk("charset_normalizer", "md__mypyc"),
            mk("markupsafe", "speedups"),
            mk("psutil", "_psutil_windows"),
        ];
        let s = summarize(&issues, "x64");
        assert_eq!(s.total, 4);
        assert_eq!(s.arch_mismatch, 4);
        assert_eq!(s.samples.len(), 3, "例子只留几条，不是全倒出来");
        assert_eq!(s.packages[0].0, "charset_normalizer");
        assert_eq!(s.packages[0].1, 2);
        assert!(s.verdict.contains("位数"), "{}", s.verdict);
        assert!(s.hint.contains("--force-reinstall"), "{}", s.hint);
    }

    #[test]
    fn a_clean_machine_says_so_without_a_wall_of_text() {
        let s = summarize(&[], "x64");
        assert_eq!(s.total, 0);
        assert!(s.samples.is_empty());
        assert!(s.hint.is_empty());
        assert!(s.verdict.contains("对得上"));
    }
}
