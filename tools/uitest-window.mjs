// 界面门禁：**真的拉起窗口，问它自己"那些东西在不在、看不看得见、画在哪"**。
//
// ## 为什么必须有这一道（它是拿两次"看着像 A、其实是 B"换的）
//
// 外壳测试（`uitest.mjs`）在浏览器里跑桩数据、读的是 `textContent`——
// **元素隐藏时照样读得到**。所以"元素在不在、多大、画在哪、有没有被挤出去"
// 这一整类问题，它一条都验不了。两次真事：
//
//   1. 打开"装着好几个项目的父目录"时，界面那句「✓ 环境看着没问题」是误导
//      ——测试全绿，是**截图**才看出来的；
//   2. `--tab graph` 深链接切页抢在"打开项目"之前跑，`S.root` 还没赋值，
//      于是页签亮了、画布在，而**数据一条都没拉**——也是截图看着不对，
//      靠 `--report` 那句 `#ghSub: 正在读取…` 才钉死根因。
//
// 这一道把第 2 类变成自动的：拉起窗口 → 让它把 DOM 状态写成 JSON → 比对不变量。
//
// ## 它不验什么
//
// 不验"好不好看"。好不好看没法判定；这里只验**可判定的**：
// 该在的在不在、可不可见、几何有没有落到窗口里、那块区域有没有卡在初始文案。
//
// ## 用法
//
//   node tools/uitest-window.mjs          对着仓库自己跑（不需要额外夹具）
//   node tools/uitest-window.mjs --keep   出错时留着窗口不关（人工看现场）

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync, statSync, readdirSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const KEEP = process.argv.includes('--keep');
const EXE = join(ROOT, 'target', 'release', 'jingwei-gui.exe');
const CLI = join(ROOT, 'target', 'release', 'jingwei.exe');
const REPORT = join(tmpdir(), `jw-ui-report-${process.pid}.json`);

const problems = [];
const notes = [];
const ok = (name, cond, detail) => {
  if (cond) notes.push(`  ok   ${name}`);
  else problems.push(`${name}${detail ? '   (' + detail + ')' : ''}`);
};

/// 造一个**跑过一次**的夹具项目。
///
/// 为什么不能拿仓库自己测：「最近运行」那张卡片在"没跑过"时按设计是收起来的，
/// 而"收起来了"与"数据没拉"**在 DOM 上长得一模一样**——拿一个没跑过的项目测，
/// 这条断言等于没测。所以先在临时目录里跑一次，让卡片必须真的长出来。
///
/// 为什么用命令行的 `test` 来造：那正是产品自己的路径（不是我们替它写数据），
/// 顺带把"命令行跑过 → 界面看得见"这条链也验了。
function makeFixture() {
  const dir = join(tmpdir(), `jw-ui-fixture-${process.pid}`);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  // 一条**不依赖任何解释器**的命令，而且它**必然以非 0 退出**。
  //
  // 为什么要这么抠：第一版用的是 `python -m unittest ...`——那台机器没装 Python 时
  // 命令起不来，运行记录照样会产生，但**产物形态跟有 Python 时不一样**，
  // 于是这道门禁的结论会随机器而变。门禁的结果不该取决于碰巧装了什么。
  //
  // `exit_code: 0` 配上"必然失败"的命令，得到的是确定的"一次失败的运行"——
  // 那正是「最近运行」卡片要显示的东西，而且是**最该显示**的那种（失败）。
  const failCmd = process.platform === 'win32'
    ? 'cmd /c exit 7'
    : 'sh -c "exit 7"';
  writeFileSync(
    join(dir, 'env.contract.json'),
    JSON.stringify({
      schema_version: 1,
      name: 'ui-fixture',
      commands: { run: failCmd },
      ready_when: { exit_code: 0, timeout_s: 30 },
    }, null, 2),
    'utf8',
  );
  // **一个产物目录**（2026-10-17）：让 `gitignore-missing` 在夹具上**必然成立**——
  // 否则「体检条目在界面上真的画出来了吗」这条断言没有确定的原料，
  // 只能写成"非空就行"，那等于什么都没验。
  mkdirSync(join(dir, 'target'), { recursive: true });
  writeFileSync(join(dir, 'target', 'junk.bin'), 'x', 'utf8');
  if (existsSync(CLI)) {
    const r = spawnSync(CLI, ['run', '--root', dir], { stdio: 'ignore' });
    // 期望它"失败"——产物是一次非 0 退出的运行。真跑成 0 反而说明夹具不对。
    if (r.status === 0) notes.push('  · 夹具那次 run 竟然退出码 0——夹具本身有问题，查一下');
  }
  return dir;
}

