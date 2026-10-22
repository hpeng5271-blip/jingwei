#!/usr/bin/env node
// 读/操作**你已经打开的那个 Chrome**（Windows UI Automation，不抢着用剪贴板）。
//
// 为什么不用 SendKeys + 剪贴板：焦点一跑，复制的就是地址栏或空串（实测两次失败）。
// UIA 能直接读 Chromium 的文档文本（连上时自动启用无障碍树），也能读写地址栏。
//
// 用法：
//   node tools/uia.mjs tab                      窗口标题 + 地址
//   node tools/uia.mjs text [--chars 6000]      当前标签页可见文本（不导航）
//   node tools/uia.mjs open <url> [--wait 4000] 开**新标签**并打开（不动你原页面）
//   node tools/uia.mjs goto <url>               在当前标签页打开
//   node tools/uia.mjs read <url> [--chars N] [--save 主题]   open + 读文本
//
// 只读原则：只读文档文本 / 设置地址栏；不发帖、不点按钮、不读 cookie 文件。

import { spawnSync } from 'node:child_process';

const args = process.argv.slice(2);
const cmd = args[0];
const getFlag = (k, d) => {
  const i = args.indexOf(`--${k}`);
  return i >= 0 && args[i + 1] && !args[i + 1].startsWith('--') ? args[i + 1] : d;
};

const PS_HEAD = `
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$ErrorActionPreference='Stop'
function Get-ChromeWindows {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, 'Chrome_WidgetWin_1')
  $wins = $root.FindAll([System.Windows.Automation.TreeScope]::Children, $cond)
  $out = @()
  foreach ($w in $wins) { $out += $w }
  return $out
}
function Get-ChromeWindow {
  # 2026-10-22：机器上不止一个 Chrome 窗口（DSH 的 Web GUI 也是 Chrome 窗口）。
  # 选**不是** DSH 的那个；都没有再退回第一个。
  $all = Get-ChromeWindows
  if ($all.Count -eq 0) { throw '没找到 Chrome 窗口' }
  $real = $all | Where-Object { $_.Current.Name -notmatch 'DeepSeek Harness|DeepSeek' }
  if ($real) { return @($real)[0] }
  return $all[0]
}
function Show-ChromeWindows {
  $all = Get-ChromeWindows
  $i = 0
  foreach ($w in $all) {
    Write-Output ("[$i] " + $w.Current.Name)
    $i++
  }
}
function Get-Omnibox($win) {
  $c = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)
  return $win.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $c)
}
function Get-DocText($win) {
  $c = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Document)
  $doc = $win.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $c)
  if (-not $doc) { throw '没找到文档节点（页面可能还没加载完）' }
  $tp = $doc.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
  return $tp.DocumentRange.GetText(-1)
}
function Focus-Chrome($win) {
  Add-Type @"
using System;
using System.Runtime.InteropServices;
public class JJW {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
}
"@
  $h = [IntPtr]$win.Current.NativeWindowHandle
  [JJW]::ShowWindow($h, 9) | Out-Null
  [JJW]::SetForegroundWindow($h) | Out-Null
  Start-Sleep -Milliseconds 300
}
`;

const run = (body) => {
  const r = spawnSync('powershell', ['-NoProfile', '-STA', '-Command', PS_HEAD + body], { encoding: 'utf8', timeout: 90000 });
  if (r.status !== 0) throw new Error((r.stderr || 'powershell 失败').slice(0, 400));
  return r.stdout;
};
const sleep = (ms) => spawnSync('powershell', ['-NoProfile', '-Command', `Start-Sleep -Milliseconds ${ms}`]);
const esc = (s) => String(s).replace(/'/g, "''");

/** 在 Chrome 里开一个地址：newTab=true 用 Ctrl+T，否则复用当前标签。 */
function openUrl(url, newTab) {
  const body = `
Add-Type -AssemblyName System.Windows.Forms
$w = Get-ChromeWindow
Focus-Chrome $w
${newTab ? "[System.Windows.Forms.SendKeys]::SendWait('^t')\nStart-Sleep -Milliseconds 450" : ''}
$e = Get-Omnibox $w
if (-not $e) { throw '没找到地址栏' }
$e.SetFocus()
$e.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('${esc(url)}')
Start-Sleep -Milliseconds 250
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
Write-Output 'ok'`;
  run(body);
}

function docText() {
  return run(`
$w = Get-ChromeWindow
Write-Output (Get-DocText $w)`).replace(/\r\n/g, '\n');
}

function windowInfo() {
  const out = run(`
$w = Get-ChromeWindow
$e = Get-Omnibox $w
$val = ''
if ($e) { try { $val = $e.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value } catch {} }
Write-Output ($w.Current.Name + '||' + $val)`);
  const [title, addr] = out.trim().split('||');
  return { title, addr };
}

const saveIfAsked = (text, url, save) => {
  if (!save) { console.log(text); return; }
  const fs = require('node:fs');
  const path = require('node:path');
  const dir = path.join(process.cwd(), 'docs', 'research');
  fs.mkdirSync(dir, { recursive: true });
  const day = new Date().toISOString().slice(0, 10);
  const file = path.join(dir, `${day}-${save.replace(/[^\w\u4e00-\u9fa5-]+/g, '-')}.md`);
  fs.writeFileSync(file, [`# ${save}（${day} 抓取）`, '',
    `> 来源：${url || '本机 Chrome 当前标签页'}`,
    '> 抓取方式：`node tools/uia.mjs`（Windows UI Automation 读文档文本），未做摘要。', '',
    text, ''].join('\n'), 'utf8');
  console.log(text);
  console.log(`\n已存：${path.relative(process.cwd(), file)}`);
};

try {
  if (cmd === 'tab') {
    const { title, addr } = windowInfo();
    console.log(`窗口：${title}`);
    console.log(`地址：${addr}`);
  } else if (cmd === 'text') {
    const chars = Number(getFlag('chars', '6000'));
    console.log(docText().slice(0, chars));
  } else if (cmd === 'windows') {
    console.log(run('Show-ChromeWindows').trim());
  } else if (cmd === 'open' || cmd === 'goto' || cmd === 'read') {
    const url = args[1];
    if (!/^https?:\/\//.test(url || '')) throw new Error('用法：node tools/uia.mjs open|goto|read <http(s)://…>');
    openUrl(url, cmd !== 'goto');
    sleep(Number(getFlag('wait', '4000')));
    if (cmd === 'read') {
      const chars = Number(getFlag('chars', '6000'));
      saveIfAsked(docText().slice(0, chars), url, getFlag('save', ''));
    } else {
      console.log(`已打开：${url}`);
    }
  } else {
    console.log(`用法：
  node tools/uia.mjs tab
  node tools/uia.mjs text [--chars N]
  node tools/uia.mjs open <url> [--wait ms]
  node tools/uia.mjs goto <url>
  node tools/uia.mjs read <url> [--chars N] [--save 主题]`);
    process.exit(2);
  }
} catch (e) {
  console.error(`UIA 失败：${e.message}`);
  process.exit(1);
}
