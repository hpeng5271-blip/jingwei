// 文档数字门禁：**只比较，不采集**。
//
// 为什么要有这一道：仓库里一直有"文档里的每个数字都要能在仓库里跑出来"这条规矩，
// 但它是靠人记得去同步的——上一轮刚把 242/35 改成 247/37，更早一轮把"跑在 CI 上"
// 那句没兑现的话改掉。靠记得就等于迟早漂。这里把它变成**能失败的东西**（§13 第 4 条）：
//
//   真源  tools/facts.json            ← 由 tools/collect_facts.mjs 现场采集
//   标记  <!-- fact:名字 -->数字<!-- /fact -->   ← 写在文档里，渲染时看不见
//   比较  本脚本（也被 gate.mjs 调用，判断只有一份）
//   写回  node tools/check_facts.mjs --write-docs   ← 把真值写进标记里
//
// **为什么要有"写回"这个模式**：上一轮修文档数字时，人手抄连着错了三次
// （tools 的行数、graph.json 的性质、测试数）。手抄一遍就是把"数字"这件事
// 从"真源说多少"变成"我记得多少"。写回让文档数字永远是真源刻上去的。
// 门禁调的是下面的 `checkFacts()`（只读），所以这个模式不会影响门禁的语义。
//
// 五种情形一律报错（一种都不许省，尤其第 2 条）：
//   1. 文档写的数字 ≠ 真值        → 打印 文件:行、文档写的 X、真值 Y
//   2. 一份文档里一个标记都没有    → 报错。**不许当作"没什么可查"而通过**，
//                                   那说明文档被改坏了（或标记格式被改了）
//   3. 标记没闭合（缺 <!-- /fact -->）
//   4. 标记的名字不在 facts.json 里 → 名字写错，或者真源忘了登记
//   5. facts.json 里的数字没人用    → 真源里躺着没人守的数字，同样是漂
//
// 跑法：node tools/check_facts.mjs

import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(import.meta.url);
const ROOT = fileURLToPath(new URL('..', import.meta.url));

const MARK = /<!--\s*fact:([A-Za-z0-9_]+)\s*-->([\s\S]*?)<!--\s*\/fact\s*-->/g;
const OPEN = /<!--\s*fact:/g;
const SKIP_DIRS = new Set(['node_modules', 'target', 'dist', '.git', '.jingwei']);

/// **这一行是不是在报测试/门禁规模？**
///
/// 单独抽成导出的纯函数，是为了**它能被自测**（`--selftest`）。
/// 这条判据的已知弱点写在下面词表那段注释里：它是**枚举式**的，
/// 换一种说法就漏——所以必须有一个地方能**演示它漏不漏**。
export const SCALE_WORDS = [
  '条通过', '外壳测试', '单元与集成测试', '测试规模',
  '测试总数', '道门禁', '节点 /', '个节点 /',
];
export function looksLikeScale(line) {
  return SCALE_WORDS.some((w) => line.includes(w));
}

/// 文档 = 仓库里所有 .md（根目录那六份就是全部，但按目录扫，
/// 以后多一份文档也不会漏掉）。
function docsAt(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name)) continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) docsAt(p, out);
    else if (name.toLowerCase().endsWith('.md')) out.push(p);
  }
  return out;
}

/// **这道门禁的自测**：证明它的"零标记"判据**能失败**，而且**不误报**。
///
/// 为什么必须自测：`check-docs.mjs` 那次教训是"自测死在 `mkdtemp` EPERM，
/// 于是七道门禁它没能整套跑起来，而没人知道"。
/// 所以自测要**只依赖内存里的字符串**，不碰磁盘——它不会再坏在环境上。
///
/// 更要紧的是：这条判据是**枚举式**的（见词表那段注释），
/// 它的弱点就是"换种说法就漏"。自测同时钉**两件相反的事**：
///   ① 已知的报规模写法 → **必须命中**（不然它会漏掉文档）
///   ② 概念性说法 → **不许命中**（不然它会把人逼去关掉这道门禁）
export function selftest() {
  const problems = [];
  const mustHit = [
    '| 测试总数 | **330**（engine 单元 268） |', // 介绍.md 那种表格写法
    '测试规模：330 条',
    '共 8 道门禁',
    '1569 节点 / 2569 边',
    '外壳测试 47 条',
  ];
  const mustNotHit = [
    '> 同一个东西只有一个节点"这条特性会把这件事拦住', // 数据层设计.md 的概念说法
    '| 修正 1 | **写入必须是 UPSERT** |',
    '详见 tools/schema.sql',
    '加一门语言只许改 lang.rs 一处',
  ];
  for (const l of mustHit) {
    if (!looksLikeScale(l)) problems.push(`该命中的没命中（会漏掉文档）：${l}`);
  }
  for (const l of mustNotHit) {
    if (looksLikeScale(l)) problems.push(`不该命中的命中了（会变成噪声）：${l}`);
  }
  return problems;
}

