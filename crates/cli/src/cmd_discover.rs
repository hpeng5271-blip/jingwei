//! `jingwei discover`：**扫盘找项目**（可扫多个根、边扫边出、可中断、有终态）。
//!
//! 为什么单开一个命令：界面那条路（Tauri command + 轮询）不好在门禁里跑，
//! 而"扫得全不全"必须有**机器能验**的入口。这个命令就是那个入口——
//! 与界面**同一套引擎**（`dirs_index::build_with` + `projects::describe_public`）。
//!
//! 三种输出，对应三态：
//! - **发现**：`+ 路径  类型  体量`；
//! - **进度**：每 400 个目录一行（`已看 N · 跳过 M · 发现 K · 当前 …`）；
//! - **终态**：`✓ 扫完：N 个目录 / M 个项目 / 用时 T 秒`，或者 `! 中断：…`
//!   ——**永远有个明确的结尾**，不许让调用方猜"它还在扫吗"。

use jingwei_engine::understand::{dirs_index, fingerprint, projects};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// 体量分档（首页用它分组）：小 <50 文件、中 <500、大 <4000、超大 ≥4000。
fn size_class(files: u64) -> &'static str {
    if files < 50 {
        "小"
    } else if files < 500 {
        "中"
    } else if files < 4000 {
        "大"
    } else {
        "超大"
    }
}

/// **同源检测**：只比"同名目录"（跨机器/克隆最常见的那一类），
/// 阈值 0.8（学习调研给的数：低于它更像"碰巧都有几个相似文件"）。
/// 只对候选算集合，不跑全量两两比较——**成本要可控**。
fn similar(list: &[(String, String, Vec<String>)], min: f64) -> Vec<(String, String, f64)> {
    let mut out = Vec::new();
    for i in 0..list.len() {
        for j in (i + 1)..list.len() {
            if list[i].1 != list[j].1 {
                continue; // 名字不同不比（"同内容不同名"是另一类，代价更高，先不做）
            }
            let s = jingwei_engine::understand::fingerprint::jaccard(&list[i].2, &list[j].2);
            if s >= min {
                out.push((list[i].0.clone(), list[j].0.clone(), s));
            }
        }
    }
    out
}

