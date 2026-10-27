// 基础知识：状态、转义、点击反馈、悬浮提示。
//
// **必须是第一个加载的**：后面的页面模块都直接用 `S`（全局状态）和 `esc` 这类工具。
// 经典脚本按 <script> 顺序求值：函数声明会提升，`const` 不会——所以放前面。
/* jingwei 外壳前端。没有打包器，直接用 Tauri 注入的全局对象。 */
// 出错必须看得见：宁可界面上挂一条红条，也不要让用户面对一个空白页面。
function showFatal(msg) {
  const el = document.getElementById('fatal');
  if (!el) return;
  el.hidden = false;
  el.textContent = '界面出错：' + msg;
}
window.addEventListener('error', (e) => showFatal((e.message || '未知错误') + '（' + (e.filename || '') + ':' + (e.lineno || 0) + '）'));
window.addEventListener('unhandledrejection', (e) => {
  const r = e.reason;
  showFatal(String((r && (r.message || r)) || '未处理的 Promise 拒绝'));
});
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s == null ? '' : s).replace(/[&<>"']/g, (ch) => ({
  '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
}[ch]));

const KINDS = [
  ['goal', '目的'], ['idea', '想法'], ['plan', '计划'],
  ['blocker', '卡住'], ['learned', '学到'], ['decision', '决定'], ['note', '随手记'],
  ['record', '记录'],
];
const kindLabel = (k) => (KINDS.find((x) => x[0] === k) || ['', '随手记'])[1];
const TOTAL = 6;
const PLAN_RUN = ['检查运行环境', '校验项目契约', '解析依赖声明', '准备解释器', '启动进程', '等待就绪'];
const PLAN_TASK = ['检查运行环境', '校验项目契约', '解析依赖声明', '准备解释器', '执行命令', '等待完成'];


/// root 来源链（T7 前置）：localStorage → `app_root()`（argv[1]）→ 空态。
/// 空态时由界面上的「选择仓库」按钮走 `engine_pick_root`。
async function initRoot() {
  if (S.root) {
    try { await invoke('probe_status', { page: 'home', rootReady: true }); } catch (_e) { /* 诊断 */ }
    return;
  }
  try {
    const r = await invoke('app_root');
    const root = r && (r.data || (typeof r === 'string' ? r : null));
    if (typeof root === 'string' && root) {
      S.root = root;
      try { localStorage.setItem('jw.root', root); } catch (_e) { /* 忽略 */ }
    } else {
      throw new Error('app_root 没给目录：' + JSON.stringify(r).slice(0, 120));
    }
    await invoke('probe_status', { page: 'home', rootReady: true });
  } catch (e) {
    // P6：不许吞错——写 probe + 页面显示
    try { await invoke('probe_status', { page: 'init', rootReady: false }); } catch (_e) { /* 诊断 */ }
    const host = document.querySelector('#homeStats');
    if (host) host.innerHTML = '<div class="hint">打开仓库失败：' + esc(String(e && e.message ? e.message : e)) + '</div>';
  }
}

const S = {
  root: (function () { try { return localStorage.getItem('jw.root') || null; } catch (_e) { return null; } })(), data: null, clean: null, picked: new Set(),
  openers: [], defaultOpener: null, live: null, current: null,
  pythons: [], profiles: [],
  // 深度扫描的结果。为 null 时界面用打开项目时就带回来的那份快扫数据。
  audit: null, auditBusy: false,
  // 项目检查里「工具可以替你改」的那些，跟着扫描结果一起回来
  fixes: [],
  // 「刚才变了什么」：进项目时读一次，之后切回窗口 / 每 45 秒问一次
  changes: null,
};

// ── 基础 ───────────────────────────────
// 设置：工具「自己动手到什么程度」。默认全开，但每一条都能关，
// 关掉之后同一件事还能在界面上手动点。
/// 系统里设了「减少动效」就按关处理——这是无障碍里的硬要求
/// （前庭功能障碍的人会被位移动画弄晕，Windows 上这个开关就是给他们用的）。
/// **只在第一次运行时跟随系统**：用户自己挑过档位之后，那就是他的明确意愿，
/// 不该再被系统设置盖掉。
function prefersReducedMotion() {
  try {
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  } catch {
    return false;
  }
}