/// 纯函数：返回 { problems, marked, keys }，不打印、不退出——
/// 这样 gate.mjs 能直接拿去用，判断不会出现第二份。
export function checkFacts(root = ROOT) {
  const problems = [];
  let facts;
  try {
    const raw = JSON.parse(readFileSync(join(root, 'tools', 'facts.json'), 'utf8'));
    facts = raw.facts;
  } catch (e) {
    problems.push(`读不了 tools/facts.json：${e.message}`);
    return { problems, marked: 0, keys: 0 };
  }
  if (!facts || typeof facts !== 'object') {
    problems.push('tools/facts.json 里没有 facts 对象——真源格式不对');
    return { problems, marked: 0, keys: 0 };
  }

  const used = new Set();
  let marked = 0;
  for (const file of docsAt(root)) {
    const text = readFileSync(file, 'utf8');
    const rel = relative(root, file).replace(/\\/g, '/');
    const opens = (text.match(OPEN) || []).length;
    let closed = 0;
    for (const m of text.matchAll(MARK)) {
      closed += 1;
      marked += 1;
      const name = m[1];
      const wrote = m[2].trim();
      const line = text.slice(0, m.index).split('\n').length;
      used.add(name);
      if (!Object.prototype.hasOwnProperty.call(facts, name)) {
        problems.push(
          `${rel}:${line} 的标记 fact:${name} 在 tools/facts.json 里没有——名字写错了，或者真源忘了登记`,
        );
        continue;
      }
      const truth = String(facts[name]);
      if (wrote !== truth) {
        problems.push(`${rel}:${line} 文档写的 ${wrote}，真值 ${truth}（fact:${name}）`);
      }
    }
    if (closed !== opens) {
      problems.push(
        `${rel} 里有 ${opens} 个 <!-- fact: --> 开头，只有 ${closed} 个闭合——缺 <!-- /fact -->，或者标记套了层`,
      );
    }
  }

  // ── 逐份文档：**在讲"多大"的文档，必须至少有一个标记** ────────────────
  //
  // 这条是 2026-10-16 外部对照复核抓出来的缺陷：原来的"一个标记都没有"是全**局**判据
  // （`marked === 0`），于是**总稿.md 一个标记都没有、却逃过了所有数字校验**——
  // 它里面写着「247 条通过」「37 条」，而真源是 277 / 47，门禁还打印"文档数字一致"。
  // 那是这个仓库最贵的那类事故：**门禁说自己验过了，其实漏了整整一份文档。**
  //
  // 判据只盯**明确在报测试/门禁规模**的行，不搞"文档里出现数字就要标记"那种宽判据
  // （设计文档里到处是编号与年限，宽判据会把人逼去关掉这道门禁）。
  //
  // 用朴素的字符串包含，不用正则——第一版写了个带全角括号的正则，
  // 当场因为未转义的 `)` 编译不过。**判据越简单越不会自己坏。**
  //
  // ⚠️ **2026-10-16 补词，但第一次补宽了**：新写的 `介绍.md` 用**表格**报规模
  // （`| 测试总数 | **330** …`、`| 门禁 | **8 道** |`、`1569 节点 / 2569 边`），
  // 原来那四个词**一个都不匹配** → 整份文档从这道门禁底下走了过去。
  //
  // 第一版补词加了 `'门禁'` 和 `'条边'`，**当场炸出 5 份文档、近百行**——
  // 因为 `门禁` 匹配到了 `docs-claims.json` 这类**文件名**，`条边` 匹配到别处。
  // 那正是这条注释下面警告的"宽判据会把人逼去关掉这道门禁"。
  //
  // 所以补的是**只在报规模时才会出现的完整说法**。
  // 第二次还收窄了一处：`'个节点'` 在 `数据层设计.md` 里命中了 5 行
  // 「同一个东西只有一个节点」——**那是概念，不是规模**。
  // 判据要盯的是"报了一个数"，不是"出现了这个词"。
  //
  // 词表与判断函数在文件顶部（`SCALE_WORDS` / `looksLikeScale`），
  // 抽出去是为了 `--selftest` 能直接验它。**这里不再重复一份**——
  // 两份词表就是"两份真源"，那正是这道门禁自己在抓的病。
  for (const file of docsAt(root)) {
    const rel = relative(root, file).replace(/\\/g, '/');
    const text = readFileSync(file, 'utf8');
    if ((text.match(OPEN) || []).length > 0) continue; // 这份有标记，gate 已经逐条比过了
    const bad = text
      .split('\n')
      .map((l, i) => [i + 1, l])
      .filter(([, l]) => looksLikeScale(l));
    if (bad.length) {
      problems.push(
        `${rel} 里有 ${bad.length} 行在报测试/门禁规模，却**一个 fact 标记都没有**——` +
          `那些数字没人守（第一处 ${rel}:${bad[0][0]}：${bad[0][1].trim().slice(0, 60)}）。` +
          '给它们加 <!-- fact:名字 -->数字<!-- /fact -->，然后用 check_facts --write-docs 写进去',
      );
    }
  }

  if (marked === 0) {
    problems.push(
      '一份文档里都没找到 <!-- fact:名字 -->数字<!-- /fact --> 这种标记：这道门禁查不出任何东西，' +
        '必须报错而不是当通过（标记被改坏了？）',
    );
  }
  for (const key of Object.keys(facts)) {
    if (!used.has(key)) {
      problems.push(`tools/facts.json 里的 ${key} 在文档里没有任何标记——真源里躺着一个没人守的数字`);
    }
  }
  return { problems, marked, keys: Object.keys(facts).length };
}

