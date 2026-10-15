// 门禁里那些"没法靠编译器守"的规矩，用这个脚本守。
//
// 为什么不写成文档里的一句话：规矩写在文档里没人执行就等于没有（§13 迭代规矩）。
// 这里只放三件能自动判定的，其余留给 clippy 和测试。
//
// 用法：node tools/gate.mjs

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const problems = [];

/** 版本号的真源是 tauri.conf.json（打包脚本读它），另外两处必须跟它一致。 */
/**
 * 动机清单：用户当初要什么、落在设计里的哪一处。
 *
 * R3 的发现是"设计跟着能力长、不跟着动机长"——而补它的办法不能是"下次记得问"，
 * 那就成了第 N 条只写在文档里的规矩（这个项目已经犯过三次，见 §13 第 4 条）。
 * 所以这里把它变成**能失败的检查**：每个动机要么在设计里指到一段真实存在的话，
 * 要么带一个触发条件挂着。
 *
 * `landing` 可以写成 `文件名#一段文字`（默认查《设计.md》）。
 */
function checkMotives() {
  let doc;
  try {
    doc = JSON.parse(readFileSync(join(ROOT, 'tools/motives.json'), 'utf8'));
  } catch (e) {
    problems.push(`读不懂 tools/motives.json：${e.message}`);
    return;
  }
  const cache = new Map();
  const textOf = (name) => {
    if (!cache.has(name)) {
      try {
        cache.set(name, readFileSync(join(ROOT, name), 'utf8'));
      } catch {
        cache.set(name, null);
      }
    }
    return cache.get(name);
  };

  let landed = 0;
  let pending = 0;
  for (const m of doc.motives || []) {
    if (!m.id || !m.want) {
      problems.push('动机清单里有条目缺 id 或 want');
      continue;
    }
    if (m.state === 'landed') {
      if (!m.landing) {
        problems.push(`动机「${m.id}」标成 landed 却没写落点`);
        continue;
      }
      const [name, phrase] = m.landing.includes('#')
        ? m.landing.split('#')
        : ['设计.md', m.landing];
      const text = textOf(name);
      if (text === null) {
        problems.push(`动机「${m.id}」的落点指向一个读不到的文件：${name}`);
      } else if (!text.includes(phrase)) {
        problems.push(
          `动机「${m.id}」的落点在 ${name} 里找不到：「${phrase}」——要么补上，要么改成 pending 并写触发条件`,
        );
      } else {
        landed += 1;
      }
    } else if (m.state === 'pending') {
      if (!m.trigger) {
        problems.push(`动机「${m.id}」挂账了却没写触发条件（§13 第 3 条：不许留无条件 TODO）`);
      } else {
        pending += 1;
      }
    } else {
      problems.push(`动机「${m.id}」的 state 只能是 landed / pending，现在是 ${m.state}`);
    }
  }
  console.log(`  · 动机清单：${landed} 条已落地、${pending} 条挂账（都带触发条件）`);
}

function checkVersion() {
  const tauri = JSON.parse(readFileSync(join(ROOT, 'src-tauri/tauri.conf.json'), 'utf8'));
  const pkg = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8'));
  const cargo = readFileSync(join(ROOT, 'Cargo.toml'), 'utf8');
  const m = cargo.match(/\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/);
  if (!m) {
    problems.push('Cargo.toml 里找不到 [workspace.package] version');
    return;
  }
  const pairs = [
    ['Cargo.toml', m[1]],
    ['package.json', pkg.version],
  ];
  for (const [where, v] of pairs) {
    if (v !== tauri.version) {
      problems.push(`版本号对不上：src-tauri/tauri.conf.json = ${tauri.version}，${where} = ${v}`);
    }
  }
}

/**
 * 单文件行数上限。前端那几个文件是有意的大文件（没有打包器是架构选择，见 设计 §10），
 * 所以单独放宽——但放宽不等于没有上限，它得有个能拦住"无限膨胀"的数。
 */
