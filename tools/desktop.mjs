#!/usr/bin/env node
// 直接操作**你已经打开的那个 Chrome 窗口**（2026-10-22，人指定）。
//
// 为什么不走 CDP：Chrome 155 起禁止默认配置开远程调试；而你现在的 Chrome 没带调试端口，
// 进程起来之后也加不了参数。所以这里走"像人一样"的路子：**窗口前置 + 键盘输入 + 剪贴板**。
//
// 代价（必须知道）：会**抢焦点**；读的是整页可见文本（不是 DOM 结构）；期间别同时用键盘。
//
// 用法：
//   node tools/desktop.mjs tab                 # 只看：当前 Chrome 窗口标题/地址（不操作）
//   node tools/desktop.mjs read                # 复制当前页面的可见文本（不导航）
//   node tools/desktop.mjs goto <url>          # 地址栏输入并回车
//   node tools/desktop.mjs read <url>          # 先 goto 再 read
//   node tools/desktop.mjs save-as <主题>       # 保存成 docs/research/<日期>-<主题>.md（配合 read 用）
//
// 只读原则：只做"地址栏输入 / 全选复制"这类**不产生副作用**的按键；不发帖、不点按钮、不登录。

import { spawnSync } from 'node:child_process';

const args = process.argv.slice(2);
const cmd = args[0];

const ps = (script) => {
  const r = spawnSync('powershell', ['-NoProfile', '-STA', '-Command', script], { encoding: 'utf8', timeout: 60000 });
  if (r.status !== 0) throw new Error((r.stderr || 'powershell 失败').slice(0, 400));
  return (r.stdout || '').trim();
};

const COMMON = `
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName UIAutomationClient
$ErrorActionPreference='Stop'
function Get-Chrome {
  $p = Get-Process chrome -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if (-not $p) { throw '没找到带窗口的 Chrome' }
  return $p
}
`;

function titleOf() {
  const out = ps(`${COMMON}
$p = Get-Chrome
$t = $p.MainWindowTitle
Write-Output ($t + "||" + $p.Id)`);
  const [title, pid] = out.split('||');
  return { title, pid };
}

function focusAnd(keys) {
  const out = ps(`${COMMON}
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
}
"@
$p = Get-Chrome
[W]::ShowWindow($p.MainWindowHandle, 9) | Out-Null
[W]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 350
[System.Windows.Forms.SendKeys]::SendWait("${keys}")
Start-Sleep -Milliseconds 250
Write-Output 'ok'`);
  return out;
}

function clipboardText() {
  return ps(`
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Clipboard]::GetText()`);
}

function setClipboard(text) {
  ps(`Set-Clipboard -Value ' '`);
}

try {
  if (cmd === 'tab') {
    const t = titleOf();
    console.log(`Chrome 窗口：${t.title}`);
    console.log(`进程：${t.pid}`);
  } else if (cmd === 'goto') {
    const url = args[1];
    if (!url) throw new Error('用法：node tools/desktop.mjs goto <url>');
    focusAnd('^l');
    focusAnd(`^a${url}`);
    focusAnd('{ENTER}');
    console.log(`已在你的 Chrome 里打开：${url}（等它加载完再 read）`);
  } else if (cmd === 'read') {
    const url = args[1];
    if (url && /^https?:\/\//.test(url)) {
      focusAnd('^l'); focusAnd('^a');
      focusAnd(`^a${url}`);
      focusAnd('{ENTER}');
      const wait = Number(args[2] || 4000);
      spawnSync('powershell', ['-NoProfile', '-Command', `Start-Sleep -Milliseconds ${wait}`]);
    }
    // 导航之后焦点常常还留在地址栏，直接 Ctrl+A 复制的会是 URL。
    // 先 {F6} 把焦点切到页面（再按一次回地址栏），再全选复制。
    focusAnd('{F6}');
    setClipboard('');
    focusAnd('^a');
    focusAnd('^c');
    spawnSync('powershell', ['-NoProfile', '-Command', 'Start-Sleep -Milliseconds 350']);
    const text = clipboardText();
    if (!text) throw new Error('没复制到内容（页面可能没焦点，或复制被拦）');
    console.log(text.slice(0, 6000));
    const theme = args.includes('--save') ? args[args.indexOf('--save') + 1] : '';
    if (theme) {
      const fs = await import('node:fs');
      const path = await import('node:path');
      const dir = path.join(process.cwd(), 'docs', 'research');
      fs.mkdirSync(dir, { recursive: true });
      const day = new Date().toISOString().slice(0, 10);
      const file = path.join(dir, `${day}-${theme.replace(/[^\w\u4e00-\u9fa5-]+/g, '-')}.md`);
      fs.writeFileSync(file, [`# ${theme}（${day} 抓取）`, '',
        '> 来源：本机 Chrome 当前标签页（人工登录态）',
        '> 抓取方式：`node tools/desktop.mjs read`（窗口前置 + 全选复制），**可见文本**，未做摘要。', '',
        text, ''].join('\\n'), 'utf8');
      console.log(`\\n已存：${path.relative(process.cwd(), file)}`);
    }
  } else {
    console.log(`用法：
  node tools/desktop.mjs tab
  node tools/desktop.mjs read [url] [等待毫秒] [--save 主题]
  node tools/desktop.mjs goto <url>
注意：这套会**抢焦点**、用你的剪贴板；只在你说"用我打开的这个"时用。`);
    process.exit(2);
  }
} catch (e) {
  console.error(`桌面操作失败：${e.message}`);
  process.exit(1);
}