/// 造一个"**装着好几个项目**"的目录——也就是 2026-10-16 那个缺陷的原始场景。
///
/// 判据是"直接子目录里 ≥2 个各自带项目标记"，所以这里放两个带标记的子目录。
/// 标记用 `requirements.txt` 与 `Cargo.toml`，两个不同语言——
/// 正好复现当时那句误导：「项目类型 Python」而依赖清单里躺着 Rust 的 Cargo.toml。
function makeContainerFixture() {
  const dir = join(tmpdir(), `jw-ui-container-${process.pid}`);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, 'proj_py'), { recursive: true });
  mkdirSync(join(dir, 'proj_rs'), { recursive: true });
  writeFileSync(join(dir, 'proj_py', 'requirements.txt'), '', 'utf8');
  writeFileSync(join(dir, 'proj_py', 'main.py'), 'print(1)\n', 'utf8');
  writeFileSync(join(dir, 'proj_rs', 'Cargo.toml'), '[package]\nname = "x"\n', 'utf8');
  return dir;
}

/** exe 比源码旧就不测——**别拿旧二进制下结论**（那是交付门禁那次的教训）。 */
function assertExeFresh() {
  if (!existsSync(EXE)) {
    problems.push(`没有 ${relative(ROOT, EXE)}——先 cargo build --release -p jingwei-app`);
    return false;
  }
  const exeMs = statSync(EXE).mtimeMs;
  let newest = 0;
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      if (['target', 'node_modules', '.git', 'dist'].includes(name)) continue;
      const p = join(dir, name);
      const st = statSync(p);
      if (st.isDirectory()) walk(p);
      else if (/\.(rs|js|html|css)$/.test(name)) newest = Math.max(newest, st.mtimeMs);
    }
  };
  walk(join(ROOT, 'src-tauri', 'src'));
  walk(join(ROOT, 'ui'));
  // **`crates/**` 也要扫**（2026-10-17，拿真 crate 当探针时撞出来的）。
  //
  // 这条检查原来只看 `src-tauri/src` 与 `ui`——而 `jingwei-app` **依赖 jingwei-engine**，
  // 在 `crates/engine/**` 里加一条规则，GUI 的行为就变了，可**exe 的时间戳没变**。
  // 实测：`target/release/jingwei-gui.exe` 停在 11:20，而 `crates/engine/src` 改到 13:18，
  // 这道闸照样绿——**我于是拿一个两小时前的二进制给"界面显示几条"下了结论**，
  // 结论是错的（界面 1 条 vs CLI 2 条，差异全是旧 exe）。
  //
  // 教训与"交付门禁 staleness 恒真"同一条：**检查的原料集写窄了，闸就形同虚设**。
  walk(join(ROOT, 'crates'));
  if (exeMs + 60_000 < newest) {
    problems.push(
      `exe 比源码旧 ${((newest - exeMs) / 60000).toFixed(1)} 分钟——先重新构建再测`
      + `（拿旧二进制测出来的"通过"没有意义）`,
    );
    return false;
  }
  return true;
}

