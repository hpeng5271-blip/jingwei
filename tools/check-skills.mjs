// skills 树门禁：**二级技能（L2）必须有守卫，而且守卫必须是真实存在的名字**。
//
// 为什么要有这一道（2026-10-18，用户原话："总是需要管理好这些副产物同时也不能写了就不管了"）：
//   技能树最容易退化成"写完就忘"。而"写完就忘"在这个仓库有过极贵的先例：
//   债表写着十几个"抓手"，其中 `Conclusion` 零调用、`ghDeps`/`ghLevel` 那些元素根本不存在，
//   **十五轮没人发现**（见 `事故账本.md`）。
//   所以这里把"写了就得有人管"变成机器判据：
//     · README 里列的技能  ↔  目录里真有的文件（**集合相等**，两边都不许多）
//     · 每个技能四段齐全（判据 / 为什么 / 不适用 / 守卫）
//     · `## 守卫` 里点名的每个名字，必须能在 `tools/` 或 `crates/` 里**真的找到**
//     · 每个技能 ≤ 80 行（技能是判断力，不是文档仓库）
//
// 诚实边界：这一层查得出"守卫名字不存在"，查不出"守卫真的守住了这条判据"。
// 后者只能靠那条守卫自己的负向验证——所以每条守卫都该是"能红的东西"。

import { readFileSync, existsSync, readdirSync, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..');

const SECTIONS = ['## 判据', '## 为什么', '## 不适用', '## 守卫'];

/// 递归收集源码文本（只收工具与 Rust 源码，够 grep 守卫名了）。
function corpusText(root) {
  const out = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      if (['target', 'node_modules', '.git', 'dist', 'skills'].includes(name)) continue;
      const p = join(dir, name);
      const st = statSync(p);
      if (st.isDirectory()) walk(p);
      else if (/\.(mjs|js|rs|json)$/.test(name)) out.push(readFileSync(p, 'utf8'));
    }
  };
  walk(join(root, 'tools'));
  walk(join(root, 'crates'));
  walk(join(root, 'src-tauri'));
  return out.join('\n');
}

/// README 表格里列的技能名（形如 `| \`三态与不压扁.md\` | ... |`）。
export function listedSkills(md) {
  const names = [];
  for (const line of md.split(/\r?\n/)) {
    const m = line.match(/^\|\s*`([^`]+\.md)`\s*\|/);
    if (m) names.push(m[1]);
  }
  return names;
}

export function checkSkillsTree(root = ROOT) {
  const problems = [];
  const dir = join(root, 'skills');
  if (!existsSync(dir)) return { problems: ['skills/ 不见了——二级技能树没有真源'], checked: 0 };

  const files = readdirSync(dir).filter((f) => f.endsWith('.md') && f !== 'README.md');
  const readmePath = join(dir, 'README.md');
  if (!existsSync(readmePath)) {
    return { problems: ['skills/README.md 不见了——没有索引的技能树等于没有'], checked: 0 };
  }
  const listed = listedSkills(readFileSync(readmePath, 'utf8'));

  // ① 集合相等：表里列的 ↔ 目录里真有的
  for (const f of files) {
    if (!listed.includes(f)) {
      problems.push(`skills/${f} 存在，但 skills/README.md 的表里没列——**写了没人知道**`);
    }
  }
  for (const f of listed) {
    if (!files.includes(f)) {
      problems.push(`skills/README.md 列了 ${f}，但那个文件不存在——**索引撒谎**`);
    }
  }

  // ② 每份技能：四段齐全 + 有守卫名字 + 不超过 80 行
  const corpus = corpusText(root);
  let checked = 0;
  for (const f of files) {
    checked++;
    const text = readFileSync(join(dir, f), 'utf8');
    const lines = text.split(/\r?\n/).length;
    if (lines > 80) problems.push(`skills/${f} 有 ${lines} 行（上限 80）——技能是判断力，不是文档仓库`);
    for (const s of SECTIONS) {
      if (!text.includes(s)) problems.push(`skills/${f} 缺一段：${s}`);
    }
    const guardAt = text.indexOf('## 守卫');
    if (guardAt >= 0) {
      const block = text.slice(guardAt + '## 守卫'.length);
      // 守卫块里点名的"名字"：反引号里的标识符（空格的聊天话不算）
      const names = [...block.matchAll(/`([^`]+)`/g)]
        .map((m) => m[1].trim())
        .filter((n) => /^[\w\u4e00-\u9fa5_\-.:/]+$/.test(n) && n.length >= 4);
      if (!names.length) {
        problems.push(`skills/${f} 的「守卫」段里一个名字都没有——**没有守卫的技能是愿望**`);
      }
      for (const n of names) {
        if (!corpus.includes(n)) {
          problems.push(`skills/${f} 的守卫「${n}」在 tools/ 与 crates/ 里都找不到——**写着但没落**`);
        }
      }
    }
  }

  return { problems, checked };
}

function main() {
  const r = checkSkillsTree();
  if (r.problems.length) {
    console.log('skills 树对不上：');
    for (const p of r.problems) console.log('   × ' + p);
    process.exit(1);
  }
  console.log(`skills 树一致：${r.checked} 条技能，四段齐全，每个守卫都找得到。`);
  process.exit(0);
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) main();
