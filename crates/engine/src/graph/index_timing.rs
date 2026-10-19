//! **索引分阶段计时**（2026-10-20，性能）：`JINGWEI_INDEX_TIMING=1` 时打印。
//!
//! 拆成单独文件的原因很实际：`index.rs` 已经贴着 1000 行上限（门禁守），
//! 而这段计时是"观察用"的，不该占主逻辑的行数。

#[derive(Default)]
pub struct IndexTiming {
    pub parse_ns: u128,
    pub write_ns: u128,
}

impl IndexTiming {
    pub fn add_parse(&mut self, t: std::time::Instant) {
        self.parse_ns += t.elapsed().as_nanos();
    }

    pub fn add_write(&mut self, t: std::time::Instant) {
        self.write_ns += t.elapsed().as_nanos();
    }

    /// 只在设了环境变量时打印；没设就什么都不做（不拖慢正常路径）。
    pub fn print(&self, files: usize) {
        if std::env::var_os("JINGWEI_INDEX_TIMING").is_none() {
            return;
        }
        eprintln!(
            "  index 计时：解析 {:.2}s · 落库 {:.2}s（{} 文件）",
            self.parse_ns as f64 / 1e9,
            self.write_ns as f64 / 1e9,
            files
        );
    }
}