/**
 * 报告够了没有？**"能解析"不等于"页面画好了"。**
 *
 * ## 这一条是补上次的一笔假账（2026-10-16，第 28 轮）
 *
 * `AGENTS.md` §3 第 7 条写着「**等条件，不等固定时长**：脚本自己轮询"页面就绪"
 * （图谱页看 `#ghSub` 不再停在初始文案；**其它页看主卡真的画出来**），12 秒兜底」。
 * **那段话只兑现了一半**：`#ghSub` 那条真的在（下面 `tab === 'graph'` 那个分支），
 * 而"其它页看主卡真的画出来"**代码里根本没有**——`waitReport` 只判
 * "报告是个数组、首条有 items"，而窗口在**卡片还空着**的时候就能交出这样一份报告。
 *
 * 后果就是第 28 轮实测到的那条红：概览页读到 `""`（容器目录那条），
 * 而同一个窗口我手工拉起来时卡片明明写着「这里装着好几个项目，不是一个项目」——
 * **产品是对的，判据在抢跑。** 而症状长得像"产品坏了"，于是查的方向也跟着错。
 *
 * ## 判据
 *
 * 按**当前是哪一页**看那一页该有的东西有没有内容：
 * - `view-home` → `#statusCard` 的文本非空（那是主卡，空 = 还没画）
 * - `view-graph` → `#ghSub` 不在初始文案上
 * - 别的页不熟，只要有 items 就放过（不越权替它们定判据）
 *
 * 不满足就继续等，由外层的硬上限兜底——**等不到就交一份"卡住了"的报告**，
 * 那比交不出来有用（门禁正是靠它看出「正在读取…」的）。
 */
function reportReady(rep) {
  if (!Array.isArray(rep) || rep.length === 0 || !Array.isArray(rep[0].items)) return false;
  const page = rep[0];
  const text = (sel) => {
    const it = (page.items || []).find((i) => i.sel === sel);
    return it && typeof it.text === 'string' ? it.text : null;
  };
  if (page.activeView === 'view-home') {
    const t = text('#statusCard');
    return t !== null && t.trim().length > 0;
  }
  if (page.activeView === 'view-graph') {
    const t = text('#ghSub');
    return t !== null && !/正在读取/.test(t);
  }
  return true;
}

/** 轮询等报告落盘。**上限必须硬**：窗口起不来时不能干等。 */
function waitReport(ms) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    if (existsSync(REPORT) && statSync(REPORT).size > 10) {
      try {
        // 去 BOM：PowerShell / Rust 写出来的可能是 UTF-8 BOM，JSON.parse 会当场炸
        const raw = readFileSync(REPORT, 'utf8').replace(/^\uFEFF/, '');
        const rep = JSON.parse(raw);
        // **报告是一个数组**：每个页签一条 `{ tab, activeView, navOn, viewport, items }`
        // （见 `src-tauri/src/selfcheck.rs` 的 `out.push(snapshot(tab))`）。
        if (reportReady(rep)) return rep;
        // 还不够就继续等。**这一条兜的是"可解析但还没画好"**——
        // 旧判据（`size > 10` + 有没有 items）会静默地把那种报告交给后面所有断言，
        // 而断言会一条条报"元素不存在"或"读到空串"，把根因指向错的地方。
      } catch {
        /* 还在写，继续等 */
      }
    }
    spawnSync(process.execPath, ['-e', 'setTimeout(()=>{},120)']);
  }
  return null;
}

function pick(rep, sel) {
  return ((rep && rep.items) || []).find((i) => i.sel === sel) || null;
}

/// 报告现在是一串（每页一条）。给"只想看一页"的调用点一个统一的取法。
function firstPage(reps) {
  if (!reps) return null;
  return Array.isArray(reps) ? (reps[0] || null) : reps;
}

const visible = (it) => !!it && it.exists && it.display !== 'none' && !it.hidden && it.rect[2] > 0 && it.rect[3] > 0;

