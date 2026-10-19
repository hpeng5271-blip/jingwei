// **工具脚本语法检查**（2026-10-20）：`tools/**/*.mjs` 必须能被 node 解析。
//
// 为什么单独一个文件：`gate.mjs` 已贴到 1000 行上限。
// 为什么要有这条：我把字面 `\n` 写进了 `selfcheck-positions.mjs`，文件语法就坏了，
// 但**没有任何门禁发现**——它是"一个跑不起来的工具"，直到真有人去跑才炸。
// 工具坏了和文档写错是同一类：没人执行就等于不存在。

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

export function checkToolScripts(root = '.') {
  const dir = path.join(root, 'tools');
  const problems = [];
  let checked = 0;

  const walk = (d) => {
    for (const name of fs.readdirSync(d)) {
      const p = path.join(d, name);
      const st = fs.statSync(p);
      if (st.isDirectory()) {
        walk(p);
      } else if (p.endsWith('.mjs')) {
        checked += 1;
        try {
          execFileSync(process.execPath, ['--check', p], { stdio: 'pipe' });
        } catch (e) {
          const first = String(e.stderr || e.message).split('\n')[0];
          problems.push(`${path.relative(root, p)} 解析失败：${first}`);
        }
      }
    }
  };

  if (fs.existsSync(dir)) walk(dir);
  return { checked, problems };
}
