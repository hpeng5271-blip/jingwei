// **包冒烟**（2026-10-20 起；2026-10-21 改成跟新包形状）：取包 → 真跑 → 断言。
//
// 包形状变了两次，这个脚本也得跟着变（不然它测的是"上一版的包"）：
// - 2026-10-20 之前：zip，里面带三套演示项目；
// - 现在：`dist\<yyyy-MM-dd>\` 目录，只放 `jingwei-gui.exe` + `jingwei.exe` + 说明书，**不带演示项目**。
//
// 所以演示项目改成**脚本自己铺**（临时目录），断言不变：
// ① `index` 真的解析了东西（有规模行）② `positions` 的"对不上"为 0 ③ `rename --write` 回读通过。
// 用法：node tools/smoke-pack.mjs [--gui] [包目录或 zip 路径]
import { execFileSync, spawn, execSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);

/// WebView2 子进程数：Windows 上能用 CIM 查；别的平台返回 -1（不做这个断言）。
async function countWebView2Children(pid) {
  if (process.platform !== 'win32') return -1;
  try {
    const out = execSync(
      `powershell -NoProfile -Command "(Get-CimInstance Win32_Process -Filter 'ParentProcessId=${pid}' | Where-Object { $_.Name -eq 'msedgewebview2.exe' } | Measure-Object).Count"`,
      { encoding: 'utf8', timeout: 20000 },
    );
    return Number(out.trim()) || 0;
  } catch {
    return 0;
  }
}

/// 窗口标题：拿不到就回空串（不当成失败依据）。
function winTitle(pid) {
  if (process.platform !== 'win32') return '';
  try {
    const out = execSync(`powershell -NoProfile -Command "(Get-Process -Id ${pid}).MainWindowTitle"`, {
      encoding: 'utf8',
      timeout: 20000,
    });
    return out.trim();
  } catch {
    return '';
  }
}
const givePath = args.find((a) => !a.startsWith('--'));

/// 最新的包：优先 `dist/<YYYY-MM-DD>/` 目录，其次 `dist/*.zip`。
function findPackage() {
  const dist = path.join(process.cwd(), 'dist');
  if (!fs.existsSync(dist)) return null;
  const names = fs.readdirSync(dist).filter((n) => !n.startsWith('_'));
  const dirs = names
    .filter((n) => /^\d{4}-\d{2}-\d{2}$/.test(n) && fs.existsSync(path.join(dist, n, 'jingwei.exe')))
    .sort();
  if (dirs.length) return { kind: 'dir', p: path.join(dist, dirs[dirs.length - 1]) };
  const zips = names.filter((n) => n.endsWith('.zip')).sort();
  if (zips.length) return { kind: 'zip', p: path.join(dist, zips[zips.length - 1]) };
  return null;
}

const chosen = givePath
  ? { kind: givePath.endsWith('.zip') ? 'zip' : 'dir', p: path.resolve(givePath) }
  : findPackage();
if (!chosen) {
  console.error('dist/ 下没有包（先跑 pack.ps1）');
  process.exit(2);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'jw-smoke-'));
/// 自铺演示项目：一个能被 rename 命中的符号 + 一次调用（positions 才有东西可核）。
function makeDemo(dir) {
  fs.mkdirSync(path.join(dir, 'src'), { recursive: true });
  fs.writeFileSync(
    path.join(dir, 'Cargo.toml'),
    '[package]\nname = "hello_rust"\nversion = "0.1.0"\nedition = "2021"\n',
  );
  fs.writeFileSync(
    path.join(dir, 'src', 'main.rs'),
    'fn greet() -> &\'static str { "hi" }\n\nfn main() {\n    println!("{}", greet());\n}\n',
  );
}

