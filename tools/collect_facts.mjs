// 刷新 tools/facts.json：**现场采集**文档里那些数字的真值。
//
// 为什么要有它：文档里的每个数字都得能在仓库里跑出来（§11 工程规矩），
// 而"跑出来"这件事不能靠人抄——抄一次就会有一处漂。这个脚本当那个唯一的抄写员：
//
//   cargo test --workspace --all-targets   → engine 单元 / 集成 / model / 合计
//   node tools/uitest.mjs                  → 外壳测试条数
//   tools/gates.json                       → 门禁道数
//   cargo run --release -p jingwei-engine --example export_graph -- . <临时文件>
//                                          → 图谱四个数（拿仓库自己当输入，现跑）
//   tools/motives.json                     → 动机落地 / 挂账
//
// 图谱那四个数**是把整个工作区都算进去的**（索引器走目录，不管有没有提交），
// 所以工作区里有未跟踪的 `.rs` 时，它们会跟着变——这种情况脚本会明说一句，
// 免得下一次有人以为"数字怎么自己漂了"。
//
// **不绿就不写**：有测试没过、外壳测试没跑通、图谱采不到，一律非零退出，
// 并且**不动 facts.json**。不绿的跑不许当成真值——否则下一步就是
// "改文档去迁就一个坏掉的跑"，那正好是这道门禁要防的事。
//
// 跑法：  node tools/collect_facts.mjs              采集并覆盖 tools/facts.json
//         node tools/collect_facts.mjs --self-test  只跑解析器自检（不起子进程）
//
// 为什么解析要单独拆出来、还带自检：解析器读的是**别人打出来的字**
// （cargo 的 `test result:` 行、外壳测试的 `N/M 通过`），那些字一改，解析就会悄悄少算。
// 拆成纯函数才能拿真实输出喂它——`--self-test` 里那几段样本就是从真跑一遍的输出里抄下来的。

import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(import.meta.url);
const ROOT = fileURLToPath(new URL('..', import.meta.url));
const FACTS = join(ROOT, 'tools', 'facts.json');
const DEFAULT_NOTE = '文档数字的唯一真源。由 tools/collect_facts.mjs 现场采集；gate.mjs 用它校验文档。';

// ── 解析器（纯函数：喂一段文本，吐几个数）────────────────────────────────

