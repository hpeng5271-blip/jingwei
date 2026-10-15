//! 文本文件的编码与换行：**读进来是什么，写回去就得是什么**。
//!
//! 为什么这件事必须做对：一个会改变文件编码与换行的编辑器，等于在
//! 用户看不见的地方改他的文件。踩过的两个具体后果：
//!   · GBK 的中文文件被 from_utf8_lossy 读成满屏乱码，一保存整个文件毁掉；
//!   · CRLF 的文件被 HTML 的 textarea 规范化成 LF，一保存 git 里显示全文重写。
//! 所以这里的规矩是：**先认，认不出就明说并只读，绝不悄悄替换**。
//!
//! GBK 的转换走 Windows 自己的代码页接口（CP 936）——不引第三方表，
//! 也就不会出现"我的表和你系统的表不一样"这种事。

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct TextFile {
    pub text: String,
    /// utf-8 / utf-8-bom / utf-16le / utf-16be / gbk
    pub encoding: String,
    /// crlf / lf
    pub eol: String,
    /// 能不能编辑。认不出编码就是 false——宁可不让改，也不能改坏。
    pub readable: bool,
    pub note: String,
}

fn win_mb_to_string(bytes: &[u8], code_page: u32) -> Option<String> {
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        extern "system" {
            fn MultiByteToWideChar(
                code_page: u32,
                flags: u32,
                mb: *const u8,
                cb: i32,
                wc: *mut u16,
                cch: i32,
            ) -> i32;
        }
        const MB_ERR_INVALID_CHARS: u32 = 0x0000_0008;
        // SAFETY: 这一步只为问长度——出参缓冲区传 null、长度为 0，API 不会写任何内存；
        // 入参切片 `bytes` 在本行归还前一直存活。
        let len = unsafe {
            MultiByteToWideChar(
                code_page,
                MB_ERR_INVALID_CHARS,
                bytes.as_ptr(),
                bytes.len() as i32,
                std::ptr::null_mut(),
                0,
            )
        };
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        // SAFETY: `buf` 按上一步给出的 `len` 个 `u16` 分配，`len` 在这两行之间没变；
        // 入参切片与上一步完全相同，仍然存活。
        let n = unsafe {
            MultiByteToWideChar(
                code_page,
                MB_ERR_INVALID_CHARS,
                bytes.as_ptr(),
                bytes.len() as i32,
                buf.as_mut_ptr(),
                len,
            )
        };
        if n <= 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(OsString::from_wide(&buf).to_string_lossy().to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, code_page);
        None
    }
}

fn win_string_to_mb(s: &str, code_page: u32) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        extern "system" {
            fn WideCharToMultiByte(
                code_page: u32,
                flags: u32,
                wc: *const u16,
                cch: i32,
                mb: *mut u8,
                cb: i32,
                def: *const u8,
                used: *mut i32,
            ) -> i32;
        }
        let wide: Vec<u16> = std::ffi::OsStr::new(s).encode_wide().collect();
        // SAFETY: 同上面那条反过来的路子——只为问长度，出参传 null、长度为 0。
        let len = unsafe {
            WideCharToMultiByte(
                code_page,
                0,
                wide.as_ptr(),
                wide.len() as i32,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        // SAFETY: `buf` 按上一步给出的 `len` 字节分配，`len` 未变；入参宽串仍存活。
        let n = unsafe {
            WideCharToMultiByte(
                code_page,
                0,
                wide.as_ptr(),
                wide.len() as i32,
                buf.as_mut_ptr(),
                len,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        if n <= 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }
    #[cfg(not(windows))]
    {
        let _ = (s, code_page);
        None
    }
}

/// 认编码 + 认换行。**不猜**：认不出就 readable=false。
pub fn decode(bytes: &[u8]) -> TextFile {
    let mk = |text: String, encoding: &str, readable: bool, note: String| {
        let crlf = text.matches("\r\n").count();
        let lf_all = text.matches('\n').count();
        let eol = if crlf > 0 && crlf * 2 >= lf_all { "crlf" } else { "lf" };
        TextFile {
            // 内部一律用 LF。
            // 为什么：textarea 会把 CRLF 规范化成 LF，如果引擎交出去的是 CRLF，
            // 两边每行差一个字符——括号配对、行列号、缩进线全都会偏一列。
            // 换行只在两个边界上处理：读进来的时候记下它是什么，写回去的时候还原。
            text: text.replace("\r\n", "\n"),
            encoding: encoding.to_string(),
            eol: eol.to_string(),
            readable,
            note,
        }
    };

    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return match String::from_utf8(bytes[3..].to_vec()) {
            Ok(t) => mk(t, "utf-8-bom", true, String::new()),
            Err(_) => mk(String::new(), "utf-8-bom", false, "带了 BOM 但里面不是合法 UTF-8，按只读打开".into()),
        };
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        // 两个一组的字节：不够一对的尾巴丢掉（半个 UTF-16 码元没有意义）
        let wide: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return mk(String::from_utf16_lossy(&wide), "utf-16le", false,
            "这是 UTF-16 文件。工具按 UTF-16 读出来了，但不支持编辑写回——动它会把编码改掉。".into());
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return mk(String::new(), "utf-16be", false, "这是 UTF-16BE 文件，暂不支持编辑".into());
    }
    if let Ok(t) = String::from_utf8(bytes.to_vec()) {
        return mk(t, "utf-8", true, String::new());
    }
    // 不是合法 UTF-8：中文 Windows 上绝大多数是 GBK（代码页 936）
    if let Some(t) = win_mb_to_string(bytes, 936) {
        let bad = t.matches('\u{FFFD}').count();
        if bad == 0 {
            return mk(t, "gbk", true,
                "这个文件是 GBK 编码（不是 UTF-8）。编辑会按 GBK 存回去，不会把编码改掉。".into());
        }
    }
    mk(String::new(), "unknown", false,
        "认不出这个文件的编码，按只读打开。硬改会把它的编码弄坏。".into())
}

/// 按原编码与原换行编码回去。
pub fn encode(text: &str, encoding: &str, eol: &str) -> Vec<u8> {
    let text = if eol == "crlf" {
        // 先把可能混进来的裸 \n 统一成 \r\n
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.replace("\r\n", "\n")
    };
    match encoding {
        "utf-8-bom" => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(text.as_bytes());
            v
        }
        "gbk" => win_string_to_mb(&text, 936).unwrap_or_else(|| text.into_bytes()),
        _ => text.into_bytes(),
    }
}

