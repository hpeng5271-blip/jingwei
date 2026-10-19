// **项目摘要**（2026-10-19）：给 pro 的上下文只给这份，**不给仓库全文**。
//
// 上限 2000 tokens（用 caps.estimateTokens 计）。超了按优先级裁剪：
//   1. 当前状态（AGENTS §3 里最近几条收口）
//   2. 债表前 6 行
//   3. git 最近一次提交 + 工作区是否干净
//   4. 最近 3 次 pro 咨询（避免重复问同一件事）
//
// 用法：node tools/llm/digest.mjs [--json]

import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { estimateTokens } from './caps.mjs';
import { summary } from './ledger.mjs';

const ROOT = process.cwd();
const MAX_TOKENS = 2000;

function read(rel) {
  try {
    return fs.readFileSync(path.join(ROOT, rel), 'utf8');
  } catch {
    return '';
  }
}

function firstRows(md, n) {
  return md
    .split(/\r?\n/)
    .filter((l) => l.trim().startsWith('|'))
    .slice(0, n)
    .join('\n');
}

function git(args) {
  try {
    return execSync(`git ${args}`, { cwd: ROOT, encoding: 'utf8' }).trim();
  } catch {
    return '';
  }
}

const agents = read('AGENTS.md');
const debt = read('docs/reference/债表.md');

// 只取「当前状态」那几行：表格里带 2026-10-19 的条目
const stateRows = agents
  .split(/\r?\n/)
  .filter((l) => l.startsWith('|') && l.includes('2026-10-19'))
  .slice(0, 6)
  .join('\n');

const parts = [
  '# 项目摘要（自动生成，≤2000 tokens）',
  '',
  '## 硬约束',
  '三态：是/不是/查不了；unknown 不许当"没有"。',
  '不生成修复命令；无自查不报错；关系必有来源。',
  '门禁红不提交；失败 2 次停下报"卡在哪"。',
  '',
  '## 当前状态（AGENTS §3 摘要）',
  stateRows || '（空）',
  '',
  '## 债（前 6 行）',
  firstRows(debt, 6),
  '',
  '## 仓库',
  `HEAD: ${git('log --oneline -1')}`,
  `工作区: ${git('status --porcelain') ? '有改动' : '干净'}`,
  '',
  '## 最近的 pro 咨询',
  JSON.stringify(summary(3).tail),
];

let text = parts.join('\n');
let tok = estimateTokens(text);

// 超了就砍"最近咨询"和债表，保留硬约束与状态
if (tok > MAX_TOKENS) {
  text = [
    '# 项目摘要（裁剪版）',
    '## 硬约束',
    '三态；不生成修复命令；无自查不报错；门禁红不提交；失败 2 次停。',
    '## 当前状态',
    stateRows || '（空）',
    `## 仓库 ${git('log --oneline -1')}`,
  ].join('\n');
  tok = estimateTokens(text);
}

if (process.argv.includes('--json')) {
  console.log(JSON.stringify({ tokens: tok, text }, null, 1));
} else {
  console.log(text);
  console.error(`\n[digest] ≈${tok} tokens（上限 ${MAX_TOKENS}）`);
}
