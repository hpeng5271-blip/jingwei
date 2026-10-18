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
import { checkToolchains, selftest as selftestToolchains } from './check-toolchains.mjs';
import { checkSkillsTree } from './check-skills.mjs';

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
  // 1807：路径规则修死分支——比较串 "C/C++" 少空格（与 lang.rs 的 "C / C++" 永不相等），
  // 顺带把 Rust 降为 Info（实测中文路径下 cargo 能编能跑）。必要修改，不是膨胀。
  // 以后每次调大这个数都要在这一行留一句为什么，否则这条规矩就成橡皮筋了。
  ['crates/engine/src/understand/project.rs', 1807],
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
// **`verify` 的字段也得有人读**（见 checkVerifyWiring 的头）。
{
  const r = checkVerifyWiring(ROOT);
  for (const p of r.problems) problems.push(p);
  if (r.checked && !r.problems.length) {
    console.log(`  · 字段接线：verify 的 ${r.checked} 个字段都有消费方`);
  }
}
// **`guide` 的字段都得有人读**（见 checkGuideWiring 的头）。
{
  const r = checkGuideWiring(ROOT);
  for (const p of r.problems) problems.push(p);
  if (r.checked && !r.problems.length) {
    console.log(`  · 字段接线：guide 的 ${r.checked} 个字段都有消费方`);
  }
}
// **"就绪"判据的两份实现要互相点名**（见 checkReadyParity 的头）。
{
  const r = checkReadyParity(ROOT);
  for (const p of r.problems) problems.push(p);
  if (r.checked && !r.problems.length) console.log('  · 就绪判据：两处都写明了「两份实现」（防悄悄漂走）');
}
// **"我们没接上"不许说成"我们自己漏的"**（见 checkLeakWording 的头）。
{
  const r = checkLeakWording(ROOT);
  for (const p of r.problems) problems.push(p);
  if (r.checked && !r.problems.length) console.log('  · 漏接措辞：不说成「我们的锅」，且写明分不开');
}
// **便携边界要有一处说清，且实测/推断分得开**（见 checkPortableDoc 的头）。
{
  const r = checkPortableDoc(ROOT);
  for (const p of r.problems) problems.push(p);
  if (r.checked && !r.problems.length) console.log("  · 便携边界：" + r.checked + " 项都在（含实测/推断标注与未验清单）");
}
// **试用包要真的能递出去**（见 checkTrialKit 的头）。
{
  const r = checkTrialKit(ROOT);
  for (const p of r.problems) problems.push(p);
  if (r.checked && !r.problems.length) console.log("  · 试用包：" + r.checked + " 样齐全（脚本 / 记录表 / 打包器）");
}
// **二级技能树：每条技能都必须有守卫，且守卫名字必须真实存在**（见 check-skills.mjs 的头）。
{
  const r = checkSkillsTree(ROOT);
  for (const p of r.problems) problems.push(p);
  if (r.checked && !r.problems.length) console.log("  · skills 树：" + r.checked + " 条技能，四段齐全，守卫都找得到");
}
// **「缺哪个件」的链路要真接上**（见 checkRunMissingWiring 的注释）。
{
  const r = checkRunMissingWiring(ROOT);
  for (const p of r.problems) problems.push(p);
  if (r.checked && !r.problems.length) console.log("  · 缺件提示：" + r.checked + " 段链路都在");
}
// **下载来源的链接表也是受检数据**（`框架.md` §4.10）。
// 先证明校验器自己会红（自测），再查真表——**校验器坏了却报"表没问题"，比没有门禁更坏**。
{
  const st = selftestToolchains();
  if (st.failed.length) {
    problems.push(
      "工具链清单的校验器自测不过（" + st.failed.length + "/" + st.cases.length + "）："
        + st.failed[0][0] + " ← " + st.failed[0][2][0],
    );
  }
  const r = checkToolchains(ROOT);
  for (const x of r.problems) problems.push(x);
  if (!r.problems.length) {
    // **空表不许说"没问题"**：那是"还没填"，不是"都对了"。
    console.log(
      r.checked
        ? "  · 工具链清单：" + r.hosts + " 个白名单主机，" + r.checked + " 条字段齐全"
        : "  · 工具链清单：白名单 " + r.hosts + " 个主机，**entries 还没填**（结构对，可下载项 0）",
    );
  }
}
// **颜色表里有的角色，画序里必须也有**（见 checkGraphRolePaint 的注释）。
{
  const r = checkGraphRolePaint(".");
  for (const p of r.problems) problems.push(p);
  // **只在真没问题时才打这一行**：明明红了还打「都在画序里」是谎报。
  if (r.checked && !r.problems.length) console.log("  · 图谱角色：" + r.checked + " 个都在画序里");
}
// **工作方法.md 得在，而且不许空谈**（见 checkWorkMethodDoc 的注释）。
{
  const r = checkWorkMethodDoc('.');
  for (const p of r.problems) problems.push(p);
  if (r.checked) console.log('  · 工作方法.md：在，行数与适用域都过了');
}
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

