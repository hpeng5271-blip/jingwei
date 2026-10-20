// 校验 skills/ 下的 SKILL.md 结构（2026-10-20）。
//
// 为什么要有它：技能是"给模型看的程序"。没有结构校验，写着写着就变成散文，
// 触发词没了、步骤没了、验收没了——那样不但不省 token，还会误导执行。
//
// 规则（都可判）：
//   1. frontmatter 必须有 name / description；name 与目录名一致；description 20–200 字。
//   2. frontmatter 之后必须有正文，且至少包含三个二级标题（做什么/怎么做/验收）。
//   3. 单文件 ≤60 行（技能是注入成本，越短越好用）。
//   4. 不许出现"TODO"/"待补"。
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const DIR = path.join(ROOT, 'skills');

export function checkSkills() {
  const problems = [];
  const rows = [];
  if (!fs.existsSync(DIR)) return { rows, problems: ['缺 skills/ 目录'] };
  for (const name of fs.readdirSync(DIR)) {
    const file = path.join(DIR, name, 'SKILL.md');
    if (!fs.existsSync(file)) continue;
    const text = fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, '');
    const lines = text.split(/\r?\n/);
    const fm = text.match(/^---\n([\s\S]*?)\n---\n/);
    if (!fm) {
      problems.push(`${name}: 缺 frontmatter`);
      continue;
    }
    const field = (k) => (fm[1].match(new RegExp(`^${k}:\\s*(.+)$`, 'm')) || [, ''])[1].trim();
    const nm = field('name');
    const desc = field('description');
    if (nm !== name) problems.push(`${name}: name 与目录名不一致（${nm}）`);
    if (desc.length < 20 || desc.length > 200) problems.push(`${name}: description 长度 ${desc.length}（要求 20–200）`);
    const body = text.slice(fm[0].length);
    const h2 = (body.match(/^## /gm) || []).length;
    if (h2 < 3) problems.push(`${name}: 正文只有 ${h2} 个二级标题（至少 3 个）`);
    if (lines.length > 60) problems.push(`${name}: ${lines.length} 行（上限 60）`);
    if (/TODO|待补/.test(body)) problems.push(`${name}: 含 TODO/待补`);
    rows.push({ name, lines: lines.length, desc: desc.length });
  }
  if (rows.length === 0) problems.push('skills/ 下没有 SKILL.md');
  return { rows, problems };
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename)) {
  const r = checkSkills();
  for (const p of r.problems) console.log('  × ' + p);
  if (!r.problems.length) console.log(`  skills：${r.rows.length} 条都合规（${r.rows.map((x) => x.name + ' ' + x.lines + '行').join('，')}）`);
  process.exit(r.problems.length ? 1 : 0);
}
