// **扫描基线**（2026-10-19，D-01）：把"扫盘快不快"变成可回归的数字。
//
// 判据（pro 定的）：
//   连跑两次，`wall_ms` 相对偏差 ≤10% 才算**可复现**；
//   否则报告写 `查不了：两次不可复现`，**不许编一个数字**。
//
// 用法：
//   node tools/bench-scan.mjs                       # 默认 D:\projects D:\research 深度 4，跑 2 次
//   node tools/bench-scan.mjs --root D:\ --depth 6 --runs 2
//
// 产物：bench/scan.json（带 sha 和时间戳）

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

const args = process.argv.slice(2);
const get = (k, d) => {
  const i = args.indexOf(`--${k}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : d;
};
const roots = [];
for (let i = 0; i < args.length; i++) if (args[i] === '--root' && args[i + 1]) roots.push(args[i + 1]);
const depth = get('depth', '4');
const runs = Number(get('runs', '2'));
const exe = path.join('target', 'release', 'jingwei.exe');

if (!roots.length) roots.push('D:\\projects', 'D:\\research');

const sha = crypto.createHash('sha256').update(fs.readFileSync(exe)).digest('hex').slice(0, 12);

/** 跑一次，reolve {wallMs, firstMs, dirs, projects, terminal} */
function once() {
  return new Promise((resolve) => {
    const t0 = Date.now();
    const p = spawn(exe, ['discover', '--depth', depth, ...roots], { cwd: process.cwd() });
    let firstMs = null;
    let text = '';
    p.stdout.on('data', (b) => {
      const s = b.toString('utf8');
      text += s;
      if (firstMs === null && /^\s*\+/m.test(s)) firstMs = Date.now() - t0;
    });
    p.stderr.on('data', () => {});
    p.on('close', () => {
      const wallMs = Date.now() - t0;
      const m = text.match(/✓ 扫完：(\d+) 个目录（跳过 (\d+)）\/ (\d+) 个项目 \/ 用时 ([\d.]+) 秒/);
      resolve({
        wallMs,
        firstMs,
        dirs: m ? Number(m[1]) : null,
        skipped: m ? Number(m[2]) : null,
        projects: m ? Number(m[3]) : null,
        terminal: m ? m[0] : null,
      });
    });
  });
}

const rows = [];
for (let i = 0; i < runs; i++) rows.push(await once());

const ok = rows.every((r) => r.dirs !== null && r.wallMs > 0);
const walls = rows.map((r) => r.wallMs);
const mean = walls.reduce((a, b) => a + b, 0) / walls.length;
const dev = mean > 0 ? Math.max(...walls.map((w) => Math.abs(w - mean))) / mean : null;
const reproducible = ok && dev !== null && dev <= 0.1;

const out = {
  ts: new Date().toISOString(),
  exe_sha: sha,
  roots,
  depth: Number(depth),
  runs: rows,
  wall_ms_mean: Math.round(mean),
  deviation: dev === null ? null : Number(dev.toFixed(3)),
  reproducible,
  verdict: !ok
    ? '查不了：数据缺失（没有终态行）'
    : reproducible
      ? '可复现：偏差 ≤10%'
      : `查不了：两次偏差 ${(dev * 100).toFixed(1)}% > 10%（冷缓存不干净）`,
};

fs.mkdirSync('bench', { recursive: true });
fs.writeFileSync('bench/scan.json', JSON.stringify(out, null, 1), 'utf8');
console.log(JSON.stringify(out, null, 1));

if (!ok) process.exit(1);
if (!reproducible) process.exit(1);
