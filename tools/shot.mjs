// tools/shot.mjs -- 真窗口截图（2026-10-16 加）
//
// 为什么单独一个脚本：文档里要放"这个界面长什么样"，而**事故账本里
// `--shot` 那条被撤掉过**（试了 7 次没收敛，因为在 GUI 子系统里看不见错误）。
// 账本自己写了下次该怎么做——但它建议的两条路都要动产品代码。
//
// 这个脚本走**第三条路**：产品一行不改。
//   GUI 本来就支持 `--tab <名字>` 深链接（界面门禁一直在用它），
//   所以"打开到某一页"是现成的；"把那一页拍下来"交给 Windows 自己截屏。
//
// 用法：node tools/shot.mjs <输出目录>
// 产物：<输出目录>/<页签>.png，每个页签一张。
//
// **它不属于门禁**（不给 gates.ps1 调用）：截图是为了文档好看，
// 不是为了守行为；把它塞进门禁只会让门禁更慢、更容易假红。

import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, existsSync } from 'node:fs';
import { join, resolve } from 'node:path';

const outDir = resolve(process.argv[2] || 'docs-shots');
const root = resolve(import.meta.dirname, '..');
const exe = join(root, 'target', 'release', 'jingwei-gui.exe');
const project = resolve(root);

if (!existsSync(exe)) {
  console.error(`没有这个可执行文件：${exe}\n先跑：cargo build --release -p jingwei-app`);
  process.exit(1);
}
mkdirSync(outDir, { recursive: true });

/// 一页一张。名字就是界面里那七个页签（`ui/events.js` 的 `TABS`）。
const TABS = ['home', 'graph', 'run', 'editor', 'note', 'machine', 'clean'];

/// 等窗口起来。**等条件，不等进程结束**（GUI 不会自己退）。
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/// 截整屏。PowerShell 的 `System.Drawing`——**不依赖 WebView2 的内部 API**，
/// 所以它对"页面里发生了什么"零假设（这正是它比 `--shot` 稳的地方）。
///
/// ⚠️ **必须先把这个进程的窗口拉回前台**：脚本等待的这几秒里，
/// **别的窗口（终端、编辑器）会抢到前台**，于是截到的是它们——
/// 第一次跑就踩到了：`graph.png` 里是我自己的终端窗口。
/// 而脚本会以为成功了（文件写出了、大小也正常），**那是最坏的一种失败**。
/// 用进程主窗口句柄 + `SetForegroundWindow`，**按 pid 找，不按标题猜**。
const PS_CAPTURE = `
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Fg {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$p = Get-Process -Id $env:JW_SHOT_PID -ErrorAction SilentlyContinue
$r = New-Object Fg+RECT
if ($p -and $p.MainWindowHandle -ne 0) {
  [void][Fg]::ShowWindow($p.MainWindowHandle, 3)   # 3 = SW_MAXIMIZE（**铺满屏幕**）
  [void][Fg]::SetForegroundWindow($p.MainWindowHandle)
  Start-Sleep -Milliseconds 900
  [void][Fg]::GetWindowRect($p.MainWindowHandle, [ref]$r)
}
# **只截这个窗口那一块**，不是整屏：整屏会把别人的窗口也拍进去。
# 拿不到窗口矩形时退回整屏（宁可拍到别的，也不要拍不到）。
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$x = if ($r.R -gt $r.L) { $r.L } else { $b.X }
$y = if ($r.B -gt $r.T) { $r.T } else { $b.Y }
$w = if ($r.R -gt $r.L) { $r.R - $r.L } else { $b.Width }
$h = if ($r.B -gt $r.T) { $r.B - $r.T } else { $b.Height }
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
$bmp.Save($env:JW_SHOT_OUT, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
`;

function capture(file, pid) {
  // ⚠️ **用 `powershell` 不是 `pwsh`**：这台机器上 PATH 里只有前者。
  // ⚠️ **`windowsHide: true` 是必须的**：不隐藏的话 PowerShell 会闪一个控制台窗口
  // 盖在 GUI 上——**拍到的是那个黑框**。
  execFileSync('powershell', ['-NoProfile', '-Command', PS_CAPTURE], {
    env: { ...process.env, JW_SHOT_OUT: file, JW_SHOT_PID: String(pid) },
    stdio: 'ignore',
    windowsHide: true,
  });
}

const shots = [];
for (const tab of TABS) {
  const png = join(outDir, `${tab}.png`);
  // `--tab` 是产品本来就有的深链接（界面门禁在用）
  const child = spawn(exe, [project, '--tab', tab], { detached: false, stdio: 'ignore' });
  try {
    // 给页面留够时间：拉起 WebView + 取数 + 画。界面门禁那边等 8 秒够用，
    // 这里多给一点——截图慢一点没关系，**拍糊了才是废片**。
    await sleep(9000);
    capture(png, child.pid);
    console.log(`  ${tab.padEnd(8)} → ${png}`);
    shots.push(png);
  } finally {
    try { child.kill(); } catch { /* 已经退了就算了 */ }
  }
  // 窗口关掉之后再开下一个：两个窗口叠在一起，拍到的就是错的
  await sleep(1500);
}

console.log(`\n${shots.length} 张，在 ${outDir}`);