try {
  let root;
  if (chosen.kind === 'zip') {
    execFileSync(
      'powershell',
      ['-NoProfile', '-Command', `Expand-Archive -LiteralPath '${chosen.p}' -DestinationPath '${tmp}' -Force`],
      { stdio: 'pipe' },
    );
    root = tmp;
  } else {
    root = chosen.p;
  }
  const exe = path.join(root, 'jingwei.exe');
  if (!fs.existsSync(exe)) throw new Error(`包里没有 jingwei.exe：${root}`);

  const demo = path.join(tmp, 'demo');
  makeDemo(demo);

  const idx = execFileSync(exe, ['index', '--root', demo], { encoding: 'utf8' });
  if (!/个文件 ·/.test(idx)) throw new Error(`index 没给出规模行：${idx.slice(0, 200)}`);

  // `positions --json` 现在带协议（2026-10-21）：领域字段在里面，先取 `positions` 再退回旧形状。
  const pos = execFileSync(exe, ['positions', '--root', demo, '--json'], { encoding: 'utf8' });
  const wire = JSON.parse(pos);
  const rep = wire.positions || wire;
  const bad = Number(rep.bad || 0);
  if (bad !== 0) throw new Error(`positions 有 ${bad} 处对不上`);
  const checked =
    Number(rep.ok || 0) + bad + Number(rep.unknown || 0) + Number(rep.imprecise || 0);
  if (checked === 0) throw new Error('positions 一个符号都没核到');
  if (wire.state && wire.state !== 'yes' && wire.state !== 'no') {
    throw new Error(`positions 协议 state 异常：${wire.state}`);
  }

  // 重命名：拷一份 demo（别改包里的东西）→ index → rename --write → 回读。
  const copy = path.join(tmp, '__rename_demo');
  fs.cpSync(demo, copy, { recursive: true });
  execFileSync(exe, ['index', '--root', copy], { encoding: 'utf8' });
  const ren = execFileSync(exe, ['rename', 'greet', '--to', 'welcome', '--write', '--root', copy], {
    encoding: 'utf8',
  });
  if (!/回读通过/.test(ren)) throw new Error(`rename 没回读通过：${ren.slice(0, 300)}`);

  // GUI 也要能起来：起进程、等 3 秒、确认**真的画出界面**，再杀掉。
  //
  // 2026-10-22 修：以前只断言'进程 3 秒没退'，而 WebView2 运行时缺失时窗口标题是 'Error'、
  // 进程照样活着 —— 这条断言给了**假通过**。现在要求真有 WebView2 子进程。
  const gui = path.join(root, 'jingwei-gui.exe');
  if (args.includes('--gui')) {
    if (!fs.existsSync(gui)) throw new Error('包里没有 jingwei-gui.exe');
    const child = spawn(gui, [], { cwd: root, stdio: 'ignore', detached: false });
    let exited = false;
    child.on('exit', () => {
      exited = true;
    });
    await new Promise((r) => setTimeout(r, 4000));
    if (exited) throw new Error('GUI 起来 3 秒内就退出了（起不来）');
    // 真的画出界面 = 有 WebView2 子进程；没有就是运行时缺失（或窗口是 Error 页）。
    const wv = await countWebView2Children(child.pid);
    const title = winTitle(child.pid);
    child.kill();
    if (wv < 1) {
      throw new Error(`GUI 进程活着但没画出界面（WebView2 子进程 0 个，窗口标题 '${title}'）`);
    }
    console.log(`  GUI 冒烟通过：起来 4 秒没退，WebView2 子进程 ${wv} 个，窗口 '${title}'`);
  }

  console.log(`包冒烟通过：${path.basename(root)}`);
  console.log(`  demo 索引：${(idx.match(/.*个文件 ·.*/) || [''])[0].trim()}`);
  console.log(`  位置回读：核了 ${checked} 个符号，对不上 0`);
  console.log('  重命名冒烟通过：改完并回读');
} catch (e) {
  console.error(`包冒烟失败：${e.message || e}`);
  process.exit(1);
} finally {
  // GUI 刚被杀，文件句柄可能还占着：重试几次，失败也不影响结论。
  try {
    fs.rmSync(tmp, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  } catch {
    /* 临时目录残留不算失败 */
  }
}
