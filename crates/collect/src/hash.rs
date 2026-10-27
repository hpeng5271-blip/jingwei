//! 内容身份：同一份内容（改名/移动后不变）给同一个 entity id。
//!
//! 只是 FNV-1a 64 位——我们不拿它做安全，只做"是不是同一份内容"的便宜判据。

/// 纯内容 → `c:<16 hex>`。用来查"库里有没有同内容的实体"（改名/移动的身份解析入口）。
pub fn content_id(bytes: &[u8]) -> String {
    format!("c:{:016x}", fnv(bytes))
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 内容 + 首次路径 → `h:<16 hex>`。
///
/// 首次路径必须进哈希输入：只哈希内容会让**同内容的两份拷贝**（vendor/生成文件）
/// 聚成一条，历史就串了；进了之后：改名/移动 → entity 不变（首路径已固化），
/// 新文件复用旧路径 → 内容不同 → entity 不同。
pub fn entity_id(bytes: &[u8], first_path: &str) -> String {
    let mut h = fnv(bytes);
    h ^= 0u64;
    for b in first_path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("h:{h:016x}")
}

/// 符号身份：`s:<16 hex>`（path|name|kind 的 FNV）。
///
/// 同路径同名同 kind、内容变了仍是同一实体（长链才能折）；改名/改路径视为新实体。
pub fn symbol_entity(path: &str, name: &str, kind: &str) -> String {
    let key = format!("{path}|{name}|{kind}");
    format!("s:{:016x}", fnv(key.as_bytes()))
}