const SET = Object.assign(
  {
    scan: true, analyze: true, contract: true, verify: true, trial: false, hover: true, autoRecord: true,
    // 外观与反馈（全部可选）
    theme: 'light', density: 'standard', motion: prefersReducedMotion() ? 'off' : 'default',
    fb: 'standard', sound: true,
    // 范围类
    scanDepth: 2, cleanLevel: 'standard', delMode: 'trash', indent: 4,
  },
  JSON.parse(localStorage.getItem('jingwei.settings') || '{}'),
);
/// 出厂默认值。设置页拿它判断「这一项被改过没有」——
/// 只在偏离默认时才显示标记和还原按钮（照 Reese's Sodium Options 的做法）。
/// 抄一份常量而不是改 SET 本身：SET 是"现在是什么"，这里是"本来是什么"。
const SET_DEFAULTS = {
  scan: true, analyze: true, contract: true, verify: true, trial: false, hover: true, autoRecord: true,
  theme: 'light', density: 'standard', motion: prefersReducedMotion() ? 'off' : 'default',
  fb: 'standard', sound: true,
  scanDepth: 2, cleanLevel: 'standard', delMode: 'trash', indent: 4,
};

/// 这个项目有没有被信任过——也就是"能不能自动执行它契约里的命令"。
///
/// **存在这里，不存进项目目录。** 设计初稿写的是项目内 `.jingwei/trust.json`，
/// 那是错的：信任记录一旦跟着项目走，别人发来的压缩包只要自带一份 trust.json，
/// 就绕过了"第一次必须由你点一次"这道闸——而那正是要防的事。
/// 它必须留在**这台机器**上，跟项目本身无关。
const TRUST_KEY = 'jingwei.trust';

function trustedSet() {
  try {
    return new Set(JSON.parse(localStorage.getItem(TRUST_KEY) || '[]'));
  } catch {
    return new Set();
  }
}

function isTrusted(root) {
  return !!root && trustedSet().has(String(root).toLowerCase());
}

function trust(root) {
  if (!root) return;
  const s = trustedSet();
  s.add(String(root).toLowerCase());
  localStorage.setItem(TRUST_KEY, JSON.stringify([...s]));
}

/// 统一信封 → `{ state, data, reason }`。
///
/// 引擎里那条"查不了 ≠ 没有"的线，到这一层**必须还活着**：
/// `unknown` 绝不允许被当成"没有"，所以调用方一律先 unpack 再按 state 分三支。
///
/// 没有 `state` 字段的（老形状）当作 `yes`——**只是兼容期**，
/// 新的命令一律带信封（Q7 查出 12 个命令原本没有"查不了"的通道）。
function unpack(v) {
  if (v && typeof v === 'object' && typeof v.state === 'string') return v;
  return { state: 'yes', data: v };
}

/// 设置项 → 控件。值列的位置都由它找，新增设置只要在这儿加一行。
const SET_CTL = {
  theme: '#setTheme', density: '#setDensity', motion: '#setMotion', fb: '#setFbLevel',
  indent: '#setIndent', sound: '#setSound', scan: '#setScan', analyze: '#setAnalyze',
  contract: '#setContract', verify: '#setVerify', trial: '#setTrial', hover: '#setHover',
  autoRecord: '#setAutoRecord', scanDepth: '#setScanDepth', cleanLevel: '#setCleanLevel',
  delMode: '#setDelMode',
};

/// 每个被改过的项打一个点 + 悬停出现「还原」，点一下回到出厂值。
/// 为什么要这个：设置页最大的怨气是「我不知道我改过什么，也退不回去」。
function paintSettingChanges() {
  Object.entries(SET_CTL).forEach(([key, sel]) => {
    const el = document.querySelector(sel);
    if (!el) return;
    const row = el.closest('.opt, .setrow, label');
    if (!row) return;
    const now = typeof SET_DEFAULTS[key] === 'boolean' ? !!el.checked : el.value;
    const changed = String(now) !== String(SET_DEFAULTS[key]);
    row.classList.toggle('changed', changed);
    let b = row.querySelector('.revert');
    if (!changed) { if (b) b.remove(); return; }
    if (!b) {
      b = document.createElement('button');
      b.className = 'revert';
      b.type = 'button';
      b.textContent = '还原';
      b.title = '回到默认：' + SET_DEFAULTS[key];
      b.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        if (typeof SET_DEFAULTS[key] === 'boolean') el.checked = SET_DEFAULTS[key];
        else el.value = String(SET_DEFAULTS[key]);
        el.dispatchEvent(new Event('change', { bubbles: true }));
        readSettings();
        paintSettingChanges();
        sfx('ok');
      });
      row.appendChild(b);
    }
  });
}

function saveSettings() { localStorage.setItem('jingwei.settings', JSON.stringify(SET)); }