/// **`工作方法.md` 得在，而且不许空谈**（2026-10-17 加）。
///
/// ## 这份文件是什么
///
/// 它是**给 agent 的操作手册**（不是给人看的产品介绍）：开工流程、用户的思维方式、
/// 我的失败模式、以及 §六/§七 那两版 skill（每条判据带「判据 / 为什么 /
/// 什么时候不适用 / 实测证据」四项）。
///
/// ## 为什么要配门禁
///
/// 它**曾经被误删，而且没有任何东西会红**——盘上没有、git 里没有（从没提交过），
/// 最后是**从会话记录里把 8 次 write/edit 重放回来**才找回来的。
/// **一份"只存在于会话里"的关键文件，必须至少有一条判据盯着它在不在。**
///
/// ## 为什么上限是 400 而不是 350
///
/// 文件自己写着「超过 350 行就说明它在空谈」。现在它是 36x 行——
/// **硬套 350 会逼着删掉有用的话，那比长更糟**（同 `AGENTS.md` 的 120/300 那条：
/// 120 是目标、300 是上限）。所以：**350 是目标，400 是上限**，到了 400 就必须往下搬。
// **「缺哪个件」这条链必须真的接上**（2026-10-18）。
//
// 为什么要有这一道：这一天里同一个形状出现了三次——
//   ① `isolated` 加进了颜色表却没进画序；② `missing_tool` 进 `Outcome` 却没人显示；
//   ③ 现在轮到界面。**名字存在 ≠ 接上线**，而名字是查得到的、接线查不到。
//
// 判据（四段链，缺一段就红）：
//   1. `ui/projects.js` 必须从 `out.missing_tool` 取（**不许自己再扫输出**）
//   2. 取到之后必须存进记录（`final.missing`）
//   3. 失败分支必须**调用**渲染函数（`missingHtml(` 出现在 bad verdict 那一段里）
//   4. `ui/style.css` 必须有 `.missing` 的样式（否则渲染出来是裸字）
export function checkRunMissingWiring(root) {
  const problems = [];
  let checked = 0;
  const jsPath = join(root, "ui", "projects.js");
  const cssPath = join(root, "ui", "style.css");
  if (!existsSync(jsPath)) return { problems: ["ui/projects.js 不见了"], checked };
  const js = readFileSync(jsPath, "utf8");

  checked++;
  if (!js.includes("out.missing_tool")) {
    problems.push("ui/projects.js 没有从 `out.missing_tool` 取——引擎算出来了但没人读");
  }

  checked++;
  if (!js.includes("final.missing")) {
    problems.push("ui/projects.js 没有把缺件存进记录（`final.missing`）——渲染时会读到 undefined");
  }

  checked++;
  // 失败分支里必须真的调用渲染函数。**在整文件里搜 `missingHtml(` 不够**——
  // 定义处自己也算一次，那正是"加了名字没接线"的样子。
  const badBlock = js.slice(js.indexOf("verdict = '<div class=\"verdict bad\""));
  const badEnd = badBlock.indexOf("</div>';");
  const inBad = badEnd > 0 ? badBlock.slice(0, badEnd) : badBlock;
  if (!inBad.includes("missingHtml(")) {
    problems.push("失败分支里没有调用 `missingHtml(...)`——函数写了但没接在渲染路径上");
  }

  checked++;
  if (!existsSync(cssPath)) {
    problems.push("ui/style.css 不见了");
  } else if (!readFileSync(cssPath, "utf8").includes(".missing {")) {
    problems.push("ui/style.css 里没有 `.missing` 样式——渲染出来会是裸字");
  }

  return { problems, checked };
}

