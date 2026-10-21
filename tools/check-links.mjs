// **链接检查**（2026-10-22）：扫 docs/ 与根目录的 .md，确认所有**相对**链接都还指得到东西。
//
// 为什么要有它：整理文档时最怕"搬完才发现断了 30 处链接"。合并那两次都是先 grep 引用再改，
// 但 grep 只能找到"文件名"，找不到"`](path)` 写错了"这类断链——这个脚本补的就是这一格。
//
// 用法：node tools/check-links.mjs        （只看 docs/ 与根目录 .md；不进 target/node_modules/dist/.git）
// 退出码：0 全通；1 有断链（打印 文件 -> 目标）。
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(process.argv[2] || '.');
const SKIP_DIRS = new Set(['node_modules', 'target', '.git', 'dist', '.idea', '.vscode']);
const bad = [];
let checked = 0;
let files = 0;

function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (!SKIP_DIRS.has(e.name)) walk(p);
      continue;
    }
    if (!p.endsWith('.md')) continue;
    files++;
    const text = fs.readFileSync(p, 'utf8');
    // 只查 .md 目标的相对链接：`](foo/bar.md)` 或 `](foo/bar.md#锚)`
    for (const m of text.matchAll(/\]\(([^)\s]+\.md)(#[^)]*)?\)/g)) {
      const target = m[1];
      if (/^[a-z]+:/i.test(target) || target.startsWith('/')) continue; // 外链/绝对路径不管
      checked++;
      const abs = path.resolve(path.dirname(p), target);
      if (!fs.existsSync(abs)) bad.push(`${path.relative(ROOT, p)} -> ${target}`);
    }
  }
}

for (const d of ['docs', '.']) {
  const abs = path.join(ROOT, d);
  if (fs.existsSync(abs)) walk(abs);
}

if (bad.length === 0) {
  console.log(`链接检查：${files} 份 .md、${checked} 条相对链接，全部指得到。`);
  process.exit(0);
}
console.error(`链接检查：${bad.length} 条断链（扫了 ${files} 份 .md）：`);
for (const b of bad) console.error('  ' + b);
process.exit(1);
