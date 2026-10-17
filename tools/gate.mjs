// 门禁里那些"没法靠编译器守"的规矩，用这个脚本守。
//
// 为什么不写成文档里的一句话：规矩写在文档里没人执行就等于没有（§13 迭代规矩）。
// 这里只放五件能自动判定的，其余留给 clippy 和测试。
//
// 用法：node tools/gate.mjs

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

import { checkFacts, selftest as selftestFacts } from './check_facts.mjs';
import { checkDocs, selftest as selftestDocs } from './check-docs.mjs';
import { checkUiDoc } from './check-ui-doc.mjs';
import { checkDocLinks } from './check-doc-links.mjs';

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

/**
 * 文档数字与真源一致。
 *
 * 判断在 check_facts.mjs 里，这里只是把它挂进门禁——同一份判断只有一处实现
 * （§3.2 判断只有一份）。为什么要挂在 gate.mjs 而不是单开一道门禁：
 * gate.mjs 管的就是"没法靠编译器守的规矩"，而"文档里的数字必须在仓库里跑得出来"
 * 正是这一类；挂在它下面，门禁道数仍是 5 道，文档里那些"五道门禁"的说法不用重写。
 */
function checkDocFacts() {
  const r = checkFacts(ROOT);
  for (const p of r.problems) problems.push(p);
  // **自测一起跑**（2026-10-16 加）：这道门禁的"零标记"判据是**枚举式**的
  // （词表见 `check_facts.mjs`），弱点就是"换种说法就漏"——
  // 新写的 `介绍.md` 用表格报规模，原来那四个词一个都不匹配，整份文档走了过去。
  // 光加词不够，**得有一个地方能演示它到底漏不漏**。
  // 自测只依赖内存字符串、不碰磁盘，所以不会像 `check-docs` 自测那样
  // 坏在 `mkdtemp` 上（那次七道门禁整套没跑起来，而没人知道）。
  const st = selftestFacts();
  for (const p of st) problems.push('check-facts 自测没过：' + p);
  if (!r.problems.length && !st.length) {
    console.log(`  · 文档数字：${r.marked} 处标记逐个等于 tools/facts.json 里的真值（${r.keys} 个数字）· 自测过（报规模的写法命中、概念性写法不命中）`);
  }
}

/**
 * 文档自洽：**声明 N 项的地方，枚举必须真的是 N 条**。
 * 判断在 check-docs.mjs 里，这里只把它挂进门禁——同一份判断只留一个实现。
 * 为什么挂在 gate.mjs 而不是往 gates.json 里加一道：理由同上（见 checkDocFacts 的注释）。
 * 五道门禁这个道数是文档里写着的数（总稿 §8.1、设计 §12.3、tools/facts.json 的 gates），
 * 再加一道就得连着改那三处；而"文档不许自己跟自己矛盾"本来就属于工程门禁这一类。
 * 每次连自测一起跑：自测证明这道门禁**能失败**——不是只会在绿的时候绿。
 */
/**
 * **crate 表必须标实现状态**（2026-10-17 加）。
 *
 * 由来：`设计.md` §3.1 那张 crate 表把 `store/` 与 `adapters/` 和真存在的 crate
 * 平铺着写，**读的人会以为六层都在跑**。而事实是那两个目录不存在
 * （持久化在 `crates/engine/src/graph/store.rs`，适配层根本没做）。
 *
 * 这与 `框架.md` 记的同一条：**设计文档 §8.1 的"过滤七步"表没有状态列，
 * 核实后只有 ⑤ 是完整的——不标实现状态，读的人会以为都在跑。**
 *
 * 判据（两条，都能失败）：
 *   ① 表里提到的每个 `--- name/`，**若 `crates/name` 不存在，该条目必须带 ⚠️**；
 *   ② **实际存在的 crate 必须在表里出现**（新增 crate 却忘了写 = 文档落后）。
 */