/// 起一次窗口、走一遍、等它交报告、然后**一定关掉**。窗口这种事最怕漏关。
///
/// ## 超时时要说清"窗口怎么了"（2026-10-16 第 29 轮加）
///
/// 原来超时只报一句「窗口 90 秒内没交出报告——它没起来，或者自检脚本没发出去（看 dlog）」，
/// 而**"看 dlog"是个空指引**：那句话把两种完全不同的失效合成了一句，
/// 于是每次偶发（实测约 1/3）都只能靠猜。
///
/// 现在把能测的都测下来：
///   - 进程**还在不在**？—— 起来了但卡住 vs 根本没起来，是两回事
///   - 退出码是多少？—— **已经退出**说明它启动就失败了（参数错、目录不存在、单实例锁）
///   - 报告文件**存在吗**？—— 存在但内容不能解析（写了一半），与"根本没写"不同
function runWindow(args) {
  rmSync(REPORT, { force: true });
  let exited = null;
  const child = spawn(EXE, [...args, '--report', REPORT], { detached: false, stdio: 'ignore' });
  child.on('exit', (code, sig) => {
    exited = sig ? `被信号 ${sig} 结束` : `退出码 ${code}`;
  });
  const kill = () => {
    if (KEEP && problems.length) return;
    try { child.kill(); } catch { /* 已经退了 */ }
    try { spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); } catch { /* 同上 */ }
  };
  try {
    const rep = waitReport(90_000);
    if (!rep) {
      const why = [];
      why.push(exited ? `**窗口自己退了**（${exited}）——启动就失败` : '窗口进程还活着（起来了但没交报告）');
      why.push(existsSync(REPORT)
        ? `报告文件存在（${statSync(REPORT).size} 字节）但读不出条目——多半写了一半`
        : '报告文件**根本不存在**——自检脚本没跑完（它只在扫完所有页之后写一次）');
      why.push(`参数：${args.join(' ')}`);
      problems.push(`窗口 90 秒内没交出报告：${why.join('；')}`);
    }
    return rep;
  } finally {
    kill();
  }
}

/// 每页必须**真的画出来**的那块区域。
///
/// 为什么挑这些：它们都是"这一页有没有东西"的抓手——
/// 空的、没接上数据的、被挤没的，几何尺寸会直接说话，而 `textContent` 说不出来。
/// `#edFiles` 对空项目可能确实没有文件（那不一定是缺陷），所以它只查"在不在"。
const TAB_EXPECT = {
  home: ['#view-home', '#statusCard'],
  run: ['#view-run'],
  editor: ['#view-editor'],
  note: ['#view-note'],
  graph: ['#view-graph', '#graphCv', '#graphHud'],
  machine: ['#view-machine'],
  clean: ['#view-clean'],
};

/// **把六个页签挨个点一遍**，每页都问：切过去了吗？那一页真的画出来了吗？
///
/// 为什么不是"一页开一次窗口"：外壳有六页，一页一次冷启动就是六次；
/// 而"挨个点一遍"本来就是用户的动作——**更真，也更便宜**。
function checkSweep(fixture) {
  const tabs = Object.keys(TAB_EXPECT);
  const before = problems.length;
  const reps = runWindow([fixture, '--tab', tabs[0], '--sweep', tabs.join(',')]);
  if (!reps) {
    problems.push('页签巡查：窗口没交出报告');
    return;
  }
  const byTab = new Map((Array.isArray(reps) ? reps : [reps]).map((r) => [r.tab, r]));

  for (const tab of tabs) {
    const r = byTab.get(tab);
    if (!r) {
      problems.push(`页签巡查：没拿到「${tab}」那一页的报告`);
      continue;
    }
    // ① 真的切过去了——**看 `#nav` 上也真的高亮了**，不只是 `.view.on` 对了。
    //    这两样会脱钩（`switchTab` 里两个 classList 各改一处）。
    if (r.activeView !== 'view-' + tab) {
      problems.push(`页签巡查：「${tab}」没切过去（当前 ${r.activeView}）`);
      continue;
    }
    if (r.navOn !== tab) {
      problems.push(`页签巡查：「${tab}」切过去了，但导航按钮没跟着高亮（高亮在 ${r.navOn}）`);
    }
    // ② 那一页该有的区域真的画出来了
    for (const sel of TAB_EXPECT[tab]) {
      const it = (r.items || []).find((i) => i.sel === sel);
      const shown = it && it.exists && it.display !== 'none' && it.rect[2] > 0 && it.rect[3] > 0;
      if (!shown) {
        problems.push(
          `页签巡查：「${tab}」的 ${sel} 没画出来`
          + `（${it ? JSON.stringify(it.rect) + ' display=' + it.display : '元素不存在'}）`,
        );
      }
    }
    // ③ 那一页**没有**零尺寸的异常信号：图谱页的状态卡不许停在初始文案
    if (tab === 'graph') {
      const sub = (r.items || []).find((i) => i.sel === '#ghSub');
      if (sub && /正在读取/.test(sub.text || '')) {
        problems.push('页签巡查：图谱页的状态卡停在初始文案（数据没拉）');
      }
    }
  }
  // 只有**这一遍真的一条问题都没出**才报 ok。
  // （第一版这里是无条件 push——失败时照样打一行 `ok`，那是谎报。）
  if (problems.length === before) {
    notes.push(`  ok   ${tabs.length} 个页签挨个点过一遍：都切过去了、导航跟着高亮、该画出来的都画出来了`);
  }
}

