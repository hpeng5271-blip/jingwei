// 页面清单门禁：**说明书里的页签集合，必须等于代码里的页签集合**。只比较，不修改。
//
// 为什么要有这一道（2026-10-16，第 20 轮从"用户视角"盘点时抓到）：
//   `ui/events.js` 里 `TABS = ['home','run','editor','note','graph','machine','clean']`
//   ——**七个**；而《使用说明.md》第三节写的是「界面上的**六个**页面」，
//   而且表里**根本没有「图谱」那一行**。`index.html` 里 `data-tab="graph"`
//   还带着 `data-needs="project"`，也就是说它是"选中项目后才能用"的那一组里的第五个。
//
// 这和同一轮抓到的 `--help` 漏列四个命令、`jingwei inspect` 被文档承诺却不存在，
// **是完全同一个形状**：**两份清单各写各的，必然漂移**。
// 那份是靠"子命令唯一真源 + 三条测试"收口的；这一份同理——
// 真源是 `ui/events.js` 的 `TABS` 与 `ui/index.html` 的 `data-tab` 标签，
// 被判据盯住的是说明书有没有跟上。
//
// 判据（**任何一条不成立就报错**，尤其后两条）：
//   1. `TABS` 与 `index.html` 里的 `data-tab` 集合必须相等——两处都是代码，但也是两份清单；
//   2. 说明书里数出来的页签集合，必须**逐个**能在 TABS 里找到（不许凭空多一个）；
//   3. TABS 里每一个，都必须**在说明书里出现过**（不许漏讲）。
//      第 3 条尤其不能省：**漏讲**正是这次实际发生的那个错，
//      而"文档说什么都对"的判据（只查第 2 条）会眼睁睁放它过去。
//
// 诚实边界：这条只认"名字出现过"，查不出"那句说明写得对不对"。
// 后者没有机器判据，只能靠人读——**但"有没有讲"是能查的，而这次错的就是那个**。

import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..');

// 这里原来有一个 `findPathTail`（全树找路径后缀）。
// **2026-10-16 删掉了**：那条判据三次负向测试都没红——"任意深度后缀匹配"几乎什么都命中，
// 而一条几乎什么都放过的判据比没有更坏（它看起来在守）。
// 文档链接那一半另起了 `tools/check-doc-links.mjs`（只查 markdown 链接，确定且可判定）；
// 这一份现在只管页面清单。

/// 代码里的页签真源：`ui/events.js` 的 `TABS = [...]`
export function tabsFromEvents(js) {
  const m = js.match(/const\s+TABS\s*=\s*\[([^\]]*)\]/);
  if (!m) return null;
  return m[1]
    .split(',')
    .map((s) => s.trim().replace(/^['"]|['"]$/g, ''))
    .filter(Boolean);
}

/// `ui/index.html` 里每个导航按钮的 `data-tab="名字"` → `<span class="lbl">中文</span>`
///
/// 返回 `{ key: '中文标签' }`。两处代码对不上也要报错：一个页签叫 `graph`
/// 却没有按钮，用户根本点不到它。
export function tabsFromHtml(html) {
  const out = {};
  // 逐个 `<button ... data-tab="x" ...>...</button>` 抠出来（按钮里还有 svg，不能跨标签乱配）
  const re = /<button\b[^>]*\bdata-tab="([^"]+)"[^>]*>([\s\S]*?)<\/button>/g;
  let m;
  while ((m = re.exec(html))) {
    const key = m[1];
    const lbl = m[2].match(/<span class="lbl">([^<]*)<\/span>/);
    out[key] = lbl ? lbl[1].trim() : '';
  }
  return out;
}