const LIMITS = { '.rs': 1000, '.mjs': 1000, '.js': 4500, '.css': 4500 };

/**
 * 历史债登记表。**只许降，不许升**：
 * 超过上限但在表里 = 放行（并提示现在多少行）；一旦比登记值更大 = 失败，
 * 因为那说明又在往里堆东西了，而不是在拆。
 *
 * 为什么不干脆把上限调高：上限就是给人看的"该拆了"。调高一次，下次还会调。
 */
const KNOWN_LONG = new Map([
  // `crates/cli/src/main.rs`（原来 2128）**债已还清，登记删掉**：2222 行时按领域
  // 拆成 main 367 + cmd_doctor 530 + cmd_project 617 + cmd_run 363 + cmd_clean 249
  // + cmd_machine 155（六个都进 1000 以内）。拆的边界按"回答同一类问题"走，
  // 不按"大小平均切"。**登记不是豁免，是记着要还**——还完就把这一行删掉。
  // 重建后路径变了（六层重排：project 层改叫 understand，machine/clean 归 sys）。
  // 这份登记表跟着走——**它按路径记，路径变了必须一起改**，否则门禁会以为
  // "老债还清了、又冒出来四个新的"。
  //
  // 1803：P10 补 `should_skip` 闸门（+7）——必要修改，不是膨胀。
  // 以后每次调大这个数都要在这一行留一句为什么，否则这条规矩就成橡皮筋了。
  ['crates/engine/src/understand/project.rs', 1803],
  ['crates/engine/src/sys/clean.rs', 1523],
  // machine.rs 已拆（mod 889 / disk 219 / ram 144 / settings 121）——**债还清了，登记表就删掉**。
  // 这条规矩的意义就在这儿：登记不是豁免，是"记着要还"。
  // 1120：Q6 拆循环，Finding/Severity 上移到 model（-27 行）。
  ['crates/engine/src/understand/rules.rs', 1120],
  // `src-tauri/src/main.rs`（原来 1005）**债已还清，登记删掉**：
  // 把"一次体检的全部材料"整段搬成 `src-tauri/src/payload.rs`（88 行）。
  // 那一段本来就是一个完整的话题——它只负责把引擎那边十几个数据源收成一份
  // JSON，和"注册命令""管窗口"是两回事。
]);

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === 'target' || name === 'node_modules' || name === '.git' || name === 'dist') continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, out);
    else out.push([p, st.size]);
  }
  return out;
}

function checkFileLength() {
  const roots = ['crates', 'src-tauri', 'ui', 'tools'].map((d) => join(ROOT, d));
  for (const r of roots) {
    let files;
    try {
      files = walk(r);
    } catch {
      continue;
    }
    for (const [p] of files) {
      const ext = p.slice(p.lastIndexOf('.'));
      const limit = LIMITS[ext];
      if (!limit) continue;
      const lines = readFileSync(p, 'utf8').split('\n').length;
      if (lines <= limit) continue;
      const rel = relative(ROOT, p).replace(/\\/g, '/');
      const known = KNOWN_LONG.get(rel);
      if (known === undefined) {
        problems.push(`单文件超长：${rel} = ${lines} 行（上限 ${limit}）——拆开，或写进 gate.mjs 的登记表并说明理由`);
      } else if (lines > known) {
        problems.push(`登记在案的长文件又变长了：${rel} = ${lines} 行（登记值 ${known}）——只许降不许升`);
      } else if (lines < known) {
        console.log(`  · ${rel} 已降到 ${lines} 行（登记值 ${known}），可以顺手把登记值改小`);
      }
    }
  }
}

checkVersion();
checkFileLength();
checkMotives();

if (problems.length) {
  console.log('门禁未通过：');
  for (const p of problems) console.log('  × ' + p);
  process.exit(1);
}
console.log('门禁通过：版本号三处一致，源文件行数在限内');