// ── 点击反馈：声音现场合成（不塞音频文件）＋ 指针位置的涟漪 ──────────
// MC 的"点下去有回应"就是这个意思：视觉回弹 + 一声轻响。
let AC = null;
function sfx(kind) {
  if (!SET.sound) return;
  try {
    AC = AC || new (window.AudioContext || window.webkitAudioContext)();
    if (AC.state === 'suspended') AC.resume();
    const t0 = AC.currentTime;
    const tone = (freq, dur, type, vol, delay) => {
      const o = AC.createOscillator();
      const g = AC.createGain();
      o.type = type; o.frequency.value = freq;
      const t = t0 + (delay || 0);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(vol, t + 0.006);
      g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
      o.connect(g); g.connect(AC.destination);
      o.start(t); o.stop(t + dur + 0.02);
    };
    if (kind === 'click') tone(680, 0.055, 'triangle', 0.10);
    else if (kind === 'ok') { tone(660, 0.10, 'sine', 0.12); tone(990, 0.16, 'sine', 0.10, 0.07); }
    else if (kind === 'bad') { tone(196, 0.20, 'sine', 0.14); tone(147, 0.26, 'sine', 0.10, 0.06); }
    else if (kind === 'done') { tone(523, 0.12, 'sine', 0.11); tone(659, 0.14, 'sine', 0.10, 0.09); tone(784, 0.22, 'sine', 0.09, 0.18); }
  } catch (e) { /* 没声音也不影响用 */ }
}

// 涟漪：把动画起点放到指针按下的位置
function rippleAt(el, ev) {
  if (!el || !el.classList) return;
  const r = el.getBoundingClientRect();
  const x = ((ev && ev.clientX ? ev.clientX : r.left + r.width / 2) - r.left);
  const y = ((ev && ev.clientY ? ev.clientY : r.top + r.height / 2) - r.top);
  el.classList.add('ripple');
  el.style.setProperty('--rx', x + 'px');
  el.style.setProperty('--ry', y + 'px');
  el.classList.remove('go');
  void el.offsetWidth;   // 强制重排，动画才能重播
  el.classList.add('go');
  setTimeout(() => el.classList.remove('go'), 560);
}

// ── 悬浮提示（MC tooltip 那套）：延迟 400ms 才出现，不闪人 ──────────
let tipBox = null, tooltipTimer = null, tipTarget = null;
function initTooltips() {
  tipBox = document.createElement('div');
  tipBox.className = 'tip';
  document.body.appendChild(tipBox);
  const hide = () => { clearTimeout(tooltipTimer); tipTarget = null; tipBox.classList.remove('on'); };
  document.addEventListener('pointerover', (e) => {
    const t = e.target.closest('[data-tip]');
    if (!t || t === tipTarget) return;
    tipTarget = t;
    clearTimeout(tooltipTimer);
    tooltipTimer = setTimeout(() => {
      const r = t.getBoundingClientRect();
      tipBox.innerHTML = esc(t.dataset.tip).replace(/\n/g, '<br>');
      tipBox.classList.add('on');
      const w = tipBox.offsetWidth, h = tipBox.offsetHeight;
      const x = Math.min(Math.max(8, r.left), window.innerWidth - w - 8);
      const above = r.top > h + 12;
      tipBox.style.left = x + 'px';
      tipBox.style.top = (above ? r.top - h - 8 : r.bottom + 8) + 'px';
    }, 400);
  }, true);
  document.addEventListener('pointerout', hide, true);
  window.addEventListener('scroll', hide, true);
  window.addEventListener('blur', hide);
}

// 鼠标视差：背景随光标轻轻反向偏移（rAF 节流，只改 transform）
function initParallax() {
  const amb = document.querySelector('.ambient');
  if (!amb) return;
  let tx = 0, ty = 0, cx = 0, cy = 0, raf = null;
  const step = () => {
    cx += (tx - cx) * .07;
    cy += (ty - cy) * .07;
    amb.style.transform = 'translate3d(' + (-cx * 1.1).toFixed(3) + '%,' + (-cy * 1.1).toFixed(3) + '%,0) scale(1.04)';
    raf = (Math.abs(tx - cx) > .002 || Math.abs(ty - cy) > .002) ? requestAnimationFrame(step) : null;
  };
  window.addEventListener('pointermove', (e) => {
    if (SET.motion === 'off') return;
    tx = (e.clientX / window.innerWidth - .5) * 2;
    ty = (e.clientY / window.innerHeight - .5) * 2;
    if (!raf) raf = requestAnimationFrame(step);
  }, { passive: true });
}

