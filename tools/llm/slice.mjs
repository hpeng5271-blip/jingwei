// **上下文切片**（2026-10-19）：给 pro 的代码片段由这个脚本产出，
// 不是"我挑几段贴上"。这样"喂多少"是可数的。
//
// 用法：
//   node tools/llm/slice.mjs crates/engine/src/graph/index.rs 640 720
//   node tools/llm/slice.mjs --grep "pub fn upsert" crates/engine/src/graph/store.rs
//
// 输出统一带行号；超过预算（默认 8000 tokens）直接报错，不截断。

import fs from 'node:fs';
import { estimateTokens, CAPS } from './caps.mjs';

const args = process.argv.slice(2);
if (!args.length) {
  console.error('用法：slice.mjs <file> <from> <to> | slice.mjs --grep <pattern> <file>');
  process.exit(2);
}

let file;
let from;
let to;
let grep = null;

if (args[0] === '--grep') {
  grep = args[1];
  file = args[2];
} else {
  file = args[0];
  from = Number(args[1]);
  to = Number(args[2]);
}

const text = fs.readFileSync(file, 'utf8').split(/\r?\n/);

if (grep) {
  // 命中行 ±20 行，最多 3 处
  const hits = [];
  text.forEach((l, i) => {
    if (l.includes(grep)) hits.push(i);
  });
  if (!hits.length) {
    console.error(`没找到 ${grep}`);
    process.exit(1);
  }
  const ranges = hits.slice(0, 3).map((i) => [Math.max(0, i - 20), Math.min(text.length - 1, i + 20)]);
  const seen = new Set();
  const lines = [];
  for (const [a, b] of ranges) {
    if (!seen.has(a)) {
      if (lines.length) lines.push('…');
      seen.add(a);
    }
    for (let i = a; i <= b; i++) lines.push(`${i + 1}: ${text[i]}`);
  }
  emit(lines.join('\n'));
} else {
  if (!Number.isFinite(from) || !Number.isFinite(to) || from < 1 || to < from) {
    console.error('行号要正整数，且 from <= to');
    process.exit(2);
  }
  const lines = text.slice(from - 1, to).map((l, i) => `${from + i}: ${l}`);
  emit(lines.join('\n'));
}

function emit(s) {
  const tok = estimateTokens(s);
  if (tok > CAPS.inputTokens) {
    console.error(`切片 ≈${tok} tokens，超过单次上限 ${CAPS.inputTokens}：把范围缩小`);
    process.exit(1);
  }
  console.log(s);
  console.error(`\n[slice] ${file} ≈${tok} tokens`);
}
