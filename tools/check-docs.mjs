// 文档自洽门禁：**声明 N 项的地方，枚举必须真的是 N 条**。只比较，不修改。
//
// 为什么要有这一道：这个项目的账本上记着四次"决定记在文档上、门禁上什么都没发生"
// （总稿 §8.3、AGENTS.md §1）。而 2026-10-15 晚上又当场抓到四处同一形态的错，
// 全在《总稿》里：标题写「engine 的六层」枚举却是 7 条（L0–L6）·「五张表」枚举却是 6 张 ·
//「排查链：八层」枚举却是 9 条 ·「代码约 5.7 万行」而它自己列的分项加总只有 3.4 万。
// 修的人当场又犯了两次同类错——所以"下次注意"不是办法，这条判据必须变成机器能跑的东西。
//
//   真源  tools/docs-claims.json   一条声明一行记录：doc / heading / declared / enum / unit
//   比较  本脚本（也被 gate.mjs 调用，判断只有一份实现）
//
// 五种情形一律报错（一种都不许省，尤其后三种）：
//   1. 声明的数字 ≠ 枚举数出来的条数 → 打印 文件、节标题、声明 X、实际 Y、判据
//   2. 声明的原文（比如「九层」）在指定位置找不到 → 节标题被改过，或声明位置写错
//   3. 节标题匹配不到、或匹配到多处 → 文档被改坏，或者 heading 写得不唯一
//   4. 那一节里根本没有可数的枚举（没有代码块 / 没有表格 / 那一行没了）→ 判据失效，
//      **不许当成"没什么可查"而通过**（这一条和 facts 门禁的第 2 条同一个理由）
//   5. 一条 claim 都没有、或者规则写法不认识 → 门禁查不出任何东西，必须报错
//
// 跑法：node tools/check-docs.mjs                （逐条比较 + 自测）
//       node tools/check-docs.mjs --selftest     （只跑自测：用临时小文档验证这道门禁**能失败**）
//       node tools/check-docs.mjs --root <目录>   （换一个根目录跑，自测也用它）

import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const HERE = fileURLToPath(import.meta.url);
const ROOT = fileURLToPath(new URL('..', import.meta.url));
const CLAIMS_IN = join('tools', 'docs-claims.json');

/* ------------------------------------------------------------------ *
 * 一、声明里的数字怎么读：中文数字与阿拉伯数字都认
 * ------------------------------------------------------------------ */

const CJK_DIGITS = { 零: 0, 〇: 0, 一: 1, 二: 2, 两: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9 };
const CJK_UNITS = { 十: 10, 百: 100, 千: 1000 };
const MULTIPLIERS = [['亿', 1e8], ['万', 1e4], ['千', 1e3], ['百', 1e2]];

/// 「七」→7 ·「十三」→13 ·「二十三」→23。认不出来就返回 null——
/// 认不出来必须报错，不许当成 0 悄悄放过。
function cjkToNumber(s) {
  let total = 0;
  let section = 0;
  let seen = false;
  for (const ch of s) {
    if (ch in CJK_DIGITS) {
      section = CJK_DIGITS[ch];
      seen = true;
    } else if (ch in CJK_UNITS) {
      total += (section || 1) * CJK_UNITS[ch];
      section = 0;
      seen = true;
    } else {
      return null;
    }
  }
  return seen ? total + section : null;
}

/// 「七层」→ 7 ·「六张表」→ 6 ·「13 实体」→ 13 ·「3.5 万」→ 35000。
/// 量词（万 / 千 / 百）只在阿拉伯数字写法上生效——中文数字里「百」已经算进去了，
/// 再乘一次就是重复计数。
export function parseDeclared(text) {
  if (typeof text !== 'string' || text.trim() === '') return null;
  const s = text.trim();
  const arabic = s.match(/\d+(?:\.\d+)?/);
  if (arabic) {
    let value = Number(arabic[0]);
    for (const [ch, mult] of MULTIPLIERS) {
      if (s.includes(ch)) {
        value *= mult;
        break;
      }
    }
    return Number.isFinite(value) ? value : null;
  }
  const cjk = s.match(/[零〇一二三四五六七八九十百千两]+/);
  if (!cjk) return null;
  const value = cjkToNumber(cjk[0]);
  return value === null ? null : value;
}