// **试用包必须真的存在、且判定写死**（2026-10-18）。
//
// 为什么要有这一道：目标里那条硬缺口「一个真人用一次」**从来没发生过**。
// 文档里写"以后要找人试用"等于没写——这个仓库为"规矩写在文档里没人执行"付过多次账。
//
// 判据（少一样就红）：
//   1. `试用脚本.md` 在，且含 §4「判定」那一节（**通过/部分通过/不通过**三档）
//   2. `试用记录表.md` 在（空白的也算——它是被填的那一份）
//   3. `tools/make-trial.ps1` 在（能打出文件夹才算"能递出去"）
//   4. 脚本里必须写清"递出去只说一句话"——不许提示，否则这次试用作废
//
// 诚实边界：这一道只能保证"递得出去"，**保证不了"真的有人试过"**。
// 有没有人试过，只能看仓库里有没有 `试用记录-*.md`。
export function checkTrialKit(root) {
  const problems = [];
  let checked = 0;
  const script = join(root, "试用脚本.md");
  const sheet = join(root, "试用记录表.md");
  const maker = join(root, "tools", "make-trial.ps1");

  checked++;
  if (!existsSync(script)) {
    problems.push("试用脚本.md 不见了——「真人用一次」就没有可执行的东西");
  } else {
    const md = readFileSync(script, "utf8");
    for (const need of ["## 4. 判定", "**通过**", "**部分通过**", "**不通过**", "不许提示"]) {
      if (!md.includes(need)) problems.push(`试用脚本.md 缺「${need}」——判定没写死，事后就会解释成"其实还行"`);
    }
  }

  checked++;
  if (!existsSync(sheet)) problems.push("试用记录表.md 不见了——没人能记「卡在哪」");

  checked++;
  if (!existsSync(maker)) problems.push("tools/make-trial.ps1 不见了——试用包打不出来，就递不出去");

  // **入口必须提到它**（2026-10-18）：否则试用包沉在 dist/ 里，
  // 和"写了但没人知道"是同一种失效。**能被找到才算存在。**
  checked++;
  const readme = join(root, "README.md");
  if (!existsSync(readme)) {
    problems.push("README.md 不见了");
  } else if (!readFileSync(readme, "utf8").includes("make-trial.ps1")) {
    problems.push("README.md 没提 make-trial.ps1——试用包沉在 dist/ 里，没人会知道");
  }

  checked++;
  const usage = join(root, "使用说明.md");
  if (!existsSync(usage)) {
    problems.push("使用说明.md 不见了");
  } else if (!readFileSync(usage, "utf8").includes("make-trial.ps1")) {
    problems.push("使用说明.md 没提怎么打试用包——使用者不会去翻 tools/");
  }

  // **`dist\trial` 里那份必须是仓库里的这一份**（2026-10-18 补，真发生过）。
  //
  // 现状：`dist\trial\试用脚本.md` 是 **11:26** 打的（3733 字节），
  // 而仓库里那份已经是 **4367 字节**——两版早就不一样了。
  // 也就是说：**现在递出去的话，递的是一份旧脚本。**
  // 这一条守的就是它：包可以还没打（不存在就放过），**打了就必须是最新的**。
  checked++;
  const trialDir = join(root, "dist", "trial");
  if (existsSync(trialDir)) {
    for (const name of ["试用脚本.md", "试用记录表.md"]) {
      const a = join(trialDir, name);
      if (!existsSync(a)) {
        problems.push(`dist\\trial\\ 里缺 ${name}`);
      } else if (readFileSync(a, "utf8") !== readFileSync(join(root, name), "utf8")) {
        problems.push(
          `dist\\trial\\${name} 与仓库里那份不一致（旧拷贝）——重跑 tools\\make-trial.ps1`,
        );
      }
    }
  }

  return { problems, checked };
}