function checkCrateTable() {
  const doc = join(ROOT, '设计.md');
  if (!existsSync(doc)) { problems.push('没有 设计.md，没法核 crate 表'); return; }
  const lines = readFileSync(doc, 'utf8').split(/\r?\n/);
  const fence = /^```/;
  // 找表头那一行：以 `crates/` 开头、**后面是空白**
  // （后面可以跟注释——我自己就在那行右边加了「状态」说明，第一版用 `=== 'crates/'`
  //   于是匹配不到，门禁报「找不到 crate 表」——**判据太脆，是我的写法问题**）
  let i = lines.findIndex((x) => /^crates\/\s/.test(x.trim()));
  if (i < 0) { problems.push('设计.md 里找不到 crate 表（第一行应为 crates/）'); return; }
  while (i > 0 && !fence.test(lines[i])) i--;
  const block = [];
  for (let k = i + 1; k < lines.length && !fence.test(lines[k]); k++) block.push(lines[k]);
  if (!block.length) { problems.push('crate 表是空的'); return; }

  const listed = new Map();
  const head = /^---\s*([A-Za-z0-9_-]+)\//;
  for (let k = 0; k < block.length; k++) {
    const m = block[k].match(head);
    if (!m) continue;
    let tail = '';
    for (let j = k + 1; j < block.length; j++) {
      if (head.test(block[j])) break;
      tail += block[j] + '\n';
    }
    listed.set(m[1], tail);
  }
  if (listed.size === 0) { problems.push('crate 表里一个 --- name/ 都没解析出来'); return; }

  const before = problems.length;
  // crate 可能住在 `crates/<name>`，也可能住在仓库根（`src-tauri/` 是 Tauri 的约定位置）。
  // **第一版只查 crates/**，于是 `src-tauri` 被报成"不存在"——判据漏了一条真实形状。
  const crateExists = (name) => existsSync(join(ROOT, 'crates', name)) || existsSync(join(ROOT, name));
  for (const [name, tail] of listed) {
    if (!crateExists(name) && !tail.includes('⚠️')) {
      problems.push(
        `设计.md 的 crate 表列了 ${name}/，但 crates/${name} 不存在，而表里没标 ⚠️`
        + '——不标实现状态，读的人会以为它在跑',
      );
    }
  }

  const cratesDir = join(ROOT, 'crates');
  if (existsSync(cratesDir)) {
    for (const e of readdirSync(cratesDir, { withFileTypes: true })) {
      if (!e.isDirectory() || e.name.startsWith('.')) continue;
      if (listed.has(e.name)) continue;
      problems.push(`crates/${e.name} 存在，但 设计.md 的 crate 表里没提它`);
    }
  }
  if (problems.length === before) {
    console.log(`  · crate 表：${listed.size} 个条目，缺的都标了 ⚠️，真存在的都在表里`);
  }
}

/**
 * **入口文件不许无限长**（2026-10-17 加）。
 *
 * `AGENTS.md` 自己写着「超过 120 行就该删内容」，而它长到过 **705 行**、
 * 被 DSH 的指令预算截断过一次（尾部静默丢失）。写完那句话之后**没人执行**——
 * 因为门禁里没有任何东西盯着 `.md` 的体积。
 *
 * **这条判据真正买的是"每轮成本"**：`AGENTS.md` 是唯一每轮都被注入上下文的文件，
 * 它的体积不是文风问题，是**每一轮都要付一次的钱**。
 *
 * 边界（别把这条判据做过头）：
 *   - 只管**入口**（`AGENTS.md`）。`设计.md` / `复审记录.md` 那些**材料本来就长**，
 *     给它们设上限只会逼着把有用的话删掉——**那比长更糟**。
 *   - 120 是**目标**，这里守的是**上限**：到了就必须往下搬（搬到 `事故账本.md`）。
 */
function checkEntryDocSize() {
  const CEILING = 300;
  const f = join(ROOT, 'AGENTS.md');
  if (!existsSync(f)) { problems.push('没有 AGENTS.md'); return; }
  const n = readFileSync(f, 'utf8').split(/\r?\n/).length;
  if (n > CEILING) {
    problems.push(
      `AGENTS.md 有 ${n} 行，超过上限 ${CEILING}——`
      + '它是唯一每轮都被注入上下文的文件，长出来的部分要搬到 事故账本.md（状态史）'
      + '或 框架.md / SOP.md（方法论）',
    );
  } else {
    console.log(`  · 入口文件体积：AGENTS.md ${n} 行（上限 ${CEILING}，目标 120）`);
  }
}

function checkDocClaims() {

  const r = checkDocs(ROOT);
  for (const p of r.problems) problems.push(p);
  const st = selftestDocs();
  for (const p of st.problems) problems.push('check-docs 自测没过：' + p);
  if (!r.problems.length && !st.problems.length) {
    console.log(
      `  · 文档自洽：${r.claims} 条「声明 N 项」逐个等于枚举数出来的条数（自测 ${st.cases} 例：一致→过、对不上 / 找不到→报错）`,
    );
  }
}

/**
 * 页面清单：**说明书讲到的页签集合 == 代码里的页签集合**；外加**文档链接点得开**。
 *
 * 两条判据分开放在两个文件里（各管一件事），这里只挂进工程门禁：
 *   - `check-ui-doc.mjs`：页签集合相等 + 标题页数 == 表行数 == 代码页数；
 *   - `check-doc-links.mjs`：markdown 链接从仓库根解析存不存在。
 *
 * **`check-doc-links` 的边界写在它自己的文件头里**：它只保证"链接不烂"，
 * 抓不到"文档承诺了一个不存在的界面"那类语义问题——那类只能靠人读。
 * 2026-10-16 我试着把它做成通用判据，**三次负向测试都没红**，于是收回到这一条。
 */
function checkPageList() {
  const r = checkUiDoc(ROOT);
  for (const p of r.problems) problems.push(p);
  if (!r.problems.length) {
    console.log(`  · 页面清单：代码里 ${r.pages} 页，说明书第三节逐页都讲了（集合相等）`);
  }
  const l = checkDocLinks(ROOT);
  for (const p of l.problems) problems.push(p);
  if (!l.problems.length) {
    console.log(`  · 文档链接：${l.checked} 条本地链接都点得开`);
  }
}

checkVersion();
checkFileLength();
checkMotives();
checkDocFacts();
checkDocClaims();
checkCrateTable();
checkEntryDocSize();
checkPageList();
// **自检巡查的选择器必须真有对应元素**（2026-10-17 加）。
// 它守的是一种"看起来在守、其实在量空气"的失效：元素改名后，
// 自检照报 `exists: false`，而界面门禁不逐条断言 `exists` 就会静默通过。
{
  const r = checkSelfcheckSels('.');
  for (const p of r.problems) problems.push(p);
  if (r.checked) console.log('  · 自检选择器：' + r.checked + ' 个，界面上都有对应元素');
}

if (problems.length) {
  console.log('门禁未通过：');
  for (const p of problems) console.log('  × ' + p);
  process.exit(1);
}
console.log('门禁通过：版本号三处一致，源文件行数在限内，文档数字与 tools/facts.json 一致，文档声明与枚举一致，页面清单与说明书一致，文档链接都点得开');

/// **自检的 SEL 里每一个选择器，界面上都得真有那个东西**（2026-10-17 加）。
///
/// 为什么需要：`selfcheck.rs` 用一串选择器去真窗口里量元素，
/// **但某个元素改名或删掉之后，量出来只是 `exists: false` 的一条记录**——
/// 界面门禁只要不逐条断言 `exists`，它就会**静默通过**。
///
/// 这个仓库栽过同型的账：债表里写着"自检 SEL 在巡查 `ghDeps`/…"，
/// 而那些元素**在 `ui/` 里根本不存在**——巡查了一年的空气。
///
/// 判据：`#id` → `index.html` 里必须有 `id="…"`；
/// `.cls`（可带多个点）→ 每个 class 在 html 或 css 里出现过。
export function checkSelfcheckSels(root) {
  const problems = [];
  let checked = 0;
  let src;
  try {
    src = readFileSync(join(root, 'src-tauri/src/selfcheck.rs'), 'utf8');
  } catch {
    return { problems, checked };
  }
  const start = src.indexOf('const SEL = [');
  if (start < 0) {
    return { problems: ['selfcheck.rs 里找不到 SEL 定义——判据失效，不许当成"没什么可查"而通过'], checked };
  }
  const end = src.indexOf('];', start);
  const NL = String.fromCharCode(10);
  const block = src
    .slice(start, end)
    .split(NL)
    .map((line) => line.replace(/\/\/.*$/, '')) // 去掉行尾注释，免得把注释里的字当成选择器
    .join(NL);
  const sels = [...block.matchAll(/'([^']+)'/g)].map((x) => x[1]);
  if (!sels.length) {
    return { problems: ['SEL 里一个选择器都没解析出来——判据失效'], checked };
  }

  const html = readFileSync(join(root, 'ui/index.html'), 'utf8');
  let css = '';
  try {
    css = readFileSync(join(root, 'ui/style.css'), 'utf8');
  } catch {
    css = '';
  }
  for (const sel of sels) {
    checked++;
    if (sel.startsWith('#')) {
      const id = sel.slice(1);
      if (!html.includes('id="' + id + '"')) {
        problems.push('SEL 指向 ' + sel + '，但 index.html 里没有这个 id');
      }
    } else if (sel.startsWith('.')) {
      for (const cls of sel.slice(1).split('.')) {
        if (!html.includes(cls) && !css.includes(cls)) {
          problems.push('SEL 指向 ' + sel + '，但 html 与 css 里都没有 ' + cls + ' 这个 class');
        }
      }
    } else if (!html.includes('<' + sel)) {
      // 裸标签名（body / main 这类）：查 <tag 在不在
      problems.push('SEL 指向 <' + sel + '>，但 index.html 里没有这个标签');
    }
  }
  return { problems, checked };
}
