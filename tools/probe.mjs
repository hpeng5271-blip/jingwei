// 探针：拿一个**真实目录**去看界面到底画了什么。
//
// ## 为什么要有这个脚本
//
// 2026-10-17 起，我手工拼 PowerShell 探针连踩四个坑，每一个都被我当成了**产品缺陷**：
//
//   33 轮  直接拿 `~/.cargo` 里的真 crate 当目标 → `--report` 会点「运行」，
//          那一页自动补契约 + 跑一次 → 我往别人的 crate 里写了 `env.contract.json` 和 `.jingwei/`
//   34 轮  绕过 `assertExeFresh()` → 拿一个**两小时前**的 exe 下结论，
//          于是"界面说 1 条、CLI 说 2 条"看起来像 bug，其实是同一个引擎两份时间
//   36 轮  拿 `--tab machine` 去量 `#audit`（它其实在**概览页**）→ 读到 `[0,0,0,0]`，
//          记成"内容被折叠"，其实是量错了页
//   40 轮  读拍平的 `textContent` → 序号徽章 `1` 与正文"1 个依赖没装"粘成 "11 个"，
//          差点写下一个不存在的 bug
//
// **四个坑有一个共同点：错的不是产品，是我的量具。**
// 所以量具必须固化——**每轮重写量具，就每轮重犯同样的错**。
//
// ## 它替你守住四件事
//
//   1. **只打副本**：目标目录先复制到临时目录，探针只碰副本（挡住 33 轮那个污染）
//   2. **exe 必须新**：比 `ui/` / `src-tauri/src` / `crates/` 都新，否则**拒绝测**（挡住 34 轮）
//   3. **量错页要提示**：零尺寸的元素会附一句"它可能不在当前页"（挡住 36 轮）
//   4. **读结构化文本**：`text` 是 `innerText`（带换行），不是拍平的 `textContent`（挡住 40 轮）
//
// ## 用法
//
//   node tools/probe.mjs <目录> [--tab home|graph|machine|...] [--keep] [--json]
//
//   --keep   跑完不删副本（想自己翻里面的产物时用）
//   --json   只打 JSON，不写人看的表
//
// 退出码：0 = 拿到了报告；2 = 起不来或被前置检查挡住（**不是**产品的问题）。

import { spawn, spawnSync } from 'node:child_process';
import {
  cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const EXE = join(ROOT, 'target', 'release', 'jingwei-gui.exe');
const REPORT = join(tmpdir(), `jw-probe-report-${process.pid}.json`);
const SKIP = new Set(['target', 'node_modules', '.git', 'dist', '.jingwei']);

const argv = process.argv.slice(2);
const flag = (n) => argv.includes(n);
const value = (n) => {
  const i = argv.indexOf(n);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : null;
};

const target = argv.find((a) => !a.startsWith('--') && a !== value('--tab'));
if (!target) {
  console.error('用法：node tools/probe.mjs <目录> [--tab home] [--keep] [--json]');
  process.exit(2);
}
const src = resolve(target);
if (!existsSync(src) || !statSync(src).isDirectory()) {
  console.error(`✗ 目录不存在：${src}`);
  process.exit(2);
}
const tab = value('--tab') || 'home';

/** ① exe 必须比所有会改变它行为的源码都新。见 34 轮那个坑。 */
function newestSource() {
  let newest = 0;
  const walk = (dir) => {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir)) {
      if (SKIP.has(name)) continue;
      const p = join(dir, name);
      const st = statSync(p);
      if (st.isDirectory()) walk(p);
      else if (/\.(rs|js|html|css)$/.test(name)) newest = Math.max(newest, st.mtimeMs);
    }
  };
  // `crates/**` 也算：`jingwei-app` 依赖 `jingwei-engine`，改 crates 就改了 GUI 的行为
  for (const d of ['src-tauri/src', 'ui', 'crates']) walk(join(ROOT, d));
  return newest;
}

if (!existsSync(EXE)) {
  console.error(`✗ 没有 ${relative(ROOT, EXE)}——先 cargo build --release -p jingwei-app`);
  process.exit(2);
}
const exeMs = statSync(EXE).mtimeMs;
const newest = newestSource();
if (exeMs + 60_000 < newest) {
  console.error(
    `✗ exe 比源码旧 ${((newest - exeMs) / 60000).toFixed(1)} 分钟——`
    + '先 cargo build --release -p jingwei-app\n'
    + '  （拿旧二进制测出来的结论没有意义——见 AGENTS.md 第 34 轮）',
  );
  process.exit(2);
}

/** ② 只打副本。见 33 轮那个污染。 */
const copy = join(tmpdir(), `jw-probe-${process.pid}-${src.split(/[\\/]/).pop()}`);
rmSync(copy, { recursive: true, force: true });
mkdirSync(copy, { recursive: true });
cpSync(src, copy, {
  recursive: true,
  filter: (p) => !SKIP.has(p.split(/[\\/]/).pop()),
});
if (!flag('--json')) {
  console.log(`  副本：${copy}`);
  console.log('  （探针会点按钮、会写契约——只碰副本，不碰原目录）');
}

/** ③ 跑窗口，等报告。 */
function run() {
  return new Promise((done) => {
    const child = spawn(EXE, [copy, '--tab', tab, '--report', REPORT], {
      detached: false, stdio: 'ignore',
    });
    const started = Date.now();
    const tick = setInterval(() => {
      if (existsSync(REPORT)) {
        clearInterval(tick);
        // 报告写了之后界面还要画完，给它一点时间
        setTimeout(() => { try { child.kill(); } catch { /* 已经退了 */ } done(true); }, 2500);
        return;
      }
      if (Date.now() - started > 90_000) {
        clearInterval(tick);
        try { child.kill(); } catch { /* 已经退了 */ }
        done(false);
      }
    }, 400);
  });
}

const ok = await run();
let reps = null;
if (ok && existsSync(REPORT)) {
  try { reps = JSON.parse(readFileSync(REPORT, 'utf8')); } catch { reps = null; }
}
if (!reps) {
  console.error('✗ 窗口 90 秒内没交出报告——它没起来，或者自检脚本没发出去');
  process.exit(2);
}
const list = Array.isArray(reps) ? reps : [reps];

if (flag('--json')) {
  console.log(JSON.stringify({ copy, reports: list }, null, 2));
} else {
  for (const rep of list) {
    console.log(`\n  页签 ${rep.tab || '?'}  活动页 ${rep.activeView || '?'}  视口 ${(rep.viewport || []).join('x')}`);
    for (const it of rep.items || []) {
      const [x, y, w, h] = it.rect || [0, 0, 0, 0];
      const zero = w === 0 && h === 0;
      const head = `  ${zero ? '·' : '▣'} ${String(it.sel).padEnd(16)} ${zero ? '[0,0,0,0]' : `[${x},${y},${w}x${h}]`}`;
      // ④ 零尺寸多半是**量错页**：元素在别的 `view-*` 里，当前页隐藏它。
      const hint = zero
        ? '  ← 零尺寸。若它不在当前页，就是**量错页了**（见 AGENTS.md 第 36 轮），不是产品坏了'
        : '';
      console.log(head + hint);
      // 打印 `visible`（尊重布局，带换行）——`text` 是拍平的，只用来判断"有没有"。
      const shown = it.visible || it.text;
      if (shown) console.log(`      ${String(shown).replace(/\n+/g, ' ⏎ ').slice(0, 220)}`);
    }
  }
}

rmSync(copy, { recursive: true, force: true });
rmSync(REPORT, { force: true });
process.exit(0);