// **"便携版在哪些语言上走不通"必须有一处集中说清**（2026-10-18）。
//
// 为什么要有这一道：`取舍记录.md` T-01 定了"绿色免安装、不提权"，
// 那么"哪些语言在这套边界下根本跑不起来"就是**产品的边界说明**，不是附录。
// 它最容易出的问题是**含糊**：把推断写成事实、把"没验过"写得像验过。
//
// 判据（少一样就红）：
//   1. `工具链-便携与安装.md` 在；
//   2. **实测与推断都标了**（`[实测]` 与 `[推断]` 各至少一处）——
//      混着写是这套东西最熟的病；
//   3. 有「走不通」的结论（Rust 那行必须明确说不能纯便携）；
//   4. 有「还没做的」清单（**没验过的事必须列出来**，不许只写好消息）。
export function checkPortableDoc(root) {
  const problems = [];
  let checked = 0;
  const p = join(root, "工具链-便携与安装.md");
  if (!existsSync(p)) {
    return { problems: ["工具链-便携与安装.md 不见了——便携边界就没人说清了"], checked };
  }
  const md = readFileSync(p, "utf8");

  checked++;
  if (!md.includes("[实测]")) problems.push("便携文档里没有一处 [实测]——那全是推断");
  checked++;
  if (!md.includes("[推断]")) problems.push("便携文档里没有一处 [推断]——把推断说成事实更难发现");

  checked++;
  if (!md.includes("不能") || !md.includes("Rust")) {
    problems.push("便携文档没说清哪门语言「不能」纯便携——那是这一页的核心结论");
  }

  checked++;
  if (!md.includes("还没做的")) {
    problems.push("便携文档缺「还没做的」——只写好消息的边界说明等于广告");
  }

  return { problems, checked };
}

// **"我们没接上"那句话不许说成"我们自己漏的"**（2026-10-18，flask 实测换的）。
//
// 实测：flask 全项目 388 条 import 里 134 条是标准库（34%），而索引输出把
// "名字在项目里找不到"的导入全算成"我们自己漏的"，报出 203 + 350——
// **比那个项目全部 import 语句还多**。那是把"外部"说成自己的锅（三态压扁）。
//
// 判据：输出这句的附近必须写明"分不开"，而且**不许**再出现「我们自己漏的」这种断言。
// 诚实边界：这一道只查措辞，查不出分类对不对——分类我们确实还分不开。
export function checkLeakWording(root) {
  const problems = [];
  let checked = 0;
  const p = join(root, "crates", "cli", "src", "cmd_graph.rs");
  if (!existsSync(p)) return { problems: ["crates/cli/src/cmd_graph.rs 不见了"], checked };
  const src = readFileSync(p, "utf8");

  // **只看代码行**：注释里出现这个词是**合法的**——那正是在解释"为什么不再这么说"。
  // 第一版扫全文，于是它抓到了我自己那句解释性注释（假红）。
  const codeLines = src
    .split(/\r?\n/)
    .filter((l) => {
      const t = l.trim();
      return !t.startsWith("//");
    })
    .join("\n");

  checked++;
  if (codeLines.includes("我们自己漏的")) {
    problems.push("cmd_graph.rs 里又出现「我们自己漏的」这句输出——它会把外部模块算成我们的锅（flask 实测 203+350 超过全项目 import 数）");
  }

  checked++;
  if (!codeLines.includes("分不开")) {
    problems.push("cmd_graph.rs 里那句「没接上」的说明必须写明「分不开」——分得开才许分开说");
  }

  return { problems, checked };
}

// **"就绪"这一个判据有两份实现，谁都不许偷偷单飞**（2026-10-18）。
//
// 门禁抖动的根因就是这个：`src-tauri/src/selfcheck.rs` 注入的 JS 里的 `ready(tab)`
// 只判"页面可见"，而 `tools/uitest-window.mjs::reportReady()` 要求"主卡有字"。
// 于是报告在"可见但还没画好"时被拍下来，门禁那边永远判它没就绪——**干等 90 秒**。
//
// 两份实现跨不了语言边界（一个在 Rust 字符串里、一个在 mjs 里），所以留了这条门禁：
// **两边都必须写明"同一个判据的两份实现"**。它挡不住逻辑被改坏，
// 但能挡住"有人只改一边、另一边悄悄漂走"。
export function checkReadyParity(root) {
  const problems = [];
  let checked = 0;
  const pairs = [
    [join(root, "src-tauri", "src", "selfcheck.rs"), "同一个判据的两份实现"],
    [join(root, "tools", "uitest-window.mjs"), "两份实现"],
  ];
  for (const [f, marker] of pairs) {
    checked++;
    if (!existsSync(f)) { problems.push(`${f} 不见了`); continue; }
    const src = readFileSync(f, "utf8");
    if (!src.includes(marker)) {
      problems.push(`${f} 没写明"就绪判据有两份实现"——两边会悄悄漂走（这正是抖动根因）`);
    }
  }
  return { problems, checked };
}

