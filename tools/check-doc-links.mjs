// 检查文档里的 **markdown 链接能不能点开**。只查这一件，刻意不做别的。
//
// ## 为什么只有这一条（2026-10-16，一个做了又收回去的判据）
//
// 第 22 轮抓到「`使用说明.md` 承诺了一行界面上不存在的字」。当时的想法是把它做成通用门禁：
// 扫描文档里的路径引用，看它们在不在仓库里。**做了三轮，三次负向测试都没红**：
//   1. 只认 `「」` 括起来的 → 太窄，只捞到 2 段；
//   2. 认反引号里含中文的 → 抓到的是跨行配对出来的垃圾；
//   3. 认"路径片段是不是某条真实路径的后缀" → **几乎什么都命中**
//      （`旧文档/总稿.md` 撞上根目录那份 `总稿.md`）。
// 而"简写"本身是合理的：`graph/query.rs`、`src/main.rs`、`hello_python/requirements.txt`
// 在自然语言里都读得懂，真实位置在几层目录下。为了它们去枚举源码根，
// **复杂度远超收益**，而且每加一层就多一批误报。
//
// **一条几乎什么都放过的判据，比没有判据更坏**：它看起来在守。
// 所以收回到唯一**确定、可判定、而且真的会坏**的那一条：
//
//   **markdown 链接**：从仓库根解析，存在就是存在。点开 404 没有解释余地。
//
// 它抓不到"承诺了不存在的界面"那类语义问题——**那类只能靠人读**（第 22 轮就是这么发现的）。
// 这条检查的作用是保证"文档里的链接不会烂"，仅此而已，注释里也不冒充更多。

import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = join(HERE, '..');

/// 扫哪几份文档的链接。
///
/// **自动发现根目录所有 `.md`，不写死清单**（2026-10-17 改）。
/// 原来是一个 5 项常量，而根目录有 11 份——**清单写死必然漂移**，
/// 这个仓库为此付过好几次账（跳过目录清单写了三遍、项目标记判据写了两份）。
/// 多一份文档就多一份覆盖，不需要谁记得改这里。
///
/// 上一轮我改过一次、失败了：拼接脚本没替换到 import 行，
/// `readdirSync` 不在作用域 → 抛异常 → 走进 `catch` 退回 `['README.md']`，
/// 门禁报「只读到 1 份文档……判据失效」。**先看 import 再改，是那一轮的教训。**
const DOCS = (() => {
  try {
    const all = readdirSync(ROOT_DIR).filter((f) => f.endsWith('.md')).sort();
    // 至少要有一份，否则说明根目录读错了——让判据自己报"失效"，别静默通过
    return all.length > 0 ? all : ['README.md'];
  } catch {
    return ['README.md'];
  }
})();

export function checkDocLinks(root, docs = DOCS) {
  const problems = [];
  let checked = 0;
  let scanned = 0;
  for (const doc of docs) {
    let text;
    try {
      text = readFileSync(join(root, doc), 'utf8');
    } catch {
      continue; // 某份文档不在（比如裁剪过的发行版）不算错
    }
    scanned += 1;
    for (const m of text.matchAll(/\[[^\]]*\]\(([^)#\s]+)\)/g)) {
      const p = m[1];
      if (/^[a-z]+:/i.test(p)) continue; // 外链
      if (p.startsWith('#')) continue; // 页内锚点
      checked += 1;
      if (!existsSync(join(root, p))) {
        problems.push(`${doc} 的 markdown 链接指向「${p}」——**点开会 404**`);
      }
    }
  }

  // **下限：不许"什么都没查"还说一致。**
  //
  // 2026-10-16 外部对照复核指出：`checkDocLinks(root, [])` 会返回
  // `{problems: [], checked: 0}`，而 `main()` 照样打印"0 条本地链接都点得开"并 exit 0。
  // 再叠上上面那个 `catch { continue }`（文档不在就跳过、不算错），
  // 只要这 5 个文件名对不上（改名 / 移走 / 清单写错），**这道判据就永久绿**。
  //
  // 现在两条都查：至少要读到 2 份文档、至少要数出 5 条链接。
  // 数不够就报"判据失效"，而**不是**"没什么可查所以通过"——
  // 这与 facts 门禁那条"一份文档一个标记都没有就报错"是同一个道理。
  const MIN_DOCS = 2;
  const MIN_LINKS = 5;
  if (scanned < MIN_DOCS) {
    problems.push(
      `只读到 ${scanned} 份文档（清单里是 ${docs.length} 份，至少要 ${MIN_DOCS} 份）——`
      + `文件名对不上？判据失效，不许当成"没什么可查"而通过`,
    );
  }
  if (checked < MIN_LINKS) {
    problems.push(
      `只数出 ${checked} 条本地链接（至少要 ${MIN_LINKS} 条）——`
      + `判据失效。**"0 条链接都点得开"是空话，不是通过。**`,
    );
  }
  return { problems, checked, scanned };
}

function main() {
  const { problems, checked, scanned } = checkDocLinks(join(HERE, '..'));
  if (problems.length) {
    console.log('文档链接对不上：');
    for (const p of problems) console.log('   × ' + p);
    process.exit(1);
  }
  console.log(`文档链接一致：扫了 ${scanned} 份文档，${checked} 条本地链接都点得开`);
  process.exit(0);
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) main();
