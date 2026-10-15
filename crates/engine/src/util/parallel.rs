//! 极简并行：N 个线程共用一个原子游标，谁空谁取下一个。
//!
//! 不引 rayon —— 为了几十个目录去引一个线程池库不划算，
//! 而且构建时间和依赖面都会变大。这里要的就是「够用且看得懂」。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// 干活线程数。上限压到 16：这活是 I/O 密集，
/// 线程再多也只是让磁盘队列更乱，反而更慢。
pub fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 16)
}

/// 保序并行 map：结果下标和输入一致。
pub fn map<T, R, F>(items: Vec<T>, f: F) -> Vec<R>
where
    T: Send,
    R: Send,
    F: Fn(T) -> R + Sync,
{
    let n = items.len();
    if n == 0 {
        return Vec::new();
    }
    let workers = threads().min(n);
    if workers <= 1 || n == 1 {
        return items.into_iter().map(f).collect();
    }

    let slots: Vec<Mutex<Option<T>>> = items.into_iter().map(|t| Mutex::new(Some(t))).collect();
    let done: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(n));
    let cursor = AtomicUsize::new(0);
    let slots = &slots;
    let done = &done;
    let f = &f;
    let cursor = &cursor;

    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let task = match slots[i].lock() {
                    Ok(mut guard) => guard.take(),
                    Err(_) => None,
                };
                if let Some(task) = task {
                    let r = f(task);
                    if let Ok(mut guard) = done.lock() {
                        guard.push((i, r));
                    }
                }
            });
        }
    });

    let collected = match done.lock() {
        Ok(mut guard) => std::mem::take(&mut *guard),
        Err(_) => Vec::new(),
    };
    let mut out: Vec<Option<R>> = (0..n).map(|_| None).collect();
    for (i, r) in collected {
        out[i] = Some(r);
    }
    out.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    // 测试里的临时目录清理失败不影响断言，也不能改变结论——这里显式豁免，
    // 而不是让 lint 在整个 crate 里静默失声。
    #![allow(clippy::let_underscore_must_use)]
    use super::*;

    #[test]
    fn results_keep_input_order() {
        let got = map((0..500).collect::<Vec<usize>>(), |i| i * 2);
        assert_eq!(got.len(), 500);
        for (i, v) in got.iter().enumerate() {
            assert_eq!(*v, i * 2);
        }
    }

    #[test]
    fn empty_and_single_inputs_are_fine() {
        assert!(map(Vec::<usize>::new(), |i| i).is_empty());
        assert_eq!(map(vec![7], |i| i + 1), vec![8]);
    }
}
