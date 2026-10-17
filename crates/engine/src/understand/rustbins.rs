//! **cargo 的"这个 crate 有几个二进制 / 有没有某个键"**——一处判据。
//!
//! ## 为什么单独一个文件
//!
//! 这两件事第一版写在 `project.rs` 里，而那个文件**登记在案、只许降不许升**，
//! 加 43 行就被工程门禁挡了。**该挡**：它已经 1800 行，再往里塞就是在奖励懒。
//! 搬出来之后 `project.rs` 反倒比原来短。
//!
//! ## 判据为什么要看 `autobins`
//!
//! `brotli-9.0.0` 的 `src/bin/` 里有 **9 个 `.rs`**，但它只声明了 **2 个 `[[bin]]`**，
//! 另外 7 个是被模块引用的——它写了 `autobins = false`，所以 cargo 只认那 2 个。
//! 不看 `autobins` 就会数成 11，把一条正确的规则变成误报。

use std::path::Path;
/// cargo 的**二进制目标**有几个（三种来源都算）。
///
/// ## 为什么要看 `autobins`
///
/// `brotli-9.0.0` 的 `src/bin/` 里有 **9 个 `.rs`**，但它只声明了 **2 个 `[[bin]]`**，
/// 另外 7 个是被 `include!` 进去的模块——**它写了 `autobins = false`**，
/// 所以 cargo 只认那 2 个。不看 `autobins` 就会数成 11，把一条正确的规则变成误报。
///
/// 取 `max(声明的, 目录里的)` 而不是相加：同一批文件常常两处都算到
/// （`[[bin]] path = "src/bin/x.rs"`），相加会重复计数。**宁可少数，多数就更错。**
pub(crate) fn rust_bin_targets(root: &Path) -> usize {
    let toml = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let main = usize::from(root.join("src").join("main.rs").is_file());
    let declared = toml
        .lines()
        .filter(|l| l.trim_start().starts_with("[[bin]]"))
        .count();
    let autobins_off = toml.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("autobins") && t.replace(' ', "").contains("=false")
    });
    let in_dir = if autobins_off {
        0
    } else {
        std::fs::read_dir(root.join("src").join("bin"))
            .map(|rd| {
                rd.flatten()
                    .filter(|e| {
                        e.path().extension().map(|x| x == "rs").unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    };
    main + declared.max(in_dir)
}

/// `Cargo.toml` 里有没有以某个键开头的行（`default-run` / `edition` 这类）。
pub(crate) fn cargo_toml_has(root: &Path, key: &str) -> bool {
    std::fs::read_to_string(root.join("Cargo.toml"))
        .map(|t| t.lines().any(|l| l.trim_start().starts_with(key)))
        .unwrap_or(false)
}