/// `cargo test --workspace --all-targets` 的输出 → 各目标的条数。
///
/// 归类不靠猜包名：cargo 每跑一个目标先打一行
/// `Running <kind> <路径> (<可执行文件>)`，可执行文件带 crate 名
/// （`jingwei_engine-<hash>.exe` / `jingwei_model-<hash>.exe`），
/// 路径在 `tests\` 下的就是集成测试。以后布局变了会数不上——
/// **数不上就报错**，让数字停下来等人看，而不是悄悄变。
export function parseCargo(out) {
  const got = { engine_unit: 0, engine_ignored: 0, integration: 0, model: 0, total: 0 };
  const seen = { engine: false, integration: false, model: false };
  const failed = [];

  // ── 为什么不能"看到 Running 就记目标、看到 result 就归给它" ──────────────
  //
  // 第一版就是那么写的，而它在真实输出上**一条都数不到**。原因不在正则，在**顺序**：
  // cargo 把 `Running …` 打在 **stderr**、`test result:` 打在 **stdout**，
  // 两个流拼成一个字符串之后，所有结果行在前、所有 `Running` 行在后
  // （实测：结果行在第 3/8/236… 行，`Running` 全在第 304–310 行）。
  // 流式写法于是在每条 result 上撞见"还不知道属于谁"，全部丢掉。
  //
  // 而结果行本身**不带可执行文件名**，所以也没法"按名字归位"。
  // 稳定可用的关系只有一条：**同一个目标内部，`running N tests` 与它那条
  // `test result:` 相邻**（都在 stdout，交替出现）。就按这条配对：
  // 记住最近一次 `running N tests` 的序号，随后第一条 result 就属于它。
  //
  // 教训一句话：**别对两段不同来源的文本做顺序假设。**
  const tails = out.split(/\r?\n/);

  // 目标表：`Running unittests src\lib.rs (…exe)` / `Running tests\index_e2e.rs (…exe)`
  // （后一种没有 kind，所以 kind 是可选的——少认一种形状就会静默少算。）
  const targets = [];
  for (const line of tails) {
    const head = line.match(/^\s*Running\s+(.*?)\s+\((.*)\)\s*$/);
    if (!head) continue;
    const parts = head[1].trim().split(/\s+/);
    const kind = /^(unittests|doc-tests|integration-test|benchmark)$/.test(parts[0]) ? parts.shift() : '';
    targets.push({ kind, path: parts.join(' '), exe: head[2] });
  }

  const runningOrder = tails
    .map((l, i) => [i, l])
    .filter(([, l]) => /^running \d+ tests?$/.test(l.trim()))
    .map(([i]) => i);
  const results = tails
    .map((l, i) => [i, l])
    .filter(([, l]) => /^test result: (ok|FAILED)\./.test(l));

  for (const [lineNo, line] of results) {
    const res = line.match(/^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;/);
    if (!res) continue;
    const passed = Number(res[2]);
    const failedHere = Number(res[3]);
    const ignored = Number(res[4]);
    got.total += passed;

    // 这条结果属于哪个目标：取它前面最近的那个 `running N tests`
    const before = runningOrder.filter((n) => n < lineNo).pop();
    if (before === undefined) {
      failed.push('有一条 test result 前面找不到 running N tests——统计口径变了，得回来看一眼');
      continue;
    }
    // 目标声明（`Running …`）里带可执行文件名，按"第几个目标"与"第几条 running"对齐
    const idx = runningOrder.indexOf(before);
    const t = targets[idx];
    if (!t) {
      failed.push(`第 ${idx + 1} 个测试目标在 Running 行里找不到——统计口径变了，得回来看一眼`);
      continue;
    }
    if (failedHere > 0) {
      // 没过的那几条，名字要留在 failed 里——报告里只说"有 1 条没过"没用，
      // 得说清是哪一条（这条是自检抓出来的：重写解析器时我把它弄丢了）。
      for (const [n, l] of tails.map((l, i) => [i, l])) {
        if (n <= lineNo) continue;
        const name = l.match(/^\s+([A-Za-z0-9_]+(?:::?[A-Za-z0-9_]+)+)\s*$/);
        if (name && !failed.includes(name[1])) failed.push(name[1]);
        if (n > lineNo + 40) break; // 失败清单就在结果行下面一小段里，别扫全文
      }
      if (!failed.some((f) => f.includes(t.exe))) failed.push(`${t.exe} 有 ${failedHere} 条没过`);
    }
    const exe = (t.exe.split(/[\\/]/).pop() || '').trim();
    if (t.kind === 'integration-test' || /^tests[\\/]/.test(t.path)) {
      got.integration += passed;
      seen.integration = true;
    } else if (/^jingwei_engine[-.]/.test(exe)) {
      got.engine_unit += passed;
      got.engine_ignored += ignored;
      seen.engine = true;
    } else if (/^jingwei_model[-.]/.test(exe)) {
      got.model += passed;
      seen.model = true;
    }
  }
  return { ...got, failed, seen };
}

/// 外壳测试的输出 → 条数。它自己会打一行 `N/M 通过`。
export function parseUitest(out) {
  const m = out.match(/(\d+)\/(\d+)\s*通过/);
  if (!m) return { ok: null, all: null };
  return { ok: Number(m[1]), all: Number(m[2]) };
}

/// export_graph 的输出 → 图谱四个数。数字是**索引器自己报的**，不是脚本猜的。
export function parseGraph(out) {
  const f = out.match(/索引\s+(\d+)\s+个文件\s+\/\s+(\d+)\s+个符号/);
  const g = out.match(/图谱：(\d+)\s+个节点\s+\/\s+(\d+)\s+条边/);
  if (!f || !g) return null;
  return {
    graph_files: Number(f[1]),
    graph_symbols: Number(f[2]),
    graph_nodes: Number(g[1]),
    graph_edges: Number(g[2]),
  };
}

// ── 采集 ────────────────────────────────────────────────────────────────

const problems = [];
const ran = [];