// **`guide` 的每个输出字段都得有人读**（2026-10-18，靠它抓到 `niceToHave` 没人读）。
//
// 这个仓库最贵的失效模式是"写了但没接上"：引擎把字段产出来，某个壳却一个字不说。
// 拿真数据数过一遍：`guide --json` 的 7 个键里，`niceToHave` 在 ui/ 与 cli/ 里 **0 处出现**，
// 而 CLI 其实一直在打它——**同一份数据，两个壳只讲一个**。
//
// ## 范围（**故意只查这一个 payload**）
//
// 只查 `Guide::to_json()` 的字段，以及它 `ready` 里那几个。**不假装全仓覆盖**：
// 别的 payload（scan/hygiene/graph）没有同样稳定的"出一份 json"入口。
//
// ## 判据
//
// 每个键必须在 **ui/*.js 或 crates/cli/src** 里出现过至少一次；否则红。
// 确实"产出来但这一轮还没人用"的，写进 `allowed` 并说明理由——**留白要留成有意的**。
export function checkGuideWiring(root) {
  const problems = [];
  let checked = 0;
  const guideRs = join(root, "crates", "engine", "src", "understand", "guide.rs");
  if (!existsSync(guideRs)) return { problems: ["guide.rs 不见了"], checked };
  const src = readFileSync(guideRs, "utf8");

  // 只取 `pub fn to_json` 这一段的字面键
  const at = src.indexOf("pub fn to_json");
  if (at < 0) return { problems: ["找不到 to_json——判据要跟着实现改"], checked };
  const body = src.slice(at, src.indexOf("pub fn make_guide", at) > 0 ? src.indexOf("pub fn make_guide", at) : at + 4000);
  const keys = [...new Set([...body.matchAll(/"([A-Za-z][A-Za-z0-9_]*)":/g)].map((m) => m[1]))];

  // **有意留白**：键 → 理由。加进来之前先问"是真没人该用，还是我还没接"。
  const allowed = new Map();

  const consumers = [];
  for (const f of readdirSync(join(root, "ui"))) {
    if (f.endsWith(".js")) consumers.push(readFileSync(join(root, "ui", f), "utf8"));
  }
  for (const f of readdirSync(join(root, "crates", "cli", "src"))) {
    if (f.endsWith(".rs")) consumers.push(readFileSync(join(root, "crates", "cli", "src", f), "utf8"));
  }
  const all = consumers.join("\n");

  for (const k of keys) {
    if (allowed.has(k)) continue;
    checked++;
    if (!all.includes(k)) {
      problems.push(`guide 产出了 \`${k}\`，但 ui/ 与 cli/ 里一处都没读它——要么接上，要么写进 allowed 说明理由`);
    }
  }
  return { problems, checked, keys };
}

// **`verify` 的每个输出字段都得在界面上有人读**（2026-10-18，与 `checkGuideWiring` 同一判据）。
//
// 为什么单独再加一条：`verify` 的 payload 是**手写 json!**（`src-tauri/src/verify.rs`），
// 加字段最便宜、也最容易加完就忘。而它是昨天才做的四态功能——正是"看着有、其实没用上"的高危区。
//
// 判据：`verify.rs` 里 `"键":` 形式出现的每个键，必须在 `ui/*.js` 里出现过。
// 确实只是给 CLI / 测试用的，写进 `allowed` 并说明理由。
export function checkVerifyWiring(root) {
  const problems = [];
  let checked = 0;
  const rs = join(root, "src-tauri", "src", "verify.rs");
  if (!existsSync(rs)) return { problems: ["src-tauri/src/verify.rs 不见了"], checked };
  const src = readFileSync(rs, "utf8");
  const keys = [...new Set([...src.matchAll(/"([a-z][a-z0-9_]*)":/g)].map((m) => m[1]))];

  const ui = [];
  for (const f of readdirSync(join(root, "ui"))) {
    if (f.endsWith(".js")) ui.push(readFileSync(join(root, "ui", f), "utf8"));
  }
  const all = ui.join("\n");

  const allowed = new Map([
    // `kind` 只用于"签名状态"那一行的内部判断，界面读的是它的中文，不是这个键本身。
  ]);

  for (const k of keys) {
    if (allowed.has(k)) continue;
    checked++;
    if (!all.includes(k)) {
      problems.push(`verify 产出了 \`${k}\`，但 ui/ 里一处都没读它——要么接上，要么写进 allowed 说明理由`);
    }
  }
  return { problems, checked, keys };
}

