#!/usr/bin/env node
// **研究对象实测表**（2026-10-18）：对每个真项目跑同样的三件事，把数字摊平。
//
// 为什么要它：`框架.md` §九说"判据的漏洞只能靠真输入撞出来"，
// 但**真输入得先被量过**才知道值不值得继续吃。这张表就是量具。
//
// 每行回答四个问题：① 能不能索引、多快 ② 解析覆盖到多少（文件/符号/调用）
// ③ 角色判不判得出来 ④ 一次性结论（guide）说的是什么
//
// 用法：node tools/research-scan.mjs [名字...]
// 落盘：docs/process/研究对象实测.json（数字只信这里，文档里引它）

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = 'D:/projects/jingwei';
const CLI = join(ROOT, 'target/release/jingwei.exe');
const BASE = 'D:/research/jingwei-objects';
const OUT = join(ROOT, 'docs/process/研究对象实测.json');

const ALL = ['fd', 'vite', 'flask', 'petclinic', 'requests', 'tauri'];
const only = process.argv.slice(2);
const names = only.length ? ALL.filter((n) => only.includes(n)) : ALL;

function run(args, cwd) {
  try {
    const out = execFileSync(CLI, args, { cwd, encoding: 'utf8', timeout: 300000, windowsHide: true });
    return { code: 0, out };
  } catch (e) {
    return { code: e.status ?? 1, out: String(e.stdout || '') + String(e.stderr || '') };
  }
}

const rows = [];
for (const name of names) {
  const dir = join(BASE, name);
  if (!existsSync(dir)) { rows.push({ name, skipped: '还没拉到本地' }); continue; }
  // **先清库**：不清的话增量会把没变的文件跳过，量出来的就不是项目规模。
  rmSync(join(dir, '.jingwei'), { recursive: true, force: true });
  const t0 = Date.now();
  const idx = run(['index'], dir);
  const ms = Date.now() - t0;
  const unsupported = idx.out.match(/没有我们能解析的源码[^\n]*\n?\s*它有：([^\n]+)/);
  // 冷索引：`24 个文件 · 451 个符号 · 88 条调用（87ms）`
  // 增量：`这次处理了 0 个文件（扫到 24 个，其余 24 个没变…）· 0 个符号 · 0 条调用`
  const cold = idx.out.match(/(\d+)\s*个文件\s*·\s*(\d+)\s*个符号\s*·\s*(\d+)\s*条调用/);
  const inc = idx.out.match(/扫到\s*(\d+)\s*个[^）]*）\s*·\s*(\d+)\s*个符号\s*·\s*(\d+)\s*条调用/);
  const m = cold || inc;
  const roles = run(['graph', 'roles'], dir);
  const roleLine = (roles.out.split(/\r?\n/).find((l) => l.includes('合计')) || '').trim();
  const buckets = {};
  for (const k of ['入口', '公共底层', '中间层', '测试', '还没连上']) {
    const mm = roles.out.match(new RegExp(k + '[^\\d]*(\\d+)\\s*个文件'));
    if (mm) buckets[k] = Number(mm[1]);
  }
  const g = run(['guide'], dir);
  const verdict = (g.out.split(/\r?\n/).map((l) => l.trim()).filter(Boolean)[0] || '').slice(0, 60);
  rows.push({
    name,
    index_ms: ms,
    files: m ? Number(m[1]) : null,
    symbols: m ? Number(m[2]) : null,
    calls: m ? Number(m[3]) : null,
    roles: buckets,
    roles_sum_line: roleLine,
    guide: verdict,
    index_exit: idx.code,
    unsupported: unsupported ? unsupported[1].trim() : null,
  });
  console.log(`  · ${name.padEnd(10)} ${ms}ms  ${m ? m[1] + ' 文件 · ' + m[2] + ' 符号 · ' + m[3] + ' 调用' : '（解析不出数字）'}`);
}

mkdirSync(join(ROOT, 'docs/process'), { recursive: true });
writeFileSync(OUT, JSON.stringify({ at: new Date().toISOString(), rows }, null, 2), 'utf8');
console.log(`  → ${OUT}`);