function run(cmd, args) {
  ran.push([cmd, ...args].join(' '));
  // 用 spawnSync 而不是 execFileSync：**cargo 把 `Running …` 打在 stderr 上**，
  // 而 execFileSync 成功时只交出 stdout。于是采集器在"测试全绿"时永远数不到测试目标，
  // 症状是报"统计口径变了"——一个只在成功路径上坏掉的采集器，等于把真源锁死在手写数字上。
  //
  // 两个流都拿、都拼进去；判定成没成**只看退出码**（cargo 写 stderr 不代表失败）。
  const r = spawnSync(cmd, args, {
    cwd: ROOT,
    encoding: 'utf8',
    maxBuffer: 1 << 28,
    shell: process.platform === 'win32',
  });
  if (r.error) return { code: 1, out: String(r.error.message || r.error) };
  const out = `${r.stdout || ''}${r.stderr || ''}`;
  return { code: r.status ?? 1, out };
}

function collectTests() {
  const r = run('cargo', ['test', '--workspace', '--all-targets', '--no-fail-fast']);
  const got = parseCargo(r.out);
  if (got.failed.length) {
    problems.push(`cargo test 有没过的东西：${got.failed.join('、')}——先把测试跑绿再刷新数字`);
  } else if (r.code !== 0) {
    problems.push(`cargo test --workspace --all-targets 退出码 ${r.code}`);
  }
  for (const [k, ok] of Object.entries(got.seen)) {
    if (!ok) problems.push(`cargo 输出里找不到 ${k} 的测试目标——统计口径变了，得回来看一眼`);
  }
  return got;
}

function collectShellTests() {
  const r = run('node', ['tools/uitest.mjs']);
  const got = parseUitest(r.out);
  if (got.ok === null) {
    problems.push(`外壳测试没打出"N/M 通过"（退出码 ${r.code}）——没跑起来，不许当 0 条`);
    return null;
  }
  if (r.code !== 0 || got.ok !== got.all) {
    problems.push(`外壳测试 ${got.ok}/${got.all} 通过（退出码 ${r.code}）——跑绿了再刷新数字`);
  }
  return got.ok;
}

function collectGates() {
  try {
    const spec = JSON.parse(readFileSync(join(ROOT, 'tools', 'gates.json'), 'utf8'));
    if (!Array.isArray(spec.steps) || spec.steps.length === 0) {
      problems.push('tools/gates.json 里没有 steps');
      return null;
    }
    return spec.steps.length;
  } catch (e) {
    problems.push(`读不了 tools/gates.json：${e.message}`);
    return null;
  }
}

function collectGraph() {
  const out = join(tmpdir(), `jw-facts-${process.pid}.json`);
  try {
    const r = run('cargo', [
      'run',
      '--release',
      '-p',
      'jingwei-engine',
      '--example',
      'export_graph',
      '--',
      '.',
      out,
    ]);
    if (r.code !== 0) {
      problems.push(`图谱导出失败（退出码 ${r.code}）`);
      return null;
    }
    const got = parseGraph(r.out);
    if (!got) {
      problems.push('图谱导出的输出里找不到"索引 N 个文件 / M 个符号"或"图谱：X 个节点 / Y 条边"');
      return null;
    }
    return got;
  } finally {
    rmSync(out, { force: true });
  }
}

function collectMotives() {
  try {
    const doc = JSON.parse(readFileSync(join(ROOT, 'tools', 'motives.json'), 'utf8'));
    let landed = 0;
    let pending = 0;
    for (const m of doc.motives || []) {
      if (m.state === 'landed') landed += 1;
      else if (m.state === 'pending') pending += 1;
      else problems.push(`动机 ${m.id} 的 state 既不是 landed 也不是 pending：${m.state}`);
    }
    return { motives_landed: landed, motives_pending: pending };
  } catch (e) {
    problems.push(`读不了 tools/motives.json：${e.message}`);
    return null;
  }
}

