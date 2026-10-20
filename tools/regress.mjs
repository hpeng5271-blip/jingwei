// **研究对象回归**（2026-10-20）：把两个外部项目跑一遍，和 `回归基线.json` 对数字。
//
// 为什么要有它：每天改解析器/索引，`flask` / `tauri` 的数字（符号、调用、位置回读）
// 是**环路外面的裁判**——单测全绿不代表真项目没退化。以前靠手跑、肉眼对文档，会漂。
//
// **三态**：项目目录不存在 → 记"查不了"，退出码 2（不是失败，也不是通过）。
// 用法：node tools/regress.mjs
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const exe = process.env.JINGWEI_EXE || path.join(root, 'target', 'release', process.platform === 'win32' ? 'jingwei.exe' : 'jingwei');
const raw = fs.readFileSync(path.join(root, 'docs/process/回归基线.json'), 'utf8').replace(/^\uFEFF/, '');
const base = JSON.parse(raw);

let failed = 0;
let checked = 0;
for (const o of base.objects) {
  if (!fs.existsSync(o.path)) {
    console.log(`  ?  ${o.name}：查不了（目录不在：${o.path}）`);
    continue;
  }
  const run = (args) => execFileSync(exe, args, { cwd: o.path, encoding: 'utf8' });
  run(['index']);
  const pos = JSON.parse(run(['positions', '--json']));
  const stats = JSON.parse(run(['graph', 'stats', '--json']));
  const got = {
    symbols: Number(stats.nodes.symbol || 0),
    calls: Number(stats.edges.calls || 0),
    files: Number(stats.nodes.file || 0),
    positions_bad: Number(pos.bad || 0),
  };
  const diffs = Object.entries(o.expect)
    .filter(([k, v]) => got[k] !== v)
    .map(([k, v]) => `${k}: 期望 ${v} 实际 ${got[k]}`);
  checked += 1;
  if (diffs.length) {
    failed += 1;
    console.log(`  ×  ${o.name}：${diffs.join('；')}`);
  } else {
    console.log(`  ✓  ${o.name}：${got.files} 文件 / ${got.symbols} 符号 / ${got.calls} 调用 / 位置对不上 ${got.positions_bad}`);
  }
}

if (checked === 0) {
  console.log('查不了：一个研究对象目录都不在（这不是"通过"）');
  process.exit(2);
}
if (failed > 0) {
  console.log(`回归失败：${failed}/${checked}`);
  process.exit(1);
}
console.log(`回归通过：${checked} 个研究对象，数字与基线一致`);
