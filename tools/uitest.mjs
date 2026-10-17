// 外壳的全量测试：一条命令跑完，失败就非零退出。
//
// 为什么必须有一个这样的东西：Rust 那边有 130 多条测试，前端却一直靠
// "开个浏览器手点"——那不是测试，那是抽检。这个脚本把主要链路固定下来：
// 打开项目 → 编辑器 → 运行阶段流 → 问题栏 → 诊断跳转 → 设置还原 → 新建弹窗。
//
// 跑法：node tools/uitest.mjs
// playwright 从 npx 缓存里找（不往项目里塞依赖）。
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

const PORT = 7317;
const ROOT = process.cwd();

function findPlaywright() {
  // 先看项目里有没有（CI 用 npm 装的那种），再退到 npx 缓存。
  // 本地习惯是"不往项目里塞依赖"，但 CI 里显式装一份更可控。
  const local = join(ROOT, 'node_modules', 'playwright');
  if (existsSync(local)) return local;
  const cache = join(process.env.LOCALAPPDATA || '', 'npm-cache', '_npx');
  if (!existsSync(cache)) return null;
  const { readdirSync } = require('node:fs');
  for (const d of readdirSync(cache)) {
    const p = join(cache, d, 'node_modules', 'playwright');
    if (existsSync(p)) return p;
  }
  return null;
}

const require = createRequire(import.meta.url);
const pwPath = findPlaywright();
if (!pwPath) {
  console.error('找不到 playwright（npx 缓存里没有）。先跑一次 npx playwright --version。');
  process.exit(2);
}
const { chromium } = require(pwPath);