/// `git ls-files --others --exclude-standard '*.rs'` 的输出 → 一句提醒（没有就 null）。
///
/// 图谱四个数是**对工作区**量的：索引器走目录，不管文件提没提交。
/// 不说一声，下次有人刷新完数字会以为数字自己漂了。
export function untrackedRustWarning(gitOut, failed = false) {
  if (failed) return null;
  const files = gitOut.split(/\r?\n/).filter(Boolean);
  if (!files.length) return null;
  const head = files.slice(0, 5).join('、');
  return (
    `注意：工作区里有 ${files.length} 个未跟踪的 .rs（${head}${files.length > 5 ? ' 等' : ''}）` +
    '——图谱那四个数把它们也算进去了；想看"提交后的仓库是什么规模"，先把它们挪开再采集。'
  );
}

function warnUntrackedRust() {
  const r = run('git', ['ls-files', '--others', '--exclude-standard', '*.rs']);
  const msg = untrackedRustWarning(r.out, r.code !== 0);
  if (msg) {
    console.log('');
    console.log(msg);
  }
}

// ── 解析器自检：样本是从真跑一遍的输出里抄下来的 ────────────────────────

function selfTest() {
  const cargoSample = [
    '     Running unittests src\\main.rs (target\\debug\\deps\\jingwei-f1f5a074bb251fcf.exe)',
    'running 0 tests',
    'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s',
    '     Running unittests src\\lib.rs (target\\debug\\deps\\jingwei_engine-8b0317fb6d4612cc.exe)',
    'running 220 tests',
    'test result: FAILED. 218 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.63s',
    '     Running tests\\index_e2e.rs (target\\debug\\deps\\index_e2e-53ad17f144c02b83.exe)',
    'running 12 tests',
    'test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s',
    '     Running unittests src\\lib.rs (target\\debug\\deps\\jingwei_model-2e8db52bf65e4516.exe)',
    'running 16 tests',
    'test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s',
    'failures:',
    '    util::jobobj::tests::terminating_the_job_takes_the_whole_tree_down',
  ].join('\n');
  const graphSample = [
    '索引 61 个文件 / 1257 个符号（0 个解析不完整）· 耗时 333ms',
    '图谱：1319 个节点 / 1923 条边（导出上限 20000，截断=false）',
    '解析：连上 50 条 · 拿不准 0 条 · 外部符号 4595 个',
  ].join('\n');

  const c = parseCargo(cargoSample);
  const u = parseUitest('浏览器：chrome\n\n37/37 通过\n');
  const g = parseGraph(graphSample);
  const checks = [
    ['engine 单元 218（样本里那条没过，所以不是 219）', c.engine_unit === 218],
    ['engine ignore 1', c.engine_ignored === 1],
    ['集成 12', c.integration === 12],
    ['model 16', c.model === 16],
    ['合计 246（218 + 12 + 16；cli 与 app 各 0）', c.total === 246],
    [
      '认出没过的那条测试名',
      c.failed.includes('util::jobobj::tests::terminating_the_job_takes_the_whole_tree_down'),
    ],
    ['三个目标都认出来了', c.seen.engine && c.seen.integration && c.seen.model],
    ['外壳 37/37', u.ok === 37 && u.all === 37],
    ['外壳全挂也读得出来（0/1）', parseUitest('0/1 通过').ok === 0],
    ['读不出"N/M 通过"必须返回 null（不许当 0 条）', parseUitest('啥也没打').ok === null],
    [
      '图谱 61 / 1257 / 1319 / 1923',
      !!g && g.graph_files === 61 && g.graph_symbols === 1257 && g.graph_nodes === 1319 && g.graph_edges === 1923,
    ],
    ['图谱输出缺行要返回 null', parseGraph('只有一句别的') === null],
    ['有未跟踪的 .rs 要提醒一句', /未跟踪的 \.rs/.test(untrackedRustWarning('tools/demos/a.rs\ncrates/b.rs\n') || '')],
    ['干净的树不提醒', untrackedRustWarning('\n') === null],
    ['git 跑不成也不提醒（那时没什么可说的）', untrackedRustWarning('', true) === null],
  ];

  let bad = 0;
  for (const [what, ok] of checks) {
    if (!ok) bad += 1;
    console.log(`  ${ok ? 'OK  ' : 'FAIL'}  ${what}`);
  }
  console.log(`解析器自检：${checks.length - bad}/${checks.length} 通过`);
  process.exit(bad ? 1 : 0);
}