/// 概览页：**前六轮最贵的那个缺陷就发生在这里**，而图谱页那一段完全覆盖不到。
///
/// 2026-10-16 打开一个"装着好几个项目"的父目录时，这张卡写着「✓ 环境看着没问题」，
/// 而同一屏的依赖清单里混着两个别的项目。截图才看出来。
/// 这一段把它变成自动的：**当前页必须是概览、主卡必须真的画出来、
/// 而且卡里那句话必须与"这是什么目录"对得上。**
function checkHome(fixture, containerFixture) {
  // ① 普通项目：主卡要画出来，而且**不能说"这里装着好几个项目"**（那是误报）
  let rep = firstPage(runWindow([fixture, '--tab', 'home']));
  if (!rep) {
    problems.push('概览页：窗口没交出报告');
    return;
  }
  ok('概览页：深链接生效', rep.activeView === 'view-home', String(rep.activeView));

  // ── **一句话指南（`#guideCard`）**：2026-10-16 起它是概览页**主位** ──────
  //
  // 为什么它也要被门禁守：它是新加的那一块，而**"新加的东西没人守"是这个仓库
  // 最熟的一种失守**（`AGENTS.md` 里记着五次"决定记在文档上、门禁上什么都没发生"）。
  // 而且它比状态卡更容易"看起来有、其实没画"——它整块是 JS 拼出来的 innerHTML，
  // 数据缺一个字段就会渲染成空 div，**而空的 div 不报错**。
  const guide = pick(rep, '#guideCard');
  ok('概览页：指南卡真的画出来了（不是 0 尺寸、不是隐藏）',
    visible(guide), guide ? JSON.stringify(guide.rect) : '不存在');
  const gtext = (guide && guide.text) || '';
  ok('概览页：指南卡必须有内容（空 = 数据没喂到）', gtext.length > 0, JSON.stringify(gtext.slice(0, 40)));
  // 三态那一句必须**是三种之一**，不能是别的东西——它是这一块存在的理由。
  ok('概览页：指南说的是三种结论之一（能跑 / 跑不起来 / 还查不出来）',
    /现在就能跑|现在跑不起来|还查不出来/.test(gtext), JSON.stringify(gtext.slice(0, 40)));
  // **"能跑"就必须给出命令**：只断言状态不给命令，等于把"该敲什么"又推回给用户。
  if (/现在就能跑/.test(gtext)) {
    ok('概览页：说"能跑"就一定要给出要敲的命令',
      /下一步/.test(gtext) && gtext.length > 20, JSON.stringify(gtext.slice(0, 60)));
  }
  // **"还查不出来"必须说清该谁去补**——少了它，用户不知道该不该动手。
  if (/还查不出来/.test(gtext)) {
    ok('概览页："查不了"要说清是谁的问题（不能只报一句查不了）',
      gtext.length > 20, JSON.stringify(gtext.slice(0, 60)));
  }

  const card = pick(rep, '#statusCard');
  // **这一条最值钱**：主卡是 `display:flex` 却在窗口外/尺寸为 0，就是"看着有、其实没画出来"
  // ——`textContent` 那类断言永远发现不了（`uitest.mjs` 就是这么漏掉它的）。
  ok('概览页：主卡真的画出来了（不是 0 尺寸、不是隐藏）', visible(card), card ? JSON.stringify(card.rect) : '不存在');
  const cardText = (card && card.text) || '';
  ok(
    '概览页：普通项目**不许**说「装着好几个项目」（那是误报）',
    !/装着好几个项目/.test(cardText),
    JSON.stringify(cardText.slice(0, 40)),
  );

  // ② 容器目录：反过来，**必须**说，而且不许说「环境看着没问题」
  rep = firstPage(runWindow([containerFixture, '--tab', 'home']));
  if (!rep) {
    problems.push('概览页（容器目录）：窗口没交出报告');
    return;
  }
  const ccard = pick(rep, '#statusCard');
  const ctext = (ccard && ccard.text) || '';
  ok('概览页：容器目录要说出「装着好几个项目」', /装着好几个项目/.test(ctext), JSON.stringify(ctext.slice(0, 40)));
  ok(
    '概览页：容器目录**不许**说「环境看着没问题」',
    !/环境看着没问题/.test(ctext),
    JSON.stringify(ctext.slice(0, 40)),
  );
  const notes = pick(rep, '#cardNotes');
  // 容器目录下那几行小字必须清空——它们的每一条都是按"一个项目"的口径写的，
  // 最典型的一句是「怎么跑：还没有契约」，而契约在子项目里明明有。
  ok(
    '概览页：容器目录下那几行小字是空的',
    !notes || (notes.text || '').trim() === '',
    notes ? JSON.stringify((notes.text || '').slice(0, 30)) : '不存在',
  );
}

