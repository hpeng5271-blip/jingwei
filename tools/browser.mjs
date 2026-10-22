#!/usr/bin/env node
// 用本机 Chrome「像人一样」取页面（2026-10-22，人授权使用他的 Chrome）。
//
// 两条通道：
//   ① CDP（默认）：开一个**独立配置**的有界面 Chrome（`%LOCALAPPDATA%\jingwei\chrome-agent`），
//      通过 DevTools 协议导航 + 读正文 + 可截图。你在这个窗口里登录一次，cookie 就留在 agent 配置里。
//   ② headless（`--headless`）：不开窗口，直接 dump-dom；适合公开页、快。
//
// 为什么独立配置：Chrome 155 起禁止**默认配置**开远程调试；而且这样**不碰**你日常那个 Chrome。
//
// 用法：
//   node tools/browser.mjs open  [url]                 # 开 agent 窗口（你登录用）
//   node tools/browser.mjs read  <url> [--chars N] [--save 主题] [--headless]
//   node tools/browser.mjs shot  <url> <out.png>
//   node tools/browser.mjs eval  <url> "<js 表达式>"     # 在页面里跑一段只读 JS（如取链接列表）
//   node tools/browser.mjs stop                        # 关掉 agent 窗口
//
// 只读原则：只导航 / 读文本 / 截图 / 跑只读表达式；**不点发帖、不投票、不读 cookie 文件**。

import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const CHROME = [
  path.join(process.env['ProgramFiles'] || 'C:/Program Files', 'Google/Chrome/Application/chrome.exe'),
  path.join(process.env['ProgramFiles(x86)'] || 'C:/Program Files (x86)', 'Google/Chrome/Application/chrome.exe'),
  path.join(process.env.LOCALAPPDATA || '', 'Google/Chrome/Application/chrome.exe'),
].find((p) => fs.existsSync(p));
if (!CHROME) { console.error('找不到 Chrome'); process.exit(2); }

const PROFILE = path.join(process.env.LOCALAPPDATA || os.tmpdir(), 'jingwei', 'chrome-agent');
fs.mkdirSync(PROFILE, { recursive: true });
const PORT = Number(process.env.JINGWEI_CDP_PORT || 9222);

const args = process.argv.slice(2);
const cmd = args[0];
const has = (k) => args.includes(`--${k}`);
const getFlag = (k, d) => {
  const i = args.indexOf(`--${k}`);
  return i >= 0 && args[i + 1] && !args[i + 1].startsWith('--') ? args[i + 1] : d;
};

const decode = (s) => String(s || '')
  .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"')
  .replace(/&#39;|&apos;/g, "'").replace(/&amp;/g, '&').replace(/&nbsp;/g, ' ');
const toText = (html) => decode(String(html || '')
  .replace(/<script[\s\S]*?<\/script>/gi, ' ').replace(/<style[\s\S]*?<\/style>/gi, ' ')
  .replace(/<noscript[\s\S]*?<\/noscript>/gi, ' ').replace(/<[^>]+>/g, ' '))
  .replace(/[ \t\u00a0]+/g, ' ').replace(/\n{3,}/g, '\n\n').trim();

const baseFlags = [`--user-data-dir=${PROFILE}`, '--profile-directory=Agent',
  '--no-first-run', '--no-default-browser-check', '--disable-features=Translate'];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function cdpAlive() {
  try {
    const r = await fetch(`http://127.0.0.1:${PORT}/json/version`, { signal: AbortSignal.timeout(1500) });
    return r.ok;
  } catch { return false; }
}

async function ensureBrowser() {
  if (await cdpAlive()) return;
  spawn(CHROME, [...baseFlags, `--remote-debugging-port=${PORT}`, 'about:blank'],
    { detached: true, stdio: 'ignore' }).unref();
  for (let i = 0; i < 40; i += 1) {
    await sleep(400);
    if (await cdpAlive()) return;
  }
  throw new Error('Chrome 没在 16 秒内起来（端口 ' + PORT + '）');
}

/** 极简 CDP 客户端：开标签页 → 导航 → 等 load → 读 DOM/跑表达式。 */
class Tab {
  constructor(ws, id) { this.ws = ws; this.id = id; this.seq = 0; this.pending = new Map(); this.events = []; }

  static async open(url) {
    const res = await fetch(`http://127.0.0.1:${PORT}/json/new?${encodeURIComponent('about:blank')}`, { method: 'PUT' });
    if (!res.ok) throw new Error(`开标签页失败 ${res.status}`);
    const t = await res.json();
    const ws = new WebSocket(t.webSocketDebuggerUrl);
    await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = (e) => bad(new Error('WS 打不开')); });
    const tab = new Tab(ws, t.id);
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.id && tab.pending.has(m.id)) {
        const { ok, bad } = tab.pending.get(m.id);
        tab.pending.delete(m.id);
        if (m.error) bad(new Error(m.error.message)); else ok(m.result);
      } else if (m.method) tab.events.push(m.method);
    };
    await tab.send('Page.enable');
    await tab.send('Runtime.enable');
    await tab.navigate(url);
    return tab;
  }

  send(method, params = {}) {
    const id = (this.seq += 1);
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((ok, bad) => {
      this.pending.set(id, { ok, bad });
      setTimeout(() => { if (this.pending.has(id)) { this.pending.delete(id); bad(new Error(`CDP 超时：${method}`)); } }, 30000);
    });
  }

  async navigate(url) {
    const done = new Promise((ok) => {
      const iv = setInterval(() => {
        if (this.events.includes('Page.loadEventFired')) { clearInterval(iv); ok(); }
      }, 200);
      setTimeout(() => { clearInterval(iv); ok(); }, 20000);
    });
    await this.send('Page.navigate', { url });
    await done;
    await sleep(600);   // 让前端把内容补上（Reddit/Discord 都是 SPA）
  }

  async eval(expression) {
    const r = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.text || '页面里报错');
    return r.result?.value;
  }

  async text() { return this.eval('document.body ? document.body.innerText : ""'); }
  close() { try { this.ws.close(); } catch { /* ignore */ } }
}