// 只有被直接跑起来才干活——被 import 时（比如以后有人拿去做别的事）
// 不许顺手起 cargo 跑测试。
const isMain = resolve(process.argv[1] || '') === resolve(HERE);

if (isMain && process.argv.includes('--self-test')) {
  selfTest();
} else if (isMain) {
  const tests = collectTests();
  const shell = collectShellTests();
  const gates = collectGates();
  const graph = collectGraph();
  const motives = collectMotives();

  const measured = {
    engine_unit: tests.engine_unit,
    engine_ignored: tests.engine_ignored,
    integration: tests.integration,
    model: tests.model,
    tests_total: tests.total,
    shell_tests: shell,
    gates,
    ...(graph || {}),
    ...(motives || {}),
  };

  console.log('现场采集（跑过的命令）：');
  for (const c of ran) console.log('  $ ' + c);
  warnUntrackedRust();
  console.log('采到的数：');
  for (const [k, v] of Object.entries(measured)) console.log(`  ${k} = ${v}`);

  if (problems.length) {
    console.log('');
    console.log('采集没跑绿，**没有覆盖 tools/facts.json**（不绿的跑不许当真值）：');
    for (const p of problems) console.log('   × ' + p);
    process.exit(1);
  }

  // ── `--check`：**真源过没过期** ─────────────────────────────────────
  //
  // 加这一档的理由（2026-10-16 外部对照复核抓出的 B 档）：
  // `tools/gates.json` 的七道门禁里**没有 `collect_facts`**，而 `check_facts` 只做
  // "文档 == facts.json"的比对。于是**改了测试不重刷，门禁照样全绿**——
  // `AGENTS.md` §1 那句"数字只信 facts.json（由 collect_facts **现场采集**）"
  // **没有任何东西在守它**。
  //
  // 这就是"地基没人守"：上面 64 处文档标记的可信度，全建立在"我记得重刷"上。
  //
  // `--check` 不写盘、只比：采到的数 vs 盘上的数，不一样就报错并印出该跑哪条命令。
  // 代价是**再跑一遍采集**（约 30 秒，里面是一遍 cargo test）——贵，但这是
  // **唯一能让"文档数字 = 代码事实"不依赖记忆的办法**。
  if (process.argv.includes('--check')) {
    let onDisk = null;
    try {
      onDisk = JSON.parse(readFileSync(FACTS, 'utf8')).facts;
    } catch (e) {
      console.log('');
      console.log(`真源读不了（${e.message}）——先跑 node tools/collect_facts.mjs`);
      process.exit(1);
    }
    const drift = [];
    for (const [k, v] of Object.entries(measured)) {
      if (String(onDisk[k]) !== String(v)) {
        drift.push(`  × ${k}：盘上 ${onDisk[k] ?? '(没有)'}，现场采到 ${v}`);
      }
    }
    for (const k of Object.keys(onDisk)) {
      if (!(k in measured)) drift.push(`  × ${k}：盘上有，这次没采到（采集口径变了？）`);
    }
    console.log('');
    if (drift.length) {
      console.log(`**真源过期了**（${drift.length} 个数字对不上）——文档里那些标记是在守旧数字：`);
      for (const d of drift) console.log(d);
      console.log('');
      console.log('修法：node tools/collect_facts.mjs → node tools/check_facts.mjs --write-docs');
      process.exit(1);
    }
    console.log(`真源是新的：${Object.keys(measured).length} 个数字逐个等于现场采到的值。`);
    process.exit(0);
  }

  let note = DEFAULT_NOTE;
  try {
    const prev = JSON.parse(readFileSync(FACTS, 'utf8'));
    if (prev.note) note = prev.note;
  } catch {
    // 第一次生成：用默认的 note。
  }
  writeFileSync(FACTS, `${JSON.stringify({ note, facts: measured }, null, 2)}\n`, 'utf8');
  console.log('');
  console.log(`已写入 tools/facts.json（${Object.keys(measured).length} 个数字）。`);
  console.log('接着跑 node tools/check_facts.mjs 看文档跟没跟上。');
}