async function main() {
  if (!assertExeFresh()) return;

  // 拿**跑过一次的夹具**来测：图谱那两张卡片必须真的长出来。
  // （拿仓库自己测的话，「最近运行」是收起的，而"收起"与"没拉到数据"在 DOM 上一样。）
  const fixture = makeFixture();
  const containerFixture = makeContainerFixture();
// ── 机器页：**条目真的画出来了吗**（2026-10-17 加）─────────────────────────
  //
  // 原来 `SEL` 里只有 `#view-machine`——只验了"这一页可见"。
  // 而账本里那条「界面门禁'等页面就绪'只兑现了一半」就是这个形状：
  // **视图在了、内容是空的，门禁照样绿**。
  //
  // 夹具这一轮加了一个 `target/`、又没有 `.gitignore`，所以
  // `gitignore-missing` **必然成立**——断言因此有确定的原料，
  // 不是"非空就行"（那种断言几乎什么都放过）。
  //
  // 判据：① **概览页**的体检清单**看得见**；② 里面有**我们已经知道会有的那一条**。
  {
    // **页要对**：`#audit` 在**概览页**（`view-home`），`#probList` 在**笔记页**。
    // 第一版我拿 `--tab machine` 去量这两个——它们在那一页是隐藏的，
    // 于是 `rect` 全是 `[0,0,0,0]`，我一度以为是产品把内容折叠了。
    // **是断言挑错了页，不是产品坏了**（`ui/index.html`：`#audit` 在 `view-home`，
    // `#probList` 在 `view-note`）。教训：**断言的"在哪一页"也是判据的一部分。**
    const rep = firstPage(runWindow([fixture, '--tab', 'home']));
    if (!rep) {
      problems.push('概览页/笔记页：窗口没交出报告');
    } else {
      const audit = pick(rep, '#audit');
      const prob = pick(rep, '#probList');
      // **`#audit` 的尺寸这一轮暂时不断言**（2026-10-17）。
      //
      // 它**有内容**，但 `rect` 是 `[0,0,0,0]`——一个装了内容却零尺寸的容器，
      // 意味着**用户在屏幕上看不见它**。这很可疑，但我这一轮查不出根因
      // （HTML 里它是个普通 `<div id="audit">`，内容由 `renderAudit` 填）。
      // 所以：**断言内容在**（这条能钉住），**把它记成待查的债**，不假装已解决。
      // 已记进 `AGENTS.md` §5.2。
      ok(
        '概览页：#audit 真的画出来了（正确的页 + 不是 0 尺寸 + 有内容）',
        audit && audit.exists && !audit.hidden && audit.rect[2] > 0 && audit.rect[3] > 0,
        audit ? JSON.stringify(audit.rect) : '不存在',
      );
      ok(
        '概览页：#audit 里有内容（体检条目真的填进去了）',
        !!(audit && audit.exists && (audit.text || '').trim().length > 0),
        audit ? JSON.stringify((audit.text || '').slice(0, 60)) : '不存在',
      );
      // 夹具必然产出的那一条：没有 .gitignore，却有 target/
      ok(
        '概览页：已知会有的体检条目真的出现在 DOM 里（没有 .gitignore + 有 target/）',
        // **用 `parts`，不只读拍平的 `text`**（2026-10-17）：
        // `text` 改成 `innerText` 之后带换行、且有 600 字上限——
        // 只读 `text` 会被截掉，**红得跟产品无关**。
        /gitignore/i.test((audit && (audit.text || '')) + '\n' + ((audit && audit.parts) || []).join('\n')),
        audit ? JSON.stringify((audit.text || '').slice(0, 80)) : '不存在',
      );
      ok(
        '笔记页：#probList 的容器没丢（它不在概览页，这里只验存在）',
        !!(prob && prob.exists),
        prob ? '在' : '不存在',
      );
    }
  }


  const rep = firstPage(runWindow([fixture, '--tab', 'graph']));
  if (!rep) {
    problems.push('窗口 90 秒内没交出报告——它没起来，或者自检脚本没发出去（看 dlog）');
  } else {
    ok('窗口起来了，报告拿到了', true);
    ok('深链接生效：当前页是图谱', rep.activeView === 'view-graph', String(rep.activeView));

    const vw = rep.viewport || [0, 0];
    const inView = (it) => {
      const [x, y, w, h] = it.rect;
      return x >= 0 && y >= 0 && x + w <= vw[0] + 1 && y + h <= vw[1] + 1;
    };

    const cv = pick(rep, '#graphCv');
    ok('画布在且可见', visible(cv), cv ? JSON.stringify(cv.rect) : '不存在');
    ok('画布没有被挤出窗口', cv && inView(cv), cv ? JSON.stringify(cv.rect) : '');

    const hud = pick(rep, '#graphHud');
    ok('图上那张状态卡在且可见', visible(hud), hud ? JSON.stringify(hud.rect) : '不存在');
    ok('状态卡没有被挤出窗口', hud && inView(hud), hud ? JSON.stringify(hud.rect) : '');

    const sub = pick(rep, '#ghSub');
    // **这条是核心**：卡在初始文案就说明 invoke 没回来——
    // 也就是"页切过去了但数据没拉"那个竞态。元素在、也不隐藏，光看存在与否看不出来。
    ok(
      '状态卡不再停在初始文案（数据真的拉了）',
      !!sub && !/正在读取/.test(sub.text || ''),
      sub ? JSON.stringify(sub.text) : '不存在',
    );

    const runs = pick(rep, '#ghRuns');
    // 夹具**跑过一次**，所以这张卡片必须真的长出来并且可见。
    // 原来这里是"hidden 就放过"——而"收起了"与"没拉到数据"在 DOM 上长得一样，
    // 那条写法等于没测（写完当场就发现了）。
    ok('最近运行卡片在且可见（夹具跑过一次，它没理由收起）', visible(runs), runs ? JSON.stringify(runs.rect) : '不存在');
    ok('最近运行卡片没有被挤出窗口', runs && inView(runs), runs ? JSON.stringify(runs.rect) : '');
    ok(
      '最近运行卡片里有内容',
      !!runs && /次/.test(runs.text || ''),
      runs ? JSON.stringify(runs.text) : '',
    );
    ok('窗口视口不是 0（窗口真的开出来了）', vw[0] > 400 && vw[1] > 300, vw.join('x'));
  }

  // 概览页那一段（前六轮最贵的缺陷就发生在那里，图谱页完全覆盖不到）
  checkHome(fixture, containerFixture);

  // **六个页签挨个点一遍**——外壳有六页，前面那两段只覆盖了两页。
  checkSweep(fixture);

  rmSync(fixture, { recursive: true, force: true });
  rmSync(containerFixture, { recursive: true, force: true });
}

await main();

for (const n of notes) console.log(n);
if (problems.length) {
  console.log(`\n界面门禁未通过（${problems.length} 条）：`);
  for (const p of problems) console.log('   × ' + p);
  process.exit(1);
}
console.log('\n界面门禁通过：图谱页该在的都在、都可见、都没被挤出去、数据真的拉了；'
  + '概览页的主卡真的画出来了，而且"普通项目"与"装着好几个项目的目录"说对了话。');
