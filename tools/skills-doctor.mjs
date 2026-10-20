// 技能登记表体检（2026-10-20）。
//
// 为什么要有：skill 是**注入成本**，也是最容易腐烂的一层——写完没人用、用了没人验、
// 验了没人复核。登记表 + 体检把"技能"当代码管：谁加的、从哪来、改了哪、怎么验、何时退役。
//
// 规则（可判）：
//   1. skills/ 下每个 SKILL.md 必须在 REGISTRY.json 里有一条，反之亦然（集合相等）。
//   2. status 只能是 active / deprecated；deprecated 必须在 reason 里写原因。
//   3. 每条必须有 verify（可执行命令）、owner、added、reviewed。
//   4. reviewed 超过 180 天 → 报"该复核"（不红，给警告）。
//   5. 结构仍由 tools/skills-check.mjs 守（这里不重复）。
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const SKILLS = path.join(ROOT, 'skills');
const REG = path.join(SKILLS, 'REGISTRY.json');

export function checkRegistry() {
  const problems = [];
  const warnings = [];
  if (!fs.existsSync(REG)) return { problems: ['缺 skills/REGISTRY.json（技能登记表）'], warnings };
  let reg;
  try {
    reg = JSON.parse(fs.readFileSync(REG, 'utf8').replace(/^\uFEFF/, ''));
  } catch (e) {
    return { problems: ['REGISTRY.json 不是合法 JSON：' + e.message], warnings };
  }
  const dirs = fs.readdirSync(SKILLS).filter((n) => fs.existsSync(path.join(SKILLS, n, 'SKILL.md')));
  const names = new Set((reg.skills || []).map((s) => s.name));
  for (const d of dirs) if (!names.has(d)) problems.push(`skills/${d} 没有登记（REGISTRY.json 里找不到）`);
  for (const s of reg.skills || []) {
    if (!dirs.includes(s.name)) problems.push(`REGISTRY 里的 ${s.name} 在 skills/ 下不存在`);
    if (!['active', 'deprecated'].includes(s.status)) problems.push(`${s.name}: status 必须是 active/deprecated`);
    if (s.status === 'deprecated' && !s.reason) problems.push(`${s.name}: deprecated 必须写 reason`);
    for (const k of ['verify', 'owner', 'added', 'reviewed', 'source']) {
      if (!s[k]) problems.push(`${s.name}: 缺字段 ${k}`);
    }
    if (s.reviewed) {
      const days = Math.floor((Date.now() - new Date(s.reviewed + 'T00:00:00Z').getTime()) / 86400000);
      if (days > 180) warnings.push(`${s.name}: ${days} 天没复核（规矩是 180 天）`);
    }
  }
  return { problems, warnings };
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename)) {
  const r = checkRegistry();
  for (const w of r.warnings) console.log('  · ' + w);
  for (const p of r.problems) console.log('  × ' + p);
  if (!r.problems.length) console.log(`  registry：${(JSON.parse(fs.readFileSync(REG, 'utf8')).skills || []).length} 条都已登记且字段齐全`);
  process.exit(r.problems.length ? 2 : 0);
}