// 拾取粒子：成功时散几颗小光点（MC 拿到成就那一下的轻量版）
function burst(el, n) {
  if (SET.motion === 'off' || !el) return;
  const r = el.getBoundingClientRect();
  const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
  for (let i = 0; i < (n || 6); i++) {
    const s = document.createElement('span');
    s.className = 'spark';
    s.style.left = cx + 'px';
    s.style.top = cy + 'px';
    s.style.setProperty('--a', (i * (360 / (n || 6))) + 'deg');
    s.style.setProperty('--d', (16 + Math.random() * 12).toFixed(0) + 'px');
    document.body.appendChild(s);
    setTimeout(() => s.remove(), 720);
  }
}

// 把外观选择写到 <html> 的 data-* 上：CSS 只认属性，组件不认识具体主题
function applyLook() {
  const h = document.documentElement;
  // 兼容旧值：darcula → dark
  const theme = SET.theme === 'darcula' ? 'dark' : SET.theme;
  SET.theme = theme;
  h.dataset.theme = theme;
  h.dataset.density = SET.density;
  h.dataset.motion = SET.motion;
  // 三档时长。标准档与 style.css 的默认值一模一样——否则"切走再切回来"会变样。
  const map = {
    fast: ['100ms', '60ms', '140ms'],
    default: ['170ms', '100ms', '260ms'],
    slow: ['300ms', '180ms', '460ms'],
    off: ['0ms', '0ms', '0ms'],
  };
  const [t, tf, ts] = map[SET.motion] || map.default;
  // **变量名必须与 style.css 对齐**：那边 60 多处用的是 --t / --t-fast / --t-slow。
  // 这里曾经写的是 --dur*，CSS 一处都没引用——设置里那三档看着能调，实际是死的。
  h.style.setProperty('--t', t);
  h.style.setProperty('--t-fast', tf);
  h.style.setProperty('--t-slow', ts);
}

// 反馈：任何动作都要有回声。量级跟着「会不会影响跑起来」走——
// 契约写不成、依赖装不上是 block（红）；扫不动某个目录、跳过某张卡是 info。
function feedback(level, title, detail, actions) {
  // 静默级别：只留"会不会影响跑起来"的红
  if (SET.fb === 'silent' && level !== 'block') return null;
  sfx(level === 'block' ? 'bad' : (level === 'warn' ? 'click' : 'click'));
  // 说过的话都进底部「消息」栏。四角的卡片只负责"浮现一下"，
  // 不再往那儿堆历史——堆多了就变成挡视线的四个角落。
  dockMsg(level, title, detail);
  const box = $('#feed');
  if (!box) return;
  box.hidden = false;
  // 合并同一条：同一件事连着说两遍，只更新时间
  const last = box.lastElementChild;
  if (last && last.dataset.t === title) {
    last.querySelector('.t').textContent = new Date().toTimeString().slice(0, 5);
    const b = last.querySelector('.fbd');
    if (b && detail) b.innerHTML = detail;
    return last;
  }
  const el = document.createElement('div');
  el.className = 'fb ' + level;
  el.dataset.t = title;
  el.dataset.icon = level === 'block' ? '✕' : (level === 'warn' ? '!' : '✓');
  const t = new Date();
  el.innerHTML = '<div class="fbt">'
    + (level === 'block' ? '✕' : level === 'warn' ? '!' : 'i')
    + '<span>' + esc(title) + '</span><span class="t">'
    + String(t.getHours()).padStart(2, '0') + ':' + String(t.getMinutes()).padStart(2, '0')
    + '</span></div>'
    + (detail ? '<div class="fbd">' + detail + '</div>' : '')
    + '<div class="fbx">关闭</div>';
  el.addEventListener('click', (e) => {
    if (e.target.classList.contains('fbx')) el.remove();
    else if (actions && actions.fn) actions.fn();
    if (!box.children.length) box.hidden = true;
  });
  box.appendChild(el);
  while (box.children.length > 2) box.firstElementChild.remove();   // 屏幕上最多两张
  // 浮现一下就散：非红 6 秒，红 14 秒。历史在底部「消息」栏里，不会丢。
  // （鼠标停在上面就等它挪开——不跟你抢那一下点击。）
  const kill = () => {
    if (!el.isConnected) return;
    if (el.matches(':hover')) { setTimeout(kill, 3000); return; }
    el.classList.add('out');
    setTimeout(() => { el.remove(); if (!box.children.length) box.hidden = true; }, 220);
  };
  setTimeout(kill, level === 'block' ? 14000 : 6000);
  return el;
}

