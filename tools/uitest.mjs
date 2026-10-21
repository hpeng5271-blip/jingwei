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
          case 'notify_gate':
            // 闸门桩（2026-10-21）：默认允许；测试里把 globalThis.__gateDeny 置真即可模拟"静音中"。
            if (globalThis.__gateDeny) return { allow: false, silence: 120, why: '静音中' };
            return { allow: true, silence: 30, why: '第一次提示' };
          case 'tasks_state':
            // 任务角标的桩（2026-10-21）：一类在跑、一类刚完成、一类没任务源。
            return { running: 1, latest: {
              index: { id: 'x', state: 'running', name: 'jingwei index', started_at: 1, ended_at: null, detail: '正在索引' },
              env: { id: 'y', state: 'done', name: '环境探测', started_at: 1, ended_at: 2, detail: '探到 3 项' },
            } };
          case 'cli_json': {
            // **net 的桩**（2026-10-22）：status 给一条临时允许；allow/deny 记账后返回成功。
            const na = (args && args.args) || [];
            if (na[0] === 'net') {
              globalThis.__netCalls = globalThis.__netCalls || [];
              globalThis.__netCalls.push(na.slice(1).join(' '));
              if (na[1] === 'status') {
                return { state: 'yes', summary: '白名单 9 个包仓库；当前额外允许 1 个',
                  whitelist: Array(9).fill('pypi.org'),
                  allowed: [{ host: 'evil.example', until: 1, left: 3600 }],
                  evidence: [], coverage: { read: 1, total: null, partial: false, note: '' } };
              }
              if (na[1] === 'allow') {
                return { state: 'yes', summary: '已允许 ' + na[2], host: na[2], hours: 24 };
              }
              if (na[1] === 'deny') {
                return { state: 'yes', summary: '已收回 ' + na[2], host: na[2], removed: true };
              }
            }

            // **协议桥的桩**（2026-10-21）：界面只消费 CLI 的 JSON，桩给一份最小协议对象。
            return { state: 'unknown', summary: '暂无法探知：桩数据', reason: '暂无法探知：桩数据',
              evidence: [{ kind: 'command', ref: 'jingwei handover', note: '桩' }],
              coverage: { read: 0, total: null, partial: true, note: '桩' },
              metrics: { files: 12, volume_bytes: 4096, capped: false,
                lifecycle: { state: 'no_git', basis: '桩：没有 .git' } } };
          }          case 'inspect':
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
            // **指南说"跑不起来"、而体检条目不报警**的那一屏（2026-10-17 加）。
            //
            // 这不是编出来的：`messy_service` 演示项目上实测就是这样——
            // `#guideCard` 写「✗ 现在跑不起来 · 4 个依赖没装」，
            // 而 `#statusCard` 写「✓ 环境看着没问题」、`#cardNotes` 写「没有挡路的问题」。
            // 因为"依赖没装"在体检里是 info，而这两张卡原来自己读 `d.counts`。
            //
            // 桩里把这两件事拆开摆：**counts 全 0、findings 空、guide 说 no**。
            // 「不适用」桩（2026-10-18）：state=unknown + why=not_applicable，nextStep 是句子。
            if (window.__guideNA) {
              return {
                summary: [{ k: '项目类型', v: 'Python' }],
                counts: { error: 0, warn: 0 },
                findings: [],
                guide: {
                  ready: {
                    state: 'unknown',
                    why: 'not_applicable',
                    whatToDo: '这里有 3 个 .py，但没有任何项目标记文件——补一个清单我就能判断依赖和运行方式',
                  },
                  what: 'Python',
                  niceToHave: ['依赖没能确认：pip list 没跑成（不是没装）'],
                  headline: '这里不像一个项目（没有任何项目标记文件）',
                  nextStep: '想让我能判断它：新增一个清单文件（记下 3 个 .py）；不需要就忽略这句',
                },
                hygiene: { items: [] },
                deps: {}, depsDetail: [], depsTree: { total: 0, edges: [] },
                depsTreeWeb: { total: 0, edges: [] },
                web: null, jvm: null, jvmBuild: null,
                notes: [], runs: [], venvs: [], declared: [],
              };
            }
            if (window.__guideNo) {
              return {
                summary: [{ k: '项目类型', v: 'Python' }],
                counts: { error: 0, warn: 0 },
                findings: [],
                guide: {
                  ready: { state: 'no' },
                  niceToHave: ['依赖没能确认：pip list 没跑成（不是没装）'],
                  what: 'Python',
                  headline: '4 个依赖没装：click、flask、numpy、requests',
                  nextStep: 'jingwei env install',
                },
                hygiene: { items: [] },
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
                // **角色是内核算好带下来的**（store.rs::dump → graph/roles.rs）：
                // 桩数据要照真形状给，否则界面读不到 role 只是"看起来画了"。
                // isolated 不是角色，是「读不出它跟谁有关系」——实测本仓库 17 个文件如此
                roles: { entry: 1, foundation: 1, middle: 0, test: 1, isolated: 2 },
                // 目录级：照真 payload 给（只放前三名，不堆）
                // 入口目录一律给 4 个：只画前三名，界面必须说出「共 4 个」
                dirs: {
                  entry: [
                    { dir: 'crates/cli/src', out: 48, in: 0 },
                    { dir: 'src-tauri/src', out: 24, in: 0 },
                    { dir: 'crates/engine/src/launch', out: 12, in: 2 },
                  ],
                  foundation: [{ dir: 'crates/engine/src/util', out: 3, in: 50 }],
                  entryTotal: 4,
                  middleTotal: 7,
                  foundationTotal: 1,
                  total: 5,
                },
                nodes: [
                  { id: 'project:', kind: 'project', name: 'demo', role: 'container' },
                  // out/in 是**只算跨文件 calls** 的度数（与 deg 不同，deg 含包含边）
                { id: 'file:src/main.rs', kind: 'file', name: 'main.rs', role: 'entry', out: 3, in: 0 },
                  { id: 'symbol:src/main.rs#main', kind: 'symbol', name: 'main', role: 'entry' },
                  { id: 'symbol:src/main.rs#helper', kind: 'symbol', name: 'helper', role: 'foundation' },
                  // 测试文件：**有它，才能验「测试不参与判断」那句话真的画得出来**
                  { id: 'file:src/main_tests.rs', kind: 'file', name: 'main_tests.rs', role: 'test' },
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
  // **协议条**（2026-10-21）：界面吃的是 CLI 的 `--json`（三数字 + 三态 + 依据）。
  // 判据：条子在、且三个数字都出现——拿不到就该显式说「查不了」，不许空白。
  // 详情面板由 openDetail() 打开（点 banner 只改选中）：这里直接走同一个入口，
  // 验证的是"界面真的渲染了协议条"，不是替它另写一条路径。
  await page.evaluate(async () => { if (typeof openDetail === 'function' && PS.sel) await openDetail(PS.sel); });
  await page.waitForTimeout(400);
  st = await page.evaluate(() => (document.getElementById('pdMetrics') || {}).textContent || '');
  // 任务角标（2026-10-21）：三个图标都在；桩里 index 在跑、env 完成、deep_graph 没任务源。
  const ti = await page.evaluate(() => {
    const box = document.getElementById('taskIcons');
    if (!box) return { n: 0, running: 0, states: [] };
    const els = Array.from(box.querySelectorAll('.ti'));
    return { n: els.length, running: els.filter((e) => e.dataset.state === 'running').length,
      states: els.map((e) => e.dataset.kind + ':' + e.dataset.state) };
  });
  check('任务角标：三个图标都在', ti.n === 3, JSON.stringify(ti.states));
  check('任务角标：只有真任务源在跑', ti.running === 1, JSON.stringify(ti.states));

  check('概览协议条：文件数 / 体积 / 生命周期都在', /文件/.test(st) && /体积/.test(st) && /生命周期/.test(st), st.slice(0, 160));

  // 更新窗（2026-10-21）：有旧快照才弹；点"知道了"收起；同一次变化不重复弹。
  await page.evaluate(() => {
    const k = 'jw.seen.files.' + PS.sel;
    localStorage.setItem(k, JSON.stringify({ files: 1, bytes: 1 }));
  });
  await page.evaluate(() => fillMetrics(PS.sel));
  await new Promise((r) => setTimeout(r, 400));
  const hasUpd = await page.evaluate(() => !!document.getElementById('updatePanel'));
  // 错误窗（2026-10-21）：同一个错只占一行并计数；error 排在 warn 前；不抢注意力（没有 error 就不出现）。
  const eg = await page.evaluate(() => {
    const rows = errGroups([
      { severity: 'error', code: 'index.fail', message: '解析失败', file: 'src/a.rs', line: 3 },
      { severity: 'error', code: 'index.fail', message: '解析失败', file: 'src/a.rs', line: 3 },
      { severity: 'warn', code: 'deps.missing', message: '缺依赖', file: 'Cargo.toml', line: 1 },
      { severity: 'info', code: 'noise', message: '不该算进来' },
      { severity: 'error', code: 'grouped', message: '已归组', group: 'x' },
    ]);
    return rows.map((r) => r.severity + ':' + r.code + ':' + r.count);
  });
  check('错误窗：同一个错合并成一行并计数', eg.length === 2 && eg[0] === 'error:index.fail:2', JSON.stringify(eg));
  check('错误窗：error 排在 warn 前', eg.length === 2 && eg[1] === 'warn:deps.missing:1', JSON.stringify(eg));
  const noErr = await page.evaluate(() => errGroups([{ severity: 'warn', code: 'w', message: '只有警告' }]).filter((r) => r.severity === 'error').length);
  check('错误窗：只有警告时不抢注意力', noErr === 0, String(noErr));
  const ep = await page.evaluate(async () => {
    localStorage.removeItem('jw.err.seen.x');
    const f = [
      { severity: 'error', code: 'index.fail', message: '解析失败', file: 'src/a.rs', line: 3 },
      { severity: 'error', code: 'index.fail', message: '解析失败', file: 'src/a.rs', line: 3 },
    ];
    await renderErrPanel('x', { findings: f });
    const box = document.getElementById('errPanel');
    const head = box ? box.querySelector('.ep-head').textContent : '';
    const rows = box ? box.querySelectorAll('.ep-row').length : 0;
    await renderErrPanel('x', { findings: [{ severity: 'warn', code: 'w', message: '只有警告' }] });
    const after = !!document.getElementById('errPanel');
    // 静音中：同一个错不该再自己跳出来（内容仍在"待办"里）
    globalThis.__gateDeny = true;
    localStorage.removeItem('jw.err.seen.x');
    await renderErrPanel('x', { findings: f });
    const denied = !!document.getElementById('errPanel');
    globalThis.__gateDeny = false;
    return { head, rows, after, denied };
  });
  check('错误窗：有 error 才自己出来', ep.rows === 1 && ep.head.indexOf('1 条') >= 0, JSON.stringify(ep));
  check('错误窗：只有警告就自己收走', ep.after === false, JSON.stringify(ep));
  check('错误窗：静音期内不自己跳出来', ep.denied === false, JSON.stringify(ep));

  check('更新窗：有旧快照时弹一次', hasUpd === true, String(hasUpd));
  await page.evaluate(() => {
    const x = document.querySelector('#updatePanel [data-close]');
    if (x) x.click();
  });
  const goneUpd = await page.evaluate(() => !document.getElementById('updatePanel'));
  check('更新窗：点知道了就收起', goneUpd === true, String(goneUpd));
  await page.evaluate(() => fillMetrics(PS.sel));
  await new Promise((r) => setTimeout(r, 400));
  const againUpd = await page.evaluate(() => !document.getElementById('updatePanel'));
  check('更新窗：同一次变化不重复弹', againUpd === true, String(againUpd));

  // 1.5 **两张卡必须说同一件事**（2026-10-16 加）。
  //
  //   这是拿真项目跑出来的：`#guideCard` 写「✗ 现在跑不起来 · 2 个依赖没装」，
  //   而同一屏的 `#cardNotes` 写「要处理 **没有挡路的问题**」——
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
  // 行号旁的诊断标记（2026-10-22）：检查结果按文件挂到行号上，点一下跳到那一行。
  const gd = await page.evaluate(async () => {
    const f = ED.path;
    DOCK.diags = [
      { level: 'error', code: 'E1', file: f, line: 2, col: 1, message: '这行有错' },
      { level: 'warn', code: 'W1', file: f, line: 3, col: 1, message: '这行是警告' },
    ];
    edPaintGutter(true);
    const marks = Array.from(document.querySelectorAll('#edNums .ed-diag'));
    const before = document.querySelector('#edText').selectionStart;
    marks[0].click();
    await new Promise((r) => setTimeout(r, 200));
    const lncol = (document.getElementById('edLnCol') || {}).textContent || '';
    return { n: marks.length, lv: marks.map((m) => m.dataset.lv).join(','), lncol,
      moved: document.querySelector('#edText').selectionStart !== before };
  });
  check('编辑器：行号上有诊断标记', gd.n === 2, JSON.stringify(gd));
  // 网络允许（2026-10-22）：列表来自 CLI 的 net status；点 × 走 net deny。
  const netSt = await page.evaluate(async () => {
    globalThis.__netCalls = [];
    await netRefresh();
    const chips = Array.from(document.querySelectorAll('#netList .net-chip'));
    const html = document.getElementById('netList').innerHTML;
    if (chips[0]) chips[0].querySelector('[data-deny]').click();
    await new Promise((r) => setTimeout(r, 200));
    return { n: chips.length, html, calls: globalThis.__netCalls || [] };
  });
  check('网络：列出临时允许（带剩余时间）', netSt.n === 1 && /小时后过期|分钟后过期/.test(netSt.html), netSt.html);
  check('网络：点 × 走 net deny', netSt.calls.some((c) => c.startsWith('deny')), JSON.stringify(netSt.calls));
  // 终端里网络被拒 → 就地给一个能点的「允许 24 小时」（2026-10-22）。
  const ta = await page.evaluate(async () => {
    globalThis.__netCalls = [];
    termLine('  ✗ 暂无法探知：这条安装命令要访问 evil.example，它不在白名单里｜缺：该仓库域名的允许｜为什么：网络默认拒绝｜怎么补：加前缀重跑');
    const btn = document.querySelector('[data-allow-host]');
    const before = !!btn;
    if (btn) btn.click();
    await new Promise((r) => setTimeout(r, 200));
    const after = document.querySelector('[data-allow-host]');
    return { before, label: after ? after.textContent : '', calls: globalThis.__netCalls || [] };
  });
  check('终端里网络被拒时给可点的允许按钮', ta.before === true, JSON.stringify(ta));
  check('点它就真的调了 net allow', ta.calls.some((c) => c.startsWith('allow evil.example')), JSON.stringify(ta.calls));
  check('编辑器：标记分得出 error 与 warn', gd.lv === 'error,warn', gd.lv);
  check('编辑器：点标记跳到那一行', gd.lncol.includes('第 2 行'), gd.lncol);

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
      roles: document.getElementById('ghRoles').textContent,
      // 每条角色把**判据**挂在 title 上：读的人悬停就能问「凭什么」
      roleTitles: Array.from(document.querySelectorAll('#ghRoles .gr-role'))
        .map((e) => e.getAttribute('title') || ''),
      dirs: document.getElementById('ghDirs').textContent,
      // 各角色的平均半径：验"入口外圈、底层核心"这条判据
      ring: (function () {
        const acc = {};
        for (const n of GS.nodes) {
          const r = Math.sqrt(n.x * n.x + n.y * n.y);
          (acc[n.role] = acc[n.role] || []).push(r);
        }
        const avg = {};
        for (const k of Object.keys(acc)) {
          avg[k] = acc[k].reduce(function (s, x) { return s + x; }, 0) / acc[k].length;
        }
        return avg;
      })(),
      rolesHidden: document.getElementById('ghRoles').hidden,
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

  // **一张图要读得出三种角色**（框架.md §4.8）。
  // 判据不是"那行有字"，而是**三种角色都点了名、都带数**——
  // 只有标签没有数，读的人不知道有几层；只有数没有名字，等于没解释。
  check(
    '图谱：三种角色都点了名、都带数',
    !st.rolesHidden
      && st.roles.includes('入口') && st.roles.includes('公共底层') && /\d/.test(st.roles),
    JSON.stringify({ hidden: st.rolesHidden, text: st.roles }),
  );
    // **「入口」只能叫「候选」**（2026-10-18，拿 3 个真应用量出来的）：
    // 判出 8 个"入口"只有 2 个是真的（25%）。标签骗人比没有标签更糟。
    // **公共面**（2026-10-18）：库的 `src/lib.rs` 既不是底层也不是入口。
    // 真项目证据：embed-resource 的 lib.rs 曾是"公共底层"，而它是库的出口。
    const surf = await page.evaluate(() => {
      ghRenderRoles({ surface: 1, entry: 1, foundation: 0, middle: 2, test: 0, isolated: 0 });
      return document.querySelector('#ghRoles').textContent;
    });
    check('图谱：库的出口叫「公共面」，不算底层',
      surf.includes('公共面') && surf.includes('1') && !surf.includes('公共底层 1'),
      surf.slice(0, 80));
    // **两条判据必须对得上名**（2026-10-18 改）：
    // 「入口」= Cargo 的 bin 目标（事实）；「入口候选」= 按度数猜的（真项目 25%）。
    // 判据要变成**名字与理由匹配**，而不是"不许出现入口"那种临时口号。
    //
    // 从 `GROLE` 直接读理由，**不从渲染结果读**：渲染只包含当前数据里出现的角色，
    // 用渲染结果去查"没出现的角色有没有理由"必然查不到（第一版就栽在这）。
    // **覆盖度不够时图例要加一句**（2026-10-18，拿 fd 的真数换的：连上 32、拿不准 141）。
    // **拿不到覆盖度就不说**——老库没有这两个数，"不知道"不许冒充"够用"。
    const tips = await page.evaluate(() => ({
      bad: ghCoverageTip({ linked: 32, unresolved: 141 }),
      good: ghCoverageTip({ linked: 200, unresolved: 10 }),
      equal: ghCoverageTip({ linked: 50, unresolved: 50 }),
      old: ghCoverageTip({}),
      none: ghCoverageTip(null),
    }));
    check('图谱：覆盖度不够时图例说「会比实际偏多」',
      tips.bad.includes('偏多') && tips.bad.includes('141'),
      JSON.stringify(tips.bad.slice(0, 50)));
    check('图谱：覆盖度够 / 相等 / 老库 / 没数据时都不吵',
      tips.good === '' && tips.equal === '' && tips.old === '' && tips.none === '',
      JSON.stringify(tips));
    const whys = await page.evaluate(() => {
      const out = {};
      for (const k of Object.keys(GROLE)) out[k] = GROLE[k].why;
      return out;
    });
    check('图谱：入口说是事实，候选说是猜（两条判据对得上名）',
      (whys.entry || '').includes('事实') && !(whys.entry || '').includes('25%')
        && (whys.entry_candidate || '').includes('25%'),
      JSON.stringify({ entry: (whys.entry || '').slice(0, 40), cand: (whys.entry_candidate || '').slice(0, 40) }));
  // **测试文件不参与判断这件事要写在脸上**：读的人会把最热闹的当成入口。
  check(
    '图谱：说明测试不参与判断',
    st.roles.includes('测试'),
    st.roles,
  );
  // **目录级那一行**：文件级之后再看一层，才叫"模块结构"。
  // 判据不是"有字"，而是**说出哪个目录是入口、哪个是公共底层**。
  check(
    '图谱：角色计数带单位（不是光一个数字）',
    st.roles.includes('个文件'),
    st.roles,
  );
  // **大小要说规模**（2026-10-17 加）。
  // 颜色说角色（方向），大小说规模——两件事分开。中间层里既有枢纽（出13/入13）
  // 也有局部胶水（出0/入2），只按角色给固定大小时它们长得一模一样。
  const radii = await page.evaluate(() => ({
    hub: ghNodeRadius('middle', false, 26),
    glue: ghNodeRadius('middle', false, 2),
    entryFile: ghNodeRadius('entry', false, 0),
    entrySym: ghNodeRadius('entry', true, 0),
  }));
  check(
    '图谱：大小要说规模（枢纽比同角色的胶水画得大）',
    radii.hub > radii.glue,
    JSON.stringify(radii),
  );
  check(
    '图谱：符号在同角色下比文件小（否则 1520 个符号会盖住 95 个文件）',
    radii.entrySym < radii.entryFile,
    JSON.stringify(radii),
  );
  check(
    '图谱：每个角色都挂着判据（悬停能问「凭什么」）',
    st.roleTitles.length > 0 && st.roleTitles.every((s) => s.length > 10
      // 判据可以写成几种：带阈值（≥ / ×）、带等号（= 0）、一句明确的规则，
      // 或者**路径规则**（测试文件 `_tests`/`_test`/`/tests/`；结构入口 `main.rs`/`src/bin/`）。
      // **但不许写成「它很重要」这种没有边界的话**——那正是误报的来源。
      && /[≥×]|= 0|不参与|main\.rs/.test(s)),
    JSON.stringify(st.roleTitles),
  );
  check(
    '图谱：一条边都没有的文件说成「还没连上」，不混进中间层',
    st.roles.includes('还没连上'),
    st.roles,
  );
  // **截断不许静默**（2026-10-17 加）：入口目录一共 4 个、只画前三名，
  // 界面必须说出「共 4 个」——否则读的人以为入口就这三个。
  // **中间层目录也要报个数**（2026-10-17 加）：13 个目录里 7 个是中间层，
  // 原来一个字都不说，读的人会以为「目录只有两类」。
  check(
    '图谱：目录级的中间层也要报个数（不报名字）',
    st.dirs.includes('中间层 7 个目录'),
    st.dirs,
  );
  check(
    '图谱：目录级被截断时要说出来（共 N 个）',
    st.dirs.includes('共 4 个'),
    st.dirs,
  );
  check(
    '图谱：目录级说出了入口与公共底层',
    st.dirs.includes('入口') && st.dirs.includes('公共底层')
      && st.dirs.includes('crates/cli/src') && st.dirs.includes('crates/engine/src/util'),
    st.dirs,
  );
  // **铺开要按角色**，不能按度数：入口在外圈、公共底层在核心，
  // 于是"依赖方向"不用读标签也看得出来。按度数铺会把入口和底层混在一起，
  // 方向就抹掉了——那正是 `框架.md` §4.8 说的错。
  // 判据用**序关系**，不写死半径（写了就成了"记着数字"，改布局就红得没道理）：
  //   入口 > 中间层 > 公共底层，且 测试 > 入口（测试垫在最外）。
  // 桩数据里可能没有某一层，所以缺的跳过——**但那不能变成"随便都过"**：
  // 入口与公共底层必须都在，这是这条判据的核心。
  check(
    '图谱：入口铺在外圈、公共底层在核心',
    typeof st.ring.entry === 'number'
      && typeof st.ring.foundation === 'number'
      && st.ring.entry > st.ring.foundation
      && (st.ring.middle === undefined || (st.ring.entry > st.ring.middle && st.ring.middle > st.ring.foundation))
      && (st.ring.test === undefined || st.ring.test > st.ring.entry),
    JSON.stringify(st.ring),
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
  // ── 指南 vs 状态卡 vs 小字：**同一个判断，不许三个答案**（2026-10-17 加）──
  //
  // 由来：`messy_service` 演示项目实测，同一屏写着
  //   #guideCard  ✗ 现在跑不起来 · 4 个依赖没装
  //   #statusCard ✓ 环境看着没问题
  //   #cardNotes  要处理 没有挡路的问题
  // 根子是第 26 轮那个病的另一半：退出了码修成问 `make_guide`，**界面这张卡还在自己算**。
  //
  // 桩里 counts 全 0、findings 空、guide 说 no —— 所以这三条**不可能空转**：
  // 只要哪张卡又回去读 `d.counts` 或者自己写死一句"没问题"，立刻红。
  {
    await page.evaluate(() => { window.__guideNo = true; });
    await page.evaluate(() => document.querySelector('#projList .banner').click());
    await page.waitForTimeout(1200);
    const g = await page.evaluate(() => ({
      guide: (document.getElementById('guideCard') || {}).textContent || '',
      status: (document.querySelector('#statusCard h2') || {}).textContent || '',
      notes: (document.getElementById('cardNotes') || {}).textContent || '',
    }));
    check('指南说跑不起来时，指南卡本身必须这么说', g.guide.includes('跑不起来'), g.guide.slice(0, 60));
    check(
      '指南说跑不起来时，状态卡**不许**说「环境看着没问题」',
      !g.status.includes('环境看着没问题'),
      g.status,
    );
    // **必须正向确认小字也换过了**，不能只验"没有那句错话"。
    // 第一版只写了 `!includes('没有挡路的问题')`，它绿了——因为小字里还是**上一份桩**的内容
    // （「要处理 2 件：未找到 cargo」），于是这条断言**验的是一个没刷新的旧屏**。
    // **绿对了原因不算数。**
    check(
      '指南说跑不起来时，小字换成了同一个结论（而不是没刷新）',
      g.notes.includes('跑不起来'),
      g.notes.slice(0, 120),
    );
    check(
      '指南说跑不起来时，小字**不许**说「没有挡路的问题」',
      !g.notes.includes('没有挡路的问题'),
      g.notes.slice(0, 120),
    );
    await page.evaluate(() => { window.__guideNo = false; });

    // **「验一下能不能构建」的四态**（2026-10-18）：一态一句，不许压成两态。
    // 尤其 `unclear` —— 它是"我们没能验成"，**不是"构建失败"**。
    const vh = await page.evaluate(() => ({
      passed: verifyHtml({ verdict: 'passed', headline: '构建跑通了（退出码 0，12ms）', cmd: 'cargo build' }),
      failed: verifyHtml({ verdict: 'failed', headline: '构建没过（退出码 1）', first_error: 'error: x' }),
      skip: verifyHtml({ verdict: 'not_applicable', headline: '这一步不适用', why: 'Python 没有构建' }),
      ask: verifyHtml({ verdict: 'unclear', headline: '我们没能验成', why: '超时' }),
      empty: verifyHtml(null),
    }));
    check('验一下：通过时是 good 且带命令', vh.passed.includes('verdict good') && vh.passed.includes('cargo build'), vh.passed.slice(0, 80));
    check('验一下：没过时是 bad 且带第一条错误', vh.failed.includes('verdict bad') && vh.failed.includes('第一条错误'), vh.failed.slice(0, 80));
    check('验一下：不适用时是 skip（不是 bad）', vh.skip.includes('verdict skip') && !vh.skip.includes('verdict bad'), vh.skip.slice(0, 80));
    check('验一下：没验成时是 ask（不是 bad）', vh.ask.includes('verdict ask') && !vh.ask.includes('verdict bad'), vh.ask.slice(0, 80));
    check('验一下：没有结论时一个字都不加', vh.empty === '', vh.empty);
  }
    const miss = await page.evaluate(() => verifyHtml({ verdict: 'failed', headline: '构建没过', missing_tool: { what: 'JDK', why: '没有 javac', shape: '装 JDK', verify: 'java -version', url: 'https://adoptium.net/temurin/releases/', expect: 'java -version 打印 openjdk version' } }));
    check('验一下：缺件要说清"缺什么/为什么/怎么装/在哪下/怎么自己验/预期输出"', ['这台机器缺', '为什么', '怎么装', '在哪下', '怎么自己验', '看着像这样才算装上'].every((k) => miss.includes(k)) && miss.includes('<a href="https://adoptium.net'), miss.slice(0, 110));
    const bad = await page.evaluate(() => verifyHtml({ verdict: 'failed', headline: 'x', missing_tool: { what: 'JDK', url: 'http://example.com/x' } }));
    check('验一下：不是 https 的下载地址不画成链接', !bad.includes('<a ') && !bad.includes('example.com'), bad.slice(0, 90));

  // 「不适用」≠「还查不出来」：界面压成一句就是三态压扁；且那支 nextStep 是句子，不许画成 code。
  await page.addInitScript(() => { window.__guideNA = true; });
  await page.reload();
  await page.waitForTimeout(1500);
  const banner = await page.$('#projList .banner');
  if (banner) await banner.click();   // 重载后没有项目开着，指南卡当然是空的
  await page.waitForTimeout(1500);
  const na = await page.evaluate(() => {
    const el = document.getElementById('guideCard') || {};
    const q = (s) => (el.querySelector ? el.querySelector(s) : null);
    return {
      text: el.textContent || '',
      hasAdvice: !!q('.gd-advice'),
      hasCode: !!q('.gd-next code'),
      nice: (q('.gd-nice') || {}).textContent || '',
    };
  });
  check('不适用：说「不适用」，不说「还查不出来」', na.text.includes('不适用') && !na.text.includes('还查不出来'), na.text.slice(0, 100));
  check('不适用：建议按说明画（.gd-advice），不装成命令', na.hasAdvice && !na.hasCode, JSON.stringify(na).slice(0, 120));
  // **"顺便说一句"CLI 一直在打、界面原来一个字都不说**（2026-10-18 接上）——这一条钉住它。
  check('锦上添花：界面也要画出来（CLI 早就说了）', na.nice.includes('没能确认'), na.nice.slice(0, 60) || '（没画）');
  // 指纹去重（纯函数）：页面里直接验，不开窗口也能跑。
  {
    const v = await page.evaluate(() => {
      const l = [{ fp: 'h1' }, { fp: 'h1' }, { fp: 'h2' }, {}], c = fingerprintCounts(l);
      return { a: c.get('h1'), b: c.get('h2'), d1: duplicateOf(l[0], c), d2: duplicateOf(l[2], c), d3: duplicateOf(l[3], c) };
    });
    check('指纹统计：同指纹计成一堆、无指纹不参与', v.a === 2 && v.b === 1 && v.d1 === 2 && v.d2 === 0 && v.d3 === 0, JSON.stringify(v));
  }

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