/// 把真源里的数字**写进**文档的标记里（`--write-docs`）。
///
/// 安全闸（缺一个都不许写）：
///   1. facts.json 读不了 → 不写（宁可数字旧着，也不能拿半个真源去刷）
///   2. 标记名不在真源里 → 不写**任何**文件（那说明有人在改标记，得先看清楚）
///   3. 有没闭合的标记 → 不写（改了会把文档结构弄坏）
/// 写完打印改了几处：改了哪份、哪个名字、从多少变到多少——**不能悄悄改文档**。
export function writeDocs(root = ROOT) {
  let facts;
  try {
    facts = JSON.parse(readFileSync(join(root, 'tools', 'facts.json'), 'utf8')).facts;
  } catch (e) {
    console.log(`读不了 tools/facts.json：${e.message}——什么都没写`);
    return 1;
  }

  const known = new Set(Object.keys(facts));
  const unknown = [];
  for (const file of docsAt(root)) {
    for (const m of readFileSync(file, 'utf8').matchAll(MARK)) {
      if (!known.has(m[1])) unknown.push(`${relative(root, file)} 的 fact:${m[1]}`);
    }
  }
  if (unknown.length) {
    console.log('有标记的名字在真源里找不到——**一个文件都没改**，先看清楚这些：');
    for (const u of unknown) console.log('   × ' + u);
    return 1;
  }

  let changed = 0;
  const lines = [];
  for (const file of docsAt(root)) {
    const text = readFileSync(file, 'utf8');
    const opens = (text.match(OPEN) || []).length;
    let closed = 0;
    const out = text.replace(MARK, (whole, name, wrote) => {
      closed += 1;
      const truth = String(facts[name]);
      if (wrote.trim() === truth) return whole;
      changed += 1;
      lines.push(`   · ${relative(root, file).replace(/\\/g, '/')} fact:${name}：${wrote.trim()} → ${truth}`);
      return `<!-- fact:${name} -->${truth}<!-- /fact -->`;
    });
    if (closed !== opens) {
      console.log(`${relative(root, file)} 里有没闭合的标记——什么都没写`);
      return 1;
    }
    if (out !== text) writeFileSync(file, out, 'utf8');
  }
  console.log(changed ? `写回 ${changed} 处：` : '文档数字已经全等于真源，没改动。');
  for (const l of lines) console.log(l);
  console.log('接着跑 node tools/check_facts.mjs 确认一致。');
  return 0;
}

if (resolve(process.argv[1] || '') === resolve(HERE) && process.argv.includes('--write-docs')) {
  process.exit(writeDocs());
}

// **自测**：`node tools/check_facts.mjs --selftest`
// 只验"这条判据能不能失败 / 会不会误报"，不读仓库里的任何文档——
// 所以它不会再坏在环境上（`mkdtemp` EPERM 那次教训）。
if (resolve(process.argv[1] || '') === resolve(HERE) && process.argv.includes('--selftest')) {
  const p = selftest();
  if (p.length) {
    console.log('真源门禁・自测未通过：');
    for (const x of p) console.log('   × ' + x);
    process.exit(1);
  }
  console.log('真源门禁・自测通过：报规模的写法命中，概念性写法不命中。');
  process.exit(0);
}

if (resolve(process.argv[1] || '') === resolve(HERE)) {
  const r = checkFacts();
  if (r.problems.length) {
    console.log(`文档数字对不上（标记 ${r.marked} 处，真源 ${r.keys} 个数字）：`);
    for (const p of r.problems) console.log('   × ' + p);
    console.log('\n修法：node tools/collect_facts.mjs（刷真源）→ node tools/check_facts.mjs --write-docs（写进文档）');
    process.exit(1);
  }
  console.log(`文档数字一致：${r.marked} 处标记逐个等于 tools/facts.json 里的真值（${r.keys} 个数字）。`);
}