pub fn cmd_discover(
    roots: Vec<PathBuf>,
    depth: usize,
    json: bool,
    want_similar: bool,
    want_why: bool,
    journal: Option<&std::path::Path>,
) -> i32 {
    let roots: Vec<PathBuf> = if roots.is_empty() {
        projects::default_scan_roots()
    } else {
        roots
    };
    if roots.is_empty() {
        eprintln!("没有可扫的根目录——给一个，或者加 --all 从盘根扫");
        return 2;
    }

    // **增量**（D-03 第 2 步）：给了 journal 就只扫"变过的项目根"。
    // 三态：读不到/overflow/空 → **明说退回全量**，不许把"没记录"当"没变化"。
    let (roots, depth) = match journal {
        None => (roots, depth),
        Some(j) => match jingwei_engine::understand::dirwatch::journal_roots(j) {
            Ok((live, bad)) if !live.is_empty() => {
                println!(
                    "增量：只扫 {} 个目录（journal {} 行，坏 {}）",
                    live.len(),
                    std::fs::read_to_string(j).map(|s| s.lines().count()).unwrap_or(0),
                    bad
                );
                (live, 1)
            }
            Ok((_, bad)) => {
                eprintln!("增量查不了：journal 里没有可用目录（坏 {bad} 行）——退回全量");
                (roots, depth)
            }
            Err(e) => {
                eprintln!("增量查不了：{e}——退回全量");
                (roots, depth)
            }
        },
    };
    // 门禁里跑的时候要能**自己停**（否则一条命令能挂十分钟）
    let stop = AtomicBool::new(false);
    let t0 = Instant::now();
    let mut found: Vec<serde_json::Value> = Vec::new();
    let mut seen = 0usize;
    let mut skips: Vec<(String, usize, Vec<String>)> = Vec::new();
    let mut skipped = 0usize;
    let mut total = 0usize;
    // (路径, 目录名, 文件集合)——只在 --similar 时填，避免白算
    let mut sets: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut last_print = 0usize;
    // 计时（JINGWEI_SCAN_TIMING=1 才打印）。

    // **指纹缓存**（债 0.30）：签名没变就复用，省掉"把项目里每个文件读一遍"。
    let fp_cache = std::sync::Arc::new(std::sync::Mutex::new(
        jingwei_engine::understand::fp_cache::FpCache::open(),
    ));

    for root in &roots {
        if !root.is_dir() {
            eprintln!("跳过（目录不存在）：{}", root.display());
            continue;
        }
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
        let idx = dirs_index::build_parallel(root, depth.max(1), threads, &stop, |v, s, cur| {
            seen = v;
            skipped = s;
            if v >= last_print + 400 {
                last_print = v;
                eprintln!("  … 已看 {v} 个目录 · 跳过 {s} · 发现 {} 个项目 · 当前 {}", found.len(), cur.display());
            }
        });
        if want_why {
            skips.extend(idx.skip_stats());
        }
          // **项目级三件事并行**（2026-10-20 性能）：describe / measure / fingerprint
          // 都是"读这个项目自己的文件"，彼此独立；串行时 fingerprint 一项就占了 86%（实测 5.08s/422 项）。
          let project_list: Vec<std::path::PathBuf> = idx.all_projects();
          let n = project_list.len();
          type Spec = (projects::Found, (u64, u64, bool), fingerprint::Fingerprint, Vec<String>);
          let mut specs: Vec<Option<Spec>> = (0..n).map(|_| None).collect();
          let t_proj = std::time::Instant::now();
          if n > 0 {
              let threads = std::thread::available_parallelism()
                  .map(|v| v.get())
                  .unwrap_or(4)
                  .min(8)
                  .min(n);
              let chunk = n.div_ceil(threads);
              std::thread::scope(|s| {
                  let mut handles = Vec::new();
                  for start in (0..n).step_by(chunk) {
                      let end = (start + chunk).min(n);
                      let pl = &project_list;
                      let have_json = json;
                      let fp_cache = fp_cache.clone();
                      handles.push(s.spawn(move || {
                          let mut local = Vec::with_capacity(end - start);
                          for (i, p) in pl.iter().enumerate().take(end).skip(start) {
                              let f = projects::describe_public(p);
                              let m = projects::measure(p);
                              // **只在需要时算指纹**（2026-10-20 性能）：它要把项目里每个文件读一遍
                              // 算哈希，温缓存下 422 个项目要 5s，冷缓存更贵。
                              // 只有 --json（界面要 hash）和 --similar（要集合）才用得上。
                              let (fp, set) = if want_similar {
                                  // 相似检测要"文件集合"，缓存里没有，老实算。
                                  fingerprint::of_detailed(p)
                              } else if have_json {
                                  use jingwei_engine::understand::fp_cache::tree_sig;
                                  let sig = tree_sig(p);
                                  let hit = { fp_cache.lock().unwrap().get(p, &sig) };
                                  match hit {
                                      Some(f) => (f, Vec::new()),
                                      None => {
                                          let f = fingerprint::of(p);
                                          fp_cache.lock().unwrap().put(p, sig, f.clone());
                                          (f, Vec::new())
                                      }
                                  }
                              } else {
                                  (fingerprint::Fingerprint::default(), Vec::new())
                              };
                              local.push((i, f, m, fp, set));
                          }
                          local
                      }));
                  }
                  for h in handles {
                      for (i, f, m, fp, set) in h.join().unwrap() {
                          specs[i] = Some((f, m, fp, set));
                      }
                  }
              });
              total += n;
          }
          let proj_secs = t_proj.elapsed().as_secs_f64();
          for (f, (files, bytes, cut), fp, set) in specs.into_iter().flatten() {
              if want_similar {
                  sets.push((f.path.clone(), f.name.clone(), set));
              }
              let kind = f.kinds.first().cloned().unwrap_or_else(|| "未知".to_string());
              if json {
                  found.push(serde_json::json!({
                      "path": f.path, "name": f.name, "kinds": f.kinds,
                      "markers": f.markers, "files": files, "bytes": bytes,
                      "size": size_class(files), "size_cut": cut,
                      "fp": fp.hash, "fp_truncated": fp.truncated,
                      "lockfile": fp.lockfile,
                      "age_days": f.age_days,
                  }));
              } else {
                  println!(
                      "  + {:<9} {:<6} {:>5} 文件  {}",
                      kind,
                      size_class(files),
                      files,
                      f.path
                  );
              }
          }
          if std::env::var_os("JINGWEI_SCAN_TIMING").is_some() {
              eprintln!("  项目级(并行)：{:.2}s / {} 项", proj_secs, n);
          }
    }

    let pairs = if want_similar { similar(&sets, 0.8) } else { Vec::new() };
    {
        let c = fp_cache.lock().unwrap();
        c.save();
        let (hits, misses) = c.stats();
        if std::env::var_os("JINGWEI_SCAN_TIMING").is_some() {
            eprintln!("  指纹缓存：命中 {hits} / 未命中 {misses}");
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    if json {
        let out = serde_json::json!({
            "roots": roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "depth": depth,
            "dirs_seen": seen, "dirs_skipped": skipped,
            "found": found,
            "seconds": secs,
            "stopped": stop.load(Ordering::Relaxed),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        0
    } else {
        if want_why {
            let with_markers = skips.len(); // 占位，下面用真实数据替换
            let _ = with_markers;
            println!("  目录行数 / 有标记的行 / 项目数：{} / … / {}", seen, total);
        }
        if want_why && !skips.is_empty() {
            println!("  ⊘ 被跳过的目录（按原因）：");
            let mut agg: std::collections::BTreeMap<String, (usize, Vec<String>)> = Default::default();
            for (reason, n, samples) in &skips {
                let e = agg.entry(reason.clone()).or_insert((0, Vec::new()));
                e.0 += n;
                for s in samples {
                    if e.1.len() < 2 {
                        e.1.push(s.clone());
                    }
                }
            }
            let mut list: Vec<_> = agg.into_iter().collect();
            list.sort_by_key(|e| std::cmp::Reverse(e.1 .0));
            for (reason, (n, samples)) in list {
                println!("    {n:>4}  {reason}");
                for s in samples {
                    println!("          例：{s}");
                }
            }
        }
        if !pairs.is_empty() {
            println!("  ≈ 同源（Jaccard ≥ 0.8）：{} 对", pairs.len());
            for (a, b, s) in &pairs {
                println!("    {:.0}%  {}\n          {}", s * 100.0, a, b);
            }
        }
        println!(
            "  ✓ 扫完：{} 个目录（跳过 {}）/ {} 个项目 / 用时 {:.1} 秒",
            seen,
            skipped,
            total,
            secs
        );
        0
    }
}
