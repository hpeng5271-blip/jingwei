// **只扫变过的目录**（2026-10-20，债 0.27 的 Node 版）：读 `watch.mjs` 写的 journal，
// 把变更路径收成"父目录集合"，只对这些目录跑一次 `discover --depth 1`。
//
// **三态口径（不许压扁）**：
//   - journal 存在且比上次扫描新 → 增量：只扫 N 个目录；
//   - journal 不存在/为空 → **查不了**（没有变更记录），退回全量，退出码 2 并说清楚；
//   - journal 里读到坏行 → 跳过坏行并报数量（不静默丢）。
//
// 用法：node tools/scan-changed.mjs [--exe <jingwei.exe>] [--dry]

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const args = process.argv.slice(2);
const get = (k, d) => {
  const i = args.indexOf(`--${k}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : d;
};
const dry = args.includes('--dry');
const exe = get('exe', path.join('target', 'release', process.platform === 'win32' ? 'jingwei.exe' : 'jingwei'));

/** 从某个目录往上找"项目根"：含清单文件或 .git 的那一层；找不到就返回起点。 */
const MARKERS = [
  'Cargo.toml',
  'package.json',
  'pyproject.toml',
  'setup.py',
  'requirements.txt',
  'go.mod',
  'pom.xml',
  'build.gradle',
  '.git',
];
function project_root_of(start) {
  let d = start;
  for (let i = 0; i < 12; i += 1) {
    for (const m of MARKERS) {
      if (fs.existsSync(path.join(d, m))) return d;
    }
    const parent = path.dirname(d);
    if (parent === d) break;
    d = parent;
  }
  return start;
}
const base = process.env.LOCALAPPDATA || path.join(os.tmpdir(), 'jingwei-local');
const journal = path.join(base, 'jingwei', 'watch.jsonl');
if (!fs.existsSync(journal)) {
  console.error(`查不了：没有变更记录（${journal}）。先跑一次 node tools/watch.mjs <目录>。`);
  process.exit(2);
}

// 去 BOM：PowerShell 的 utf8 会带 BOM，JSON.parse 会把它当坏数据。
const lines = fs
  .readFileSync(journal, 'utf8')
  .replace(/^\uFEFF/, '')
  .split(/\r?\n/)
  .filter(Boolean);
let bad = 0;
const dirs = new Set();
for (const l of lines) {
  let o;
  try {
    o = JSON.parse(l);
  } catch {
    bad += 1;
    continue;
  }
  if (!o || !o.path) continue;
  const p = path.resolve(o.path);
  const start = fs.existsSync(p) && fs.statSync(p).isDirectory() ? p : path.dirname(p);
  // **往上找到项目根**：改的是 `src/lib.rs`，要扫的是它所在项目，不是 `src/`。
  // 找不到根（没有清单/.git）就用最近的一层——那也是我们唯一知道的事实。
  dirs.add(project_root_of(start));
}

// 只保留仍然存在的目录（删掉的目录扫它没有意义）
const live = [...dirs].filter((d) => fs.existsSync(d));
if (bad) console.error(`注意：journal 里有 ${bad} 行坏数据，已跳过。`);
if (!live.length) {
  console.error('查不了：journal 里没有可用目录（可能刚清过或全被删了）——退回全量更稳。');
  process.exit(2);
}

console.log(`增量：只扫 ${live.length} 个目录（journal ${lines.length} 行，坏 ${bad}）`);
for (const d of live.slice(0, 10)) console.log('  ' + d);
if (live.length > 10) console.log(`  …还有 ${live.length - 10} 个`);

if (dry) {
  console.log('（--dry：不跑 discover）');
  process.exit(0);
}

const out = execFileSync(exe, ['discover', '--depth', '1', ...live], { encoding: 'utf8' });
process.stdout.write(out);
