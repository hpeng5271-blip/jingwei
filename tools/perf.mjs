#!/usr/bin/env node
// 性能量具（2026-10-22，D-20261022-03 Q7-B：**进但只报不阻断**）。
//
// 用法：
//   node tools/perf.mjs            # 跑一轮，打印表格 + 与上次比
//   node tools/perf.mjs --json     # 只出 JSON
//   node tools/perf.mjs --quiet    # 只写日志，不打印表格（门禁里用）
//
// 记账：`%LOCALAPPDATA%\jingwei\perf.jsonl`（与其它数据同一目录；Q1 定的位置）。
// 为什么只报不阻断：现在"能跑最重要"；但必须**持续可见**，否则退化没人知道。

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import os from 'node:os';

const args = process.argv.slice(2);
const has = (k) => args.includes(`--${k}`);

const root = process.cwd();
const candidates = [
  path.join(root, 'target', 'release', process.platform === 'win32' ? 'jingwei.exe' : 'jingwei'),
  path.join(root, 'target', 'debug', process.platform === 'win32' ? 'jingwei.exe' : 'jingwei'),
];
const exe = candidates.find((p) => fs.existsSync(p));
if (!exe) {
  console.error('找不到 jingwei 可执行文件（先 cargo build）');
  process.exit(0);   // 只报不阻断：量具失败不该拦人
}

// 挑一个真实存在的文件给 `file` 用；找不到就跳过这一项（不编路径）
const sampleFile = ['crates/cli/src/main.rs', 'crates/cli/src/protocol.rs', 'Cargo.toml']
  .map((p) => path.join(root, p))
  .find((p) => fs.existsSync(p));

const actions = [
  ['index', ['index', '--root', root]],
  ['handover', ['handover', '--root', root, '--json']],
  ['doctor', ['doctor', '--root', root, '--json']],
  ['next', ['next', '--root', root, '--json']],
  ...(sampleFile ? [['file', ['file', path.relative(root, sampleFile), '--root', root, '--json']]] : []),
];

const dataDir = path.join(process.env.LOCALAPPDATA || os.tmpdir(), 'jingwei');
fs.mkdirSync(dataDir, { recursive: true });
const logPath = path.join(dataDir, 'perf.jsonl');

const readLast = () => {
  if (!fs.existsSync(logPath)) return {};
  const lines = fs.readFileSync(logPath, 'utf8').trim().split('\n').filter(Boolean).slice(-12);
  const last = {};
  for (const line of lines) {
    try {
      const row = JSON.parse(line);
      if (row && row.action) last[row.action] = row.ms;   // 后出现的覆盖先出现的（取每项最近一次）
    } catch { /* 坏行当没有 */ }
  }
  return last;
};

const before = readLast();
const rows = [];
for (const [name, argv] of actions) {
  const t0 = process.hrtime.bigint();
  const r = spawnSync(exe, argv, { cwd: root, encoding: 'utf8', timeout: 120000 });
  const ms = Number(process.hrtime.bigint() - t0) / 1e6;
  rows.push({ at: new Date().toISOString(), action: name, ms: Math.round(ms), code: r.status ?? -1, exe });
}

fs.appendFileSync(logPath, rows.map((r) => JSON.stringify(r)).join('\n') + '\n', 'utf8');

if (has('json')) {
  console.log(JSON.stringify(rows, null, 2));
  process.exit(0);
}
if (!has('quiet')) {
  console.log('动作耗时（毫秒；「上次」来自 perf.jsonl 的同名项）');
  console.log('  ' + '动作'.padEnd(10) + '本次'.padStart(8) + '上次'.padStart(8) + '变化'.padStart(9) + '  退出码');
  for (const r of rows) {
    const prev = before[r.action];
    let delta = '—';
    if (typeof prev === 'number') {
      const d = r.ms - prev;
      delta = (d >= 0 ? '+' : '') + d + 'ms';
    }
    console.log(
      '  ' + r.action.padEnd(10) + String(r.ms).padStart(8) + String(prev ?? '—').padStart(8)
        + delta.padStart(9) + '  ' + r.code,
    );
  }
  console.log(`\n已记入：${logPath}`);
  console.log('（只报不阻断：这里慢不会让门禁红；要拦人请另议，见 D-20261022-03 Q7）');
}
process.exit(0);
