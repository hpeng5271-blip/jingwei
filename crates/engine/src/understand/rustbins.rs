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

/// **这个 crate 的"跑起来"就是 `cargo run` 吗**（2026-10-18 加）。
///
/// 来源：拿本仓库自己的 `crates/cli`（有 `src/main.rs`）跑 `guide`——它说
/// 「还查不出来；补一份 env.contract.json」。**可 `cargo run` 是 cargo 自己就知道的命令**，
/// 让用户为它写一份契约纯属噪音（Node 那边同样的洞刚修过）。
///
/// **只有一个 bin 才给**：多个 bin 时 `cargo run` 会报
/// 「could not determine which binary to run」，那就是**答非所问**了。
pub(crate) fn run_hint(root: &Path) -> Option<String> {
    if rust_bin_targets(root) == 1 {
        Some("cargo run".to_string())
    } else {
        None
    }
}

/// `Cargo.toml` 里有没有以某个键开头的行（`default-run` / `edition` 这类）。
pub(crate) fn cargo_toml_has(root: &Path, key: &str) -> bool {
    std::fs::read_to_string(root.join("Cargo.toml"))
        .map(|t| t.lines().any(|l| l.trim_start().starts_with(key)))
        .unwrap_or(false)
}
/// `Cargo.toml` **没有** `edition` 声明吗（没写就是 2015）。
///
/// ## 为什么值得单独一条
///
/// 2015 与现在的 edition 之间有几个**语义**差别，不是风格问题：
///   · `array.into_iter()` 在 2015 里拿到的是**引用**，2021 里是**值**；
///   · 裸 trait object（`Box<Trait>`）在 2015 里合法（只警告），2021 里必须写 `dyn`；
///   · `?` 在 `main` 里的用法、闭包捕获规则也都变了。
///
/// 所以一个没写 `edition` 的 crate，**照着现代教程改代码会编译不过**，
/// 而报错信息不会告诉你是 edition 的问题。
///
/// ## 校准（466 个真 crate）
///
/// **48 个命中（10%）**，其中 43 个是 0.x/1.x 的老 crate，
/// 只有 **6 个**声明了 `rust-version`（MSRV）——也就是说**绝大多数就是"很老"**，
/// 不是"为了兼容刻意留在 2015"。所以报 `Info` 合适：**对被接手的人有信息量**，
/// 对作者本人只是复述。边界那栏写清了"要支持 1.30 以前就只能留在 2015"。
pub(crate) fn edition_missing(root: &Path) -> bool {
    std::fs::read_to_string(root.join("Cargo.toml"))
        .map(|t| !t.lines().any(|l| l.trim_start().starts_with("edition")))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    // 临时目录清理失败不影响断言（与别的测试模块同一句）。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    /// **只有一个 bin 才敢说 `cargo run`**（2026-10-18，本仓库 `crates/cli` 换的）。
    ///
    /// 多个 bin 时 `cargo run` 会报「could not determine which binary to run」——
    /// 那就成了答非所问，所以宁可说"不知道"，不给一条会错的命令。
    #[test]
    fn 单个bin才给cargo_run() {
        let d = std::env::temp_dir().join(format!("dd-rustbins-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"a\"\nversion = \"0.1.0\"\n").unwrap();
        // 没有 bin 目标 → 不给命令
        assert_eq!(run_hint(&d), None);
        // 一个 main.rs → `cargo run`
        std::fs::write(d.join("src/main.rs"), "fn main() {}").unwrap();
        assert_eq!(run_hint(&d).as_deref(), Some("cargo run"));
        let _ = std::fs::remove_dir_all(&d);
    }
}