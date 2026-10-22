#!/usr/bin/env node
// skill 命中验证（kill test）骨架 —— 2026-10-22，pro B4 的落法。
//
// 为什么不是"全自动"：**判断"这次有没有用到这个技能"需要一个裁判**，脚本不能代替模型。
// 所以这个工具只做三件事，把"真被用"变成可累积的数据：
//   1. 列出每个 skill 的**触发语**（人写的，存在 skills/KILLTEST.json）；
//   2. 记录一次试验结果（命中 / 未命中）；
//   3. 报告：命中率、连续未命中次数（连续 3 次未命中 → 建议退役，pro 定的判据）。
//
// 用法：
//   node tools/skill-killtest.mjs --list
//   node tools/skill-killtest.mjs --record file-focus --hit "问了'这个文件依赖谁'"
//   node tools/skill-killtest.mjs --report
//   node tools/skill-killtest.mjs --check      # 每个 active skill 都必须有触发语（可进门禁）

import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const root = process.cwd();
const skillsDir = path.join(root, 'skills');
const specPath = path.join(skillsDir, 'KILLTEST.json');
const ledgerPath = path.join(process.env.LOCALAPPDATA || os.tmpdir(), 'jingwei', 'skill-killtest.jsonl');

const args = process.argv.slice(2);
const get = (k) => {
  const i = args.indexOf(`--${k}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const has = (k) => args.includes(`--${k}`);

const spec = fs.existsSync(specPath)
  ? JSON.parse(fs.readFileSync(specPath, 'utf8'))
  : { skills: {} };

const activeSkills = fs
  .readdirSync(skillsDir, { withFileTypes: true })
  .filter((d) => d.isDirectory())
  .map((d) => d.name)
  .sort();

if (has('list')) {
  for (const name of activeSkills) {
    const s = spec.skills[name];
    console.log(`\n${name}`);
    console.log(`  触发语：${s?.trigger || '（缺，补到 skills/KILLTEST.json）'}`);
    console.log(`  省掉：${s?.saves || '—'}`);
  }
  process.exit(0);
}

if (has('record')) {
  const name = get('record');
  const text = args[args.length - 1];
  if (!name) { console.error('要 --record <技能名> --hit|--miss "一句话"'); process.exit(2); }
  const row = {
    at: new Date().toISOString(),
    skill: name,
    hit: has('hit') ? true : has('miss') ? false : null,
    note: text || '',
  };
  if (row.hit === null) { console.error('要么 --hit 要么 --miss'); process.exit(2); }
  fs.mkdirSync(path.dirname(ledgerPath), { recursive: true });
  fs.appendFileSync(ledgerPath, JSON.stringify(row) + '\n', 'utf8');
  console.log(`已记：${name} ${row.hit ? '命中' : '未命中'}  ${row.note}`);
  process.exit(0);
}

if (has('check')) {
  const missing = activeSkills.filter((n) => !spec.skills[n]?.trigger);
  if (missing.length) {
    console.error(`缺触发语：${missing.join(', ')}（补到 skills/KILLTEST.json）`);
    process.exit(1);
  }
  console.log(`kill test 规格齐全：${activeSkills.length} 个技能都有触发语`);
  process.exit(0);
}

// 默认：报告
const rows = fs.existsSync(ledgerPath)
  ? fs.readFileSync(ledgerPath, 'utf8').trim().split('\n').filter(Boolean).map((l) => {
    try { return JSON.parse(l); } catch { return null; }
  }).filter(Boolean)
  : [];
console.log(`试验记录：${rows.length} 条（${ledgerPath}）`);
console.log('技能'.padEnd(24) + '命中'.padStart(6) + '未中'.padStart(6) + '命中率'.padStart(9) + '  连续未中');
for (const name of activeSkills) {
  const mine = rows.filter((r) => r.skill === name);
  const hit = mine.filter((r) => r.hit).length;
  const miss = mine.filter((r) => r.hit === false).length;
  let streak = 0;
  for (const r of mine.slice().reverse()) {
    if (r.hit === false) streak += 1; else break;
  }
  const rate = mine.length ? `${Math.round((hit / mine.length) * 100)}%` : '—';
  const flag = streak >= 3 ? '  ← 连续 3 次未命中，建议退役' : '';
  console.log(name.padEnd(24) + String(hit).padStart(6) + String(miss).padStart(6) + rate.padStart(9) + String(streak).padStart(10) + flag);
}
console.log('\n判据（pro B4）：连续 3 次未命中即退役；命中后应少走 ≥3 步或省 ≥500 tokens。');
