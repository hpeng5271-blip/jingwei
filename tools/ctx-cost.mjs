#!/usr/bin/env node
// **上下文成本表**（2026-10-18）：每一份文档"读进去要花多少"，按"每轮必读 / 按需读"分开算。
//
// 为什么要有它：用户的原话是"输入上下文用量非常庞大，哪怕缓存命中率很高"。
// 单靠感觉管不住这件事——**只有把它变成一张表，才能决定"哪一段该搬走"**。
//
// 判据（能执行的两条）：
//   1. **每轮必读**只有 `AGENTS.md`（DSH 注入的那份），它的体积有上限（门禁守着）；
//   2. 其余都是**按需读**：没有上限，但**读之前先看这张表**，超过 ~500 行的交给子代理摘要。
//
// 估算方式：中文按 **2.2 字符/token** 粗估（英文偏低、中文偏高，量级够用）。
// 不追求精确——追求的是"哪一块占了大头"这种可以拿来决策的比例。

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const ALWAYS = new Set(['AGENTS.md']);
const SKIP = new Set(['node_modules', 'target', 'dist', '.git', '.jingwei']);

function docs(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) {
      if (!SKIP.has(name)) docs(p, out);
    } else if (name.toLowerCase().endsWith('.md')) {
      out.push(p);
    }
  }
  return out;
}

const rows = docs(ROOT).map((p) => {
  const text = readFileSync(p, 'utf8');
  const chars = text.length;
  return {
    name: p.slice(ROOT.length).replace(/\\/g, '/'),
    lines: text.split(/\r?\n/).length,
    chars,
    tokens: Math.round(chars / 2.2),
    always: ALWAYS.has(p.slice(ROOT.length).replace(/\\/g, '/')),
  };
});

const alwaysTok = rows.filter((r) => r.always).reduce((a, b) => a + b.tokens, 0);
const onceTok = rows.filter((r) => r.always).reduce((a, b) => a + b.tokens, 0);
const needTok = rows.filter((r) => !r.always).reduce((a, b) => a + b.tokens, 0);

console.log('\n  **每轮必读**（DSH 注入，成本 × 轮数）');
console.log('  ' + '文件'.padEnd(22) + '行'.padStart(6) + '字符'.padStart(9) + '≈tokens'.padStart(10));
for (const r of rows.filter((x) => x.always)) {
  console.log('  ' + r.name.padEnd(22) + String(r.lines).padStart(6) + String(r.chars).padStart(9) + String(r.tokens).padStart(10));
}
console.log(`  → 每轮固定 ${alwaysTok} tokens（按 20 轮算 ≈ ${alwaysTok * 20} tokens）`);

console.log('\n  **按需读**（只在需要时打开，不占每轮成本）—— 最贵的 8 份');
console.log('  ' + '文件'.padEnd(30) + '行'.padStart(6) + '≈tokens'.padStart(10) + '  建议');
for (const r of rows.filter((x) => !x.always).sort((a, b) => b.tokens - a.tokens).slice(0, 8)) {
  const hint = r.lines > 500 ? '先让子代理摘要（>500 行）' : '可直接读';
  console.log('  ' + r.name.padEnd(30) + String(r.lines).padStart(6) + String(r.tokens).padStart(10) + '  ' + hint);
}
console.log(`  → 全部按需文档合计 ${needTok} tokens（**不是每轮都付**）`);

const limit = 260;
const agents = rows.find((r) => r.name === 'AGENTS.md');
console.log('');
if (!agents) {
  console.log('  ✗ 找不到 AGENTS.md');
  process.exit(1);
}
if (agents.lines > limit) {
  console.log(`  ✗ AGENTS.md ${agents.lines} 行，超过上限 ${limit}——每轮成本会线性增长`);
  process.exit(1);
}
console.log(`  ✓ AGENTS.md ${agents.lines} 行 / ≈${agents.tokens} tokens（上限 ${limit} 行）`);
console.log(`  · 一次会话读 3 份大材料的成本 ≈ ${((alwaysTok + 3 * 4000) / 1000).toFixed(1)}k tokens —— 这就是要省的地方\n`);
