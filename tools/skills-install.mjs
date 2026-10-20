// 把 skills/ 安装到 DSH 的用户技能目录（2026-10-20）。
//
// 为什么需要安装这一步：DSH 只从 `%USERPROFILE%\.dsh\skills\` 加载用户技能，
// 而仓库里的 `skills/` 是**源**（跟着代码走、能 review）。安装是**复制**，幂等。
//
// 用法：node tools/skills-install.mjs [--dry]
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const SRC = path.join(ROOT, 'skills');
const DST = path.join(os.homedir(), '.dsh', 'skills');
const dry = process.argv.includes('--dry');

const names = fs.readdirSync(SRC).filter((n) => fs.existsSync(path.join(SRC, n, 'SKILL.md')));
if (!names.length) {
  console.error('  skills/ 下没有可安装的技能');
  process.exit(1);
}
if (!dry) fs.mkdirSync(DST, { recursive: true });
let n = 0;
for (const name of names) {
  const from = path.join(SRC, name, 'SKILL.md');
  const toDir = path.join(DST, name);
  if (!dry) {
    fs.mkdirSync(toDir, { recursive: true });
    fs.copyFileSync(from, path.join(toDir, 'SKILL.md'));
  }
  n += 1;
  console.log(`  ${dry ? '将安装' : '已安装'} ${name}`);
}
console.log(`  ${dry ? '（dry-run）' : '装到'} ${DST}：${n} 条。重开会话后生效。`);