export function checkWorkMethodDoc(root) {
  const problems = [];
  let checked = 0;
  const file = join(root, '工作方法.md');
  if (!existsSync(file)) {
    problems.push('工作方法.md 不见了——那是 agent 的操作手册 + skill 本体，先把它找回来');
    return { problems, checked };
  }
  checked++;
  const text = readFileSync(file, 'utf8');
  const lines = text.split(String.fromCharCode(10)).length;
  if (lines > 400) {
    problems.push('工作方法.md = ' + lines + ' 行（上限 400，目标 350）——该往下搬了');
  }
  // §六/§七 是 skill 本体：**每条判据必须有"什么时候不适用"**（文件自己立的准入门槛）。
  const skillPart = text.slice(text.indexOf('## 六、'));
  if (!skillPart) {
    problems.push('工作方法.md 里找不到 §六（skill 第一版）——判据失效，不许当成没什么可查而通过');
  } else {
    const nBuShiYong = (skillPart.match(/不适用/g) || []).length;
    if (nBuShiYong < 3) {
      problems.push('§六/§七 里「不适用」只出现 ' + nBuShiYong + ' 次——每条判据都该写明适用域');
    }
  }
  return { problems, checked };
}

/// **颜色表里有的角色，画序里必须也有**（2026-10-17 加）。
///
/// ## 它守的是哪一次事故
///
/// 上一轮给图谱页加 `isolated`（「还没连上」）时，颜色表、图例、环形半径都加了，
/// **唯独忘了画布上那行 `for (const role of [...])` 的数组**。后果：
///
/// - 图例说「还没连上 17 个文件」
/// - 画布上**一个都找不到**
/// - 兜底那一段也救不了：它对"有角色的节点"直接 `continue`
///
/// 而且**没有任何门禁会红**——图例文本对、节点数对、界面门禁全绿。
/// 这种"加了名字但没接上线"的失效，**只能靠静态一致性检查抓**。
///
/// 判据：`GROLE` 的每个键，都必须出现在画序数组里。
/// 不查顺序（顺序是设计判断），只查**有没有被画**。
export function checkGraphRolePaint(root) {
  const problems = [];
  let checked = 0;
  const file = join(root, "ui", "graph.js");
  if (!existsSync(file)) return { problems: ["ui/graph.js 不见了"], checked };
  const src = readFileSync(file, "utf8");

  // GROLE 表：形如 `  entry:      { c: "#cc7832", ... }`
  const block = src.match(/var GROLE = \{([\s\S]*?)\n\};/);
  if (!block) {
    return { problems: ["ui/graph.js 里找不到 GROLE 表——判据失效，不许当成没什么可查而通过"], checked };
  }
  const keys = [];
  for (const m of block[1].matchAll(/^\s{2}([a-z_]+):\s*\{/gm)) keys.push(m[1]);
  if (!keys.length) return { problems: ["GROLE 里一个角色都没解析出来"], checked };

  // 画序数组：`for (const role of [...])`
  const order = src.match(/for \(const role of \[([^\]]*)\]\)/);
  if (!order) {
    return { problems: ["ui/graph.js 里找不到按角色绘制的那行循环——判据失效"], checked };
  }
  const painted = order[1].split(",").map((s) => s.trim().replace(/^['"]|['"]$/g, "")).filter(Boolean);

  for (const k of keys) {
    checked++;
    if (!painted.includes(k)) {
      problems.push(
        "ui/graph.js：角色「" + k + "」在颜色表里，但画序数组里没有——它不会被画出来",
      );
    }
  }
  return { problems, checked };
}