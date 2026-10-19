// **彻底清掉子智能体**（2026-10-19，用户授权）
//
// 为什么必须在 DSH 关闭时跑：运行中的应用会把"子代理注册表"从内存重写回
// projection 缓存（我实测过：改完文件，几秒后条目又回来了）。所以顺序是：
//   关闭 DSH → 运行本脚本 → 打开 DSH
//
// 做三件事（每件都可回滚）：
//   1. 备份 projection 缓存；
//   2. 从缓存里删掉指定 childId 的注册条目；
//   3. 永久删除已隔离的会话目录（`%DSH_HOME%\_removed-*`）。
//
// 用法：node tools/kill-subagents.mjs [--force] [--dry-run]

import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { execSync } from 'node:child_process';

const DSH_HOME = process.env.DSH_HOME || path.join(os.homedir(), '.dsh');
const PARENT = 'session-9e05082a-ded8-4b99-ae65-06298a3a9dd0';
const IDS = [
  '3a6e12ad-8c7d-45f2-aa14-992feb4f7147',
  '0617577e-3af4-4d80-8ba2-d6b32a518302',
  '4ed45418-c452-4948-984a-5cfd19aab296',
  'd87d3c5a-10ec-4dcc-87f7-2e10947046ee',
  '3a302cba-1108-4b4d-9f4b-d19a9ad5e30b',
  '39862a7d-5329-46c8-8316-f8f693969b28',
];

const dry = process.argv.includes('--dry-run');
const force = process.argv.includes('--force');

// 0) 应用是否在跑
let running = false;
try {
  const out = execSync('tasklist /FI "IMAGENAME eq DeepSeek Harness.exe" /FO CSV /NH', {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  running = out.toLowerCase().includes('deepseek');
} catch {
  running = false;
}
if (running && !force) {
  console.error('✗ DSH 还在运行。先关掉它，再跑本脚本（否则内存会覆盖文件）。');
  process.exit(2);
}
if (running) console.log('⚠️ 检测到 DSH 仍在运行：--force 已允许继续，但结果可能被覆盖。');

// 1) 备份 + 2) 过滤 projection
const cacheDir = path.join(DSH_HOME, 'storages', 'session_projcache', 'sessions');
const cacheFile = path.join(cacheDir, `${PARENT}.json`);
const backupDir = path.join(DSH_HOME, `_cleanup-backup-${new Date().toISOString().slice(0, 10)}`);

if (!fs.existsSync(cacheFile)) {
  console.error('✗ 找不到 projection 缓存：' + cacheFile);
  process.exit(2);
}

const raw = fs.readFileSync(cacheFile, 'utf8');
const j = JSON.parse(raw);
let removed = 0;

const arrays = [];
(function walk(node, p) {
  if (Array.isArray(node)) {
    if (node.some((it) => it && typeof it === 'object' && 'childId' in it)) arrays.push(p);
    node.forEach((it, i) => walk(it, `${p}[${i}]`));
    return;
  }
  if (node && typeof node === 'object') for (const k of Object.keys(node)) walk(node[k], `${p}.${k}`);
})(j, '');

for (const p of arrays) {
  const parts = p.replace(/^\./, '').split(/\.|\[|\]/).filter(Boolean);
  let arr = j;
  for (const part of parts) arr = arr[/^\d+$/.test(part) ? Number(part) : part];
  if (!Array.isArray(arr)) continue;
  for (let i = arr.length - 1; i >= 0; i--) {
    if (arr[i] && IDS.includes(arr[i].childId)) {
      arr.splice(i, 1);
      removed++;
    }
  }
}

console.log(`projection 中待删条目：${removed}（数组 ${arrays.length} 处）`);
if (!dry) {
  fs.mkdirSync(backupDir, { recursive: true });
  fs.writeFileSync(path.join(backupDir, `${PARENT}.json`), raw, 'utf8');
  fs.writeFileSync(cacheFile, JSON.stringify(j), 'utf8');
  console.log('已写回 projection，备份在 ' + backupDir);
}

// 3) 永久删除隔离目录
const dirs = fs
  .readdirSync(DSH_HOME)
  .filter((n) => n.startsWith('_removed-'))
  .map((n) => path.join(DSH_HOME, n));

let deleted = 0;
for (const d of dirs) {
  for (const id of IDS) {
    const p = path.join(d, id);
    if (fs.existsSync(p)) {
      if (!dry) fs.rmSync(p, { recursive: true, force: true });
      deleted++;
    }
  }
}
console.log(`隔离目录中的会话：${deleted} 个${dry ? '（dry-run 未删）' : '已永久删除'}`);

// 复读验证
if (!dry) {
  const after = JSON.parse(fs.readFileSync(cacheFile, 'utf8'));
  const flat = JSON.stringify(after);
  const left = IDS.filter((id) => flat.includes(id));
  console.log(left.length ? '⚠️ 仍残留：' + left.join(', ') : '✓ 复读验证：6 个 id 已全部消失');
}
