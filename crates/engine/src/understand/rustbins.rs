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