// 桩直接写成函数交给 playwright 序列化——不要用模板字符串拼 JS 源码：
// 里面每多一层转义就是一次踩坑（第一版就是死在 `\\` 和 `\n` 上）。
function stubTauri() {
  window.__subs = {};
  window.__calls = [];
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        const wait = (ms) => new Promise((r) => setTimeout(r, ms));
        window.__calls.push({ cmd, args });
        switch (cmd) {
          case 'list_projects':
            return { projects: [
              { name: 'demo', path: 'D:/projects/demo', age_days: 0, markers: ['Cargo.toml'], kinds: ['Rust'] },
              // 第二个是**容器目录**（里面装着好几个项目）。它专门用来验：
              // 打开它的时候，那一屏**不许**说「环境看着没问题」——
              // 那句话在字面上没错、在用途上是误导（2026-10-16 真窗口里截出来的）。
              { name: 'demo-parent', path: 'D:/projects/demo-parent', age_days: 0,
                markers: [], kinds: ['认不出类型'] },
            ] };
          case 'inspect':
            // 容器目录那一支：依赖清单里混着两个不同语言的子项目，
            // 外加那条 `projects-dir-not-a-project`。
            if (args && String(args.root || '').includes('demo-parent')) {
              return {
                summary: [{ k: '项目类型', v: 'Python' }, { k: '扫描文件', v: '60' }],
                counts: { error: 0, warn: 0 },
                findings: [],
                hygiene: { items: [
                  { id: 'projects-dir-not-a-project', severity: 'info', group: '依赖清单',
                    title: '这是个装着 3 个项目的目录，不是一个项目',
                    evidence: ['里面有：hello_python、hello_rust、messy_service'],
                    verify: ['Get-ChildItem -Directory'], boundary: '当成一份教学材料看时不用管',
                    next: '在项目列表里直接点某一个项目' },
                ] },
                deps: {}, depsDetail: [], depsTree: { total: 0, edges: [] },
                depsTreeWeb: { total: 0, edges: [] },
                web: null, jvm: null, jvmBuild: null,
                notes: [], runs: [], venvs: [], declared: [],
              };
            }
            return {
              summary: [{ k: '项目类型', v: 'Rust' }],
              counts: { error: 1, warn: 1 },
              findings: [
                { id: 'f1', severity: 'error', layer: '第 1 层 · 用的是哪个', title: '未找到 cargo',
                  evidence: ['PATH 中不存在'], verify: ['where cargo'], next: '安装 Rust 工具链',
                  boundary: '仅运行 Python 时可忽略' },
                { id: 'f2', severity: 'warn', layer: '第 2 层 · 依赖', title: '依赖未安装' }],
              hygiene: { items: [], manifests: [] },
              deps: { serde: true },
              depsDetail: [
                { name: 'serde', want: '1', have: '1.0.229', ok: true, unknown: false, from: 'Cargo.toml' },
                { name: 'anyhow', want: '1', have: '', ok: false, unknown: false, from: 'Cargo.toml' }],
              depsTree: { total: 3, edges: [{ from: 'serde', to: 'serde_core' }] },
              // Node 的传递依赖图 + **来源**。桩里两样都给：
              // Rust 那张会赢，而 Node 的 note 走"没有树时"那条路——
              // 两条路都要能显示，所以断言也分两条。
              depsTreeWeb: {
                total: 2,
                edges: [{ from: 'app', to: 'vue' }],
                source: 'package-lock.json（lockfileVersion 3）的 packages 段',
                note: null,
              },
              // Node / 前端那一摊。桩里给的是**有内容的**，
              // 因为这一段的价值全在"凭据"上——空对象测不出任何东西。
              web: {
                package_manager: 'pnpm',
                pm_from: 'pnpm-lock.yaml（锁文件）',
                pm_pinned: null,
                link_kind: 'pnpm 隔离布局（依赖在 .pnpm 里，node_modules 下是符号链接）',
                link_from: 'node_modules/.pnpm',
                node_pins: [{ from: '.nvmrc', spec: '20.11.0' }],
                workspaces: 'package.json · workspaces（2 项）',
                typescript: true,
                scripts: [['dev', 'vite'], ['build', 'vite build']],
                frameworks: ['Vite'],
                notes: ['依赖装在仓库根，成员目录里没有 node_modules'],
              },
              // Java 那一摊：**这次会用哪个 JDK**。
              // 桩里给的是"装了三个、说不清用哪个"这个真实处境——
              // 它正是这一栏最该说清楚的一种。
              jvm: {
                java_home: null,
                java_home_ok: false,
                jdks: [
                  { major: 17, home: 'D:/SDK/jdk-17', is_jdk: true },
                  { major: 21, home: 'D:/SDK/jdk-21.0.12', is_jdk: true },
                  { major: 8, home: 'D:/SDK/jdk-8', is_jdk: true },
                ],
                java_on_path: null,
                required: 17,
                required_from: 'pom.xml',
                maven: true,
                gradle: false,
                resolved: null,
              },
              jvmBuild: { name: 'mvnw', wrapper: true },
              runs: [], hasTest: true, contractName: 'env.contract.json', runCommand: 'cargo run',
            };
          case 'list_files':
            return { files: ['Cargo.toml', 'src/main.rs'] };
          // 下面这几条原来**没有桩**（返回 undefined，走"老形状"兼容分支）。
          // 补成信封形状，测试才真的覆盖到新逻辑。
          case 'default_parent':
            return { state: 'yes', data: { parent: 'D:/projects' } };
          case 'initial_project':
            return { state: 'yes', data: null }; // 第一次用：正常状态
          case 'default_opener':
            return { state: 'yes', data: null };
          case 'profiles_list':
            return { state: 'yes', data: [] };
          case 'sample_now':
            return { state: 'yes', data: { cpu: 12, mem: 34 } };
          case 'check_project':
            return {
              state: 'yes',
              // 桩要返回**真实形状的诊断**：`check_project` 的结果会整体替换问题栏里
              // 的编译器那一组（一次新检查覆盖上一次），返回空数组等于把它清掉。
              data: {
                diagnostics: [
                  {
                    level: 'error',
                    code: 'E0308',
                    file: 'src/main.rs',
                    line: 2,
                    col: 9,
                    message: 'mismatched types',
                  },
                ],
                notes: ['没装 cargo——装它才能查'],
              },
            };
          // ── 图谱那三条：这是"整体联调"要验的那条链 ──────────────
          case 'graph_stats':
            return { state: 'yes', data: { nodes: 4, edges: 3 } };
          case 'graph_index':
            return {
              state: 'yes',
              data: { files: 2, symbols: 4, calls: 1, resolved: 1, ambiguous: 0, external: 3, ms: 12 },
            };
          case 'graph_dump':
            return {
              state: 'yes',
              data: {
                total_nodes: 4,
                total_edges: 3,
                truncated: false,
                nodes: [
                  { id: 'project:', kind: 'project', name: 'demo' },
                  { id: 'file:src/main.rs', kind: 'file', name: 'main.rs' },
                  { id: 'symbol:src/main.rs#main', kind: 'symbol', name: 'main' },
                  { id: 'symbol:src/main.rs#helper', kind: 'symbol', name: 'helper' },
                ],
                edges: [
                  { src: 'project:', dst: 'file:src/main.rs', kind: 'contains' },
                  { src: 'file:src/main.rs', dst: 'symbol:src/main.rs#main', kind: 'defines' },
                  { src: 'symbol:src/main.rs#main', dst: 'symbol:src/main.rs#helper', kind: 'calls' },
                ],
              },
            };
          // ── 最近运行（M5 那条链的出口）：两种调用形状都要给 ──────
          // 不带 runId → 列表；带 runId → 某一次的明细。
          // 桩里**故意让第一次运行失败**（退出码 3）、第二次成功——
          // 界面上要能一眼分清，所以两条都要有。
          case 'graph_runs':
            if (args && args.runId) {
              return {
                state: 'yes',
                data: {
                  runId: args.runId,
                  command: 'python main.py',
                  exitCode: 3,
                  at: 1792112000000,
                  diagnostics: [
                    { title: 'boom', file: { state: 'yes', data: 'file:src/main.py' } },
                    { title: 'Missing module: requests', file: { state: 'no', reason: '这条问题没有说是哪个文件' } },
                    { title: 'E0308 mismatched types', file: { state: 'unknown', reason: '它说了是哪个文件，但那个文件不在图谱里' } },
                  ],
                },
              };
            }
            return {
              state: 'yes',
              data: {
                runs: [
                  { runId: 'run:2', command: 'python main.py', exitCode: 3, at: 1792112000000, diagnostics: 3 },
                  { runId: 'run:1', command: 'python -m unittest', exitCode: 0, at: 1792111000000, diagnostics: 0 },
                ],
              },
            };
          case 'watch_start':
            return { state: 'yes', data: { watching: 'D:/projects/demo', interval_ms: 1500 } };          case 'watch_stop':
            return { state: 'yes', data: { watching: null } };
          case 'open_file':
            return { text: 'fn main() {\n    let n: i32 = 1;\n}\n',
              readable: true, encoding: 'gbk', eol: 'crlf', note: '', drafts: [] };
          case 'apply_edit':
            return { opened: { drafts: [] } };
          case 'check_project':
            return { diagnostics: [{ level: 'error', file: 'src/main.rs', line: 2, col: 9,
              message: 'mismatched types', code: 'E0308' }], notes: [] };
          case 'run_task':
            await wait(2500);
            return { exit_code: 101, crux: 'test_parse 失败' };
          case 'sample_now':
            return { cpu_percent: 4, mem_used_mb: 8000, mem_total_mb: 32000,
              disk_free_gb: 88, disk_total_gb: 500, python_procs: 0, dev_procs: [['cargo', 1]] };
          case 'default_parent':
            return { parent: 'D:/projects' };
          case 'profiles_list':
            return [];
          case 'list_pythons':
            return { items: [] };
          case 'initial_project':
            return null;
          default:
            return {};
        }
      },
    },
    event: { listen: async (name, cb) => { window.__subs[name] = cb; return () => {}; } },
  };
}