// 决策树：<0.1s 的操作不给任何反馈（NN/g 0.1s = 瞬时），除非它是红的
function feedbackTimed(t0, level, title, detail) {
  if (level !== 'block' && performance.now() - t0 < 100) return null;
  return feedback(level, title, detail);
}

function readSettings() {
  ['scan', 'analyze', 'contract', 'verify', 'trial', 'hover', 'autoRecord'].forEach((k) => {
    const el = $('#set' + k[0].toUpperCase() + k.slice(1));
    if (el) SET[k] = el.checked;
  });
  if ($('#setSound')) SET.sound = $('#setSound').checked;
  ['theme', 'density', 'motion', 'fb'].forEach((k) => {
    const id = { theme: '#setTheme', density: '#setDensity', motion: '#setMotion', fb: '#setFbLevel' }[k];
    const el = $(id);
    if (el) SET[k] = el.value;
  });
  // 范围类选择
  if ($('#setScanDepth')) SET.scanDepth = parseInt($('#setScanDepth').value, 10) || 2;
  if ($('#setCleanLevel')) SET.cleanLevel = $('#setCleanLevel').value;
  if ($('#setDelMode')) SET.delMode = $('#setDelMode').value;
  if ($('#setIndent')) SET.indent = parseInt($('#setIndent').value, 10) || 4;
  saveSettings();
  paintSettingChanges();
  applyLook();
  feedback('info', '设置已保存', Object.entries(SET)
    .map(([k, v]) => ({
      scan: '自动扫描项目', analyze: '自动分析卡片', contract: '自动生成契约',
      verify: '自动跑测试', trial: '自动试运行', hover: '停留即选中',
      autoRecord: '自动写记录',
      theme: '主题', density: '密度', motion: '动效', fb: '反馈级别',
      scanDepth: '扫描深度', cleanLevel: '清理力度', delMode: '删除方式', indent: '编辑器缩进',
    }[k]) + '：' + (typeof v === 'boolean' ? (v ? '开' : '关') : v))
    .join(' · '));
}

/// 自动记录：把「刚刚发生了什么」写进项目自己的记录里。
/// 失败不吵人——记录写不上不该影响正在做的事，但会在反馈条里留一句。
async function autoRecord(title, body, tags) {
  if (!SET.autoRecord || !S.root) return;
  try {
  } catch (e) {
    feedback('warn', '记录没写进项目', esc(typeof e === 'string' ? e : (e && e.message) || String(e)));
  }
}

let toastTimer = null;
function toast(msg, bad) {
  const t = $('#toast');
  t.textContent = msg;
  t.className = 'toast' + (bad ? ' bad' : '');
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 4200);
}
async function guard(fn) {
  busy(true);
  try { await fn(); } catch (e) {
    toast(typeof e === 'string' ? e : (e && e.message) || String(e), true);
  } finally { busy(false); }
}

// 忙碌信号：光标变「进行中」，外加一个跟着指针的小圈。
// 用计数而不是布尔——嵌套调用时外层收工不该把内层的信号抹掉。
let busyDepth = 0;
function busy(on) {
  busyDepth = Math.max(0, busyDepth + (on ? 1 : -1));
  const on2 = busyDepth > 0;
  document.body.classList.toggle('busy', on2);
  const c = $('#busyCursor');
  if (!c) return;
  if (on2 === !c.hidden) return;
  c.hidden = !on2;
  if (on2) {
    document.addEventListener('pointermove', moveBusyCursor, { passive: true });
  } else {
    document.removeEventListener('pointermove', moveBusyCursor);
  }
}
// 只改 transform：不触发布局重算，指针怎么甩都不掉帧。
function moveBusyCursor(e) {
  const c = $('#busyCursor');
  if (c) c.style.transform = 'translate3d(' + e.clientX + 'px,' + e.clientY + 'px,0)';
}
function human(b) {
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = b, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return i === 0 ? b + ' B' : v.toFixed(1) + ' ' + u[i];
}
// 点一块就复制。webview 里 navigator.clipboard 不一定可用，退到 execCommand。
async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    try { document.execCommand('copy'); } catch (_) {}
    ta.remove();
  }
  toast('已复制到剪贴板');
}
function copyBlock(cmd, note) {
  if (!cmd) return '';
  return '<div class="copy" data-copy="' + esc(cmd) + '"><code>' + esc(cmd) + '</code>'
    + '<span class="cp">点击复制</span></div>'
    + (note ? '<div class="hint" style="margin-top:5px">' + esc(note) + '</div>' : '');
}

