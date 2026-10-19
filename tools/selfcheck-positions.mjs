// **收口自核**（2026-10-19）：用自己的图谱核自己的代码。
//
// 判据（两条，缺一不算）：
//   1. `checked > 0`（**核了东西**，不是空转）；
//   2. `bad == 0`（没有"名字不在那一行"的真错）。
// `unknown` / `imprecise` 是三态与容差，单列不计失败。
//
// 用法：node tools/selfcheck-positions.mjs [--json]

import { execFileSync } from 'node:child_process';
import path from 'node:path';
import fs from 'node:fs';

const exe =\n  process.env.JINGWEI_EXE ||\n  path.join('target', 'release', process.platform === 'win32' ? 'jingwei.exe' : 'jingwei');
const root = process.cwd();

if (!fs.existsSync(exe)) {
  console.error(`找不到 ${exe}：先 cargo build --release`);
  process.exit(2);
}

function run(args) {
  return execFileSync(exe, args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

run(['index']);
const raw = run(['positions', '--json']);
const rep = JSON.parse(raw);
const checked = (rep.ok || 0) + (rep.bad || 0) + (rep.unknown || 0) + (rep.imprecise || 0);

const out = {
  checked,
  ok: rep.ok,
  bad: rep.bad,
  unknown: rep.unknown,
  imprecise: rep.imprecise,
  pass: checked > 0 && (rep.bad || 0) === 0,
};

if (process.argv.includes('--json')) console.log(JSON.stringify(out, null, 1));
else {
  console.log(`  自核：核了 ${out.checked} 个符号`);
  console.log(`    ✓ ${out.ok}   × ${out.bad}   ? ${out.unknown}   ~ ${out.imprecise}`);
  console.log(`  ${out.pass ? '通过' : '未通过'}：${out.pass ? '核到了东西，且没有真错' : '核不到东西 或 有真错'}`);
}
process.exit(out.pass ? 0 : 1);