async function headlessDom(url, budget = 12000) {
  const r = spawnSync(CHROME, [...baseFlags, '--headless=new', '--disable-gpu', '--hide-scrollbars',
    `--virtual-time-budget=${budget}`, '--dump-dom', url],
  { encoding: 'utf8', timeout: 90000, maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(`chrome 退出码 ${r.status}：${(r.stderr || '').slice(0, 300)}`);
  return r.stdout || '';
}

function saveIfAsked(text, url, save) {
  if (!save) { console.log(text); return; }
  const dir = path.join(process.cwd(), 'docs', 'research');
  fs.mkdirSync(dir, { recursive: true });
  const day = new Date().toISOString().slice(0, 10);
  const file = path.join(dir, `${day}-${save.replace(/[^\w\u4e00-\u9fa5-]+/g, '-')}.md`);
  fs.writeFileSync(file, [`# ${save}（${day} 抓取）`, '', `> 来源：${url}`,
    '> 抓取方式：`node tools/browser.mjs read`（本机 Chrome，独立 agent 配置），去标签正文，未做摘要。', '',
    text, ''].join('\n'), 'utf8');
  console.log(text);
  console.log(`\n已存：${path.relative(process.cwd(), file)}`);
}

try {
  if (cmd === 'open' || cmd === 'login') {
    const url = args[1] || 'https://www.reddit.com';
    spawn(CHROME, [...baseFlags, `--remote-debugging-port=${PORT}`, url], { detached: true, stdio: 'ignore' }).unref();
    console.log(`已用独立配置打开：${url}`);
    console.log(`配置目录：${PROFILE}`);
    console.log('在这个窗口里登录一次（Reddit / Discord 网页版都行），登录态会留在上面的目录；之后 read 就能读到。');
    console.log('这个配置与你日常的 Chrome 完全独立；关窗口用 `node tools/browser.mjs stop`。');
  } else if (cmd === 'stop') {
    const r = spawnSync('powershell', ['-NoProfile', '-Command',
      `Get-Process chrome -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.MainWindowTitle -ne '' -and $_.CommandLine -like '*chrome-agent*' } | Stop-Process -Force`]);
    // 上面一行拿不到 CommandLine（需要 CIM），退化成按启动参数匹配：
    const r2 = spawnSync('powershell', ['-NoProfile', '-Command',
      `Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" | Where-Object { $_.CommandLine -like '*chrome-agent*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }`]);
    console.log(`已尝试关闭 agent 窗口（退出码 ${r2.status}）`);
  } else if (cmd === 'read') {
    const url = args[1];
    if (!/^https?:\/\//.test(url || '')) throw new Error('用法：node tools/browser.mjs read <http(s)://…> [--chars N] [--save 主题] [--headless]');
    const chars = Number(getFlag('chars', '6000'));
    let text;
    if (has('headless')) {
      text = toText(await headlessDom(url)).slice(0, chars);
    } else {
      await ensureBrowser();
      const tab = await Tab.open(url);
      text = String(await tab.text() || '').slice(0, chars);
      tab.close();
    }
    if (/blocked by network security|Welcome to Reddit|log in to Reddit|Enable JavaScript and cookies to continue/i.test(text)) {
      console.error('（提示：这看着像登录墙/风控页。先 `node tools/browser.mjs open https://www.reddit.com` 登录一次，或换一条通道）');
    }
    saveIfAsked(text, url, getFlag('save', ''));
  } else if (cmd === 'eval') {
    const url = args[1];
    const expr = args[2];
    if (!url || !expr) throw new Error('用法：node tools/browser.mjs eval <url> "<js 表达式>"');
    await ensureBrowser();
    const tab = await Tab.open(url);
    const v = await tab.eval(expr);
    tab.close();
    console.log(typeof v === 'string' ? v : JSON.stringify(v, null, 2));
  } else if (cmd === 'shot') {
    const url = args[1];
    const outFile = path.resolve(args[2] || `shot-${Date.now()}.png`);
    await ensureBrowser();
    const tab = await Tab.open(url);
    const r = await tab.send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(outFile, Buffer.from(r.data, 'base64'));
    tab.close();
    console.log(`已截图：${outFile}`);
  } else {
    console.log(`用法：
  node tools/browser.mjs open [url]        用独立配置开窗口（登录一次）
  node tools/browser.mjs read <url> [--chars N] [--save 主题] [--headless]
  node tools/browser.mjs eval <url> "<js>"
  node tools/browser.mjs shot <url> <out.png>
  node tools/browser.mjs stop
配置：${PROFILE}
Chrome：${CHROME}`);
    process.exit(2);
  }
} catch (e) {
  console.error(`浏览器取信失败：${e.message}`);
  process.exit(1);
}