/// 子进程输出的一行 → 文字。
///
/// 顺序是有讲究的：先按**严格 UTF-8** 试（cargo / node / 大部分现代工具都吐 UTF-8），
/// 不行再按**本机 ANSI 代码页**（中文 Windows 是 936）——中文环境下 Python、
/// 老工具往管道里写中文时用的就是它。都不行才用有损兜底。
///
/// 为什么不能一上来就有损：`from_utf8_lossy` 会把 GBK 的中文变成一串
/// U+FFFD，用户看到的是满屏问号，而那条报错本来是能读懂的。
pub fn decode_output(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    if let Some(s) = win_mb_to_string(bytes, ansi_code_page()) {
        return s;
    }
    String::from_utf8_lossy(bytes).to_string()
}

/// 本机 ANSI 代码页（中文 Windows = 936）。非 Windows 上按 UTF-8 处理。
pub fn ansi_code_page() -> u32 {
    #[cfg(windows)]
    {
        extern "system" {
            fn GetACP() -> u32;
        }
        // SAFETY: 无入参，返回一个代码页编号。
        let cp = unsafe { GetACP() };
        if cp == 0 { 936 } else { cp }
    }
    #[cfg(not(windows))]
    {
        65001
    }
}

pub fn read(path: &Path) -> std::io::Result<TextFile> {
    let bytes = std::fs::read(path)?;
    Ok(decode(&bytes))
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    #[test]
    fn plain_utf8_round_trips() {
        let t = decode("fn main() {}\n// 中文\n".as_bytes());
        assert_eq!(t.encoding, "utf-8");
        assert_eq!(t.eol, "lf");
        assert!(t.readable);
        assert_eq!(encode(&t.text, &t.encoding, &t.eol), "fn main() {}\n// 中文\n".as_bytes());
    }

    /// CRLF 必须原样保住。这条是"整文件被重写"那类事故的守门测试。
    #[test]
    fn crlf_is_detected_and_restored() {
        let t = decode(b"a\r\nb\r\n");
        assert_eq!(t.eol, "crlf");
        // textarea 会把 CRLF 规范化成 LF，所以写回时这里给的是 LF 版本
        let back = encode("a\nb\n", "utf-8", "crlf");
        assert_eq!(back, b"a\r\nb\r\n");
    }

    #[test]
    fn utf8_bom_is_kept() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("hi".as_bytes());
        let t = decode(&bytes);
        assert_eq!(t.encoding, "utf-8-bom");
        assert_eq!(t.text, "hi");
        assert_eq!(encode("hi", "utf-8-bom", "lf"), bytes);
    }

    /// GBK 的中文：读得出来，而且写得回去（Windows 上）。
    #[cfg(windows)]
    #[test]
    fn gbk_chinese_survives_a_round_trip() {
        let original = "# 中文注释\nkey = 值\n";
        let gbk = win_string_to_mb(original, 936).expect("这台机器应该有 CP936");
        assert_ne!(gbk, original.as_bytes(), "GBK 与 UTF-8 的字节应该不同，否则这测试没意义");
        let t = decode(&gbk);
        assert_eq!(t.encoding, "gbk");
        assert_eq!(t.text, original.replace("\r\n", "\n"));
        assert!(t.readable, "GBK 应该可编辑，但要按 GBK 存回去");
        assert_eq!(encode(&t.text, "gbk", "lf"), gbk);
    }

    /// 子进程输出：UTF-8 原样留下，GBK 中文要解出来而不是变成问号。
    #[cfg(windows)]
    #[test]
    fn console_output_decodes_gbk_chinese() {
        let gbk = win_string_to_mb("错误：找不到模块\n", 936).unwrap();
        assert_eq!(decode_output(&gbk), "错误：找不到模块\n");
        // 已经是 UTF-8 的不能被动到
        assert_eq!(decode_output("error: not found".as_bytes()), "error: not found");
    }

    /// 认不出的编码：只读，并且说清为什么。**不猜、不替换**。
    #[test]
    fn unknown_encoding_is_read_only_not_replaced() {
        // 0xFF 开头、不是合法 UTF-8，也不是能解出干净文本的 GBK
        let t = decode(&[0xFF, 0xFF, 0xFF]);
        assert!(!t.readable);
        assert!(!t.note.is_empty(), "只读必须给出理由");
    }
}
