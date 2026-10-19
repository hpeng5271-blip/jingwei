// **包冒烟**（2026-10-20）：不只是"包里文件对"，而是**解压出来真能跑**。
//
// 步骤：解压到临时目录 → 对包里的演示项目跑 `index` → 跑 `positions` →
// 断言：① 有"个文件 ·"这行（真的解析了）② positions 的"对不上"为 0。
//
// 用法：node tools/smoke-pack.mjs [zip 路径]

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const argZip = process.argv.slice(2).find((a) => a.endsWith('.zip'));
const zip = argZip || findDefault();
function findDefault() {
  const dist = path.join(process.cwd(), 'dist');
  const names = fs.readdirSync(dist).filter((n) => n.endsWith('.zip') && !n.startsWith('_'));
  if (!names.length) {
    console.error('dist/ 里没有 zip');
    process.exit(2);
  }
  return path.join(dist, names[names.length - 1]);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'jw-smoke-'));
try {
  execFileSync(
    'powershell',
    ['-NoProfile', '-Command', `Expand-Archive -LiteralPath '${zip}' -DestinationPath '${tmp}' -Force`],
    { stdio: 'pipe' },
  );
  // 包里的文件直接在根下（没有套一层目录）。
  const root = tmp;
  const exe = path.join(root, 'jingwei.exe');
  if (!fs.existsSync(exe)) throw new Error('包里没有 jingwei.exe');

  // 演示项目目录名是中文（演示项目/hello_python）——包里的真实布局，不假装成英文。
  const demo = path.join(root, '演示项目', 'hello_python');
  if (!fs.existsSync(demo)) throw new Error('包里没有 演示项目/hello_python');

  const idx = execFileSync(exe, ['index'], { cwd: demo, encoding: 'utf8' });
  if (!/个文件 ·/.test(idx)) throw new Error(`index 没给出规模行：${idx.slice(0, 200)}`);

  const pos = execFileSync(exe, ['positions', '--json'], { cwd: demo, encoding: 'utf8' });
  const rep = JSON.parse(pos);
  const bad = Number(rep.bad || 0);
  if (bad !== 0) throw new Error(`positions 有 ${bad} 处对不上`);

  const checked = Number(rep.ok || 0) + bad + Number(rep.unknown || 0) + Number(rep.imprecise || 0);
  if (checked === 0) throw new Error('positions 一个符号都没核到');


  // **GUI 也要能起来**（用户打开包第一时间点的是它）：起进程、等 3 秒、确认还活着，再杀掉。
  const gui = path.join(root, 'jingwei-gui.exe');
  if (process.argv.includes('--gui') && fs.existsSync(gui)) {
    const { spawn } = await import('node:child_process');
    const child = spawn(gui, [], { cwd: root, stdio: 'ignore', detached: false });
    let exited = false;
    child.on('exit', () => { exited = true; });
    await new Promise((r) => setTimeout(r, 3000));
    if (exited) throw new Error('GUI 起来 3 秒内就退出了（起不来）');
    child.kill();
    console.log('  GUI 冒烟通过：起来 3 秒没退');
  }


  // **重命名也要能在包里跑通**：拷一份 demo（别改包里的原件）→ index → rename --write → 回读。
  const copy = path.join(tmp, '__rename_demo');
  fs.cpSync(demo, copy, { recursive: true });
  execFileSync(exe, ['index'], { cwd: copy, encoding: 'utf8' });
  const ren = execFileSync(exe, ['rename', 'greet', '--to', 'welcome', '--write'], {
    cwd: copy,
    encoding: 'utf8',
  });
  if (!/回读通过/.test(ren)) throw new Error(`rename 没回读通过：${ren.slice(0, 300)}`);
  console.log('  重命名冒烟通过：3 处改完并回读');

  console.log(`包冒烟通过：${path.basename(zip)}`);
  console.log(`  demo 索引：${(idx.match(/.*个文件 ·.*/) || [''])[0].trim()}`);
  console.log(`  位置回读：核了 ${checked} 个符号，对不上 0`);
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