/* ------------------------------------------------------------------ *
 * 二、把一节从文档里切出来（节 = 标题行 → 下一个同级或更高级的标题）
 * ------------------------------------------------------------------ */

function sectionRange(lines, headingNeedle) {
  const hits = [];
  for (let i = 0; i < lines.length; i++) {
    if (/^#{1,6}\s/.test(lines[i]) && lines[i].includes(headingNeedle)) hits.push(i);
  }
  if (hits.length === 0) {
    return { error: `节标题「${headingNeedle}」在这种文档里找不到——标题被改过，还是 heading 写错了？` };
  }
  if (hits.length > 1) {
    return {
      error: `节标题「${headingNeedle}」匹配到 ${hits.length} 处（第 ${hits.map((i) => i + 1).join(' / ')} 行）——heading 要写成唯一能定位一节的那一段`,
    };
  }
  const start = hits[0];
  const level = lines[start].match(/^#+/)[0].length;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    const m = lines[i].match(/^(#+)\s/);
    if (m && m[1].length <= level) {
      end = i;
      break;
    }
  }
  return { start, end };
}

const FENCE = /^\s*```/;
const TABLE_ROW = /^\s*\|/;
const TABLE_SEP = /^\s*\|[\s|:-]+\|\s*$/;

/// 节内所有 ``` 围起来的块（不含围栏行本身）。
function fencedBlocks(lines, from, to) {
  const blocks = [];
  let cur = null;
  for (let i = from; i < to; i++) {
    if (FENCE.test(lines[i])) {
      if (cur === null) cur = [];
      else {
        blocks.push(cur);
        cur = null;
      }
    } else if (cur !== null) {
      cur.push(lines[i]);
    }
  }
  return { blocks, unclosed: cur !== null };
}

/// 节内所有 Markdown 表格，只返回**数据行**（去掉表头与 `|---|---|` 分隔行）。
function tablesIn(lines, from, to) {
  const tables = [];
  let cur = null;
  for (let i = from; i < to; i++) {
    if (TABLE_ROW.test(lines[i])) {
      if (cur === null) cur = [];
      cur.push(lines[i]);
    } else if (cur) {
      tables.push(cur);
      cur = null;
    }
  }
  if (cur) tables.push(cur);
  return tables.map((rows) => rows.filter((r, idx) => idx > 0 && !TABLE_SEP.test(r)));
}

/// 节内的段落（空行分段），只看标题行以后的部分。
function paragraphsIn(lines, from, to) {
  const out = [];
  let cur = null;
  for (let i = from; i < to; i++) {
    if (lines[i].trim() === '') {
      if (cur) {
        out.push(cur);
        cur = null;
      }
    } else {
      if (cur === null) cur = [];
      cur.push(lines[i]);
    }
  }
  if (cur) out.push(cur);
  return out;
}

/* ------------------------------------------------------------------ *
 * 三、按 enum 规则数出实际条数
 * ------------------------------------------------------------------ */

const RE = (pattern) => new RegExp(pattern, 'gm');

/// 返回 { text, rule } 或 { error }：text 是判据作用的范围，rule 是给人看的判据说明。
function scopeOf(claim, lines, range) {
  const e = claim.enum;
  const from = range.start + 1;
  const to = range.end;
  switch (e.region) {
    case 'heading':
      return { text: lines[range.start], rule: '节标题那一行' };
    case 'fence': {
      const { blocks, unclosed } = fencedBlocks(lines, from, to);
      const idx = (e.block || 1) - 1;
      if (blocks.length <= idx) {
        return {
          error: `这一节里数不到第 ${idx + 1} 个代码块（${unclosed ? '或代码块没闭合' : `只有 ${blocks.length} 个`}）——判据失效，不是"没什么可查"`,
        };
      }
      return { text: blocks[idx].join('\n'), rule: `节内第 ${idx + 1} 个代码块` };
    }
    case 'table': {
      const tables = tablesIn(lines, from, to);
      const idx = (e.table || 1) - 1;
      if (tables.length <= idx) {
        return { error: `这一节里数不到第 ${idx + 1} 张表（只有 ${tables.length} 张）——判据失效，不是"没什么可查"` };
      }
      return { text: tables[idx].join('\n'), rule: `节内第 ${idx + 1} 张表的数据行` };
    }
    case 'paragraph': {
      const paras = paragraphsIn(lines, from, to);
      const pi = (e.paragraph || 1) - 1;
      if (paras.length <= pi) {
        return { error: `这一节里数不到第 ${pi + 1} 段（只有 ${paras.length} 段）——判据失效，不是"没什么可查"` };
      }
      const li = (e.lineInParagraph || 1) - 1;
      if (paras[pi].length <= li) {
        return { error: `第 ${pi + 1} 段只有 ${paras[pi].length} 行，取不到第 ${li + 1} 行` };
      }
      return { text: paras[pi][li], rule: `节内第 ${pi + 1} 段的第 ${li + 1} 行` };
    }
    case 'line': {
      const re = new RegExp(e.linePattern, 'm');
      for (let i = from; i < to; i++) {
        if (re.test(lines[i])) {
          return { text: lines[i], rule: `节内第一行匹配 /${e.linePattern}/ 的那一行` };
        }
      }
      return { error: `这一节里找不到匹配 /${e.linePattern}/ 的那一行——判据失效，不是"没什么可查"` };
    }
    case 'section':
      return { text: lines.slice(range.start, to).join('\n'), rule: '整节' };
    default:
      return { error: `不认识的 enum.region：${e.region}（只认 heading / fence / table / paragraph / line / section）` };
  }
}

function countOf(claim, scope) {
  const e = claim.enum;
  const matches = [...scope.text.matchAll(RE(e.pattern))];
  if (e.mode === 'sum') {
    let total = 0;
    for (const m of matches) {
      const raw = (m[1] !== undefined ? m[1] : m[0]).replace(/,/g, '');
      const n = Number(raw);
      if (!Number.isFinite(n)) return { error: `匹配到「${m[0]}」但不是数字，加总不了` };
      total += n;
    }
    return { actual: total * (e.multiplier || 1), matches: matches.length, rule: `${scope.rule}里匹配 /${e.pattern}/ 的数字加总` };
  }
  return { actual: matches.length, matches: matches.length, rule: `${scope.rule}里匹配 /${e.pattern}/ 的条数` };
}

/* ------------------------------------------------------------------ *
 * 四、逐条比较
 * ------------------------------------------------------------------ */

const titleOf = (line) => line.replace(/^#+\s*/, '').trim();

/// 纯函数：返回 { problems, claims, checked, open }，不打印、不退出——
/// 这样 gate.mjs 能直接拿去用，判断不会出现第二份。
export function checkDocs(root = ROOT) {
  const problems = [];
  const entries = [];
  let doc;
  try {
    doc = JSON.parse(readFileSync(join(root, CLAIMS_IN), 'utf8'));
  } catch (e) {
    return { problems: [`读不了 ${CLAIMS_IN}：${e.message}`], claims: 0, checked: 0 };
  }
  const claims = doc.claims;
  if (!Array.isArray(claims)) {
    return { problems: [`${CLAIMS_IN} 里没有 claims 数组——真源格式不对`], claims: 0, checked: 0 };
  }
  if (claims.length === 0) {
    return { problems: [`${CLAIMS_IN} 里一条 claim 都没有：这道门禁查不出任何东西，必须报错而不是当通过`], claims: 0, checked: 0 };
  }

  const seenIds = new Set();
  const cache = new Map();
  const textOf = (name) => {
    if (!cache.has(name)) {
      const p = join(root, name);
      try {
        cache.set(name, readFileSync(p, 'utf8'));
      } catch {
        cache.set(name, null);
      }
    }
    return cache.get(name);
  };

  let checked = 0;
  for (const claim of claims) {
    const id = claim.id || '(没有 id)';
    if (seenIds.has(id)) problems.push(`claim id 重复：${id}`);
    seenIds.add(id);
    if (!claim.doc || !claim.heading || !claim.enum || !claim.unit) {
      problems.push(`claim「${id}」缺字段：doc / heading / enum / unit 都是必填的`);
      continue;
    }
    if (claim.doc.includes('..') || /^([A-Za-z]:|[\\/])/.test(claim.doc)) {
      problems.push(`claim「${id}」的 doc 只能写仓库内的相对路径：${claim.doc}`);
      continue;
    }

    const text = textOf(claim.doc);
    if (text === null) {
      problems.push(`${claim.doc} 读不到（claim「${id}」）——文档被改名或删了？不许当通过`);
      continue;
    }
    const lines = text.split('\n');
    const range = sectionRange(lines, claim.heading);
    if (range.error) {
      problems.push(`${claim.doc}：${range.error}（claim「${id}」）`);
      continue;
    }
    const title = titleOf(lines[range.start]);

    // 声明的数字**从文档里读**：这是"声明"二字的唯一来源。
    // 读不到 = 声明被改过或 declaredPattern 写错了，必须报错（不许当通过）。
    const whereIsHeading = claim.declaredWhere !== 'section';
    const where = whereIsHeading ? lines[range.start] : lines.slice(range.start, range.end).join('\n');
    const whereName = whereIsHeading ? '节标题' : '这一节';
    const dm = typeof claim.declaredPattern === 'string' ? where.match(new RegExp(claim.declaredPattern, 'm')) : null;
    if (!dm) {
      problems.push(
        `${claim.doc} §「${title}」${whereName}里找不到声明：/${claim.declaredPattern}/ 匹配不到（claim「${id}」）——声明被改过，或者 declaredPattern 写错了`,
      );
      continue;
    }
    const raw = dm[1] !== undefined ? dm[1] : dm[0];
    const declaredDoc = parseDeclared(raw);
    if (declaredDoc === null) {
      problems.push(`${claim.doc} §「${title}」读到的声明原文「${raw}」认不出数字（claim「${id}」）——中文数字与阿拉伯数字都认，别的写法要改写`);
      continue;
    }

    const scope = scopeOf(claim, lines, range);
    if (scope.error) {
      problems.push(`${claim.doc} §「${title}」：${scope.error}（claim「${id}」）`);
      continue;
    }
    const counted = countOf(claim, scope);
    if (counted.error) {
      problems.push(`${claim.doc} §「${title}」：${counted.error}（claim「${id}」）`);
      continue;
    }

    checked += 1;
    const unit = claim.unit;
    const tol = claim.tolerance || 0;
    const diff = Math.abs(counted.actual - declaredDoc);
    const gap = counted.actual === 0 ? '' : `（差 ${((diff / Math.abs(counted.actual)) * 100).toFixed(1)}%${tol > 0 ? `，容差 ${(tol * 100).toFixed(0)}%` : ''}）`;
    const stale = declaredDoc !== claim.declared
      ? `；另外 tools/docs-claims.json 里记的是 ${claim.declared} ${unit}，三处要对得上`
      : '';
    const within = declaredDoc === counted.actual || (tol > 0 && diff <= tol * Math.abs(counted.actual));
    entries.push({ id, doc: claim.doc, title, declaredDoc, actual: counted.actual, unit, rule: counted.rule, ok: within && !stale });
    if (!within) {
      problems.push(
        `${claim.doc} §「${title}」声明 ${declaredDoc} ${unit}（原文「${raw}」），实际 ${counted.actual} ${unit}${gap} —— 判据：${counted.rule}${stale}`,
      );
    } else if (stale) {
      problems.push(
        `${claim.doc} §「${title}」文档里写的声明是 ${declaredDoc} ${unit}（原文「${raw}」），tools/docs-claims.json 里记的是 ${claim.declared} ${unit} —— 改了声明就要一起改真源（枚举数出来是 ${counted.actual} ${unit}）`,
      );
    }
  }
  return { problems, claims: claims.length, checked, entries };
}

/* ------------------------------------------------------------------ *
 * 五、自测：用临时小文档证明「一致→过、不一致→失败、找不到→报错」
 * ------------------------------------------------------------------ */

/// 自测用的两份小文档：一份声明与枚举对得上，一份对不上（只有标题上的声明不同，
/// 枚举一字不动——这正是真文档里出事的方式：枚举是事实，声明会写错）。
const DOC_OK = [
  '# 自测文档',
  '',
  '## 1.1 三条，说得对',
  '',
  '```',
  '① 甲',
  '② 乙',
  '③ 丙',
  '```',
  '',
  '## 1.2 两项，表里数得清',
  '',
  '| 项 | 数 |',
  '|---|---|',
  '| a | 1 |',
  '| b | 2 |',
  '',
].join('\n');

const DOC_BAD = DOC_OK
  .replace('## 1.1 三条，说得对', '## 1.1 四条，标题说错了')
  .replace('## 1.2 两项，表里数得清', '## 1.2 三项，标题也说错了');

const TABLE_ENUM = { region: 'table', table: 1, pattern: '^\\| ', mode: 'count' };

function claim(over = {}) {
  return {
    id: over.id || 'selftest',
    doc: 'docs/selftest.md',
    heading: '## 1.1 三条，说得对',
    declared: 3,
    declaredPattern: '([零〇一二三四五六七八九十两]+条)',
    declaredWhere: 'heading',
    unit: '条',
    enum: over.enum || { region: 'fence', pattern: '^[①②③④⑤⑥⑦⑧⑨⑩]', mode: 'count' },
    ...over,
  };
}

/// 每个场景一个临时根目录（自己的 doc + 自己的 docs-claims.json），跑完就删。
function scenario(dir, name, docText, claims) {
  const root = join(dir, name);
  mkdirSync(join(root, 'docs'), { recursive: true });
  mkdirSync(join(root, 'tools'), { recursive: true });
  if (docText !== null) writeFileSync(join(root, 'docs', 'selftest.md'), docText, 'utf8');
  writeFileSync(join(root, CLAIMS_IN), JSON.stringify({ claims }, null, 2), 'utf8');
  return checkDocs(root);
}

/// 返回 { cases, problems }：problems 是"自测本身没通过"的理由，空数组才算自测过。
export function selftest() {
  const dir = mkdtempSync(join(tmpdir(), 'jingwei-docs-selftest-'));
  const problems = [];
  let cases = 0;

  const expect = (label, r, want, patterns) => {
    cases += 1;
    if (want === 'pass' && r.problems.length !== 0) {
      problems.push(`自测「${label}」本该通过，却报了 ${r.problems.length} 条：${r.problems[0]}`);
      return;
    }
    if (want === 'fail') {
      if (r.problems.length === 0) {
        problems.push(`自测「${label}」本该失败，却通过了——这道门禁在"总是通过"，等于没有`);
        return;
      }
      for (const pat of patterns || []) {
        if (!r.problems.some((p) => pat.test(p))) {
          problems.push(`自测「${label}」的报错里没出现 ${pat}：${r.problems.join(' | ')}`);
        }
      }
    }
  };

  const r1 = scenario(dir, 'ok', DOC_OK, [claim()]);
  expect('数量与枚举一致（数代码块）→ 通过', r1, 'pass');
  if (r1.checked !== 1) problems.push(`自测「一致」场景里 checked 应该是 1，实际 ${r1.checked}`);

  const r2 = scenario(dir, 'ok-table', DOC_OK, [
    claim({ id: 'ok-table', heading: '## 1.2 两项，表里数得清', declared: 2, declaredPattern: '([零〇一二三四五六七八九十两]+项)', enum: TABLE_ENUM }),
  ]);
  expect('数量与枚举一致（数表格）→ 通过', r2, 'pass');

  const r3 = scenario(dir, 'bad-fence', DOC_BAD, [
    claim({ id: 'bad-fence', heading: '## 1.1 四条，标题说错了', declared: 4 }),
  ]);
  expect('数量与枚举不一致（数代码块）→ 失败，并给出声明 / 实际 / 判据', r3, 'fail', [
    /声明 4 条/,
    /实际 3 条/,
    /判据：/,
    /docs\/selftest\.md/,
  ]);

  const r4 = scenario(dir, 'bad-table', DOC_BAD, [
    claim({ id: 'bad-table', heading: '## 1.2 三项，标题也说错了', declared: 3, declaredPattern: '([零〇一二三四五六七八九十两]+项)', enum: TABLE_ENUM }),
  ]);
  expect('数量与枚举不一致（数表格）→ 失败', r4, 'fail', [/声明 3 条/, /实际 2 条/, /判据：/]);

  const r4b = scenario(dir, 'stale-record', DOC_OK, [claim({ id: 'stale-record', declared: 4 })]);
  expect('声明与枚举对得上、但真源记的数不一样 → 报错', r4b, 'fail', [/文档里写的声明是 3 条/, /记的是 4 条/]);

  const r5 = scenario(dir, 'bad-heading', DOC_OK, [claim({ id: 'bad-heading', heading: '## 9.9 这一节不存在' })]);
  expect('节标题找不到 → 报错，不许当通过', r5, 'fail', [/找不到/]);

  const r6 = scenario(dir, 'bad-declared', DOC_OK, [claim({ id: 'bad-declared', declaredPattern: '([零〇一二三四五六七八九十两]+张表)' })]);
  expect('声明在文档里找不到 → 报错，不许当通过', r6, 'fail', [/找不到声明/]);

  const r7 = scenario(dir, 'no-file', DOC_OK, [claim({ id: 'no-file', doc: 'docs/never-written.md' })]);
  expect('doc 文件不存在 → 报错', r7, 'fail', [/读不到/]);

  const r8 = scenario(dir, 'no-enum', DOC_OK, [claim({ id: 'no-enum', enum: { region: 'fence', block: 3, pattern: '^x', mode: 'count' } })]);
  expect('节里数不到那个枚举 → 报错，不许当通过', r8, 'fail', [/数不到/]);

  const r9 = scenario(dir, 'empty', DOC_OK, []);
  expect('claims 一条都没有 → 报错', r9, 'fail', [/一条 claim 都没有/]);

  rmSync(dir, { recursive: true, force: true });
  return { cases, problems };
}

/* ------------------------------------------------------------------ *
 * 六、命令行
 * ------------------------------------------------------------------ */

function argValue(flag) {
  const i = process.argv.indexOf(flag);
  return i >= 0 ? process.argv[i + 1] : null;
}

if (resolve(process.argv[1] || '') === resolve(HERE)) {
  const root = argValue('--root') ? resolve(argValue('--root')) : ROOT;
  const onlySelf = process.argv.includes('--selftest');
  let bad = 0;

  const st = selftest();
  if (st.problems.length) {
    console.log(`自测没过（${st.cases} 例）：`);
    for (const p of st.problems) console.log('   × ' + p);
    bad += st.problems.length;
  } else if (!process.argv.includes('--quiet')) {
    console.log(`自测通过：${st.cases} 例——一致→过、数量不一致 / 节标题找不到 / 声明原文找不到 / 文件不存在 / 枚举数不到 → 一律报错。`);
  }

  if (!onlySelf) {
    if (!existsSync(join(root, CLAIMS_IN))) {
      console.log(`读不到 ${join(root, CLAIMS_IN)}`);
      process.exit(2);
    }
    const r = checkDocs(root);
    if (r.problems.length) {
      console.log(`文档自洽门禁未通过（${r.claims} 条 claim 里 ${r.problems.length} 条对不上）：`);
      for (const p of r.problems) console.log('   × ' + p);
      bad += r.problems.length;
    } else {
      console.log(`文档自洽：${r.claims} 条 claim 逐个等于枚举数出来的条数（比较了 ${r.checked} 条）。`);
    }
    if (process.argv.includes('--list')) {
      for (const e of r.entries) {
        console.log(`   ${e.ok ? '·' : '×'} ${e.doc} §「${e.title}」声明 ${e.declaredDoc} ${e.unit}，数出来 ${e.actual} ${e.unit}（${e.id}）`);
      }
    }
  }

  if (bad) process.exit(1);
}