/// 说明书第三节里讲到的页签标签（中文）＋ 标题声明的页数。
///
/// 只从**第三节那张"界面上的 N 个页面"**里取——`## 三、` 到下一个 `## ` 之间。
/// 不全文搜：全文搜会把别处顺口提到的词也算进来，那样判据就松了。
export function tabsFromDoc(md) {
  const sec = md.split(/^## /m).find((s) => s.startsWith('三、'));
  if (!sec) return null;
  const end = sec.indexOf('\n## ');
  const body = end >= 0 ? sec.slice(0, end) : sec;
  // 表格行 `| **概览** | ... |` → 取第一格里的加粗词
  const names = [];
  for (const line of body.split(/\r?\n/)) {
    const m = line.match(/^\|\s*\*\*([^*]+)\*\*\s*\|/);
    if (m) names.push(m[1].trim());
  }
  // 标题里声明的页数。**这一条是负向测试逼出来的**：
  // 光比"集合"的话，标题改回「六个页面」而表里有七行——**照样通过**。
  // 我刚写完这条检查就当场试出来了，所以补上。
  const cn = { 一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9, 十: 10 };
  const dm = body.match(/界面上的\s*([零一二三四五六七八九十\d]+)\s*个页面/);
  let declared = null;
  if (dm) {
    const w = dm[1];
    declared = /^\d+$/.test(w) ? Number(w) : (cn[w] ?? null);
  }
  return { names, declared };
}

// README 那一份也查（2026-10-17 加）。
//
// **为什么**：README 写着「界面上的六个页面」并列了六个，**漏了图谱页**——
// 而它是仓库门面、也是最能说明"这是什么工具"的那一页。门禁当时没抓到，
// 因为它只核对《使用说明.md》。**覆盖面是"按文档逐个加的"，这本身是个洞。**
//
// README 那一节不是表格，是行内加粗：`**概览**（…）· **运行**（…）`。
// 判据与说明书那半边一样：**标题声明的页数 = 代码页数**，且**每页都被点到**。
export function tabsFromReadme(md) {
  const m = md.match(/^##\s*界面上的\s*([零一二三四五六七八九十\d]+)\s*个页面[^\n]*\n([\s\S]*?)(?=^##\s|\Z)/m);
  if (!m) return null;
  const cn = { 一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9, 十: 10 };
  const w = m[1];
  const declared = /^\d+$/.test(w) ? Number(w) : (cn[w] ?? null);
  const names = [];
  for (const mm of m[2].matchAll(/\*\*([^*\n+]{1,20})\*\*/g)) names.push(mm[1].trim());
  return { names, declared };
}

/// 跑一遍判据，返回 `{ problems, pages }`。**不自己打印、不自己退出**——
/// 这样 `gate.mjs` 才能把它和其它工程门禁合在一起报（与 `check-docs` 同一个接口形状）。
export function checkUiDoc(root = ROOT) {
  const problems = [];
  const ev = readFileSync(join(root, 'ui', 'events.js'), 'utf8');
  const html = readFileSync(join(root, 'ui', 'index.html'), 'utf8');
  const md = readFileSync(join(root, '使用说明.md'), 'utf8');

  const tabs = tabsFromEvents(ev);
  const labels = tabsFromHtml(html);
  const doc = tabsFromDoc(md);
  const docNames = doc && doc.names;

  if (!tabs) return { problems: ['读不出 ui/events.js 里的 TABS —— 真源没了，这条门禁失效'], pages: 0 };
  if (!Object.keys(labels).length) {
    return { problems: ['读不出 ui/index.html 里的 data-tab —— 真源没了'], pages: 0 };
  }
  if (!docNames || !docNames.length) {
    return {
      problems: ['读不出《使用说明.md》第三节的页面表 —— 判据失效，不许当成"没什么可查"而通过'],
      pages: 0,
    };
  }

  // ⓪ 标题声明的页数，必须等于它自己表里的行数，也等于代码里的页数
  if (doc.declared === null) {
    problems.push('《使用说明.md》第三节的标题里读不出页数（形如「界面上的七个页面」）——判据失效');
  } else {
    if (doc.declared !== docNames.length) {
      problems.push(`标题写「${doc.declared} 个页面」，而它自己的表里有 ${docNames.length} 行——自相矛盾`);
    }
    if (doc.declared !== tabs.length) {
      problems.push(`标题写「${doc.declared} 个页面」，而代码里有 ${tabs.length} 页`);
    }
  }

  // ① 两处代码必须一致（`TABS` vs 导航按钮）
  for (const t of tabs) {
    if (!(t in labels)) problems.push(`TABS 里有 ${t}，但 index.html 里没有它的导航按钮——用户点不到那一页`);
  }
  for (const k of Object.keys(labels)) {
    if (!tabs.includes(k)) problems.push(`index.html 有 ${k} 的按钮，但 TABS 里没有——切过去会认不出`);
  }

  // ② 说明书里不许凭空多一个页签
  const labelSet = new Set(Object.values(labels));
  for (const n of docNames) {
    if (!labelSet.has(n)) problems.push(`说明书里写了「${n}」，但界面上没有这一页`);
  }

  // ③ TABS 里每一个都要在说明书里出现过（**漏讲**是这次真正发生的错）
  for (const k of tabs) {
    if (labels[k] && !docNames.includes(labels[k])) {
      problems.push(
        `界面上有「${labels[k]}」这一页（TABS 里的 ${k}），而《使用说明.md》第三节**一个字都没提**——`
        + `用户不知道它存在`,
      );
    }
  }
  // ④ README 那一份（同一套判据：页数对得上、每页都点到）
  const readmePath = join(root, "README.md");
  if (!existsSync(readmePath)) {
    problems.push("README.md 不见了——判据失效");
  } else {
    const rd = tabsFromReadme(readFileSync(readmePath, "utf8"));
    if (!rd) {
      problems.push("读不出 README 里「界面上的 N 个页面」那一节——判据失效，不许当成没什么可查而通过");
    } else {
      if (rd.declared === null) {
        problems.push("README 的标题里读不出页数（形如「界面上的七个页面」）——判据失效");
      } else if (rd.declared !== tabs.length) {
        problems.push(`README 标题写「${rd.declared} 个页面」，而代码里有 ${tabs.length} 页`);
      }
      for (const k of tabs) {
        const lbl = labels[k];
        if (lbl && !rd.names.includes(lbl)) {
          problems.push(`界面上有「${lbl}」这一页，而 README 的页面那段一个字都没提——门面里没有它`);
        }
      }
    }
  }

  return { problems, pages: tabs.length };
}

function main() {
  const { problems, pages } = checkUiDoc();
  if (problems.length) {
    console.log('页面清单对不上：');
    for (const p of problems) console.log('   × ' + p);
    process.exit(1);
  }
  console.log(`页面清单一致：代码里 ${pages} 页，说明书逐页都讲了`);
  process.exit(0);
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) main();