const results = [];
const check = (name, ok, detail) => {
  results.push({ name, ok: !!ok, detail });
};

const server = spawn(process.execPath, ['tools/serve-ui.js'], { cwd: ROOT, stdio: 'ignore' });
const stop = () => { try { server.kill(); } catch {} };

let browser;
let lastErr;
const pageErrors = [];
let usedBrowser;
try {
  await new Promise((r) => setTimeout(r, 1200));
  // 浏览器来源按顺序试：自带的 → 系统 Edge → 系统 Chrome。
  // 为什么要退：playwright 的浏览器二进制不一定装了，但 Windows 上一定有 Edge。
  for (const opt of [{}, { channel: 'chrome' }, { channel: 'msedge' }]) {
    try { browser = await chromium.launch(opt); usedBrowser = opt.channel || '自带'; break; } catch (e) { lastErr = e; }
  }
  console.log('浏览器：' + usedBrowser);
  if (!browser) throw new Error('起不了浏览器：' + String(lastErr).slice(0, 120));
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.on('pageerror', (e) => pageErrors.push(String(e)));
  await page.addInitScript(stubTauri);
  await page.goto(`http://127.0.0.1:${PORT}/index.html`, { waitUntil: 'load' });
  await page.waitForTimeout(1500);

  // 1. 打开项目 + 结构树
  await page.evaluate(() => scanProjects('D:\\projects', 2));
  await page.waitForTimeout(1200);
  await page.evaluate(() => document.querySelector('#projList .banner').click());
  await page.waitForTimeout(1300);
  let st = await page.evaluate(() => ({ root: S.root, tree: document.querySelectorAll('#edFiles .ed-node').length }));
  // 路径分隔符不参与判断：Windows 上两种写法都合法
  check('打开项目：全局选中生效', !!st.root && st.root.replace(/\\/g, '/') === 'D:/projects/demo', st.root);
  check('结构树渲染出文件节点', st.tree >= 2, `${st.tree} 个节点`);

  // 1.5 **两张卡必须说同一件事**（2026-10-16 加）。
  //
  //   这是拿真项目跑出来的：`#guideCard` 写「✗ 现在跑不起来 · 2 个依赖没装」，
  //   而同一屏的 `#cardNotes` 写「要处理 **没有挡路的问题**」——
  //   因为它读的是 `d.error`，而 payload 把计数放在 `d.counts.error` 里，
  //   于是那句"没有"**永远**会出现。
  //
  //   这个 bug 从 `cbb5e18`（前端拆七个脚本那次）就在了，
  //   **47 条外壳测试全绿都没抓到**——因为它们在验别处（树、编辑器、图谱）。
  //   **一个确定的"没有"而事实是"有"，是最不该错的那种错。**
  //
  //   桩里 `counts: { error: 1, warn: 1 }` 且两条 finding 各一条：
  //   所以小字**必须**报出数来，**不许**说"没有挡路的问题"。
  st = await page.evaluate(() => ({
    notes: (document.getElementById('cardNotes') || {}).textContent || '',
    head: (document.querySelector('#statusCard h2') || {}).textContent || '',
    guide: (document.getElementById('guideCard') || {}).textContent || '',
  }));
  check(
    '概览：有挡路的时候，小字**不许**说「没有挡路的问题」',
    !st.notes.includes('没有挡路的问题'),
    st.notes.slice(0, 120),
  );
  check(
    '概览：小字的件数与实际 findings 对得上（桩里是 2 件）',
    st.notes.includes('2 件'),
    st.notes.slice(0, 120),
  );

  // 2. 编辑器：打开文件（编码/换行要跟着来）
  await page.evaluate(() => { switchTab('editor'); const f = document.querySelector('#edFiles .ed-node.file'); if (f) f.click(); });
  await page.waitForTimeout(900);
  st = await page.evaluate(() => ({
    tabs: document.querySelectorAll('#edTabs .etab').length,
    enc: document.getElementById('edEnc').textContent,
    crumb: document.getElementById('edCrumb').textContent.trim(),
    lncol: document.getElementById('edLnCol').textContent,
  }));
  check('编辑器：文件标签出现', st.tabs === 1, `tabs=${st.tabs}`);
  check('编辑器：显示编码与换行', st.enc.includes('GBK') && st.enc.includes('CRLF'), st.enc);
  check('编辑器：面包屑有内容', st.crumb.length > 3, st.crumb);
  check('编辑器：行列号在', /第 \d+ 行/.test(st.lncol), st.lncol);

  // 2.5 依赖：点一行要能"点开看详情"（不是只有 tooltip）
  await page.evaluate(() => { const r = document.querySelector('#railBody .dep-row'); if (r) r.click(); });
  await page.waitForTimeout(300);
  st = await page.evaluate(() => {
    const f = document.querySelector('#railBody .dep-fact');
    return { has: !!f, text: f ? f.textContent.replace(/\s+/g, ' ').trim() : '' };
  });
  check('依赖：点行展开详情（声明/实际/来源/状态）',
    st.has && st.text.includes('声明') && st.text.includes('来源'), st.text.slice(0, 60));

  // 2.55 下游树的**来源**必须显示：两棵树看起来一样，凭据完全不同
  st = await page.evaluate(() => {
    const rows = Array.from(document.querySelectorAll('#railBody .rail-empty'));
    const src = rows.map((r) => r.textContent.trim()).find((t) => t.includes('下游树来自'));
    return { src: src || '' };
  });
  check('依赖：下游树标了来源（Rust 那张优先）',
    st.src.includes('Cargo.lock') && st.src.includes('3 条边'), st.src);

  // 另一条路：**没有树可画时要说清是"没有"还是"还没做"**。
  // 直接喂两个形状给 treeIndex，比再造一个项目快得多，也精准得多。
  st = await page.evaluate(() => {
    treeIndex({ depsTreeWeb: { total: 0, edges: [], source: null, note: 'pnpm 的锁文件（pnpm-lock.yaml）格式还没做' } });
    const gap = TREE_GAP;
    const ready = TREE.ready;
    treeIndex(S.data || {}); // 把状态还原回去，别影响后面的检查
    return { gap, ready };
  });
  check('依赖：没有图时明说"还没做"，不画一张空图',
    st.gap.includes('还没做') && st.ready === false, `${st.gap} / ready=${st.ready}`);

  // 2.6 Node / 前端那一摊：包管理器、依赖装到哪去了、版本要求写在哪。
  //     这一段是"为什么依赖显示未安装"的答案，所以断言要落到**内容**上，
  //     不是"有这个区块就算过"。
  st = await page.evaluate(() => {
    const rows = Array.from(document.querySelectorAll('#railBody .rt-row'));
    const note = document.querySelector('#railBody .rt-note');
    return {
      n: rows.length,
      keys: rows.map((r) => r.querySelector('.k').textContent.trim()),
      tip: rows.length ? (rows[1].getAttribute('data-tip') || '') : '',
      pm: rows.length ? rows[0].querySelector('.v').textContent.trim() : '',
      note: note ? note.textContent.trim() : '',
    };
  });
  check('Node：包管理器与凭据都显示出来了',
    st.pm.startsWith('pnpm') && st.tip.includes('.pnpm'),
    `${st.pm} / ${st.tip}`);
  check('Node：依赖位置说清了（PnP / 隔离布局那一类）',
    st.keys.includes('依赖位置'), st.keys.join(','));
  check('Node：说清"为什么可能看着不对"的那句话在',
    st.note.includes('仓库根'), st.note);

  // 2.7 JVM 那一摊：一台机器上三个 JDK 的时候，"用哪个"比"有没有"重要
  st = await page.evaluate(() => {
    const rows = Array.from(document.querySelectorAll('#railBody .rt-row'));
    const secs = Array.from(document.querySelectorAll('#railBody .rail-sec')).map((s) => s.textContent.trim());
    const pick = rows.find((r) => r.querySelector('.k').textContent.trim() === '会用哪个');
    const jdks = rows.find((r) => r.querySelector('.k').textContent.trim() === '装了哪些');
    const notes = Array.from(document.querySelectorAll('#railBody .rt-note')).map((n) => n.textContent.trim());
    return {
      secs,
      pick: pick ? pick.querySelector('.v').textContent.trim() : '',
      pickTip: pick ? (pick.getAttribute('data-tip') || '') : '',
      jdks: jdks ? jdks.querySelector('.v').textContent.trim() : '',
      notes,
    };
  });
  check('JVM：有独立的一栏', st.secs.includes('JVM / 运行时'), st.secs.join('|'));
  check('JVM：说不清用哪个就如实说"说不清"（不编一个）',
    st.pick === '说不清' && st.pickTip.includes('JAVA_HOME'), `${st.pick} / ${st.pickTip}`);
  check('JVM：三个 JDK 都列出来了',
    st.jdks.includes('17') && st.jdks.includes('21') && st.jdks.includes('8'), st.jdks);
  check('JVM：给出"怎么让它确定"的那句话',
    st.notes.some((n) => n.includes('JAVA_HOME')), st.notes.join('|'));

  // 3. 运行：阶段流（面板 + 关键行上限 3 + 原始输出折叠）
  await page.evaluate(() => { switchTab('run'); doRun('test'); });
  await page.waitForTimeout(400);
  await page.evaluate(() => {
    const E = window.__subs['run://event'];
    const ev = (p) => E({ payload: p });
    ev({ type: 'stage', index: 1, title: '装依赖', status: 'ok', detail: 'cargo fetch', ms: 1200 });
    ev({ type: 'stage', index: 2, title: '构建', status: 'ok', detail: 'cargo build', ms: 8400 });
    ev({ type: 'line', source: 'stdout', text: 'Compiling demo v0.1.0' });
    ev({ type: 'stage', index: 3, title: '测试', status: 'fail', detail: 'cargo test', ms: 12100 });
    for (const t of ['error: 甲', 'error: 乙', 'warning: 丙', 'error: 丁', 'error: 戊']) {
      ev({ type: 'line', source: 'stderr', text: t });
    }
  });
  await page.waitForTimeout(400);
  st = await page.evaluate(() => {
    const cards = Array.from(document.querySelectorAll('#stageStream .stage-card'));
    return {
      n: cards.length,
      states: cards.map((c) => c.className.replace('stage-card ', '')),
      keys: cards.length ? cards[cards.length - 1].querySelectorAll('.sc-key').length : 0,
      folded: cards.length ? !cards[cards.length - 1].querySelector('details').open : false,
      raw: cards.length ? cards[cards.length - 1].querySelector('.sc-raw').textContent.length : 0,
    };
  });
  check('运行：阶段面板流式涌出', st.n === 3, `${st.n} 块`);
  check('运行：成功/失败状态分得清', st.states.includes('ok') && st.states.includes('bad'), st.states.join(','));
  check('运行：关键行最多三条（不糊屏）', st.keys === 3, `keys=${st.keys}`);
  check('运行：原始输出默认折叠但已收全', st.folded && st.raw > 0, `raw=${st.raw}`);
  await page.waitForTimeout(2600); // 等 run_task 回来，确认跑完不消失
  st = await page.evaluate(() => document.querySelectorAll('#stageStream .stage-card').length);
  check('运行：跑完面板不消失', st === 3, `${st} 块`);

  // 4. 问题栏 + 详情
  await page.evaluate(() => dockShow('problems'));
  await page.waitForTimeout(400);
  st = await page.evaluate(() => ({ rows: document.querySelectorAll('#probList .drow').length }));
  check('问题栏：列出诊断条目', st.rows >= 2, `${st.rows} 条`);
  await page.evaluate(() => document.querySelector('#probList .drow').click());
  await page.waitForTimeout(300);
  st = await page.evaluate(() => document.getElementById('probDetail').textContent.replace(/\s+/g, ''));
  check('问题栏：点条目出详情（判定依据/复核命令）', st.includes('判定依据') && st.includes('复核命令'), st.slice(0, 40));

  // 5. 真实诊断 → 跳到编辑器精确位置
  await page.evaluate(() => runCheck());
  await page.waitForTimeout(1200);
  st = await page.evaluate(() => {
    const g = Array.from(document.querySelectorAll('#probList .dgroup')).map((x) => x.textContent);
    return { groups: g, rows: Array.from(document.querySelectorAll('#probList .drow')).map((r) => r.textContent.replace(/\s+/g, ' ').trim()) };
  });
  check('检查：编译器诊断单独成组', st.groups.some((x) => x.includes('编译器')), st.groups.join('|'));
  check('检查：带错误码与位置', st.rows.some((x) => x.includes('E0308') && x.includes('src/main.rs:2')), st.rows.join('|').slice(0, 80));
  await page.evaluate(() => { const r = Array.from(document.querySelectorAll('#probList .drow')).find((x) => x.textContent.includes('E0308')); if (r) r.click(); });
  await page.waitForTimeout(1200);
  st = await page.evaluate(() => ({ view: document.querySelector('.view.on').id, lncol: document.getElementById('edLnCol').textContent }));
  check('检查：点条目跳到编辑器那一行', st.view === 'view-editor' && st.lncol.includes('第 2 行'), `${st.view} / ${st.lncol}`);

  // 6. 设置：偏离默认才标记，点了能还原
  await page.evaluate(() => document.getElementById('btnSettings').click());
  await page.waitForTimeout(400);
  await page.evaluate(() => { const s = document.getElementById('setMotion'); s.value = 'fast'; s.dispatchEvent(new Event('change', { bubbles: true })); });
  await page.waitForTimeout(300);
  st = await page.evaluate(() => ({ changed: document.querySelectorAll('#settingsPanel .changed').length, revert: document.querySelectorAll('#settingsPanel .revert').length }));
  check('设置：改动后出现标记与还原', st.changed === 1 && st.revert === 1, JSON.stringify(st));

  // 档位必须真的落到 CSS 变量上。这里曾经漏过:前端设的是 --dur*，
  // 而 style.css 用的是 --t*，两边不是同一批变量——设置项看着能调，实际是死的。
  st = await page.evaluate(() => ({
    t: getComputedStyle(document.documentElement).getPropertyValue('--t-fast').trim(),
  }));
  check('设置：动效档位真的落到 CSS 变量上', st.t === '60ms', JSON.stringify(st));

  await page.evaluate(() => document.querySelector('#settingsPanel .revert').click());
  await page.waitForTimeout(300);
  st = await page.evaluate(() => ({ motion: SET.motion, changed: document.querySelectorAll('#settingsPanel .changed').length }));
  check('设置：还原回到默认', st.motion === 'default' && st.changed === 0, JSON.stringify(st));

  // 统一信封：**`unknown` 不许被当成"没有"**——那是这套系统最核心的那条线，
  // 它从引擎一直贯到界面，所以在这一层也要有一条断言盯着。
  // 顺带验老形状（没有 state 的）仍按 yes 处理，兼容期不能把界面搞崩。
  st = await page.evaluate(() => ({
    yes: unpack({ state: 'yes', data: [1, 2] }),
    no: unpack({ state: 'no', reason: '看过了，确实没有' }),
    unknown: unpack({ state: 'unknown', reason: '这些地方都读不到' }),
    legacy: unpack([3]),
  }));
  check(
    '信封：三态分得开，老形状仍可用',
    st.yes.state === 'yes' &&
      st.no.state === 'no' &&
      st.unknown.state === 'unknown' &&
      st.legacy.state === 'yes' &&
      st.unknown.reason.includes('读不到'),
    JSON.stringify(st),
  );

  // `check_project` 给的是 **notes（复数、数组）**，而前端原来读的是 `r.note`（单数）——
  // 于是"没装 cargo，装它才能查"这类**最该被看见的提示**从来没显示过。
  // 这条盯着它，因为这种错不报任何异常，只是信息静静地消失。
  st = await page.evaluate(async () => {
    const r = unpack(await window.__TAURI__.core.invoke('check_project', { root: 'D:/projects/demo' }));
    return { state: r.state, notes: (r.data || {}).notes || [] };
  });
  check(
    'check_project：notes 是数组且读得出来',
    st.state === 'yes' && st.notes.length === 1,
    JSON.stringify(st),
  );

  // ── 图谱：整体联调的验收点 ────────────────────────────────────
  // 这条链是这一轮新接的：外壳加了三条命令 + 实时层，前端加了画布与跟随。
  // **验的就是"接上没有"**——数字要显示出来、画布上要有东西。
  await page.evaluate(() => switchTab('graph'));
  await page.waitForTimeout(900);
  st = await page.evaluate(() => {
    const cv = document.getElementById('graphCv');
    const ctx = cv && cv.getContext('2d');
    // **要证明画了东西**，不是只看元素在不在：
    // 取画布中间一小块像素，看有没有非背景色。
    let painted = 0;
    if (ctx && cv.width > 0) {
      const d = ctx.getImageData(Math.floor(cv.width / 2) - 60, Math.floor(cv.height / 2) - 60, 120, 120).data;
      const first = [d[0], d[1], d[2]];
      for (let i = 0; i < d.length; i += 4) {
        if (Math.abs(d[i] - first[0]) + Math.abs(d[i + 1] - first[1]) + Math.abs(d[i + 2] - first[2]) > 24) painted++;
      }
    }
    return {
      nodes: document.getElementById('ghNodes').textContent,
      edges: document.getElementById('ghEdges').textContent,
      sub: document.getElementById('ghSub').textContent,
      painted,
    };
  });
  check(
    '图谱：规模显示出来了',
    st.nodes === '4' && st.edges === '3',
    JSON.stringify(st),
  );
  check(
    '图谱：画布上真的画了东西',
    st.painted > 20,
    '中心区域非背景像素 ' + st.painted + ' 个',
  );

  // 6b. 最近运行：M5 那条链的出口。
  //     **这是这一页最容易被"看起来做了"骗过去的地方**——列表出来了不等于点得开，
  //     点得开不等于三态分得清。所以三段都验：列表、明细、三态。
  st = await page.evaluate(() => {
    const box = document.getElementById('ghRuns');
    const rows = Array.from(document.querySelectorAll('#ghRunsList .gr'));
    return {
      hidden: box.hidden,
      n: rows.length,
      // 失败那一条的退出码要带 bad 类（一眼分得出成功与失败）
      bad: rows.filter((r) => r.querySelector('.gr-code.bad')).length,
      cmd: rows[0] ? rows[0].querySelector('.gr-cmd').textContent : '',
      diag: rows[0] ? rows[0].querySelector('.gr-n').textContent : '',
    };
  });
  check(
    '最近运行：两条都列出来了，失败那条标了出来',
    !st.hidden && st.n === 2 && st.bad === 1,
    JSON.stringify(st),
  );
  check(
    '最近运行：一行要有命令和问题条数',
    st.cmd.includes('python') && st.diag.includes('3'),
    JSON.stringify({ cmd: st.cmd, diag: st.diag }),
  );

  await page.evaluate(() => document.querySelector('#ghRunsList .gr').click());
  await page.waitForTimeout(500);
  const det = await page.evaluate(() => {
    const gs = Array.from(document.querySelectorAll('#ghRunsList .gd'));
    const txt = gs.map((g) => g.textContent).join(' | ');
    return {
      n: gs.length,
      hasFile: txt.includes('src/main.py'),
      noPos: txt.includes('没有位置'),
      unk: txt.includes('没索引到'),
    };
  });
  check(
    '最近运行：点开能看到每条问题',
    det.n === 3,
    JSON.stringify(det),
  );
  check(
    '最近运行：三种落点分得开（有文件 / 没说位置 / 没索引到）',
    det.hasFile && det.noPos && det.unk,
    JSON.stringify(det),
  );

  // 6c. **容器目录**：打开一个"装着好几个项目"的目录时，那一屏不许说「环境看着没问题」。
  //
  //     这条是 2026-10-16 在真窗口里截图时抓出来的：界面上写着「项目类型 Python」
  //     和「✓ 环境看着没问题」，而同一屏的依赖清单里躺着 hello_rust/Cargo.toml——
  //     一个"项目"的体检里混着两个别的项目。**跑测试和 CLI 都不会暴露它。**
  await page.evaluate(() => load('D:\\projects\\demo-parent'));
  await page.waitForTimeout(900);
  await page.evaluate(() => switchTab('home'));
  await page.waitForTimeout(400);
  st = await page.evaluate(() => ({
    head: document.querySelector('#statusCard h2').textContent,
    cls: document.getElementById('statusCard').className,
    finding: document.getElementById('findings').textContent.slice(0, 40),
  }));
  check(
    '容器目录：首屏说的是「这里装着好几个项目」',
    st.head.includes('好几个项目'),
    JSON.stringify(st),
  );
  check(
    '容器目录：**不许**说「环境看着没问题」',
    !st.head.includes('环境看着没问题'),
    st.head,
  );
  check(
    '容器目录：诊断区改成「先选一个项目」',
    st.finding.includes('先选一个项目'),
    st.finding,
  );
  // 这两条是**同一个坑的第二次**：`renderHome` 的提前 return 只挡住了首次渲染，
  // 后台刷新（`refreshQuiet`）会直接调 `renderCardNotes` / `renderChanges`，
  // 绕过那个判断——真窗口里就是这么漏出「怎么跑：还没有契约」和变更卡的。
  // 所以断言要盯住"那两片区域是空的"，而不是只盯首屏那张卡。
  st = await page.evaluate(() => ({
    notes: (document.getElementById('cardNotes') || {}).textContent || '',
    chg: (document.getElementById('changeCard') || {}).textContent || '',
    star: document.querySelector('#statusCard').textContent.includes('**'),
  }));
  check(
    '容器目录：小字那几行不出现（不把子项目的事算在父目录头上）',
    st.notes.trim() === '',
    st.notes.slice(0, 60),
  );
  check('容器目录：变更卡不出现', st.chg.trim() === '', st.chg.slice(0, 60));
  check(
    '容器目录：卡片里不许有字面星号（HTML 不是 Markdown）',
    st.star === false,
    String(st.star),
  );

  // 回到正常项目，后面的界面还原与设置那几条要接着在普通项目上跑
  await page.evaluate(() => load('D:\\projects\\demo'));
  await page.waitForTimeout(800);

  // 7. 新建项目弹窗
  await page.evaluate(() => document.getElementById('btnNew2').click());
  await page.waitForTimeout(400);
  st = await page.evaluate(() => ({ open: !document.getElementById('newModal').hidden, preview: document.querySelectorAll('#newPreview .np-row').length }));
  check('新建项目：弹窗打开且有「将创建什么」', st.open && st.preview === 3, JSON.stringify(st));
  await page.evaluate(() => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
  await page.waitForTimeout(300);
  st = await page.evaluate(() => document.getElementById('newModal').hidden);
  check('新建项目：Esc 能关', st === true, String(st));

  // 8. 全程不得有 JS 异常、不得出现红条
  check('全程无 JS 异常', pageErrors.length === 0, pageErrors.slice(0, 2).join(' | '));
  const fatal = await page.evaluate(() => { const f = document.getElementById('fatal'); return f && !f.hidden ? f.textContent : null; });
  check('界面红条未出现', !fatal, fatal || '');
} catch (e) {
  check('测试脚本自身异常', false, String(e).slice(0, 160) + ' || 页面异常: ' + pageErrors.slice(0, 2).join(' | '));
} finally {
  if (browser) await browser.close();
  stop();
}

const bad = results.filter((r) => !r.ok);
for (const r of results) console.log(`${r.ok ? '  ok  ' : ' FAIL '} ${r.name}${r.detail ? '   (' + r.detail + ')' : ''}`);
console.log(`\n${results.length - bad.length}/${results.length} 通过`);
process.exit(bad.length ? 1 : 0);
