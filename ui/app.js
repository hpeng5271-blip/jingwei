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

const S = {
  root: null, data: null, clean: null, picked: new Set(),
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
    await invoke('save_note', {
      root: S.root,
      note: { kind: 'record', title: title, body: body, tags: (tags || []).concat(['自动记录']) },
    });
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

// ── 右侧诊断栏（常驻）：任何页面都能看到"现在有没有挡路的问题" ──
// ── 面板策略：四角信息只在需要时出现 ─────────────────────────
// 常驻的面板会一直占着屏幕，看久了很吵。所以：
//   诊断栏 → 只在「概览 + 打开了项目」时出现；跑/构建/测试期间强制出现
//   活动带 → 只在跑/构建/测试时出现，结束后停一会儿自己收掉
// 用户手动点过收起/展开后，以用户的意愿为准（pinned）。
const PANEL = { pinned: null, tab: 'home', running: false };
let bandHideTimer = null;

function panelPolicy() {
  const wantRail = PANEL.pinned === true
    || (PANEL.pinned === null && ((PANEL.tab === 'home' && !!S.root) || PANEL.running));
  document.body.classList.toggle('rail-off', !wantRail);
}
function bandAutoHide(ms) {
  clearTimeout(bandHideTimer);
  if (!ms) return;
  bandHideTimer = setTimeout(() => {
    const b = $('#band');
    if (b && !b.matches(':hover')) b.hidden = true;
    else bandAutoHide(4000);   // 鼠标还停在上面就先不收
  }, ms);
}

/// 左侧栏顶上的「项目」：全局选中的入口。
///
/// 为什么要放这儿：项目列表在「概览」页，打开一个项目之后那一页就收起来了——
/// 想换项目得回概览再点一次，很烦。放一条常驻的在这儿，选谁都是同一处生效：
/// 依赖栏、文件树、记录、流水线全跟着走。
function renderRailProjs() {
  const box = $('#railProjs');
  if (!box) return;
  const cnt = $('#railCounts');
  if (cnt) cnt.textContent = PS.all.length ? PS.all.length + ' 个' : '未扫描到项目';
  const list = (PS.all || []).slice(0, 24);
  if (!list.length) {
    box.innerHTML = '<div class="rp-empty">还没扫到项目。去「概览」页选个目录扫一次。</div>';
    return;
  }
  box.innerHTML = list.map((p) => {
    const on = isCurrent(p.path);
    const kind = String((p.markers && p.markers[0]) || '目录').replace(/\.(toml|json|xml|txt)$/i, '');
    return '<button class="rp' + (on ? ' on' : '') + '" data-rp="' + esc(p.path) + '"'
      + ' data-tip="' + esc(p.path) + (on ? '（当前）' : '，点一下切过去') + '">'
      + '<span class="dot"></span><span class="nm">' + esc(p.name) + '</span>'
      + '<span class="k">' + esc(kind.toLowerCase()) + '</span></button>';
  }).join('') + (PS.all.length > 24
    ? '<div class="rp-empty">还有 ' + (PS.all.length - 24) + ' 个没列出来，去「概览」翻页</div>' : '');
}

// 依赖工具窗的状态：折叠了哪几组、筛选了什么词。
// 折叠只记「折起来的那些」——展开的集合会随项目变化失效。
/// 传递依赖图（目前只有 Rust 有可靠来源：Cargo.lock 里写着谁依赖谁）。
/// 一次算好「从某个直接依赖出发能走到多少个包」，行渲染时直接用——
/// 这就是 EMI 那条 recipe tree 想回答的问题：这一样东西一共拉进来多少别的。
let TREE = { adj: new Map(), cache: new Map(), ready: false };
const RAIL = { collapsed: new Set(), q: '' };

/// 依赖清单：按 Maven 工具窗的读法组织——一个清单、若干分组、一行一项。
/// 每行两列：清单里声明的版本 → 机器上实际的版本。对不上就是要处理的事。
/// 用回来的一堆边建一张邻接表
function treeIndex(d) {
  const t = (d && d.depsTree) || null;
  if (!t || !t.edges || !t.edges.length) { TREE = { adj: new Map(), cache: new Map(), ready: false, total: 0 }; return; }
  const adj = new Map();
  t.edges.forEach((e) => {
    if (!adj.has(e.from)) adj.set(e.from, []);
    adj.get(e.from).push(e.to);
  });
  TREE = { adj, cache: new Map(), ready: true, total: t.total || 0 };
}

/// 从某个包出发能走到多少个包（去重）。广度优先，顺带防环。
function treeReach(name) {
  if (!TREE.ready) return 0;
  if (TREE.cache.has(name)) return TREE.cache.get(name);
  const seen = new Set();
  const q = [name];
  while (q.length) {
    const cur = q.shift();
    for (const nxt of (TREE.adj.get(cur) || [])) {
      if (nxt === name || seen.has(nxt)) continue;
      seen.add(nxt);
      q.push(nxt);
    }
  }
  TREE.cache.set(name, seen.size);
  return seen.size;
}

/// 展开一棵子树（最多两层，最多 40 行）——比一次铺开 400 个包有用
function treeRows(name, depth, budget) {
  const kids = TREE.adj.get(name) || [];
  let html = "";
  for (const k of kids) {
    if (budget.n <= 0) break;
    budget.n--;
    html += '<div class="dep-row sub" style="--d:' + depth + '" data-tip="' + esc(k + " ← " + name) + '">'
      + '<span class="st">·</span><span class="nm">' + esc(k) + "</span>"
      + '<span class="vd">' + (TREE.adj.get(k) ? treeReach(k) + " 个下游" : "叶子") + "</span></div>";
    if (depth < 2) html += treeRows(k, depth + 1, budget);
  }
  return html;
}

function railDepHtml(detail, deps) {
  const q = RAIL.q.trim().toLowerCase();
  const rows = (detail || []).filter((x) => !q || x.name.toLowerCase().includes(q));
  if (detail && detail.length) {
    const miss = rows.filter((x) => !x.ok);
    const ok = rows.filter((x) => x.ok);
    const one = (x) => {
      const tip = ['声明：' + (x.want || '未指定版本'), '实际：' + (x.have || '无'),
        '来源：' + (x.from || '未知')].join(' ｜ ');
      const cls = x.ok ? '' : (x.unknown ? ' unk' : ' miss');
      const state = x.ok ? (x.have ? esc(x.have) : '已安装') : (x.unknown ? '无法确认' : '未安装');
      const open = RAIL.detail && RAIL.detail.has(x.name);
      return '<div class="dep-row' + cls + ' fact-row" data-dep="' + esc(x.name) + '" data-tip="' + esc(tip) + '">'
        + '<span class="st">' + (x.ok ? '✓' : (x.unknown ? '?' : '✕')) + '</span>'
        + '<span class="nm">' + esc(x.name) + '</span>'
        + '<span class="vd">' + (x.want ? esc(x.want) : '—') + '<i>→</i>'
        + state + '</span>'
        + (TREE.ready && treeReach(x.name)
          ? '<span class="tree-n" data-tree="' + esc(x.name) + '" data-tip="展开它拉进来的包">'
            + (RAIL.expanded && RAIL.expanded.has(x.name) ? '▾' : '▸') + treeReach(x.name) + '</span>'
          : '') + '</div>'
        // 点开看详情：声明 / 实际 / 来源 / 下游，逐条摆出来。
        // 只给 tooltip 是不够的——"点得开"和"悬停一下"在可用性上不是一回事。
        + (open
          ? '<div class="dep-fact">'
            + '<div class="df-k">声明</div><div class="df-v">' + esc(x.want || '未指定版本') + '</div>'
            + '<div class="df-k">实际</div><div class="df-v">' + esc(x.have || '无') + '</div>'
            + '<div class="df-k">来源</div><div class="df-v">' + esc(x.from || '未知') + '</div>'
            + '<div class="df-k">状态</div><div class="df-v">'
            + (x.ok ? '已安装' : x.unknown ? '无法确认（这个语言没有可靠的检查手段）' : '未安装')
            + '</div>'
            + (TREE.ready && treeReach(x.name)
              ? '<div class="df-k">下游</div><div class="df-v">它会拉进来 ' + treeReach(x.name) + ' 个包（点行尾数字展开）</div>'
              : '')
            + '</div>'
          : '')
        + (TREE.ready && RAIL.expanded && RAIL.expanded.has(x.name)
          ? treeRows(x.name, 1, { n: 40 }) : '');
    };
    const grp = (key, title, list) => {
      // 筛选时忽略折叠状态：正在找东西的人不该再点开一层
      const open = !!q || !RAIL.collapsed.has(key);
      return '<div class="dep-grp' + (open ? ' open' : '') + '" data-grp="' + key + '">'
        + '<span class="chev">' + (open ? '▾' : '▸') + '</span><b>' + title + '</b>'
        + '<span class="cnt">' + list.length + '</span></div>'
        + (open ? list.map(one).join('') : '');
    };
    return (miss.length ? grp('miss', '未安装', miss) : '')
      + (ok.length ? grp('ok', '已安装', ok) : '')
      + (rows.length ? '' : '<div class="rail-empty">没有匹配「' + esc(RAIL.q) + '」的依赖项。</div>');
  }
  // 没有明细（老引擎或非 Python 项目）：退回「名字 + 装没装」
  const names = Object.keys(deps || {});
  return names.map((n) => '<div class="dep-row' + (deps[n] ? '' : ' miss') + '"'
    + ' data-tip="' + esc(n + (deps[n] ? '：已安装' : '：未安装')) + '">'
    + '<span class="st">' + (deps[n] ? '✓' : '✕') + '</span>'
    + '<span class="nm">' + esc(n) + '</span><span class="vd">—</span></div>').join('');
}

const RAIL_SEEN = new Set();
function renderRail() {
  const body = $('#railBody');
  if (!body) return;
  const sub = $('#railSub');
  const label = $('#railSubCounts');
  // 未选择项目：整块隐藏，不显示空面板（按设计：无内容即不占位）
  if (!S.root) {
    body.innerHTML = '';
    if (label) label.textContent = '';
    if (sub) sub.hidden = true;
    return;
  }
  if (sub) sub.hidden = false;
  // 常态显示「声明 → 实际」两列，按 Maven 工具窗的读法：
  // 左列是清单里要求的，右列是机器上装着的，对不上就是这一条要处理的事。
  const d = S.data || {};
  const detail = d.depsDetail || [];
  treeIndex(d);
  const deps = d.deps || {};
  const names = Object.keys(deps);
  const manifests = ((d.hygiene && d.hygiene.manifests) || []);
  const total = detail.length || names.length;
  const missN = detail.length
    ? detail.filter((x) => !x.ok).length
    : names.filter((n) => !deps[n]).length;
  if (label) {
    label.textContent = total
      ? total + ' 项 · ' + (missN ? missN + ' 项未安装' : '全部已安装')
      : '未声明依赖';
    popNum(label);
  }
  let html = '';
  // 工具窗的工具条：和 Maven 那一栏一样，重载、一键补装、按名筛选
  if (total) {
    html += '<div class="rail-tools">'
      + '<input id="railQ" placeholder="筛选依赖" value="' + esc(RAIL.q) + '">'
      + '<button class="iconbtn" id="railReload" title="重新检查依赖状态">重载</button>'
      + (missN ? '<button class="iconbtn hot" id="railInstall" title="按契约执行安装依赖">补装 ' + missN + '</button>' : '')
      + '</div>';
  }
  if (manifests.length) {
    html += '<div class="rail-sec">清单</div>';
    html += manifests.map((m) => '<div class="dep-manifest" data-tip="' + esc(m.note || '') + '">'
      + '<b>' + esc(m.path) + '</b><span>' + esc(m.note || '') + '</span></div>').join('');
  }
  if (!total) {
    html += '<div class="rail-empty">该项目未声明依赖。'
      + (manifests.length ? '' : '<br>也未找到依赖清单文件。') + '</div>';
  } else {
    html += railDepHtml(detail, deps);
    html += '<div class="rail-empty">左列为清单声明，右列为实际安装版本。</div>';
  }
  body.innerHTML = html;
  // 工具条与分组：就地绑事件（这一段每次重画，事件也跟着重绑）
  const qi = $('#railQ');
  if (qi) {
    qi.addEventListener('input', () => { RAIL.q = qi.value; renderRail(); $('#railQ').focus(); });
    qi.addEventListener('keydown', (e) => { if (e.key === 'Escape') { RAIL.q = ''; qi.value = ''; renderRail(); } });
  }
  const rl = $('#railReload');
  if (rl) rl.addEventListener('click', () => { RAIL.q = RAIL.q; refreshQuiet(true); sfx('click'); });
  const ri = $('#railInstall');
  if (ri) ri.addEventListener('click', () => { switchTab('run'); doRun('install'); });
  // 点一行 → 展开它的详情（声明/实际/来源/状态/下游）
  body.querySelectorAll('.fact-row').forEach((r) =>
    r.addEventListener('click', () => {
      RAIL.detail = RAIL.detail || new Set();
      const n = r.dataset.dep;
      if (RAIL.detail.has(n)) RAIL.detail.delete(n); else RAIL.detail.add(n);
      sfx('click');
      renderRail();
    }));
  // 展开/收起某个依赖的下游树
  body.querySelectorAll('[data-tree]').forEach((b) =>
    b.addEventListener('click', (e) => {
      e.stopPropagation();
      RAIL.expanded = RAIL.expanded || new Set();
      const n = b.dataset.tree;
      if (RAIL.expanded.has(n)) RAIL.expanded.delete(n); else RAIL.expanded.add(n);
      sfx('click');
      renderRail();
    }));
  body.querySelectorAll('.dep-grp').forEach((g) =>
    g.addEventListener('click', () => {
      const k = g.dataset.grp;
      if (RAIL.collapsed.has(k)) RAIL.collapsed.delete(k); else RAIL.collapsed.add(k);
      sfx('click');
      renderRail();
    }));
}
// ── 底部活动带：任何页面都能看到"上次跑成什么样 / 现在跑到哪" ──
const BAND = { kind: null, stages: [] };

// 长时间操作时轮播的提示（MC 的 splash 文字那味儿，但内容是真有用的）
const TIPS = [
  '卡在 ModuleNotFoundError 时，先确认用的是哪个解释器，而不是先重装。',
  '虚拟环境的意义是隔离；没绑进契约的话，装包会落到全局。',
  '锁文件决定「下次装到的是不是同一个版本」。',
  '路径里有中文或空格时，老式工具链更容易报「找不到文件」。',
  '进程没占端口，多半是没监听成功，而不是防火墙。',
  '改了依赖清单要重新装一次，否则装的还是旧的。',
  '编译报错先看第一条，后面大多是它的连锁。',
  '同一个地方改第三遍还在报错，先停下来看上一层。',
];
let tipTimer = null;
function startTips(el) {
  if (!el) return;
  let i = Math.floor(Math.random() * TIPS.length);
  const paint = () => {
    el.innerHTML = '<span class="tipin">提示 · ' + esc(TIPS[i % TIPS.length]) + '</span>';
    i++;
  };
  paint();
  clearInterval(tipTimer);
  tipTimer = setInterval(paint, 4600);
}
function stopTips() { clearInterval(tipTimer); tipTimer = null; }

// 数字变化时跳一下
function popNum(el) {
  if (!el) return;
  el.classList.add('num');
  el.classList.remove('pop');
  void el.offsetWidth;
  el.classList.add('pop');
  setTimeout(() => el.classList.remove('pop'), 320);
}

// 启动浮层：跑起来时的那一屏（可后台继续，不挡着你干活）
function launchOverlay(kind, command) {
  document.querySelector('.launch')?.remove();
  const el = document.createElement('div');
  el.className = 'launch';
  el.innerHTML = '<div class="lcard">'
    + '<div class="lhead"><span class="lmark"></span><div><h2>'
    + (kind === 'run' ? '正在启动项目' : kind === 'test' ? '正在跑测试' : '正在装依赖') + '</h2>'
    + '<div class="lsub mono">' + esc(command || '') + '</div></div></div>'
    + '<div class="progress pulse"><div class="fill" id="lFill"></div></div>'
    + '<ol class="steps" id="lSteps"></ol>'
    + '<div class="ltip" id="lTip"></div>'
    + '<div class="lfoot"><button class="btn ghost sm" data-launch="bg">后台继续</button>'
    + '<span class="hint">跑起来后这一屏会自动收掉，结果留在底部那条带里。</span></div>'
    + '</div>';
  document.body.appendChild(el);
  startTips(el.querySelector('#lTip'));
  el.addEventListener('click', (e) => {
    const a = e.target.closest('button')?.dataset.launch;
    if (a === 'bg') closeLaunch();
  });
  return el;
}
function launchSteps(stages, plan) {
  const ol = document.querySelector('#lSteps');
  const fill = document.querySelector('#lFill');
  if (!ol) return;
  ol.innerHTML = plan.map((name, i) => {
    const st = stages.find((s) => s.index === i + 1);
    const cls = !st ? '' : (st.status === 'ok' ? 'done' : st.status === 'fail' ? 'fail' : 'now');
    return '<li class="' + cls + '"><span class="mk">'
      + (cls === 'done' ? '✓' : cls === 'fail' ? '✕' : '•') + '</span>'
      + '<span>' + esc(st ? st.title : name) + '</span>'
      + '<span class="ms">' + (st && st.ms != null ? st.ms + 'ms' : '') + '</span></li>';
  }).join('');
  const done = stages.filter((s) => s.status === 'ok').length;
  // 交给 CSS 的 scaleX 去画（见 style.css 里 .progress .fill 的说明）
  if (fill) fill.style.setProperty('--p', String(Math.max(0.04, done / plan.length)));
}
function closeLaunch() {
  const el = document.querySelector('.launch');
  stopTips();
  if (!el) return;
  el.classList.add('out');
  setTimeout(() => el.remove(), 300);
}

function bandShow(kind) {
  const b = $('#band');
  if (!b) return;
  BAND.kind = kind;
  BAND.stages = [];
  b.hidden = false;
  b.className = 'band run';
  $('#bandTitle').textContent = kind === 'run' ? '正在运行项目' : (kind === 'test' ? '正在跑测试' : '正在装依赖');
  $('#bandDetail').textContent = '';
  $('#bandStages').innerHTML = '';
  $('#btnBandOpen').hidden = false;
  $('#btnBandClose').hidden = false;
  // 进度条：按阶段数推进（跑完再收束）
  const p = $('#bandProgress');
  if (p) { p.hidden = false; p.classList.remove('full'); p.classList.add('pulse'); }
  if ($('#bandFill')) $('#bandFill').style.width = '4%';
  // 不再弹那个「很多字、看不清、闪完就没」的浮层。
  // 运行过程改成运行页上一串小面板，流式涌出、跑完留着。
  streamReset(kind);
  sfx('click');
}
// ═══ 运行时：阶段流 ════════════════════════════════════════
//
// 一条规矩：**屏幕上只放「结论 + 关键几行」，其余折起来。**
// 之前的做法是一块浮层塞满原始输出——看不清、跑完就收，等于没看。
// 现在每一步是一个小面板：名字 / 命令 / 状态 / 用时，
// 下面最多三条关键行，展开才有原始输出。面板按顺序涌出，跑完不走。
const STREAM = { blocks: new Map(), cur: null };

/// 这一行值不值得摆在面板上（不值得的就只留在折叠区里）
const KEY_LINE = /(error|错误|warning|警告|failed|失败|panic|Traceback|Exception|超时|拒绝|找不到|E\d{4}\b|TS\d{3,})/i;

function streamReset(kind) {
  const box = $('#stageStream');
  if (!box) return;
  STREAM.blocks = new Map();
  STREAM.cur = null;
  box.hidden = false;
  box.innerHTML = '';
  const head = document.createElement('div');
  head.className = 'stream-head';
  const label = kind === 'run' ? '运行过程' : kind === 'test' ? '测试过程' : kind === 'build' ? '构建过程' : '安装过程';
  // 轮播小贴士原来挂在那个浮层上，浮层没了就搬到这儿：
  // 面板上只占一行，跑的时候有人跟你说话，但不挡任何东西。
  head.innerHTML = '<b>' + esc(label) + '</b><span class="hint">每一步的面板会留在这儿；原始输出折在每块里面</span>'
    + '<span class="grow"></span><span class="stream-tip" id="streamTip"></span>';
  box.appendChild(head);
  startTips($('#streamTip'));
}

function streamStage(ev) {
  const box = $('#stageStream');
  if (!box || !ev || ev.index == null) return;
  let b = STREAM.blocks.get(ev.index);
  if (!b) {
    b = document.createElement('section');
    b.className = 'stage-card';
    b.style.setProperty('--i', String(ev.index || 0));
    b.innerHTML = '<div class="sc-head"><span class="sc-dot"></span>'
      + '<b class="sc-name"></b><span class="sc-cmd"></span>'
      + '<span class="grow"></span><span class="sc-ms"></span><span class="sc-state"></span></div>'
      + '<div class="sc-keys"></div>'
      + '<details class="sc-more"><summary>展开原始输出</summary><pre class="sc-raw"></pre></details>';
    box.appendChild(b);
    STREAM.blocks.set(ev.index, b);
    sfx('click');
  }
  STREAM.cur = b;
  b.className = 'stage-card ' + (ev.status === 'fail' ? 'bad' : ev.status === 'ok' ? 'ok' : 'now');
  b.querySelector('.sc-name').textContent = ev.title || ('第 ' + ev.index + ' 步');
  if (ev.detail) b.querySelector('.sc-cmd').textContent = ev.detail;
  b.querySelector('.sc-ms').textContent = ev.ms != null ? ev.ms + ' ms' : '';
  b.querySelector('.sc-state').textContent = ev.status === 'ok' ? '完成' : ev.status === 'fail' ? '失败' : '进行中';
}

/// 输出的一行：只有「关键行」才浮到面板上（最多三条），其余进折叠区。
/// 面板上永远是最新的、最要紧的三句——不用在几百行里找。
function streamLine(text, source) {
  const b = STREAM.cur;
  if (!b) return;
  const raw = b.querySelector('.sc-raw');
  if (raw && raw.textContent.length < 200000) raw.textContent += text + '\n';
  if (!KEY_LINE.test(text)) return;
  const keys = b.querySelector('.sc-keys');
  if (!keys) return;
  const d = document.createElement('div');
  d.className = 'sc-key' + (source === 'stderr' ? ' err' : '');
  d.textContent = text.trim().slice(0, 200);
  keys.appendChild(d);
  while (keys.children.length > 3) keys.firstElementChild.remove();   // 只留最近三条
}

function bandStage(ev) {
  if (!BAND.kind) return;
  BAND.stages = BAND.stages.filter((s) => s.index !== ev.index);
  BAND.stages.push(ev);
  BAND.stages.sort((a, b) => a.index - b.index);
  const plan = BAND.kind === 'run' ? PLAN_RUN : PLAN_TASK;
  const ol = $('#bandStages');
  if (!ol) return;
  ol.innerHTML = plan.map((name, i) => {
    const st = BAND.stages.find((s) => s.index === i + 1);
    const cls = !st ? '' : (st.status === 'ok' ? 'ok' : st.status === 'fail' ? 'fail' : 'now');
    return '<li class="' + cls + '">' + esc(st ? st.title : name)
      + (st && st.ms != null ? ' ' + st.ms + 'ms' : '') + '</li>';
  }).join('');
  // 进度 = 已完成阶段 / 总阶段
  const done = BAND.stages.filter((s) => s.status === 'ok').length;
  const fill = $('#bandFill');
  if (fill) fill.style.width = Math.max(4, Math.round((done / plan.length) * 100)) + '%';
  launchSteps(BAND.stages, plan);
}
function bandFinish(out, err) {
  const b = $('#band');
  if (!b) return;
  const ok = !err && out && out.exit_code === 0;
  b.className = 'band ' + (ok ? 'ok' : 'bad');
  const p = $('#bandProgress');
  if (p) {
    p.classList.remove('pulse');
    const fill = $('#bandFill');
    if (fill) fill.style.width = '100%';
    if (ok) { p.classList.add('full'); }
  }
  sfx(ok ? 'done' : 'bad');
  if (ok) burst($('#bandDot'), 6);
  b.classList.add('settle');
  setTimeout(() => b.classList.remove('settle'), 400);
  launchSteps(BAND.stages, BAND.kind === 'run' ? PLAN_RUN : PLAN_TASK);
  setTimeout(closeLaunch, ok ? 700 : 1400);
  if (err) {
    $('#bandTitle').textContent = '这次没跑起来';
    $('#bandDetail').textContent = String(err);
  } else {
    $('#bandTitle').textContent = ok ? '上次运行：成功' : '上次运行：退出码 ' + out.exit_code;
    const crux = out.diagnosis && out.diagnosis.crux;
    $('#bandDetail').textContent = crux ? '症结 · ' + crux : '';
  }
}
function bandFromHistory() {
  const b = $('#band');
  if (!b) return;
  const last = (S.data && S.data.runs || [])[0];
  if (!last) { b.hidden = true; return; }
  b.hidden = false;
  b.className = 'band ' + (last.exit_code === 0 ? 'ok' : 'bad');
  $('#bandTitle').textContent = '上次' + (last.kind === 'run' ? '运行' : last.kind === 'test' ? '测试' : '装依赖')
    + '：' + (last.exit_code === 0 ? '成功' : '退出码 ' + last.exit_code);
  $('#bandDetail').textContent = last.crux ? '症结 · ' + last.crux : '';
  $('#bandStages').innerHTML = '';
  $('#btnBandOpen').hidden = false;
  $('#btnBandClose').hidden = false;
}

// ── 顶部项目切换器 ────────────────────────────────────────
function renderSwitcher() {
  const menu = $('#switchMenu');
  const cnt = $('#switchCount');
  if (!menu) return;
  const list = (PS.all || []).slice(0, 40);
  if (cnt) cnt.textContent = list.length;
  menu.innerHTML = list.length
    ? list.map((p) => '<button data-path="' + esc(p.path) + '">' + esc(p.name)
        + (S.root === p.path ? '<span class="hint"> · 当前</span>' : '')
        + '<small>' + esc(p.path) + '</small></button>').join('')
    : '<div class="rail-empty">还没扫到项目。去「概览 → 选择目录…」扫一次。</div>';
}

// ── 项目列表：发现 → 一路异步分析 → 附属卡片 ─────────────
// 生命周期第一段：还没进项目的时候，先把「你手上有哪些项目」摆出来。
// 卡片先全部出现，再一张张填内容——新手需要的是「有反馈」，
// 而不是盯着一个转圈等十秒。
const PAGE_SIZE = 8;
const PS = { parent: '', depth: 2, all: [], page: 0, sel: null, info: {}, draft: null, busy: false,
  pinned: false, hoverTimer: null };

// 全局选中：整个外壳只有一处「当前项目」——S.root。
// 项目列表、笔记页、编辑器、依赖栏、流水线都读它，不再各记各的。
function isCurrent(path) {
  return !!S.root && !!path && path.toLowerCase() === S.root.toLowerCase();
}

/// 一条横幅该不该亮：全局选中的那个，或者正在预览详情的那个。
function bannerOn(path) {
  return isCurrent(path) || !!(PS.sel && PS.sel.path === path);
}

// 一个项目 = 一条宽横幅：左边名字与路径，中间关键事实，右边动作。
// 关键事实单独拆出来，是为了「就地更新」——分析完一个只重画它那一格，
// 不重画整张列表（整张重画会让所有横幅的入场动画重放一遍，看着就是闪）。
function projFacts(p, info) {
  // 还没分析完：摆的是「和真数据一一对应的空位」，不是一个转圈。
  // 这样数据回来时是原位填字，不是整张卡跳动——看着才是稳稳地在加载。
  if (!info) {
    const sk = (k) => '<span class="f"><span class="k">' + k
      + '</span><span class="v skel">正在加载中…</span></span>';
    return sk('类型') + sk('最近改动') + sk('契约') + sk('待处理') + sk('卫生');
  }
  const when = p.age_days === 0 ? '今天' : (p.age_days === 1 ? '昨天' : p.age_days + ' 天前');
  return '<span class="f"><span class="k">类型</span><span class="v">' + esc(info.kinds) + '</span></span>'
    + '<span class="f"><span class="k">最近改动</span><span class="v">' + when + '</span></span>'
    + '<span class="f"><span class="k">契约</span><span class="v">'
    + (info.contract ? '<span class="ok">有</span>' : '<span class="bad">没有</span>') + '</span></span>'
    + '<span class="f"><span class="k">待处理</span><span class="v">'
    + (info.todo ? '<span class="bad">' + info.todo + ' 件</span>' : '<span class="ok">无</span>') + '</span></span>'
    + '<span class="f"><span class="k">卫生</span><span class="v">'
    + (info.auditHot ? '<span class="bad">' + info.auditHot + ' 条</span>' : String(info.auditAll || 0) + ' 条')
    + '</span></span>'
    + (info.run ? '<span class="f"><span class="k">怎么跑</span><span class="v" data-tip="' + esc(info.run) + '">'
        + esc(info.run) + '</span></span>' : '');
}

function projCard(p, i) {
  const info = PS.info[p.path];
  const on = bannerOn(p.path);
  return '<div class="banner stagger' + (on ? ' on' : '') + '" style="--i:' + (i || 0) + '"'
    + ' data-path="' + esc(p.path) + '">'
    + '<div><div class="b-name">' + esc(p.name)
    + '<span class="b-kind">' + esc(String(p.markers[0] || '目录').toLowerCase()) + '</span></div>'
    + '<div class="b-path" data-tip="' + esc(p.path) + '">' + esc(p.path) + '</div></div>'
    + '<div class="b-facts">' + projFacts(p, info) + '</div>'
    + '<div class="b-acts">'
    + '<button class="btn ghost sm" data-act="detail">项目详情</button>'
    + '<button class="btn sm" data-act="open">打开</button>'
    + '</div></div>';
}

/// 就地更新一条横幅（以及它的详情卡，如果正开着）——不碰别的节点。
function patchBanner(path, full) {
  const list = $('#projList');
  if (list) {
    for (const el of Array.from(list.children)) {
      if (el.dataset && el.dataset.path === path) {
        const p = PS.all.find((x) => x.path === path);
        const box = el.querySelector('.b-facts');
        if (p && box) box.innerHTML = projFacts(p, PS.info[path]);
        el.classList.toggle('on', isCurrent(path));
        break;
      }
    }
  }
  if (PS.sel && PS.sel.path === path && !full) renderDetail();
}
function renderProjList() {
  const list = $('#projList');
  if (!list) return;
  const page = PS.all.slice(PS.page * PAGE_SIZE, (PS.page + 1) * PAGE_SIZE);
  list.innerHTML = page.map((p, i) => projCard(p, i)).join('')
    || '<div class="hint" style="padding:10px">这个目录里没扫到项目。换个父目录，或者直接把项目打开。</div>';
  const pages = Math.max(1, Math.ceil(PS.all.length / PAGE_SIZE));
  const pager = $('#projPager');
  if (!pager) return;
  pager.hidden = PS.all.length <= PAGE_SIZE;
  pager.innerHTML = '<button class="btn ghost sm" data-page="prev"' + (PS.page === 0 ? ' disabled' : '') + '>上一页</button>'
    + '<span>第 ' + (PS.page + 1) + ' / ' + pages + ' 页 · 共 ' + PS.all.length + ' 个项目</span>'
    + '<button class="btn ghost sm" data-page="next"' + (PS.page >= pages - 1 ? ' disabled' : '') + '>下一页</button>';
  if (PS.sel) renderDetail();
}

async function scanProjects(parent, depth) {
  if (PS.busy) return;
  PS.busy = true;
  PS.parent = parent;
  PS.depth = depth;
  PS.page = 0;
  PS.info = {};
  PS.sel = null;
  PS.draft = null;
  if ($('#scanParent')) $('#scanParent').value = parent;
  if ($('#scanDepth')) $('#scanDepth').value = String(depth);
  const list = $('#projList');
  const empty = $('#projPager');
  if (empty) empty.hidden = true;
  if (list) list.innerHTML = '<div class="hint" style="padding:10px"><span class="spin"></span> 正在看这个目录里有哪些项目…</div>';
  if ($('#projDetail')) $('#projDetail').hidden = true;
  try {
    const r = await invoke('list_projects', { parent, depth });
    PS.all = r.projects || [];
    localStorage.setItem('jingwei.parent', parent);
    localStorage.setItem('jingwei.depth', String(depth));
    renderProjList();
    renderSwitcher();
    renderRailProjs();
    feedback(PS.all.length ? 'info' : 'warn',
      '扫到 ' + PS.all.length + ' 个项目',
      esc(parent) + (PS.all.length ? '：卡片已经出来了，正在一张张分析' : '：这个目录下没有认出的项目文件'));
    // 卡片已经在了，内容慢慢填：先这一页，再后台补剩下的
    if (SET.analyze) {
      analyzeRange(0, PAGE_SIZE);
      if (PS.all.length > PAGE_SIZE) setTimeout(() => analyzeRange(PAGE_SIZE, PS.all.length), 400);
    }
  } catch (e) {
    const msg = typeof e === 'string' ? e : (e && e.message) || String(e);
    feedback('warn', '这个目录扫不动', esc(msg) + '<br>项目本身没受影响——换个父目录再试，或者直接把它打开。');
    if (list) {
      list.innerHTML = '<div class="hint" style="padding:10px">扫不动：' + esc(msg) + '</div>';
    }
  } finally {
    PS.busy = false;
  }
}

async function analyzeRange(from, to) {
  if (!SET.analyze) return;
  let failed = 0;
  for (let i = from; i < Math.min(to, PS.all.length); i++) {
    const p = PS.all[i];
    if (PS.info[p.path]) continue;
    try {
      const d = await invoke('inspect', { root: p.path });
      const todo = (d.findings || []).filter((f) => (f.severity === 'error' || f.severity === 'warn') && !f.group);
      const hy = (d.hygiene && d.hygiene.items) || [];
      PS.info[p.path] = {
        kinds: (d.summary || []).find((f) => f.k === '项目类型')?.v || '认不出类型',
        contract: !!d.contractName || !!d.runCommand,
        run: d.runCommand || null,
        todo: todo.length,
        first: todo[0] ? todo[0].title : null,
        auditHot: hy.filter((f) => f.group && f.severity !== 'info').length,
        auditAll: hy.filter((f) => f.group).length,
      };
    } catch (e) {
      PS.info[p.path] = { kinds: '读不出来', contract: false, todo: 0, auditHot: 0, auditAll: 0 };
      failed++;
    }
    patchBanner(p.path);
    await new Promise((r) => setTimeout(r, 30));
  }
  if (failed) {
    feedback('warn', '有 ' + failed + ' 个项目没分析出来',
      '多数是目录里文件太多、或者没有权限。卡片的其余信息照常显示。');
  }
}

async function openDetail(p) {
  PS.sel = p;
  // 只改高亮，不重建列表：停留预览时整张重画会把入场动画重放一遍，看着就是闪。
  paintProjectBanners();
  const box = $('#projDetail');
  if (!box) return;
  box.hidden = false;
  box.innerHTML = '<div class="pd-bar"><span class="pd-title">' + esc(p.name) + '</span>'
    + '<span class="hint"><span class="spin"></span> 正在解析项目声明…</span></div>'
    + '<div class="pd-body"></div>';
  try {
    PS.draft = await invoke('contract_draft', { root: p.path, save: false });
  } catch (e) {
    PS.draft = null;
  }
  renderDetail();
}

function renderDetail() {
  const box = $('#projDetail');
  if (!box || !PS.sel) return;
  const p = PS.sel;
  const info = PS.info[p.path] || {};
  const c = PS.draft && PS.draft.contract;
  const why = (PS.draft && PS.draft.why) || [];
  const run = (c && c.commands && c.commands.run) || info.run;

  box.innerHTML = '<div class="pd-bar">'
    + '<span class="pd-title">' + esc(p.name) + '</span>'
    + '<button class="btn sm" data-pd="open">打开项目</button>'
    + '<button class="btn ghost sm" data-pd="contract">' + (info.contract ? '重新起草契约' : '生成契约') + '</button>'
    + '<button class="btn ghost sm" data-pd="editor">用编辑器打开</button>'
    + '<button class="btn ghost sm" data-pd="clean">去清理</button>'
    + '<button class="btn ghost sm" data-pd="close">关闭</button>'
    + '</div>'
    + '<div class="pd-body">'
    + '<div class="pd-sec"><div class="pd-k">它会这样跑（从项目文件里看出来的，不是猜的）</div>'
    + '<div class="pd-run">' + (run ? esc(run) : '还没看出启动方式——下面自己填，或者点「生成契约」') + '</div>'
    + (why.length ? '<div class="pd-why">' + why.map(esc).join('<br>') + '</div>' : '')
    + '</div>'
    + '<div class="pd-sec"><div class="pd-k">三步命令（可以直接改，改完点保存）</div>'
    + '<input id="pdInstall" value="' + esc((c && c.commands.install) || '') + '" placeholder="装依赖，例如 npm install">'
    + '<input id="pdRun" value="' + esc((c && c.commands.run) || '') + '" placeholder="启动，例如 npm run dev">'
    + '<input id="pdTest" value="' + esc((c && c.commands.test) || '') + '" placeholder="跑测试（可以空着）">'
    + '<div class="row2"><button class="btn sm" data-pd="save">保存契约</button>'
    + '<span class="hint">存成项目里的 env.contract.json，下次打开直接按它跑</span></div>'
    + '</div>'
    + '<div class="pd-sec"><div class="pd-k">这个项目的现状</div>'
    + '<div class="pd-why">' + esc(info.kinds || '…')
    + ' · ' + (info.contract ? '有契约' : '没有契约')
    + ' · ' + (info.todo ? info.todo + ' 件要处理' : '没有挡路的问题')
    + (info.auditAll != null ? ' · 项目检查 ' + info.auditAll + ' 条（' + info.auditHot + ' 条值得处理）' : '')
    + '</div></div></div>';
}

async function detailAct(act) {
  const p = PS.sel;
  if (!p) return;
  if (act === 'close') {
    PS.sel = null; PS.draft = null; PS.pinned = false;
    $('#projDetail').hidden = true;
    paintProjectBanners();
    return;
  }
  if (act === 'open') { PS.pinned = false; await load(p.path); return; }
  if (act === 'editor') {
    await guard(async () => {
      if (!S.defaultOpener) { toast('还没有默认编辑器：先打开一个项目再设置'); return; }
      await invoke('open_in', { root: p.path, id: S.defaultOpener });
      toast('已交给它打开');
    });
    return;
  }
  if (act === 'clean') { await load(p.path); switchTab('clean'); return; }
  if (act === 'contract') {
    await guard(async () => {
      await invoke('contract_draft', { root: p.path, save: true });
      PS.info[p.path] = null;
      PS.draft = await invoke('contract_draft', { root: p.path, save: false });
      toast('契约写好了：' + p.name + '\\env.contract.json');
      renderDetail();
      analyzeRange(0, PS.all.length);
    });
    return;
  }
  if (act === 'save') {
    await guard(async () => {
      const c = JSON.parse(JSON.stringify((PS.draft && PS.draft.contract) || {}));
      c.schema_version = c.schema_version || 1;
      c.commands = {
        install: $('#pdInstall').value.trim() || null,
        run: $('#pdRun').value.trim() || null,
        test: $('#pdTest').value.trim() || null,
      };
      const saved = await invoke('contract_save', { root: p.path, contract: c });
      PS.draft = { contract: saved, why: ['这是你改过的版本，工具不会再覆盖命令'], saved: true };
      PS.info[p.path] = null;
      toast('契约已保存');
      renderDetail();
      analyzeRange(0, PS.all.length);
    });
  }
}

async function scanInit() {
  let parent = localStorage.getItem('jingwei.parent');
  // 上次扫的深度优先；没有就用设置里的默认
  const depth = parseInt(localStorage.getItem('jingwei.depth') || '0', 10) || (SET.scanDepth || 2);
  if (!parent) {
    try {
      const d = await invoke('default_parent');
      const pd = unpack(d);
      parent = pd.state === 'yes' ? (pd.data?.parent || null) : null;
      // 一个候选都没有是**异常**（连盘根都在候选里），要说话，别静默。
      if (pd.state !== 'yes') toast(pd.reason, true);
    } catch (e) { parent = null; }
  }
  if (parent) await scanProjects(parent, depth);
}

// ── 项目 ───────────────────────────────
async function pickProject() {
  await guard(async () => {
    const dir = await invoke('pick_folder', { title: '选择项目文件夹' });
    if (dir) await load(dir);
  });
}
async function load(root) {
  const data = await invoke('inspect', { root });
  S.root = root; S.data = data; S.clean = null; S.picked.clear(); S.live = null; S.audit = null;
  S.changes = null;
  // 换项目时把编辑器状态清干净：不清的话第二、第三个项目打开时还是上一个项目的文件列表
  ED.path = null; ED.files = []; ED.dirty = false; ED.disk = ''; ED.text = ''; edDrafts = [];
  ED.busy = false; ED.closedDirs = new Set(); ED.encoding = 'utf-8'; ED.eol = 'lf';
  if ($('#edEnc')) $('#edEnc').textContent = '';
  ED.tabs = []; ED.charW = 0;
  if ($('#edTabs')) $('#edTabs').innerHTML = '';
  if ($('#edFind')) $('#edFind').hidden = true;
  if ($('#edBrkA')) $('#edBrkA').hidden = true;
  if ($('#edBrkB')) $('#edBrkB').hidden = true;
  if ($('#edText')) { $('#edText').value = ''; $('#edText').disabled = false; }
  if ($('#edPath')) $('#edPath').textContent = '还没有打开文件';
  if ($('#edDirty')) $('#edDirty').hidden = true;
  if ($('#edCode')) $('#edCode').innerHTML = '';
  if ($('#edNums')) $('#edNums').innerHTML = '1';
  if ($('#edCrumb')) { $('#edCrumb').hidden = true; $('#edCrumb').innerHTML = ''; }
  if ($('#edGuides')) $('#edGuides').innerHTML = '';
  if ($('#edHl')) $('#edHl').hidden = true;
  if ($('#edGl')) $('#edGl').hidden = true;
  if ($('#edLnCol')) $('#edLnCol').textContent = '第 1 行，第 1 列';
  ED.guideN = -1;
  if ($('#edStat')) $('#edStat').textContent = '—';
  if ($('#edFiles')) {
    $('#edFiles').innerHTML = '<div class="hint" style="padding:10px">'
      + '<span class="spin"></span> 正在加载中…</div>';
  }
  const name = root.split(/[\\/]/).filter(Boolean).pop() || root;
  $('#projectName').textContent = name;
  $('#projectPath').textContent = root;
  $('#sfProject').textContent = name;
  $('#homeBody').hidden = false;
  $('#welcome').hidden = true;
  if ($('#newModal')) $('#newModal').hidden = true;
  $('#openSplit').hidden = false;
  paintNav();
  $('#cleanSummary').textContent = '切到这一页时自动清点。';
  $('#cleanList').innerHTML = '';
  renderHome();
  renderPipeline(data);
  renderRail();
  renderSwitcher();
  renderRailProjs();
  dockProblems(data);
  // 全局选中落到实处：编辑器文件树、笔记页的项目卡片都跟着这一处走
  edLoadFiles();
  renderNoteProjects();
  paintProjectBanners();
  // 活动带只在跑/构建/测试时出现——不在静止状态下占着屏幕（这里刻意不还原历史）
  panelPolicy();
  loadChanges();
  renderRuns(data.runs || []);
  renderNotes();
  paintOpenButton();
  feedback('info', '打开了 ' + name, esc(root) + ' · 类型：'
    + esc(((data.summary || []).find((f) => f.k === '项目类型') || {}).v || '认不出'));
  afterAuto();
}

// （paintProjectBanners 定义在 load() 之后）

// ═══ 底部工具窗：问题 / 终端 / 消息 ═══════════════════════════
// 学 IDEA 的形状：报错、跑起来的东西、系统说的话，都在底部这一处，
// 不在屏幕四角各飘各的。密度靠的是「一行一条 + 列对齐 + 等宽数字」，
// 不是靠把字缩小。左边一列是清单，右边写清这一条「为什么、怎么自己验证」。
const DOCK = { tab: 'problems', open: false, msgs: [], term: [], probs: [], filter: 'all' };

function dockSetTab(tab) {
  DOCK.tab = tab;
  document.querySelectorAll('.dtab').forEach((b) => b.classList.toggle('on', b.dataset.dtab === tab));
  document.querySelectorAll('.dpane').forEach((p) => p.classList.toggle('on', p.id === 'pane' + tab[0].toUpperCase() + tab.slice(1)));
  const clear = $('#dockClear');
  if (clear) clear.hidden = tab === 'problems';
}

function dockShow(tab) {
  const d = $('#dock');
  if (!d) return;
  if (tab) dockSetTab(tab);
  if (!DOCK.open) {
    d.hidden = false;
    DOCK.open = true;
    document.body.classList.add('dock-open');
    sfx('click');
  }
  dockBadges();
}

function dockHide() {
  const d = $('#dock');
  if (!d) return;
  d.hidden = true;
  DOCK.open = false;
  document.body.classList.remove('dock-open');
}

function dockToggle(tab) {
  if (DOCK.open && (!tab || DOCK.tab === tab)) dockHide();
  else dockShow(tab || DOCK.tab);
}

function dockBadges() {
  // 状态栏「终端」在输出流动时轻轻亮一下——不弹窗、不换页，只是有反应
  const tb = $('#btnDockTerm');
  if (tb) {
    const on = !!(S.live && S.live.kind);
    tb.classList.toggle('pulse', on);
  }
  const errs = DOCK.probs.filter((p) => p.severity === 'error').length;
  const wars = DOCK.probs.filter((p) => p.severity === 'warn').length;
  const n = DOCK.probs.length;
  const setC = (sel, v) => {
    const el = $(sel);
    if (!el) return;
    el.textContent = v;
    el.dataset.zero = v ? '0' : '1';
  };
  setC('#dCntProb', n);
  setC('#dCntMsg', DOCK.msgs.length);
  setC('#barProb', n);
  const t = document.querySelector('.dtab[data-dtab="problems"]');
  if (t) t.classList.toggle('hot', errs > 0);
  const hint = $('#dockHint');
  if (hint) {
    hint.textContent = n
      ? errs + ' 项阻断运行 · ' + wars + ' 项建议处理'
      : (DOCK.probs.length === 0 && S.root ? '未发现诊断项' : '');
  }
}

/// 把一次检查的结论摊到「问题」栏。severity 只用两档：要修 / 注意。
/// 顺序不按严重度排，按排查链的层排——先确认「用的是哪个」，再谈「缺什么」。
function dockProblems(d) {
  const out = [];
  const push = (f, group) => out.push({
    severity: f.severity === 'error' ? 'error' : (f.severity === 'warn' ? 'warn' : 'info'),
    title: f.title || f.id || '（没有标题）',
    layer: group || f.layer || '其他',
    next: f.next || '',
    boundary: f.boundary || '',
    evidence: f.evidence || [],
    verify: f.verify || [],
    file: f.file || null,
    line: f.line || null,
  });
  (d.findings || []).filter((f) => !f.group && (f.severity === 'error' || f.severity === 'warn')).forEach((f) => push(f));
  // 编译器诊断单独一组：这一组是"你写的代码"的错，和环境检查不是一回事，
  // 混在一起会让人分不清"是我的问题还是机器的问题"。
  (DOCK.diags || []).forEach((x) => out.push({
    severity: x.level === 'error' ? 'error' : 'warn',
    title: (x.code ? x.code + ' · ' : '') + x.message,
    layer: '编译器',
    next: '点这一条跳到 ' + x.file + ':' + x.line,
    boundary: '',
    evidence: [x.file + ':' + x.line + ':' + x.col],
    verify: [],
    code: x.code, file: x.file, line: x.line, col: x.col,
  }));
  ((d.hygiene && d.hygiene.items) || [])
    .filter((f) => f.group && f.severity !== 'info')
    .forEach((f) => push(f, '项目检查'));
  const last = (d.runs || [])[0];
  if (last && last.exit_code !== 0 && last.crux) {
    out.unshift({ severity: 'error', title: '上次运行失败 · ' + last.crux, layer: '运行结果',
      next: '在「终端」栏查看输出末尾若干行', boundary: '', evidence: last.tail || [], verify: [] });
  }
  DOCK.probs = out;
  dockPaintProblems();
  dockBadges();
  // 有会挡住运行的，就把这一栏顶出来——报错留在下面，不用去别处找
  if (out.some((p) => p.severity === 'error')) dockShow('problems');
}

function dockPaintProblems() {
  const list = $('#probList');
  const filt = $('#probFilter');
  if (!list) return;
  const cnt = (s) => DOCK.probs.filter((p) => s === 'all' || p.severity === s).length;
  if (filt) {
    filt.innerHTML = [['all', '全部'], ['error', '要修'], ['warn', '注意']]
      .map(([k, t]) => '<button class="dchip' + (DOCK.filter === k ? ' on' : '')
        + '" data-pf="' + k + '">' + t + ' ' + cnt(k) + '</button>').join('');
  }
  const rows = DOCK.probs.filter((p) => DOCK.filter === 'all' || p.severity === DOCK.filter);
  if (!rows.length) {
    list.innerHTML = '<div class="dempty" style="padding:12px">'
      + (S.root ? '该分类下无条目。' : '当前未选择项目。') + '</div>';
    return;
  }
  let html = '';
  let layer = null;
  rows.forEach((p) => {
    if (p.layer !== layer) {
      layer = p.layer;
      const n = rows.filter((x) => x.layer === layer).length;
      html += '<div class="dgroup"><b>' + esc(layer) + '</b><span>' + n + ' 条</span></div>';
    }
    const src = p.file ? esc(p.file.replace(/\\/g, '/').split('/').slice(-2).join('/')) + (p.line ? ':' + p.line : '') : '';
    html += '<div class="drow ' + p.severity + '" data-prob="' + esc(p.title) + '">'
      + '<span class="sev">' + (p.severity === 'error' ? '✕' : '!') + '</span>'
      + '<span class="tt">' + esc(p.title) + '</span>'
      + '<span class="src">' + src + '</span>'
      + '<span class="tag">' + (p.severity === 'error' ? '阻断' : '建议') + '</span></div>';
  });
  list.innerHTML = html;
}

/// 跳到某一行（诊断条目点一下就到）。用的是编辑器自己那套坐标，
/// 不新开窗口、不弹提示——"点了就到地方"是这类工具最该做好的动作。
async function gotoDiag(file, line, col) {
  if (!file) return;
  switchTab('editor');
  await edOpen(file);
  const t = $('#edText');
  if (!t || t.disabled) { toast('这个文件打不开（可能是二进制或超大）', true); return; }
  const lines = t.value.split('\n');
  let pos = 0;
  for (let i = 0; i < Math.min(Math.max(line - 1, 0), lines.length); i++) pos += lines[i].length + 1;
  pos = Math.min(pos + Math.max(0, (col || 1) - 1), t.value.length);
  t.focus();
  t.selectionStart = t.selectionEnd = pos;
  edScrollTo(pos);
  edPaintCaret();
  sfx('click');
  feedback('info', '定位到 ' + file + ':' + line,
    '从编译器报的那一处打开。改完再点一次「运行检查」看是否干净了。');
}

/// 跑一次真实编译器检查，把结果并进「问题」栏。
async function runCheck() {
  if (!S.root) { toast('先打开一个项目', true); return; }
  const btn = $('#dockCheck');
  if (btn) { btn.disabled = true; btn.textContent = '检查中…'; }
  dockShow('problems');
  termSection('编译器检查 · ' + S.root);
  try {
    const r = await invoke('check_project', { root: S.root });
    const ck = unpack(r);
    const cr = (ck.state === 'yes' && ck.data) || {};
    const list = cr.diagnostics || [];
    DOCK.diags = list;
    if (ck.state !== 'yes') termLine(ck.reason, 'e');
    // 后端给的是 notes（复数、数组）——这里原来读的是 r.note（单数），
    // 于是"没装 cargo，装它才能查"这类**最该看见的提示**从来没显示过。
    (cr.notes || []).forEach((n) => termLine(n, 'l'));
    list.forEach((d) => termLine(
      d.level + (d.code ? ' ' + d.code : '') + ' ' + d.file + ':' + d.line + ':' + d.col
      + ' — ' + d.message, d.level === 'error' ? 'e' : ''));
    dockProblems(S.data || {});
    const errs = list.filter((d) => d.level === 'error').length;
    feedback(list.length ? (errs ? 'block' : 'warn') : 'info',
      list.length ? '检查出 ' + list.length + ' 条（' + errs + ' 个错误）' : '编译检查干净',
      list.length
        ? '在「问题」栏逐条列出，点一条直接跳到那一行。'
        : (r.note ? esc(r.note) : '编译器没话说。'));
  } catch (e) {
    feedback('block', '检查没跑起来', esc(typeof e === 'string' ? e : (e && e.message) || String(e)));
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = '运行检查'; }
  }
}

function dockProblemDetail(title) {
  const box = $('#probDetail');
  const p = DOCK.probs.find((x) => x.title === title);
  if (!box || !p) return;
  const ul = (arr) => (arr && arr.length)
    ? '<ul>' + arr.map((x) => '<li>' + esc(x) + '</li>').join('') + '</ul>' : '';
  box.innerHTML = '<h4>' + esc(p.title) + '</h4>'
    + (p.next ? '<div class="dsec"><div class="dk">处理建议</div><div>' + esc(p.next) + '</div></div>' : '')
    + ((p.evidence || []).length
      ? '<div class="dsec"><div class="dk">判定依据</div>' + ul(p.evidence) + '</div>' : '')
    + ((p.verify || []).length
      ? '<div class="dsec"><div class="dk">复核命令（可自行验证）</div>'
        + '<ul>' + p.verify.map((x) => '<li><code>' + esc(x) + '</code></li>').join('') + '</ul></div>' : '')
    + (p.boundary ? '<div class="dsec"><div class="dk">可忽略的条件</div><div>' + esc(p.boundary) + '</div></div>' : '');
}

/// 消息：系统说过的话都留在这儿（四角的小卡片只负责"浮现一下"，不再记历史）
function dockMsg(level, title, detail) {
  DOCK.msgs.unshift({
    at: new Date().toLocaleTimeString('zh-CN', { hour12: false }).slice(0, 5),
    level, title, detail: detail || '',
  });
  if (DOCK.msgs.length > 300) DOCK.msgs.length = 300;
  const box = $('#msgList');
  if (box) {
    box.insertAdjacentHTML('afterbegin', '<div class="msg ' + level + '">'
      + '<span class="t">' + DOCK.msgs[0].at + '</span>'
      + '<span class="i">' + (level === 'block' ? '✕' : level === 'warn' ? '!' : 'i') + '</span>'
      + '<span class="txt"><b>' + esc(title) + '</b>'
      + (detail ? '<div>' + detail + '</div>' : '') + '</span></div>');
    while (box.children.length > 300) box.lastElementChild.remove();
  }
  dockBadges();
}

/// 终端：跑起来的输出都往这儿流。stderr 红、系统话灰、我们自己的阶段提示暖色。
function termLine(text, kind) {
  DOCK.term.push({ text, kind: kind || '' });
  if (DOCK.term.length > 4000) DOCK.term.splice(0, 1000);
  const out = $('#termOut');
  if (!out) return;
  const d = document.createElement('div');
  if (kind) d.className = kind;
  d.textContent = text;
  out.appendChild(d);
  out.parentElement.scrollTop = out.parentElement.scrollHeight;
}

function termSection(title) {
  termLine('');
  termLine('── ' + title + ' ──', 's');
}

function termReset(title) {
  DOCK.term = [];
  const out = $('#termOut');
  if (out) out.innerHTML = '';
  termSection(title);
}

function dockClearPane() {
  if (DOCK.tab === 'terminal') { DOCK.term = []; if ($('#termOut')) $('#termOut').innerHTML = ''; return; }
  if (DOCK.tab === 'messages') { DOCK.msgs = []; if ($('#msgList')) $('#msgList').innerHTML = ''; dockBadges(); }
}

function initDock() {
  const d = $('#dock');
  if (!d) return;
  document.querySelectorAll('.dtab').forEach((b) =>
    b.addEventListener('click', () => { dockShow(b.dataset.dtab); sfx('click'); }));
  document.querySelectorAll('[data-dock]').forEach((b) =>
    b.addEventListener('click', () => dockToggle(b.dataset.dock)));
  if ($('#dockClose')) $('#dockClose').addEventListener('click', dockHide);
  if ($('#dockClear')) $('#dockClear').addEventListener('click', dockClearPane);
  if ($('#dockCheck')) $('#dockCheck').addEventListener('click', () => { sfx('click'); runCheck(); });
  const list = $('#probList');
  if (list) {
    list.addEventListener('click', (e) => {
      const row = e.target.closest('.drow');
      if (!row) return;
      list.querySelectorAll('.drow').forEach((r) => r.classList.toggle('on', r === row));
      const p = DOCK.probs.find((x) => x.title === row.dataset.prob);
      dockProblemDetail(row.dataset.prob);
      // 编译器给的条目带位置：点一下直接到那一行
      if (p && p.file) gotoDiag(p.file, p.line, p.col);
    });
  }
  const pf = $('#probFilter');
  if (pf) {
    pf.addEventListener('click', (e) => {
      const c = e.target.closest('[data-pf]');
      if (!c) return;
      DOCK.filter = c.dataset.pf;
      dockPaintProblems();
    });
  }
  // 拖上沿改高度：工程师对底部这一栏的期待
  const grip = $('#dockGrip');
  if (grip) {
    grip.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      grip.setPointerCapture(e.pointerId);
      const move = (ev) => {
        const h = Math.max(120, Math.min(window.innerHeight * 0.72, window.innerHeight - ev.clientY - 30));
        document.documentElement.style.setProperty('--dock-h', Math.round(h) + 'px');
      };
      const up = () => {
        grip.removeEventListener('pointermove', move);
        grip.removeEventListener('pointerup', up);
      };
      grip.addEventListener('pointermove', move);
      grip.addEventListener('pointerup', up);
    });
  }
  deviceKey();
}

/// 快捷键：Ctrl+J 开合底部（IDEA 是同一个键），Esc 收起。
function deviceKey() {
  document.addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.key.toLowerCase() === 'j') { e.preventDefault(); dockToggle(); }
    else if (e.key === 'Escape' && DOCK.open && document.activeElement === document.body) dockHide();
  });
}

// 打开项目之后自动做的那几件事。全部受「设置」控制，
// 每一件都在时间线上留一条回声——不写契约就启动，是最容易白忙的路径。

/// 自动流程的进度线：挂在一张固定的节点上，只换文字。
/// 为什么不重建：重建节点 = 重新挂载动画 = 一次闪。用户要的是「稳稳地动」。
function autoLine(text, kind) {
  const el = $('#autoLine');
  if (!el) return;
  if (!text) { el.hidden = true; return; }
  el.hidden = false;
  el.dataset.kind = kind || 'busy';
  let t = el.querySelector('.al-t');
  if (!t) {
    el.innerHTML = '<span class="al-bar"></span><span class="al-t"></span>';
    t = el.querySelector('.al-t');
  }
  if (t.textContent !== text) t.textContent = text;
}

async function afterAuto() {
  if (!S.root || S.autoDone === S.root) return;
  S.autoDone = S.root;
  const d = S.data || {};
  if (SET.contract && !d.contractName && !d.runCommand) {
    autoLine('正在解析项目声明，生成契约草稿…');
    try {
      await invoke('contract_draft', { root: S.root, save: true });
      feedback('warn', '已自动生成契约草稿',
        '写进 ' + esc(S.root) + '\\env.contract.json。它只是草稿：命令不对就在「运行」页改，或者关掉「自动生成契约」再自己填。');
      await autoRecord('起草了契约', '工具按项目文件推了一份 env.contract.json。\n\n'
        + '这不是最终答案：命令是「从哪个文件看出来的」推的，跑之前先扫一眼，不对就改。');
      S.data = await invoke('inspect', { root: S.root });
      renderHome();
    } catch (e) {
      feedback('block', '契约写入失败，将影响启动',
        '原因：' + esc(typeof e === 'string' ? e : (e && e.message) || String(e))
        + '<br>项目本身没事，只是「运行」还不知道该敲哪条命令。');
    }
  }
  if (SET.verify && S.data && S.data.hasTest) {
    autoLine('正在按契约执行测试…');
    feedback('info', '正在按契约执行测试', '用于确认当前项目状态，非必需步骤。');
    try {
      const r = await invoke('run_task', { root: S.root, task: 'test' });
      const code = r && (r.exit_code != null ? r.exit_code : r.exitCode);
      autoLine(code === 0 ? '自动测试通过 · 退出码 0' : '自动测试未通过 · 退出码 ' + code, code === 0 ? 'ok' : 'bad');
      setTimeout(() => { if ($('#autoLine') && $('#autoLine').dataset.kind !== 'busy') autoLine(null); }, 8000);
      feedback(code === 0 ? 'info' : 'block',
        '自动测试' + (code === 0 ? '通过' : '未通过'),
        '退出码 ' + code + (r && r.crux ? ' · 症结：' + esc(r.crux) : '')
        + '<br>详细输出在「运行」页的记录里。');
      await autoRecord('自动跑了一次测试：' + (code === 0 ? '通过' : '没通过'),
        '退出码 ' + code + (r && r.crux ? '，症结：' + r.crux : '')
        + '\n\n这条是自动记的：以后回头看，能知道这个项目在什么时候是好的。');
    } catch (e) {
      autoLine('自动测试未能启动', 'bad');
      feedback('warn', '自动测试未能启动', esc(typeof e === 'string' ? e : (e && e.message) || String(e)));
    }
  }
  if (SET.trial) {
    const oneShot = !!(S.data && S.data.runCommand);
    if (!oneShot) {
      feedback('info', '未执行自动试运行', '契约中未定义启动命令，无法推断运行方式。请在「运行」页手动执行一次。');
    } else if (!isTrusted(S.root)) {
      // 自动执行一个项目的命令 = 把控制权交给它的契约文件。陌生项目（从压缩包解开、
      // 别人发来的）不该在你不注意的时候跑起来。所以：**手动在「运行」页跑过一次**，
      // 才算你点过头；在那之前自动流程只做只读的事。
      autoLine('这个项目还没被信任，自动试运行已跳过', 'warn');
      feedback('warn', '跳过自动试运行',
        '自动执行一个项目的命令，等于把控制权交给它的契约文件。第一次要你自己点一次。'
        + '<br>将执行：<code>' + esc(S.data.runCommand || '') + '</code>'
        + '<br>去「运行」页手动跑一次，以后这个项目就会自动试运行。');
      setTimeout(() => {
        const el = $('#autoLine');
        if (el && el.dataset.kind !== 'busy') autoLine(null);
      }, 9000);
    } else {
      autoLine('正在执行试运行…');
      try {
        await invoke('launch_project', { root: S.root, extra: [] });
        autoLine('试运行已启动，输出记录于「运行」页', 'ok');
        feedback('info', '试运行结束', '执行结果记录在「运行」页。');
      } catch (e) {
        autoLine('试运行失败', 'bad');
        feedback('block', '试运行失败，当前项目无法运行',
          esc(typeof e === 'string' ? e : (e && e.message) || String(e))
          + '<br>请在「概览」页查看诊断项，已按排查顺序排列。');
      }
      setTimeout(() => {
        const el = $('#autoLine');
        if (el && el.dataset.kind !== 'busy') autoLine(null);
      }, 9000);
    }
  }
  // 自动流程走完了：还挂着「进行中」就收掉（正常路径在每一步里已经收过了）
  setTimeout(() => {
    const el = $('#autoLine');
    if (el && !el.hidden && el.dataset.kind === 'busy') autoLine(null);
  }, 1500);
}

// ── 概览 ───────────────────────────────
function renderHome() {
  const d = S.data;
  const { error: err, warn } = d.counts;
  const card = $('#statusCard');
  if (err || warn) {
    card.className = 'status warn';
    card.innerHTML = '<div class="face">!</div><div>'
      + '<h2>有 ' + (err + warn) + ' 件事值得看一眼</h2>'
      + '<p>' + (err ? err + ' 件会直接挡住运行，' : '')
      + (warn ? warn + ' 件现在能跑、但迟早会咬你一口。' : '')
      + '下面每一条都写了「下一步」，照着做就行。</p></div>';
  } else {
    card.className = 'status ok';
    card.innerHTML = '<div class="face">✓</div><div><h2>环境看着没问题</h2>'
      + '<p>解释器、依赖、契约都对得上。去「运行」那一页跑它吧。</p></div>';
  }
  $('#facts').innerHTML = d.summary.map((f) =>
    '<div class="fact"><div class="k">' + esc(f.k) + '</div><div class="v">' + esc(f.v) + '</div></div>'
  ).join('');

  const box = $('#findings');
  // 带 group 的是项目检查的条目，归下面那张项目检查卡；这里只留排查链上的结论。
  const list = (d.findings || [])
    .filter((f) => (f.severity === 'error' || f.severity === 'warn') && !f.group);
  if (!list.length) {
    box.innerHTML = '<div class="finding info"><div class="ftitle">没有诊断</div>'
      + '<div class="boundary">项目检查没发现问题。要是你总觉得哪里不对，去「运行」那页跑一次，'
      + '它挂掉的时候会把症结直接说出来。</div></div>';
    renderAudit();
    return;
  }
  // 按排查链的层分组。这不是排版偏好：顺序本身就是方法——
  // 「实际用的是哪个」没确认之前，先去修「缺什么」是白费力气。
  let html = '';
  let lastLayer = '';
  list.forEach((f) => {
    const layer = f.layer || '其他';
    if (layer !== lastLayer) {
      lastLayer = layer;
      html += '<div class="layer"><b>' + esc(layer) + '</b>'
        + (f.layer_note ? '<span>' + esc(f.layer_note) + '</span>' : '') + '</div>';
    }
    const ev = (f.evidence || []).map((x) => '<li>' + esc(x) + '</li>').join('');
    const vf = (f.verify || []).length
      ? '<div class="boundary">自己验证：</div><ul>'
        + f.verify.map((x) => '<li><code>' + esc(x) + '</code></li>').join('') + '</ul>'
      : '';
    html += '<div class="finding ' + f.severity + '" data-id="' + esc(f.id) + '">'
      + '<div class="ftitle"><span class="chip ' + f.severity + '">'
      + (f.severity === 'error' ? '要修' : '注意') + '</span><span>' + esc(f.title) + '</span></div>'
      + (f.next ? '<div class="next">下一步 · ' + esc(f.next) + '</div>' : '')
      + (f.boundary ? '<div class="boundary">什么时候不用管：' + esc(f.boundary) + '</div>' : '')
      + '<details><summary>看证据和自查命令</summary><ul>' + ev + '</ul>' + vf + '</details></div>';
  });
  box.innerHTML = html;
  renderAudit();
  renderCardNotes(d);
  renderChanges();
}

// ── 项目卡下方那几行小字 ───────────────
// 只放「新手看一眼就知道自己在哪」的 4-6 条；更细的收在下面的「细节」里。
function renderCardNotes(d) {
  const box = $('#cardNotes');
  if (!box) return;
  const hy = (d && d.hygiene) || null;
  const todo = (d.findings || []).filter(
    (f) => (f.severity === 'error' || f.severity === 'warn') && !f.group);
  const auditInfo = hy ? (hy.items || []).filter((f) => f.group) : [];
  const auditHot = auditInfo.filter((f) => f.severity !== 'info');
  const rows = [];

  rows.push(['项目类型', (d.summary || []).find((f) => f.k === '项目类型')?.v || '认不出来']);

  const runCmd = d.runCommand;
  rows.push(['怎么跑', runCmd
    ? '契约里写着：' + runCmd
    : '还没有契约。去「运行」页跑一次，它会自动补上']);

  if (d.error || d.warn) {
    rows.push(['要处理', '<span class="bad">' + todo.length + ' 件</span>'
      + (todo[0] ? '：' + esc(todo[0].title) : '')]);
  } else {
    rows.push(['要处理', '<span class="ok">没有挡路的问题</span>']);
  }

  if (hy) {
    rows.push(['项目卫生', (auditHot.length
      ? '<span class="bad">' + auditHot.length + ' 条值得处理</span>，'
      : '<span class="ok">没有要处理的</span>，')
      + '另有 ' + (auditInfo.length - auditHot.length) + ' 条日常提示']);
    const arts = hy.artifacts || [];
    const bytes = arts.reduce((a, x) => a + (x.size_mb || 0), 0) * 1048576;
    rows.push(['能清出', bytes > 0 ? human(bytes) + '（可重新生成的目录，去「清理」页）'
      : (arts.length
          ? arts.length + ' 个可重新生成的目录（项目检查里点「深度扫描」看体积）'
          : '暂时没有值得清的产物')]);
  }

  const ch = S.changes;
  rows.push(['最近变化', ch && !ch.first_run
    ? esc(ch.headline)
    : (ch ? '开始记账了：以后改了什么都会说一声' : '还没有记录')]);

  // 每行小字都配一条悬浮解释：鼠标停一下才知道这行到底在说什么
  const TIPS_ROW = {
    '项目类型': '从项目文件识别出来的类型，比如 Rust 看到 Cargo.toml。',
    '怎么跑': '契约里的启动命令。没有契约时，先在「概览 → 项目详情」里生成。',
    '要处理': '挡住运行的问题。每条都带证据和自查命令。',
    '项目卫生': '和语言无关的日常维护：忽略规则、锁文件、密钥、体积。',
    '能清出': '可重新生成的目录合计（删了下次构建会再长出来）。',
    '最近变化': '从上次看过算起，这个项目被改了哪些地方。',
  };
  box.innerHTML = rows.slice(0, 6).map(([k, v]) =>
    '<div class="cn" data-tip="' + esc(TIPS_ROW[k] || '') + '"><b>' + esc(k) + '</b><span>' + v + '</span></div>').join('');
}

// ── 变更卡：你（或 AI）刚才做了什么 ──────
function renderChanges() {
  const box = $('#changeCard');
  if (!box) return;
  const c = S.changes;
  if (!c || c.first_run || c.lines.length === 0) {
    box.innerHTML = '';
    return;
  }
  const when = String(c.at || '').slice(0, 16).replace('T', ' ');
  box.innerHTML = '<div class="chg"><h2>刚才这几分钟，这个项目变了<span class="when">'
    + esc(when) + '</span></h2>'
    + '<div style="color:#6B6258;font-size:13.5px">' + esc(c.headline) + '</div>'
    + '<ul>' + c.lines.map((l) => {
      const cls = l.startsWith('新多出来的') ? ' class="risk"'
        : l.startsWith('已经解决的') ? ' class="fixed"' : '';
      return '<li' + cls + '>' + esc(l) + '</li>';
    }).join('') + '</ul></div>';
}

async function loadChanges() {
  if (!S.root) return;
  try {
    S.changes = await invoke('changes_take', { root: S.root });
    renderChanges();
    renderCardNotes(S.data);
  } catch (e) { /* 后台动作，失败不打扰人 */ }
}

// ── 项目检查 ───────────────────────────
// 打开项目时就已经有一份快扫结果（跟着 facts 一起回来的），所以这里是零成本渲染。
// 只有要算体积、找大文件时才去叫 deep——那一次真的要读一遍 node_modules。
const AUDIT_GROUPS = ['忽略与提交', '依赖清单', '工程习惯', '目录与体积'];
const SEV_WORD = { error: '要处理', warn: '注意', info: '提示' };

function auditScan() {
  return S.audit || (S.data && S.data.hygiene) || null;
}

function auditItemHtml(f) {
  const sev = f.severity || 'info';
  const ev = (f.evidence || []).map((x) => '<li>' + esc(x) + '</li>').join('');
  const vf = (f.verify || []).map((x) => copyBlock(x)).join('');
  // 能代做的，就给一个按钮。工具改的东西只有「往里加行」这一种，
  // 所以按钮文案写清楚会加什么，别让人闭着眼点。
  const fx = (S.fixes || []).find((x) => x.id === f.id);
  const fixBtn = fx
    ? '<div class="afix"><button class="btn sm" data-fix="' + esc(f.id) + '">应用</button>'
      + '<span>' + esc(fx.target) + ' 里会加：' + esc(fx.lines.join('  ')) + '</span></div>'
    : '';
  return '<details class="ai ' + sev + '"><summary>'
    + '<span class="chip ' + sev + '">' + SEV_WORD[sev] + '</span><span>' + esc(f.title) + '</span>'
    + '<span class="aid">' + esc(f.id) + '</span></summary>'
    + '<div class="aibody">'
    + (ev ? '<div class="lbl">看到的事实</div><ul>' + ev + '</ul>' : '')
    + (vf ? '<div class="lbl">自己验证</div>' + vf : '')
    + (fx ? '<div class="lbl">工具可以替你做的</div>' + fixBtn : '')
    + (f.boundary ? '<div class="lbl">什么时候不用管</div><div>' + esc(f.boundary) + '</div>' : '')
    + (f.next ? '<div class="lbl">下一步</div><div class="nx">' + esc(f.next) + '</div>' : '')
    + '</div></details>';
}

function renderAudit() {
  const box = $('#audit');
  if (!box) return;
  const when = $('#auditWhen');
  const s = auditScan();
  if (!s) { box.innerHTML = ''; if (when) when.textContent = ''; return; }
  const items = s.items || [];
  const mans = s.manifests || [];
  S.fixes = s.fixes || [];
  const n = (sev) => items.filter((f) => f.severity === sev).length;
  const todo = n('error') + n('warn');

  if (when) {
    // 标明这次是快扫还是深度扫描：数字来源不同，看到的人有权知道自己拿到的
    // 是「只看了目录名」还是「连体积都算了」。
    const parts = ['看了 <b>' + (s.files_scanned || 0) + '</b> 个文件'
      + (s.deep ? '（含体积）' : '（快扫）')];
    if (mans.length) parts.push('依赖清单 ' + mans.length + ' 份');
    if (s.deep && (s.artifacts || []).length) {
      const bytes = s.artifacts.reduce((a, x) => a + (x.size_mb || 0), 0) * 1048576;
      if (bytes > 0) parts.push('其中可重新生成的目录 <b>' + human(bytes) + '</b>');
    }
    parts.push(todo
      ? '<b style="color:var(--warn)">' + todo + ' 条值得处理</b>'
      : (items.length ? '没有要处理的' : '没什么可说的'));
    when.innerHTML = parts.join(' · ');
  }

  let html = '';
  if (mans.length) {
    html += '<div class="amans">' + mans.map((m) =>
      '<span class="aman"><b>' + esc(m.path) + '</b> '
      + esc(m.note || '（没读出内容）') + '</span>').join('') + '</div>';
  }
  if (!items.length) {
    html += '<div class="panel" style="margin:0"><div class="hint">'
      + '看过的：忽略规则、依赖清单和锁文件、IDE 配置、路径长度、垃圾文件。'
      + '没报出来不等于没问题——真觉得哪里不对，去「运行」那页跑一次，它挂掉时会把症结说出来。</div></div>';
  }
  AUDIT_GROUPS.forEach((g) => {
    const gs = items.filter((f) => f.group === g);
    if (!gs.length) return;
    const hot = gs.filter((f) => f.severity !== 'info').length;
    html += '<details class="agroup"' + (hot ? ' open' : '') + '><summary>' + esc(g)
      + '<span class="n">' + gs.length + ' 条'
      + (hot ? '，其中 ' + hot + ' 条值得处理' : '') + '</span></summary>'
      + '<div class="aitems">' + gs.map(auditItemHtml).join('') + '</div></details>';
  });
  box.innerHTML = html;
}

async function runAudit(deep) {
  if (!S.root || S.auditBusy) return;
  S.auditBusy = true;
  const btns = [$('#btnAuditScan'), $('#btnAuditDeep')].filter(Boolean);
  btns.forEach((b) => { b.disabled = true; });
  const when = $('#auditWhen');
  if (when) {
    when.textContent = deep
      ? '正在算体积：要真的读一遍 node_modules / target 这类目录，慢一点…'
      : '正在重新扫描…';
  }
  try {
    S.audit = await invoke('audit', { root: S.root, deep });
    renderAudit();
  } catch (e) {
    toast(typeof e === 'string' ? e : (e && e.message) || String(e), true);
    renderAudit();
  } finally {
    S.auditBusy = false;
    btns.forEach((b) => { b.disabled = false; });
  }
}

/// 让工具代做一条修改，然后把新结果接回来（不用再手点一次扫描）。
async function applyFix(id, btn) {
  if (!S.root) return;
  if (btn) { btn.disabled = true; btn.textContent = '正在改…'; }
  try {
    const r = await invoke('apply_fix', { root: S.root, ids: [id] });
    const rep = r.report || {};
    if (rep.nothing_to_do) toast('文件里已经有这几行了，没有改动');
    else toast('改好了：' + ((rep.changed_files || []).join('、') || '无'));
    S.audit = r.scan || null;
    renderAudit();
    // .gitignore 变了，概览上的结论也该跟着变
    await refreshQuiet(true);
  } catch (e) {
    toast(typeof e === 'string' ? e : (e && e.message) || String(e), true);
    if (btn) { btn.disabled = false; btn.textContent = '应用'; }
  }
}

/// 静默重扫：切回窗口时、或者每隔一阵子跑一次。
/// 结论没变就一个像素都不动——重渲染会把人正在展开的卡片关闭来。
function auditSig(d) {
  const f = (d && d.findings) || [];
  return f.map((x) => x.id).sort().join(',') + '|' + f.length;
}
async function refreshQuiet(force) {
  if (!S.root) return;
  try {
    const d = await invoke('inspect', { root: S.root });
    const before = auditSig(S.data);
    const after = auditSig(d);
    S.data = d;
    S.audit = null;
    if (force || before !== after) renderHome();
  } catch (e) {
    // 静默刷新失败不打扰人：本来就是后台动作
  }
}

// ── 启动：卡片会留下来 ─────────────────
function stageRows(plan, stages) {
  return plan.map((name, i) => {
    const st = stages.find((s) => s.index === i + 1);
    const mark = !st ? '·' : ({ ok: '✓', note: '!', fail: '✗' }[st.status] || '·');
    const tip = st && st.detail ? ' data-tip="' + esc(st.detail) + '"' : '';
    return '<li' + tip + '><span class="st ' + (st ? st.status : 'idle') + '">' + mark + '</span>'
      + '<span class="t">' + esc(st ? st.title : name) + '</span>'
      + '<span class="d">' + esc(st ? st.detail : '') + '</span>'
      + '<span class="ms">' + (st && st.ms != null ? st.ms + 'ms' : '') + '</span></li>';
  }).join('');
}

function runCardHtml(rec, live) {
  const plan = rec.kind === 'run' ? PLAN_RUN : PLAN_TASK;
  let pill, verdict = '';
  if (live) {
    pill = '<span class="pill run">进行中</span>';
  } else if (rec.ready || rec.exit_code === 0) {
    pill = '<span class="pill ok">成功</span>';
    verdict = '<div class="verdict good"><b>✓ 跑起来了</b>'
      + (rec.ready ? '<div class="boundary">' + esc(rec.ready) + '</div>' : '') + '</div>';
  } else {
    pill = '<span class="pill bad">失败</span>';
    verdict = '<div class="verdict bad"><b>✗ 没跑起来（退出码 ' + rec.exit_code + '）</b>'
      + (rec.crux ? '<div class="crux">症结 · ' + esc(rec.crux) + '</div>' : '')
      + '</div>';
  }
  const when = String(rec.at || '').slice(0, 16).replace('T', ' ');
  // 内存这一段只在真采到时才出现——没跑过东西的卡片上不该有一条空行
  let mem = '';
  if (rec.memory) {
    const mm = rec.memory;
    const tight = mm.phys_total_mb && mm.phys_avail_at_peak_mb < mm.phys_total_mb / 8;
    mem = '<div class="hint" style="margin-top:8px">内存 · 进程峰值 ' + mm.peak_working_mb
      + ' MB · 当时系统 ' + (mm.sys_commit_used_mb / 1024).toFixed(1) + '/'
      + (mm.phys_total_mb / 1024).toFixed(1) + ' GB'
      + (mm.paging ? ' <b style="color:var(--warn)">· 走到换页了，它慢就慢在这儿</b>'
          : (tight ? ' <b style="color:var(--warn)">· 物理内存已经贴边</b>' : ''))
      + ((mm.others || []).length
          ? '<br>当时还占着：' + mm.others.slice(0, 5)
              .map((p) => esc(p.name) + ' ' + p.working_mb + ' MB').join(' · ')
          : '')
      + '</div>';
  }
  return '<div class="runcard' + (live ? ' live' : '') + '" data-at="' + esc(rec.at) + '">'
    + '<div class="rc-head"><span class="rc-kind">'
    + (rec.kind === 'run' ? '启动' : rec.kind === 'test' ? '测试' : '装依赖') + '</span>'
    + '<span class="rc-when">' + esc(when) + '</span>'
    + '<span class="rc-cmd">' + esc(rec.command || '') + '</span>' + pill + '</div>'
    + '<div class="rc-body' + (live ? '' : ' hide') + '">'
    + '<ol class="stages">' + stageRows(plan, rec.stages || []) + '</ol>'
    + verdict
    + mem
    + '<div class="console">' + (rec.tail || []).map((l) => '<div>' + esc(l) + '</div>').join('') + '</div>'
    + '</div></div>';
}

function renderRuns(list) {
  $('#runs').innerHTML = list.length
    ? list.map((r) => runCardHtml(r, false)).join('')
    : '<div class="panel"><div class="hint">还没有跑过。点上面的「启动项目」，'
      + '每一步和输出都会记成一张卡片留在这里——不用怕刷掉。</div></div>';
}

// ── 一条龙流水线：装依赖 → 构建 → 测试 → 运行 ──────────────────
// 每一步都显示契约里那条命令；没有就淡下去（点它会给理由，不是哑按钮）。
function renderPipeline(d) {
  const map = {
    install: d && d.installCommand,
    build: d && d.buildCommand,
    test: d && d.testCommand,
    run: d && d.runCommand,
  };
  const label = { install: '#cmdInstall', build: '#cmdBuild', test: '#cmdTest', run: '#cmdRun' };
  Object.keys(map).forEach((k) => {
    const el = document.querySelector(label[k]);
    const step = document.querySelector('.pstep[data-task="' + k + '"]');
    if (!step) return;
    const cmd = map[k];
    step.classList.toggle('none', !cmd);
    step.dataset.cmd = cmd || '';
    if (el) el.textContent = cmd || '契约里没写';
    step.dataset.tip = cmd ? ('命令：' + cmd) : '契约里没有这一步；打开项目详情可以自己填。';
  });
}
function paintPipeline(kind, state) {
  const step = document.querySelector('.pstep[data-task="' + kind + '"]');
  if (!step) return;
  step.classList.remove('busy', 'done', 'fail');
  if (state === 'busy') { step.classList.add('busy'); sfx('click'); }
  else if (state === 'done') { step.classList.add('done'); }
  else if (state === 'fail') { step.classList.add('fail'); }
}

async function doRun(kind) {
  if (!S.root) { toast('先打开一个项目', true); return; }
  const btns = ['#btnRun', '#btnTest', '#btnInstall', ...['install', 'build', 'test', 'run'].map((t) => '.pstep[data-task="' + t + '"]')]
    .map((s) => (typeof s === 'string' && s.startsWith('.')) ? document.querySelector(s) : $(s))
    .filter(Boolean);
  btns.forEach((b) => { b.disabled = true; });
  paintPipeline(kind, 'busy');
  const plan = kind === 'run' ? PLAN_RUN : PLAN_TASK;
  const rec = { at: new Date().toISOString(), kind, command: '准备中…', exit_code: 0, stages: [], tail: [] };
  S.live = rec;
  bandShow(kind);
  // 输出同时往底部「终端」流。跑起来就把这一栏顶出来——
  // 新人第一次跑项目，最需要知道的正是「它到底在说什么」。
  termReset((kind === 'run' ? '运行' : kind === 'test' ? '测试' : kind === 'build' ? '构建' : '装依赖')
    + ' · ' + S.root);
  // 但不自动弹出来：底部那一条是你的，不该被抢。状态栏上「终端」会亮，
  // 想看就点它，或者 Ctrl+J——输出一条都不会丢。
  dockBadges();   // 亮状态栏那个「终端」（原来这里调了一个不存在的函数，一跑就抛）
  PANEL.running = true;
  panelPolicy();          // 跑起来时诊断栏让位给"当前在干什么"
  bandAutoHide(0);
  const ph = $('#runs').querySelector('.panel');
  if (ph) ph.remove();
  $('#runs').insertAdjacentHTML('afterbegin', runCardHtml(rec, true));
  const card = $('#runs').firstElementChild;
  card.querySelector('.rc-body').classList.remove('hide');
  const consoleEl = card.querySelector('.console');

  try {
    const promise = kind === 'run'
      ? invoke('launch_project', { root: S.root, extra: [] })
      : invoke('run_task', { root: S.root, task: kind });
    const out = await promise;
    S.live = null;
    // 你手动点过「运行」= 你为这个项目点过头。之后它才允许自动试运行。
    if (kind === 'run') trust(S.root);
    paintPipeline(kind, out.exit_code === 0 ? 'done' : 'fail');
    bandFinish(out, null);
    if (kind !== 'run') { await load(S.root); return; }
    const final = {
      at: rec.at, kind, command: (S.data && S.data.runCommand) || rec.command,
      exit_code: out.exit_code, ready: out.ready,
      crux: out.diagnosis ? out.diagnosis.crux : null,
      stages: rec.stages, tail: rec.tail,
    };
    final.hint = out.diagnosis ? out.diagnosis.hint : null;
    final.memory = out.memory || null;
    card.outerHTML = runCardHtml(final, false);
    $('#runs').firstElementChild.querySelector('.rc-body').classList.remove('hide');
  } catch (e) {
    S.live = null;
    paintPipeline(kind, 'fail');
    bandFinish(null, typeof e === 'string' ? e : (e && e.message) || String(e));
    toast(typeof e === 'string' ? e : e.message, true);
    card.remove();
  } finally {
    paintPipeline(kind, 'idle');
    PANEL.running = false;
    dockBadges();          // 跑完把状态栏那个脉冲收掉
    panelPolicy();
    bandAutoHide(9000);   // 结束后停 9 秒自己收掉，不长期占屏幕
    btns.forEach((b) => { b.disabled = false; });
  }
}

/// 事件流：边跑边把卡片填起来
function applyEvent(ev) {
  const rec = S.live;
  if (!rec) return;
  const card = $('#runs').firstElementChild;
  if (!card) return;
  if (ev.type === 'stage') {
    rec.stages = rec.stages.filter((s) => s.index !== ev.index);
    rec.stages.push({ index: ev.index, title: ev.title, status: ev.status, detail: ev.detail, ms: ev.ms });
    rec.stages.sort((a, b) => a.index - b.index);
    card.querySelector('.stages').innerHTML =
      stageRows(rec.kind === 'run' ? PLAN_RUN : PLAN_TASK, rec.stages);
    if (ev.title) termLine((ev.status === 'ok' ? '✓ ' : ev.status === 'fail' ? '✕ ' : '▶ ') + ev.title, 's');
    streamStage(ev);
    bandStage(ev);
  } else if (ev.type === 'line') {
    if (rec.tail.length < 400) rec.tail.push(ev.text);
    termLine(ev.text, ev.source === 'stderr' ? 'e' : (ev.source === 'log' ? 'l' : ''));
    streamLine(ev.text, ev.source);
    const c = card.querySelector('.console');
    const d = document.createElement('div');
    d.className = ev.source === 'stderr' ? 'err' : (ev.source === 'log' ? 'log' : '');
    d.textContent = ev.text;
    c.appendChild(d);
    c.scrollTop = c.scrollHeight;
  }
}

// ── 清理 ───────────────────────────────
async function scanClean() {
  $('#cleanSummary').textContent = '正在清点…';
  await guard(async () => {
    // 不打开项目也要能清：全局缓存跟项目无关
    const d = await invoke('scan_clean', { root: S.root || null });
    S.clean = d;
    // 勾选范围跟着「清理力度」走：保守=只勾够大的，标准=勾推荐的，激进=A 区全勾
    const del = d.items.filter((i) => i.action === 'delete');
    const pick = SET.cleanLevel === 'aggressive' ? del
      : SET.cleanLevel === 'conservative' ? del.filter((i) => i.recommended && i.size_bytes >= 10 * 1024 * 1024)
      : del.filter((i) => i.recommended);
    S.picked = new Set(pick.map((i) => i.path));
    $('#cleanScope').textContent = d.hasProject
      ? (SET.delMode === 'permanent'
          ? '当前设置为永久删除：删掉就找不回来了，看清楚再动手。'
          : '默认走回收站，删掉的东西还能找回来。')
      : '现在没有打开项目，所以只清点全局缓存（项目里的产物要打开项目才看得到）。';
    renderClean();
  });
}
function itemCard(it, checkable) {
  const tag = it.action === 'command'
    ? '<span class="tag cmd">要自己跑命令</span>'
    : (it.recommended ? '<span class="tag rec">推荐清</span>' : '<span class="tag opt">自己斟酌</span>');
  return '<div class="item" data-path="' + esc(it.path) + '">'
    + (checkable ? '<input type="checkbox" ' + (S.picked.has(it.path) ? 'checked' : '') + '>' : '')
    + '<div class="body"><div class="l1"><span class="label">' + esc(it.label) + '</span>' + tag
    + '<span class="size">' + (it.size_bytes ? human(it.size_bytes) : '') + '</span></div>'
    + '<div class="path">' + esc(it.path) + '</div>'
    + '<div class="why">可删 · ' + esc(it.reason) + '</div>'
    + '<div class="cost">代价 · ' + esc(it.consequence) + '</div>'
    + copyBlock(it.command, it.command_note) + '</div></div>';
}
function renderClean() {
  const d = S.clean;
  const A = d.items.filter((i) => i.zone === 'a' && i.action === 'delete');
  const Acmd = d.items.filter((i) => i.zone === 'a' && i.action === 'command');
  const B = d.items.filter((i) => i.zone === 'b');
  const C = d.items.filter((i) => i.zone === 'c');
  const pickedBytes = A.filter((i) => S.picked.has(i.path)).reduce((a, i) => a + i.size_bytes, 0);
  $('#cleanSummary').innerHTML = '可以清理 <b>' + d.human + '</b> / ' + d.zoneA.count + ' 项'
    + '，其中标「推荐清」的 ' + d.humanRecommended
    + '；<span style="color:var(--primary)">已勾选 ' + human(pickedBytes) + '</span>';

  let html = '<div class="zone"><h3>可以清（工具代删，走回收站）</h3>'
    + '<div class="zhint">都是缓存和产物，删掉会重新生成。标「自己斟酌」的先看一眼再勾。</div>'
    + (A.map((i) => itemCard(i, true)).join('') || '<div class="hint">没有可清理的项目产物。</div>')
    + Acmd.map((i) => itemCard(i, false)).join('') + '</div>';
  if (B.length) {
    html += '<div class="zone"><h3>要管理员权限（工具不代跑，给你命令）</h3>'
      + '<div class="zhint">系统级的东西，代跑风险不可控。点命令块复制，粘到管理员 PowerShell 里跑。</div>'
      + B.map((i) => itemCard(i, false)).join('') + '</div>';
  }
  if (C.length) {
    html += '<div class="zone noTouch"><h3>不要碰</h3>'
      + '<div class="zhint">这些经常被网上的「清理教程」推荐删除——删了会出事。这里是为什么。</div>'
      + C.map((i) => '<div class="item"><div class="body">'
        + '<div class="l1"><span class="label">✗ ' + esc(i.label) + '</span></div>'
        + '<div class="path">' + esc(i.path) + '</div>'
        + '<div class="why">' + esc(i.consequence) + '</div></div></div>').join('') + '</div>';
  }
  $('#cleanList').innerHTML = html;
}

// ── 笔记 ───────────────────────────────
// 「笔记」页的第一层是项目：左边摆的是同一套项目卡片，但这里不等项目检查、
// 不跑环境——只看它留下了什么（记录、笔记、运行日志）。
// 选中方式可以在设置里切成「停留即选中」，鼠标扫过去就换，不用点。
const NL = { sel: null, log: null, hoverTimer: null };

function renderNoteProjects() {
  const box = $('#nbProjects');
  if (!box) return;
  if (!PS.all.length) {
    box.innerHTML = '<div class="hint" style="padding:8px">还没有项目列表。点右边「扫描项目目录」，'
      + '或者去「概览」页扫一遍。</div>';
    return;
  }
  box.innerHTML = PS.all.slice(0, 24).map((p) => {
    const on = (NL.sel && NL.sel.path === p.path) || isCurrent(p.path);
    const when = p.age_days === 0 ? '今天' : (p.age_days === 1 ? '昨天' : p.age_days + ' 天前');
    return '<div class="pcard' + (on ? ' on' : '') + '" data-npath="' + esc(p.path) + '">'
      + '<div class="pc-name">' + esc(p.name) + '</div>'
      + '<div class="pc-note">' + esc((p.kinds || []).join(' + ') || '认不出类型') + ' · ' + when + '</div>'
      + '<div class="pc-acts">'
      + '<button class="btn ghost sm" data-nact="addnote">＋笔记</button>'
      + '<button class="btn ghost sm" data-nact="addrecord">＋记录</button>'
      + '</div></div>';
  }).join('') + (PS.all.length > 24
    ? '<div class="hint" style="padding:8px">还有 ' + (PS.all.length - 24) + ' 个项目没摆出来，先在「概览」页翻页看看</div>'
    : '');
}

/// 开一篇新的（笔记或记录）。记录会顺手把时间填进标题——
/// 记录的价值一半在「什么时候」。
function newNote(kind) {
  if (!NL.sel && !S.root) { toast('先在上面选一个项目'); return; }
  openEditor(null);
  const k = $('#neKind');
  if (k) k.value = kind;
  const t = $('#neTitle');
  if (t) {
    t.value = kind === 'record'
      ? '记录 · ' + new Date().toLocaleString('zh-CN').slice(0, 16)
      : '';
    t.focus();
  }
}

/// 把一条运行日志引到当前笔记里。引用是把现场固定下来：
/// 以后改了命令，还能看到当时到底跑的是什么、结果如何。
function appendRef(at) {
  const r = ((NL.log && NL.log.runs) || []).find((x) => x.at === at);
  const body = $('#neBody');
  if (!r) return;
  if (!body) { toast('先打开一篇笔记，再引用'); return; }
  const line = '> 来自运行日志 ' + String(r.at).slice(0, 16).replace('T', ' ')
    + '\n> 命令：' + (r.command || '')
    + '\n> 结果：' + (r.exit_code === 0 ? '成功' : '退出码 ' + r.exit_code)
    + (r.crux ? '\n> 症结：' + r.crux : '');
  body.value += (body.value.trim() ? '\n\n' : '') + line + '\n';
  feedback('info', '把那条日志引到了笔记里', '引用写成了普通的 Markdown 引用块，不依赖这个工具也能看懂。');
}

async function selectNoteProject(p) {
  if (NL.sel && NL.sel.path === p.path) return;
  NL.sel = p;
  renderNoteProjects();
  const scope = $('#nbScope');
  if (scope) scope.innerHTML = '<span class="spin"></span> 正在读 ' + esc(p.name) + ' 的记录…';
  try {
    NL.log = await invoke('project_log', { root: p.path });
  } catch (e) {
    NL.log = { notes: [], runs: [] };
    feedback('warn', '这个项目的记录读不出来', esc(String(e)));
  }
  if (scope) {
    const n = (NL.log.notes || []).length;
    const r = (NL.log.runs || []).length;
    scope.textContent = p.name + ' · ' + n + ' 条记录/笔记 · ' + r + ' 次运行';
  }
  renderNotes($('#nbSearch') ? $('#nbSearch').value : '');
}

function linkify(text) {
  return esc(text).replace(/\[\[([^\]]+)\]\]/g, '<span class="wikilink" data-link="$1">$1</span>');
}
function renderNotes(filter) {
  const all = NL.sel ? ((NL.log && NL.log.notes) || []) : ((S.data && S.data.notes) || []);
  const runs = NL.sel ? ((NL.log && NL.log.runs) || []) : ((S.data && S.data.runs) || []);
  const f = (filter || '').trim().toLowerCase();
  const hit = (n) => !f || (n.title + n.body + (n.tags || []).join(' ')).toLowerCase().includes(f);
  const records = all.filter((n) => n.kind === 'record' && hit(n));
  const notes = all.filter((n) => n.kind !== 'record' && hit(n));
  const box = $('#nbItems');
  const item = (n) =>
    '<div class="nb-item' + (S.current === n.slug ? ' on' : '') + '" data-slug="' + esc(n.slug) + '">'
    + '<div class="t">' + esc(n.title) + '</div>'
    + '<div class="m"><b>' + esc(kindLabel(n.kind)) + '</b>'
    + '<span>' + esc(String(n.updated || '').slice(0, 10)) + '</span>'
    + (n.links && n.links.length ? '<span>→ ' + n.links.length + ' 处链接</span>' : '')
    + '</div></div>';

  let html = '';
  if (records.length) {
    html += '<div class="nb-sec"><div class="h">记录 · 大多是它自己留的痕<span>'
      + records.length + ' 条</span></div>' + records.map(item).join('') + '</div>';
  }
  if (notes.length) {
    html += '<div class="nb-sec"><div class="h">笔记 · 你自己写的<span>'
      + notes.length + ' 篇</span></div>' + notes.map(item).join('') + '</div>';
  }
  if (runs.length) {
    html += '<div class="nb-sec"><div class="h">运行日志 · 每次跑都留着<span>'
      + runs.length + ' 次</span></div>'
      + runs.slice(0, 10).map((r) =>
        '<div class="nb-item">'
        + '<div class="t">' + (r.exit_code === 0 ? '✔' : '✕') + ' '
        + esc(r.kind || 'run') + ' · ' + esc(String(r.command || '').slice(0, 44)) + '</div>'
        + '<div class="m"><span>' + esc(String(r.at || '').slice(0, 16).replace('T', ' ')) + '</span>'
        + (r.crux ? '<span class="run-ref">症结：' + esc(String(r.crux).slice(0, 40)) + '</span>' : '')
        + '<span class="run-ref" data-ref="' + esc(r.at) + '">引用到这篇笔记</span>'
        + '</div></div>').join('') + '</div>';
  }
  box.innerHTML = html || '<div class="hint" style="padding:14px">'
    + (NL.sel
        ? '这个项目还没有留下东西。跑一次、生成契约、清理，都会自动留痕；也可以自己写。'
        : '还没有内容。上面选一个项目，或者点「＋ 新笔记」。')
    + '</div>';
  const root = NL.sel ? NL.sel.path : S.root;
  const foot = $('#nbFoot');
  if (foot) {
    foot.innerHTML = '共 ' + all.length + ' 条 · 存成 Markdown：<span class="mono">'
      + esc(root ? root + '\\.jingwei\\notes' : '（先选项目）') + '</span>';
  }
}

function openEditor(note) {
  S.current = note ? note.slug : null;
  renderNotes($('#nbSearch').value);
  const n = note || { slug: '', kind: 'note', title: '', body: '', tags: [] };
  $('#nbEditor').innerHTML =
    '<div class="row"><input id="neTitle" placeholder="标题" value="' + esc(n.title) + '">'
    + '<select id="neKind" style="flex:0 0 130px">'
    + KINDS.map((k) => '<option value="' + k[0] + '"'
        + (k[0] === n.kind ? ' selected' : '') + '>' + k[1] + '</option>').join('')
    + '</select></div>'
    + '<div class="row"><input id="neTags" placeholder="标签，逗号分开：记账, 界面" value="'
    + esc((n.tags || []).join(', ')) + '"></div>'
    + '<textarea id="neBody" placeholder="正文。可以写 Markdown，也可以写 [[另一篇笔记]] 做链接。">'
    + esc(n.body) + '</textarea>'
    + '<div class="nb-bar"><button class="btn" id="neSave">保存</button>'
    + (note ? '<button class="btn ghost" id="neDel" data-armed="0">删除</button>' : '')
    + '<span class="grow"></span><span class="hint">Ctrl+S 保存</span></div>'
    + '<div class="nb-links" id="neLinks"></div>';

  const paint = () => {
    const links = (($('#neBody').value.match(/\[\[([^\]]+)\]\]/g) || [])
      .map((x) => x.slice(2, -2)));
    $('#neLinks').innerHTML = links.length
      ? '<span class="hint">链接：</span>'
        + links.map((l) => '<span class="wikilink" data-link="' + esc(l) + '">' + esc(l) + '</span>').join('')
      : '';
  };
  paint();
  $('#neBody').addEventListener('input', paint);
  $('#neSave').addEventListener('click', saveNote);
  // 删除要按两下。自己写的字不该被一次误点带走——
  // 这个坑真的踩了：调试时的随手一点，把一条笔记送进了 .trash。
  if (note) $('#neDel').addEventListener('click', (e) => {
    const b = e.currentTarget;
    if (b.dataset.armed === '1') { deleteNote(note.slug); return; }
    b.dataset.armed = '1';
    b.textContent = '再点一下真删';
    b.style.color = 'var(--danger)';
    setTimeout(() => {
      if (!b.isConnected) return;
      b.dataset.armed = '0';
      b.textContent = '删除';
      b.style.color = '';
    }, 4000);
  });
}

async function saveNote() {
  const body = $('#neBody').value;
  if (!body.trim() && !$('#neTitle').value.trim()) { toast('空笔记不用存'); return; }
  await guard(async () => {
    const r = await invoke('save_note', {
      root: (NL.sel && NL.sel.path) || S.root,
      note: {
        slug: S.current,
        kind: $('#neKind').value,
        title: $('#neTitle').value.trim(),
        body,
        tags: $('#neTags').value.split(',').map((s) => s.trim()).filter(Boolean),
      },
    });
    S.data.notes = r.all;
    S.current = r.saved.slug;
    renderNotes($('#nbSearch').value);
    toast('已保存');
  });
}
async function deleteNote(slug) {
  await guard(async () => {
    const r = await invoke('delete_note', { root: S.root, slug });
    S.data.notes = r.all;
    S.current = null;
    openEditor(null);
    renderNotes($('#nbSearch').value);
    toast('已移进笔记库的 .trash/，可以找回');
  });
}

// ── 打开方式 ───────────────────────────
function paintOpenButton() {
  const def = S.openers.find((o) => o.id === S.defaultOpener) || S.openers[0];
  $('#btnOpen').textContent = def ? ('用 ' + def.name + ' 打开') : '打开项目位置';
  $('#openMenu').innerHTML = S.openers.map((o) =>
    '<button data-open="' + esc(o.id) + '"><span class="mname">' + esc(o.name) + '</span>'
    + '<span class="mkind">' + esc(o.kind === 'editor' ? '编辑器' : o.kind === 'terminal' ? '终端'
      : o.kind === 'shell' ? '命令行' : '文件管理器') + '</span>'
    + '<span class="star ' + (o.id === S.defaultOpener ? 'on' : '') + '" data-star="' + esc(o.id)
    + '" title="设为默认">' + (o.id === S.defaultOpener ? '★' : '☆') + '</span></button>').join('')
    + '<div class="sep"></div><button data-normal="1"><span class="mname">用文件资源管理器打开</span></button>';
}
async function openIn(id) {
  await guard(async () => { await invoke('open_in', { root: S.root, id }); toast('已交给它打开'); });
}

// ── 新建项目 ───────────────────────────
async function createProject() {
  const name = $('#newName').value.trim();
  if (!name) { toast('先给项目起个名字'); return; }
  await guard(async () => {
    const parent = await invoke('pick_folder', { title: '项目建在哪个文件夹里' });
    if (!parent) return;
    const r = await invoke('create_project', {
      parent, name, template: $('#newTpl').value,
      lang: $('#newLang').value,
      purpose: $('#newPurpose').value.trim() || null,
      pythonExe: $('#newPython').value || null,
      makeVenv: $('#newVenv').checked,
    });
    toast('建好了：' + (r.venv || r.dir));
    await load(r.dir);
  });
}

/// 环境那一段：列出这台机器上的解释器，外加配置档
async function loadEnvs() {
  await guard(async () => {
    S.pythons = await invoke('list_pythons', { project: S.root || null });
    // 引擎用信封回答：`unknown` = "这些地方都找过了，没找到"，**不等于"没装"**。
    // 原来的写法直接把它当数组用——空数组在界面上就是"一个都没有"，
    // 而那时候我们真正知道的只是"我们没找到"。
    const py = unpack(S.pythons);
    S.pythons = py.state === 'yes' ? py.data : [];
    if (py.state !== 'yes') feedback('warn', '没找到 Python', py.reason);
    const pf = unpack(await invoke('profiles_list'));
    S.profiles = (pf.state === 'yes' && Array.isArray(pf.data)) ? pf.data : [];
    paintEnvs();
  });
}
function paintEnvs() {
  const py = $('#newPython');
  if (py) {
    py.innerHTML = '<option value="">自动挑一个可用的解释器</option>'
      + S.pythons.map((p) => '<option value="' + esc(p.path) + '">'
        + esc((p.version || '?') + ' · ' + p.source + ' · ' + p.path) + '</option>').join('');
  }
  const pr = $('#newProfile');
  if (pr) {
    pr.innerHTML = '<option value="">— 配置档 —</option>'
      + S.profiles.map((p) => '<option value="' + esc(p.name) + '">' + esc(p.name) + '</option>').join('');
  }
}
async function saveProfile() {
  const name = $('#newName').value.trim() || '我的配置';
  await guard(async () => {
    S.profiles = await invoke('profile_save', {
      profile: {
        name, template: $('#newTpl').value, lang: $('#newLang').value,
        // 注意：这个结构体的字段名是 snake_case（serde 按原名读），
        // 写成 pythonExe 不会报错，只会被默默丢掉——踩过一次。
        python_exe: $('#newPython').value || null,
        make_venv: $('#newVenv').checked,
      },
    });
    paintEnvs();
    toast('已存成配置档「' + name + '」，下次直接选它');
  });
}

/// 语言一换，下半截的选项就该换：Python 才有解释器和虚拟环境，
/// 别的语言只有它自己的骨架。绝不能拿 Python 那套去套别的语言。
function paintNewLang() {
  const lang = ($('#newLang') || {}).value || 'python';
  const py = lang === 'python';
  const row = $('#newPyRow');
  if (row) row.hidden = !py;
  const tpl = $('#newTpl');
  if (tpl) {
    tpl.disabled = !py;
    tpl.dataset.tip = py ? '' : '这个语言用的是它自己的标准骨架，模板选项只对 Python 有效';
  }
  const hint = $('#newHint');
  if (hint) {
    hint.textContent = py
      ? '配置档记的是「模板 + 用哪个解释器 + 建不建虚拟环境」，下次直接沿用。'
      : '这门语言用不了 Python 那套模板与虚拟环境选项，已按它自己的东西收起。';
  }
  // 「将创建什么」按语言各说各的——把 Python 的清单套到别的语言上，
  // 是这个环节最容易出的错（也会让新手照着错的清单去查资料）。
  const LANG_INFO = {
    python: ['requirements.txt', 'python -m pip install -r requirements.txt', 'python main.py'],
    rust: ['Cargo.toml', 'cargo fetch', 'cargo run'],
    node: ['package.json', 'npm install', 'npm start'],
    java: ['pom.xml', 'mvn dependency:go-offline', 'mvn compile exec:java'],
    cpp: ['CMakeLists.txt', 'cmake -S . -B build', 'cmake --build build'],
  };
  const info = LANG_INFO[lang] || LANG_INFO.python;
  const pv = $('#newPreview');
  if (pv) {
    pv.innerHTML = '<div class="np-row"><span class="np-k">清单文件</span><span class="np-v">'
      + esc(info[0]) + '</span></div>'
      + '<div class="np-row"><span class="np-k">装依赖</span><span class="np-v">' + esc(info[1]) + '</span></div>'
      + '<div class="np-row"><span class="np-k">跑起来</span><span class="np-v">' + esc(info[2]) + '</span></div>'
      + '<div class="np-note">建完会连着写好契约（这三个命令）并留一条记录，'
      + '所以打开就能直接点「运行」。</div>';
  }
}

// ── 事件 ───────────────────────────────
function switchTab(name) {
  PANEL.tab = name;
  panelPolicy();
  document.querySelectorAll('#nav button').forEach((b) => b.classList.toggle('on', b.dataset.tab === name));
  document.querySelectorAll('.view').forEach((v) => v.classList.toggle('on', v.id === 'view-' + name));
  if (name === 'clean' && S.root && !S.clean) scanClean();
  if (name === 'clean' && !S.root && !S.clean) scanClean();
  if (name === 'machine') loadMachine(false);
  if (name === 'note') renderNoteProjects();
  if (name === 'editor' && S.root && !ED.files.length) edLoadFiles();
  if (name === 'editor') edPaintCode();
}

/// 「这台机器」和「清理」不需要项目；其余三页需要。
/// 没项目时把它们变灰，比点进去看到一个空壳要好。
function paintNav() {
  const has = !!S.root;
  document.querySelectorAll('#nav button[data-needs="project"]').forEach((b) => {
    b.classList.toggle('dim', !has);
    b.title = has ? '' : '先打开一个项目';
  });
}

window.addEventListener('DOMContentLoaded', async () => {
  applyLook();   // 外观先落地，免得闪一下默认主题
  initTooltips();
  initParallax();
  initDock();
  // 全局点击反馈：任何可点的东西按下去都有涟漪 + 一声轻响
  document.addEventListener('pointerdown', (e) => {
    const hit = e.target.closest('button, .pcard, .diag, .nb-item, .copy, .agroup>summary, .item');
    if (!hit || hit.disabled) return;
    rippleAt(hit, e);
    sfx('click');
  }, true);
  $('#nav').addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (b) switchTab(b.dataset.tab);
  });
  ['#btnPick', '#btnPick2'].forEach((s) => $(s).addEventListener('click', pickProject));
  // 新建项目：独立弹窗卡片（IDEA 的 New Project 也是独立窗口，
  // 不是塞在页面下方的一块面板——那种做法会把"先看哪个"搞乱）。
  const openNew = () => {
    switchTab('home');
    if ($('#newModal')) $('#newModal').hidden = false;
    paintNewLang();
    loadEnvs();
    setTimeout(() => $('#newName') && $('#newName').focus(), 60);
  };
  ['#btnNew', '#btnNew2'].forEach((s) => $(s) && $(s).addEventListener('click', openNew));
  const closeNew = () => { if ($('#newModal')) $('#newModal').hidden = true; };
  if ($('#btnNewX')) $('#btnNewX').addEventListener('click', closeNew);
  if ($('#newModal')) {
    $('#newModal').addEventListener('click', (e) => { if (e.target.id === 'newModal') closeNew(); });
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && !$('#newModal').hidden) closeNew();
    });
  }
  if ($('#newLang')) $('#newLang').addEventListener('change', paintNewLang);
  $('#btnSaveProfile').addEventListener('click', saveProfile);
  $('#newProfile').addEventListener('change', (e) => {
    const p = S.profiles.find((x) => x.name === e.target.value);
    if (!p) return;
    $('#newTpl').value = p.template || 'package';
    if ($('#newLang')) $('#newLang').value = p.lang || 'python';
    $('#newPython').value = p.python_exe || '';
    $('#newVenv').checked = p.make_venv !== false;
    paintNewLang();
    toast('已沿用配置档「' + p.name + '」');
  });
  $('#btnNewCancel').addEventListener('click', () => { $('#newModal').hidden = true; });
  $('#btnCreate').addEventListener('click', createProject);
  // 运行页的旧按钮已经被"一条龙流水线"取代；这几行保留兼容，
  // 但元素可能不存在——不判空就会整天弹「界面出错」红条。
  [['#btnRun', 'run'], ['#btnTest', 'test'], ['#btnInstall', 'install']].forEach(([sel, task]) => {
    const b = $(sel);
    if (b) b.addEventListener('click', () => doRun(task));
  });
  // 流水线的四个步骤：点了就跑对应的那一步
  document.querySelectorAll('.pstep').forEach((s) => {
    s.addEventListener('click', () => {
      if (s.classList.contains('none')) { toast('契约里没有这一步，可以在项目详情里自己填'); return; }
      doRun(s.dataset.task);
    });
  });
  $('#btnRescan').addEventListener('click', scanClean);
  const bs = $('#btnAuditScan');
  if (bs) bs.addEventListener('click', () => runAudit(false));
  const bd = $('#btnAuditDeep');
  if (bd) bd.addEventListener('click', () => runAudit(true));
  // ── 诊断栏 / 项目切换 / 活动带 ──
  const rb = $('#btnRail');
  if (rb) {
    rb.addEventListener('click', () => {
      const off = document.body.classList.toggle('rail-off');
      PANEL.pinned = !off;          // 手点过就以用户为准
      localStorage.setItem('jingwei.railOff', off ? '1' : '0');
      rb.textContent = off ? '⇤' : '⇥';
    });
    if (localStorage.getItem('jingwei.railOff') === '1') {
      document.body.classList.add('rail-off');
      PANEL.pinned = false;
      rb.textContent = '⇤';
    }
  }
  const railBody = $('#railBody');
  if (railBody) {
    railBody.addEventListener('click', (e) => {
      const d = e.target.closest('.diag');
      if (!d) return;
      switchTab('home');
      const target = document.querySelector('.finding[data-id="' + d.dataset.jump + '"]');
      if (target) {
        target.scrollIntoView({ behavior: 'smooth', block: 'center' });
        const det = target.querySelector('details');
        if (det) det.open = true;
      }
    });
  }
  const bsw = $('#btnSwitch');
  const swMenu = $('#switchMenu');
  if (bsw && swMenu) {
    bsw.addEventListener('click', (e) => {
      e.stopPropagation();
      renderSwitcher();
      swMenu.hidden = !swMenu.hidden;
    });
    swMenu.addEventListener('click', (e) => {
      const b = e.target.closest('button[data-path]');
      if (!b) return;
      swMenu.hidden = true;
      load(b.dataset.path).catch((err) => toast(String(err), true));
    });
    document.addEventListener('click', () => { swMenu.hidden = true; });
  }
  const bbOpen = $('#btnBandOpen');
  if (bbOpen) bbOpen.addEventListener('click', () => switchTab('run'));
  const bbClose = $('#btnBandClose');
  if (bbClose) bbClose.addEventListener('click', () => { const b = $('#band'); if (b) b.hidden = true; });

  // ── 编辑器 ──
  if ($('#btnResetSettings')) {
    $('#btnResetSettings').addEventListener('click', () => {
      localStorage.removeItem('jingwei.settings');
      location.reload();
    });
  }
  if ($('#btnEdRefresh')) $('#btnEdRefresh').addEventListener('click', edLoadFiles);
  if ($('#edFilter')) $('#edFilter').addEventListener('input', edPaintFiles);
  if ($('#edFiles')) {
    $('#edFiles').addEventListener('click', (e) => {
      const dir = e.target.closest('.ed-node.dir');
      if (dir) {
        const key = dir.dataset.dir;
        if (ED.closedDirs.has(key)) ED.closedDirs.delete(key);
        else ED.closedDirs.add(key);
        sfx('click');
        edPaintFiles();
        return;
      }
      const f = e.target.closest('.ed-node.file');
      if (f) edOpen(f.dataset.file);
    });
  }
  if ($('#edText')) {
    $('#edText').addEventListener('input', edMarkDirty);
    $('#edText').addEventListener('scroll', () => {
      $('#edGutter').scrollTop = $('#edText').scrollTop;
      edPaintCode();          // 只重画可见窗口，2–4ms
      edPaintCaret();
    }, { passive: true });
    // 光标一动就要更新行列和当前行——「我在第几行第几列」是读代码时的基本坐标
    ['keyup', 'click', 'select', 'focus'].forEach((ev) =>
      $('#edText').addEventListener(ev, edPaintCaret));
    // 写代码的几条基本期待：Tab 缩进、回车继承缩进、Ctrl+S 落盘
    $('#edText').addEventListener('keydown', (e) => {
      const t = e.target;
      const pad = ' '.repeat(SET.indent || 4);
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') { e.preventDefault(); edFindShow('find'); return; }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'g') { e.preventDefault(); edFindShow('line'); return; }
      if ((e.ctrlKey || e.metaKey) && e.altKey && e.key.toLowerCase() === 'l') { e.preventDefault(); edFormat(); return; }
      if (e.key === 'Escape' && $('#edFind') && !$('#edFind').hidden) { e.preventDefault(); edFindHide(); return; }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault();
        if (ED.dirty) edApply(); else toast('没有未应用的改动');
        return;
      }
      if (e.key === 'Tab') {
        e.preventDefault();
        const s = t.selectionStart;
        if (e.shiftKey) {
          // 反缩进：先把这一行开头多余的空格吃掉
          const ls = t.value.lastIndexOf('\n', s - 1) + 1;
          const head = t.value.slice(ls, Math.min(ls + pad.length, s));
          const drop = head.match(/^ +/)[0].length;
          if (drop) {
            t.value = t.value.slice(0, ls) + t.value.slice(ls + drop);
            t.selectionStart = t.selectionEnd = Math.max(ls, s - drop);
          }
        } else {
          t.value = t.value.slice(0, s) + pad + t.value.slice(t.selectionEnd);
          t.selectionStart = t.selectionEnd = s + pad.length;
        }
        edMarkDirty();
        edPaintCaret();
        return;
      }
      if (e.key === 'Enter' && t.selectionStart === t.selectionEnd) {
        // 回车继承上一行缩进；上一行以 { ( [ : 结尾就再多缩一层
        const s = t.selectionStart;
        const ls = t.value.lastIndexOf('\n', s - 1) + 1;
        const cur = t.value.slice(ls, s);
        const head = cur.match(/^[ \t]*/)[0].replace(/\t/g, pad);
        if (!head) return;                    // 顶格的行交给浏览器默认行为
        const extra = /[{\[(:]\s*$/.test(cur) ? pad : '';
        e.preventDefault();
        const ins = '\n' + head + extra;
        t.value = t.value.slice(0, s) + ins + t.value.slice(t.selectionEnd);
        t.selectionStart = t.selectionEnd = s + ins.length;
        edMarkDirty();
        edPaintCaret();
      }
    });
  }
  if ($('#edCrumb')) {
    $('#edCrumb').addEventListener('click', (e) => {
      const b = e.target.closest('button[data-crumb]');
      if (!b || !$('#edFilter')) return;
      // 点面包屑 = 把左边的树筛到这一层，同级文件一眼看全
      const dir = b.dataset.crumb;
      const f = $('#edFilter');
      f.value = dir.includes('/') ? dir.slice(0, dir.lastIndexOf('/') + 1) : '';
      edPaintFiles();
      sfx('click');
    });
  }
  // 标签条：点一下切文件，点 ✕ 关掉
  if ($('#edTabs')) {
    $('#edTabs').addEventListener('click', (e) => {
      const x = e.target.closest('[data-ex]');
      if (x) { e.stopPropagation(); edTabClose(x.dataset.ex); sfx('click'); return; }
      const tab = e.target.closest('[data-etab]');
      if (tab && tab.dataset.etab !== ED.path) { edOpen(tab.dataset.etab); sfx('click'); }
    });
  }
  // 查找栏
  if ($('#edFindInput')) {
    $('#edFindInput').addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); edFindGo(e.shiftKey ? -1 : 1); }
      else if (e.key === 'Escape') { e.preventDefault(); edFindHide(); }
      else if (FIND.mode === 'line' && e.key !== 'Backspace') setTimeout(() => edFindGo(1), 0);
    });
    $('#edFindInput').addEventListener('input', () => { if (FIND.mode === 'line') edFindGo(1); });
    $('#edFindNext').addEventListener('click', () => edFindGo(1));
    $('#edFindPrev').addEventListener('click', () => edFindGo(-1));
    $('#edFindClose').addEventListener('click', edFindHide);
  }
  if ($('#btnEdFormat')) $('#btnEdFormat').addEventListener('click', edFormat);
  if ($('#btnEdApply')) $('#btnEdApply').addEventListener('click', edApply);
  if ($('#btnEdMore')) $('#btnEdMore').addEventListener('click', (e) => edMoreMenu(e.currentTarget));
  if ($('#btnEdBuild')) $('#btnEdBuild').addEventListener('click', () => { switchTab('run'); doRun('build'); });
  if ($('#btnEdTest')) $('#btnEdTest').addEventListener('click', () => { switchTab('run'); doRun('test'); });
  if ($('#btnEdRun')) $('#btnEdRun').addEventListener('click', () => { switchTab('run'); doRun('run'); });
  // 设置按钮在底部状态栏上：什么时候想改「自动到什么程度」都够得着
  const bs2 = $('#btnSettings');
  if (bs2) {
    bs2.addEventListener('click', () => {
      const p = $('#settingsPanel');
      if (!p) return;
      p.hidden = !p.hidden;
      if (!p.hidden) {
        paintSettingChanges();
        ['scan', 'analyze', 'contract', 'verify', 'trial'].forEach((k) => {
          const el = $('#set' + k[0].toUpperCase() + k.slice(1));
          if (el) el.checked = !!SET[k];
          if (el) el.onchange = readSettings;
        });
        ['hover', 'autoRecord'].forEach((k) => {
          const el = $('#set' + k[0].toUpperCase() + k.slice(1));
          if (el) { el.checked = !!SET[k]; el.onchange = readSettings; }
        });
        if ($('#setTheme')) $('#setTheme').value = SET.theme;
        if ($('#setDensity')) $('#setDensity').value = SET.density;
        if ($('#setMotion')) $('#setMotion').value = SET.motion;
        if ($('#setFbLevel')) $('#setFbLevel').value = SET.fb;
        if ($('#setScanDepth')) $('#setScanDepth').value = String(SET.scanDepth);
        if ($('#setCleanLevel')) $('#setCleanLevel').value = SET.cleanLevel;
        if ($('#setDelMode')) $('#setDelMode').value = SET.delMode;
        if ($('#setIndent')) $('#setIndent').value = String(SET.indent);
        ['#setTheme', '#setDensity', '#setMotion', '#setFbLevel'].forEach((s) => {
          const el = $(s);
          if (el) el.onchange = readSettings;
        });
        ['#setScanDepth', '#setCleanLevel', '#setDelMode', '#setIndent'].forEach((s) => {
          const el = $(s);
          if (el) el.onchange = readSettings;
        });
        p.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
      }
    });
  }
  const ab = $('#audit');
  if (ab) {
    ab.addEventListener('click', (e) => {
      const b = e.target.closest('button[data-fix]');
      if (b) applyFix(b.dataset.fix, b);
    });
  }
  // 「时不时扫一眼」：切回这个窗口时扫一次，开着不动每 10 分钟扫一次。
  // 都是快扫（毫秒级），结论没变就不重绘，不抢注意力。
  window.addEventListener('focus', () => refreshQuiet(false));
  setInterval(() => { if (!document.hidden) refreshQuiet(false); }, 10 * 60 * 1000);
  // 变更检测要比项目检查勤：它回答的是「我刚才那一下做了什么」，
  // 隔十分钟才说一次就失去意义了。这一趟只 stat 文件，不读内容。
  window.addEventListener('focus', () => loadChanges());
  // 变更检测：在前台 45 秒一次；切到后台就放宽到 3 分钟一次（省电省 CPU）
  setInterval(() => { if (!document.hidden) loadChanges(); }, 45 * 1000);
  setInterval(() => { if (document.hidden) loadChanges(); }, 180 * 1000);
  // 后台时把环境光的动画停掉：没人看还动着，纯浪费 GPU
  const amb = document.querySelector('.ambient');
  document.addEventListener('visibilitychange', () => {
    if (!amb) return;
    amb.style.animationPlayState = document.hidden ? 'paused' : 'running';
    const inner = amb.querySelector('.ambient-i');
    if (inner) inner.style.animationPlayState = document.hidden ? 'paused' : 'running';
  });

  // ── 项目列表：扫描、分页、卡片、附属卡片 ──
  const bscan = $('#btnScan');
  if (bscan) {
    bscan.addEventListener('click', () => scanProjects($('#scanParent').value.trim(), parseInt($('#scanDepth').value, 10)));
  }
  const bsd = $('#btnScanDir');
  if (bsd) {
    bsd.addEventListener('click', () => guard(async () => {
      const dir = await invoke('pick_folder', { title: '选一个放项目的目录' });
      if (dir) await scanProjects(dir, parseInt($('#scanDepth').value, 10));
    }));
  }
  const sp = $('#scanParent');
  if (sp) {
    sp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') scanProjects(sp.value.trim(), parseInt($('#scanDepth').value, 10));
    });
  }
  const pl = $('#projList');
  if (pl) {
    pl.addEventListener('click', (e) => {
      const card = e.target.closest('.banner');
      if (!card) return;
      const p = PS.all.find((x) => x.path === card.dataset.path);
      if (!p) return;
      const act = e.target.closest('button')?.dataset.act;
      if (act === 'detail') { PS.pinned = true; openDetail(p); return; }
      // 点横幅 = 全局选中 + 打开。选中之后依赖栏、文件树、记录、流水线都跟着换，
      // 不用再回「概览」手动找一次。
      PS.pinned = false;
      sfx('click');
      load(p.path).catch((err) => toast(String(err), true));
    });
    // 停留预览：鼠标停 220ms 展开附属卡。不用先点进去，也就不会打断你手上正在做的事。
    pl.addEventListener('mouseover', (e) => {
      if (!SET.hover || PS.pinned) return;
      const card = e.target.closest('.banner');
      if (!card) return;
      const p = PS.all.find((x) => x.path === card.dataset.path);
      if (!p || (PS.sel && PS.sel.path === p.path)) return;
      clearTimeout(PS.hoverTimer);
      PS.hoverTimer = setTimeout(() => { if (!PS.pinned) openDetail(p); }, 220);
    });
    pl.addEventListener('mouseleave', () => {
      clearTimeout(PS.hoverTimer);
      if (PS.pinned || !PS.sel) return;
      PS.sel = null;
      const d = $('#projDetail');
      if (d) d.hidden = true;
      paintProjectBanners();
    });
  }
  const pg = $('#projPager');
  if (pg) {
    pg.addEventListener('click', (e) => {
      const d = e.target.closest('button')?.dataset.page;
      if (d === 'prev' && PS.page > 0) PS.page--;
      if (d === 'next') PS.page++;
      renderProjList();
      analyzeRange(PS.page * PAGE_SIZE, (PS.page + 1) * PAGE_SIZE);
    });
  }
  const pd = $('#projDetail');
  if (pd) {
    pd.addEventListener('click', (e) => {
      const a = e.target.closest('button')?.dataset.pd;
      if (a) detailAct(a);
    });
  }
  // 一进来先把「你手上有哪些项目」摆出来，不用先点任何东西
  if (SET.scan) await guard(scanInit);

  // ── 笔记页：项目卡片（选中/停留）+ 记录与日志 ──
  const nbp = $('#nbProjects');
  if (nbp) {
    nbp.addEventListener('click', (e) => {
      const card = e.target.closest('.banner, .pcard');
      if (!card) return;
      const p = PS.all.find((x) => x.path === card.dataset.npath);
      if (!p) return;
      const act = e.target.closest('button')?.dataset.nact;
      if (act === 'addnote') { NL.sel = p; selectNoteProject(p); newNote('note'); }
      else if (act === 'addrecord') { NL.sel = p; selectNoteProject(p); newNote('record'); }
      else selectNoteProject(p);
    });
    // 停留即选中：默认开，可以在设置里关掉换成点击
    nbp.addEventListener('mouseover', (e) => {
      if (!SET.hover) return;
      const card = e.target.closest('.banner, .pcard');
      if (!card) return;
      const p = PS.all.find((x) => x.path === card.dataset.npath);
      if (!p) return;
      clearTimeout(NL.hoverTimer);
      NL.hoverTimer = setTimeout(() => selectNoteProject(p), 260);
    });
    nbp.addEventListener('mouseleave', () => clearTimeout(NL.hoverTimer));
  }
  const nbi = $('#nbItems');
  if (nbi) {
    nbi.addEventListener('click', (e) => {
      const ref = e.target.closest('[data-ref]');
      if (ref) appendRef(ref.dataset.ref);
    });
  }
  const nbs = $('#btnNbScan');
  if (nbs) {
    nbs.addEventListener('click', () => guard(async () => {
      if (!PS.parent) await scanInit();
      else await scanProjects(PS.parent, PS.depth);
      renderNoteProjects();
    }));
  }
  $('#btnClean').addEventListener('click', async () => {
    if (!S.picked.size) { toast('还没有勾选任何项'); return; }
    await guard(async () => {
      const r = await invoke('clean_items', {
        root: S.root || '',
        paths: Array.from(S.picked),
        permanent: SET.delMode === 'permanent',
      });
      toast('清了 ' + r.removed + ' 项，释放 ' + r.human
        + (r.failed.length ? '，' + r.failed.length + ' 项没删掉' : ''));
      await scanClean();
    });
  });
  if ($('#setSound')) { $('#setSound').checked = !!SET.sound; $('#setSound').onchange = readSettings; }
  $('#btnNewNote').addEventListener('click', () => {
    S.current = null;
    openEditor(null);
    $('#neTitle').focus();
  });
  $('#nbSearch').addEventListener('input', (e) => renderNotes(e.target.value));

  // 打开方式菜单
  $('#btnOpen').addEventListener('click', () => {
    const def = S.openers.find((o) => o.id === S.defaultOpener) || S.openers[0];
    if (def) openIn(def.id);
  });
  $('#btnOpenMenu').addEventListener('click', (e) => {
    e.stopPropagation();
    $('#openMenu').hidden = !$('#openMenu').hidden;
  });
  $('#openMenu').addEventListener('click', async (e) => {
    const star = e.target.closest('[data-star]');
    if (star) {
      e.stopPropagation();
      const id = star.dataset.star;
      const next = S.defaultOpener === id ? null : id;
      await guard(async () => {
        await invoke('set_default_opener', { id: next });
        S.defaultOpener = next;
        paintOpenButton();
        toast(next ? '已设为默认打开方式' : '已取消默认');
      });
      return;
    }
    const btn = e.target.closest('[data-open]');
    $('#openMenu').hidden = true;
    if (btn) return openIn(btn.dataset.open);
    if (e.target.closest('[data-normal]')) return openIn('explorer');
  });
  document.addEventListener('click', () => { $('#openMenu').hidden = true; });

  // 点命令块复制；点卡片头折叠；点链接跳笔记
  document.addEventListener('click', (e) => {
    const cp = e.target.closest('.copy');
    if (cp) {
      copyText(cp.dataset.copy).then(() => {
        cp.classList.add('done');
        cp.querySelector('.cp').textContent = '已复制';
        setTimeout(() => { cp.classList.remove('done'); cp.querySelector('.cp').textContent = '点击复制'; }, 1600);
      });
      return;
    }
    const head = e.target.closest('.rc-head');
    if (head) { head.nextElementSibling.classList.toggle('hide'); return; }
    const nbItem = e.target.closest('.nb-item');
    if (nbItem) {
      const n = (S.data.notes || []).find((x) => x.slug === nbItem.dataset.slug);
      if (n) openEditor(n);
      return;
    }
    const link = e.target.closest('.wikilink');
    if (link) {
      const name = link.dataset.link;
      const found = (S.data.notes || []).find((x) => x.title === name || x.slug === name);
      if (found) openEditor(found);
      else toast('还没有叫《' + name + '》的笔记');
    }
  });
  document.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 's' && $('#neBody')) { e.preventDefault(); saveNote(); }
  });

  await guard(async () => {
    // 兜底：IPC 回来的东西形状不对时不许崩——一次异常就白屏，用户只会以为工具坏了
    const ops = unpack(await invoke('openers'));
    S.openers = ops.state === 'yes' && Array.isArray(ops.data) ? ops.data : [];
    if (ops.state !== 'yes') feedback('warn', '没检测到编辑器', ops.reason);
    const op = unpack(await invoke('default_opener'));
    S.defaultOpener = (op.state === 'yes' && op.data) || null;
    paintOpenButton();
  });
  $('#btnRefreshMachine').addEventListener('click', () => loadMachine(true));
  await listen('run://event', (e) => applyEvent(e.payload));
  // 状态栏：以「主动拉」为主路径（事件推送可能因为权限/时序拿不到，
  // 那样会让状态栏一直空着）。有事件就当加分，没有也照样有数字。
  const pollSample = async () => {
    if (document.hidden) return;
    // 采样拿不到（状态锁被占）时**保持上一次的数字**，别把状态栏清空——
    // 清空会让人以为"机器闲下来了"，那是假的。
    try {
      const sm = unpack(await invoke('sample_now'));
      if (sm.state === 'yes' && sm.data) paintBar(sm.data);
    } catch (e) { /* 拉不到就保持上一次 */ }
  };
  pollSample();
  setInterval(pollSample, 5000);
  await listen('sys://sample', (e) => paintBar(e.payload));
  renderNotes();
  paintNav();
  invoke('initial_project').then((v) => {
    const r = unpack(v);
    if (r.state === 'yes' && r.data) load(r.data).catch((e) => toast(String(e), true));
  });
});

// ── 底部状态栏：数据由 Rust 推，前端只负责画 ─────────────
const cpuHist = [];
function drawSpark() {
  const c = $('#spark'), g = c.getContext('2d');
  g.clearRect(0, 0, c.width, c.height);
  if (cpuHist.length < 2) return;
  g.beginPath();
  cpuHist.forEach((v, i) => {
    const x = i / (cpuHist.length - 1) * c.width;
    const y = c.height - Math.min(v, 100) / 100 * (c.height - 2) - 1;
    if (i) g.lineTo(x, y); else g.moveTo(x, y);
  });
  g.strokeStyle = '#E09A75';
  g.lineWidth = 1.5;
  g.stroke();
}
function paintBar(s) {
  // 采样出问题时也要说清楚，而不是让状态栏一直挂个「—」
  if (!s || typeof s.cpu_percent !== 'number') {
    $('#barAt').textContent = '采样没拿到数据';
    return;
  }
  $('#barCpu').textContent = s.cpu_percent.toFixed(0) + '%';
  $('#barMem').textContent = (s.mem_used_mb / 1024).toFixed(1) + ' / '
    + (s.mem_total_mb / 1024).toFixed(1) + ' GB';
  $('#barDisk').textContent = s.disk_free_gb.toFixed(0) + ' GB 可用';
  // 解释器进程：一次快照分别数几种。都没有就说清楚是「没有」，而不是挂个 0。
  const dp = (s.dev_procs || []).filter((x) => x[1] > 0);
  const bp = $('#barPy');
  bp.textContent = dp.length ? dp.map((x) => x[0] + ' ' + x[1]).join(' · ') : '没有解释器在跑';
  bp.dataset.tip = '按进程名数的：python / node / java / cargo / rustc / dotnet。'
    + '跑起来的东西在这儿能看见，不用去任务管理器找。';
  $('#barAt').textContent = new Date().toLocaleTimeString('zh-CN', { hour12: false }) + ' 采样';
  // CPU 曲线只在真的有东西在跑的时候画。没在跑就没人看它，白占 GPU。
  const sp = $('#spark');
  if (PANEL.running) {
    if (sp) sp.hidden = false;
    cpuHist.push(s.cpu_percent);
    while (cpuHist.length > 48) cpuHist.shift();
    drawSpark();
  } else if (cpuHist.length) {
    cpuHist.length = 0;
    if (sp) {
      sp.getContext('2d').clearRect(0, 0, sp.width, sp.height);
      sp.hidden = true;
    }
  } else if (sp && !sp.hidden) {
    sp.hidden = true;
  }
}

// ── 详细区：平时不取；点开先摆上次的，再刷新 ─────────────
const DEEP = { opened: false, at: null, m: null, d: null };
const kv = (k, v) => '<div class="fact"><div class="k">' + esc(k) + '</div><div class="v" title="'
  + esc(v) + '">' + esc(v) + '</div></div>';

function renderDeep(stale) {
  const m = DEEP.m, D = DEEP.d;
  let h = '';
  if (DEEP.at) {
    h += '<div class="hint">上次获取 ' + DEEP.at.toLocaleTimeString('zh-CN', { hour12: false })
      + (stale ? ' · 正在刷新…' : '') + '</div>';
  }
  if (m) {
    const x = m.machine || {};
    // 这一块只放**快路径拿不到的**：换个地方再画一遍 CPU/内存没有意义，
    // 用户要的是"深入一层多看到了什么"。
    const tools = Object.entries(m.tools || {});
    if (tools.length) {
      h += '<h3 class="secTitle">工具链的出处（快读只给版本，这里给路径）</h3><div class="dgrid">'
        + tools.map(([k, t]) => kv(k, t.ok ? (t.version || '（没报版本）') + '  ' + (t.path || '') : '没找到')).join('')
        + '</div>';
    }
    h += '<h3 class="secTitle">注册表原文（和上面显示名不一样是正常的）</h3><div class="dgrid">'
      + kv('ProductName', x.os_product || '—')
      + kv('DisplayVersion', x.os_version || '—')
      + kv('CurrentBuild', x.os_build || '—')
      + kv('盘符', (x.disks || []).map((d) => d.drive).join('  ') || '—')
      + '</div>';
    const env = Object.entries(m.env || {});
    if (env.length) {
      h += '<h3 class="secTitle">会影响行为的环境变量</h3><div class="dgrid">'
        + env.map(([k, v]) => v.length > 90
          ? '<div class="fact"><div class="k">' + esc(k) + '</div><div class="v long">'
            + '<details><summary>' + esc(v.slice(0, 60)) + '…（点击展开全部 ' + v.length + ' 字符）</summary>'
            + '<div class="fullval">' + esc(v) + '</div></details></div></div>'
          : kv(k, v)).join('')
        + '</div>';
    }
  }
  if (D) {
    if (!D.ok) {
      h += '<div class="finding warn"><div class="ftitle">' + esc(D.why) + '</div></div>';
    } else {
      h += '<h3 class="secTitle">解释器底下那一层</h3><div class="dgrid">'
        + kv('解释器', D.exe)
        + kv('位数 / 架构', D.bits + ' 位 · ' + D.arch)
        + kv('ABI 标签', D.abi || '—')
        + kv('site-packages', (D.siteDirs || []).join('\n'))
        + kv('._pth 文件', (D.pthFiles || []).length ? D.pthFiles.join('\n') : '无（不是便携版）')
        + kv('能否编译扩展', (D.build.has_cl ? '有 cl.exe' : '没有 cl.exe（装不了源码包）')
            + ' · setuptools ' + (D.build.has_setuptools ? '有' : '无')
            + ' · wheel ' + (D.build.has_wheel ? '有' : '无'))
        + '</div>';
      // 原生扩展：几十个同类问题只出**一张卡**（总数 + 涉及哪些包 + 几个例子）。
      // 一条文件一张卡就是刷屏——那是这个工具最该避免的事。
      const ns = D.native || { total: (D.issues || []).length, samples: D.issues || [] };
      h += '<h3 class="secTitle">原生扩展（.pyd）</h3>';
      if (!ns.total) {
        h += '<div class="finding info"><div class="ftitle"><span class="chip info">正常</span>'
          + '<span>' + esc(ns.verdict || '原生扩展都对得上') + '</span></div>'
          + '<div class="boundary">扫过的 .pyd 都解析得到依赖，位数也对得上。</div></div>';
      } else {
        const sev = ns.arch_mismatch ? 'error' : 'warn';
        h += '<div class="finding ' + sev + '">'
          + '<div class="ftitle"><span class="chip ' + sev + '">'
          + (ns.arch_mismatch ? '位数不对' : '缺依赖') + '</span>'
          + '<span>' + esc(ns.verdict) + '</span></div>'
          + ((ns.packages || []).length
            ? '<div class="boundary">涉及：' + (ns.packages).map((p) => esc(p[0]) + '（' + p[1] + '）').join('、') + '</div>'
            : '')
          + '<details><summary>看两个例子</summary><ul>'
          + (ns.samples || []).map((i) => '<li>' + esc(i.pyd)
              + (i.missing && i.missing.length ? ' —— 找不到 ' + esc(i.missing.join(', ')) : '')
              + '（它是 ' + esc(i.arch) + '）</li>').join('')
          + '</ul></details>'
          + (ns.hint ? '<div class="next">下一步 · ' + esc(ns.hint) + '</div>' : '')
          + '</div>';
      }
    }
  }
  $('#deepBody').innerHTML = h;
}

/// 没有项目也能看。硬件那一段要问 CIM，约一秒，所以只在进这一页时才去取。
const HW = { at: null, data: null };
async function loadMachine(force) {
  if (HW.at && !force) {
    renderHardware(true);
    return;
  }
  if (HW.at) renderHardware(true); // 先把上次的摆出来，别留空窗
  $('#deepWhen').textContent = '读取中…';
  await guard(async () => {
    // 主路径是毫秒级的那条；WMI 慢到不能用，改成按钮让人自己点
    const [hw, m, d] = await Promise.all([
      invoke('hardware_fast'),
      invoke('machine_report'),
      invoke('deep_check', { project: S.root || null }),
    ]);
    // 硬件这两路也会回信封：读不到时**如实说读不到**，
    // 而不是给一个空对象让界面显示成"这台机器没有这些东西"。
    const h = unpack(hw);
    HW.data = h.state === 'yes' ? h.data : {};
    if (h.state !== 'yes') feedback('warn', '硬件信息没读全', h.reason);
    const mm = unpack(m);
    DEEP.m = mm.state === 'yes' ? mm.data : {};
    if (mm.state !== 'yes') feedback('warn', '机器清单没读全', mm.reason);
    HW.at = new Date();
    DEEP.d = d; DEEP.at = HW.at;
    renderHardware(false);
    renderDeep(false);
  });
}

function renderHardware(stale) {
  const h = HW.data;
  const when = HW.at ? HW.at.toLocaleTimeString('zh-CN', { hour12: false }) : '';
  $('#deepWhen').textContent = when ? ('读取于 ' + when + (stale ? '，正在刷新…' : '')) : '';
  const m = (DEEP.m && DEEP.m.machine) || {};
  const part = (title, rows) => rows.filter(Boolean).length
    ? '<h3 class="secTitle">' + title + '</h3><div class="dgrid">' + rows.filter(Boolean).join('') + '</div>'
    : '';
  let s = '';
  if (h && h.cpu) {
    s += part('硬件（读注册表，毫秒级）', [
      kv('处理器', h.cpu),
      kv('主频 / 核心', (h.cpu_mhz ? (h.cpu_mhz / 1000).toFixed(2) + ' GHz · ' : '')
        + (h.cores || '?') + ' 个逻辑处理器'),
      kv('显卡', (h.gpus || []).join('   ') || '—'),
      kv('整机', h.system_model || '—'),
      kv('主板', h.baseboard || '—'),
      kv('BIOS', h.bios || '—'),
      kv('内存', h.ram_total_gb ? h.ram_total_gb.toFixed(1) + ' GB' : '—'),
      kv('屏幕', h.screen || '—'),
      kv('磁盘', (h.disks || []).map((d) => d.drive + ' 剩余 ' + d.free_gb.toFixed(0)
        + '/' + d.total_gb.toFixed(0) + ' GB').join('   ')),
      ((h.ram || []).length
        ? kv('内存条', h.ram.map((r) => (r.size_mb / 1024).toFixed(0) + 'GB ' + r.kind
            + (r.speed ? ' ' + r.speed + 'MT/s' : '')
            + (r.maker ? ' ' + r.maker : '') + (r.part ? ' ' + r.part : '')).join('   '))
        : ''),
      ((h.disks_model || []).length ? kv('硬盘型号', h.disks_model.join('   ')) : ''),
    ]);
  } else {
    s += part('硬件', [
      kv('处理器', m.cpu || '—'),
      kv('核心', m.cores ? m.cores + ' 个逻辑处理器' : '—'),
      kv('内存', m.ram_total_mb ? (m.ram_total_mb / 1024).toFixed(1) + ' GB' : '—'),
      kv('磁盘', (m.disks || []).map((d) => d.drive + ' 剩余 ' + d.free_gb.toFixed(0)
        + '/' + d.total_gb.toFixed(0) + ' GB').join('   ')),
    ]);
  }
  s += part('系统', [
    kv('版本', (m.os_name || '') + ' ' + (m.os_version || '') + '  build ' + (m.os_build || '')),
    kv('注册表原文', m.os_product || '—'),
  ]);
  const d = HW.deep;
  if (d && d.cpu) {
    const c = d.cpu || {};
    s += part('再深一层（WMI 读到的）', [
      kv('内存条', (d.ram || []).map((r) => (r.gb || '?') + 'GB ' + (r.mhz || '?')
        + 'MHz ' + [r.maker, r.part].filter(Boolean).join(' ')).join('   ') || '—'),
      kv('硬盘型号', (d.disk || []).map((x) => (x.model || '?') + ' ' + (x.gb || '?')
        + 'GB ' + (x.bus || '')).join('   ') || '—'),
      kv('已安装软件', d.programs == null ? '—' : d.programs + ' 个'),
      kv('BIOS 版本', (d.bios && d.bios.ver) || '—'),
    ]);
  }
  $('#hwBody').innerHTML = s;
}

// ── 编辑器：工作副本 ─────────────────────────────────────────
// 规则：改动留在输入框里，点「应用」才写文件；应用前自动留一份快照，能回滚。
const ED = { files: [], path: null, disk: '', text: '', dirty: false, busy: false,
  closedDirs: new Set(), guideN: -1, tabs: [], charW: 0 };

async function edLoadFiles() {
  if (!S.root) return;
  await guard(async () => {
    const r = await invoke('list_files', { root: S.root });
    ED.files = r.files || [];
    edPaintFiles();
  });
}

function edPaintFiles() {
  const box = $('#edFiles');
  if (!box) return;
  const f = ($('#edFilter') ? $('#edFilter').value : '').trim().toLowerCase();
  const list = f ? ED.files.filter((x) => x.toLowerCase().includes(f)) : ED.files;
  if (!ED.files.length) {
    box.innerHTML = '<div class="hint" style="padding:10px">这个项目里没找到可编辑的源码或配置文件。</div>';
    return;
  }
  // 树化：按 / 分层，文件夹可折叠（像 IDEA 的项目树那样）
  const root = { dirs: new Map(), files: [] };
  list.slice(0, 400).forEach((p) => {
    const parts = p.split('/');
    let node = root;
    for (let i = 0; i < parts.length - 1; i++) {
      const d = parts[i];
      if (!node.dirs.has(d)) node.dirs.set(d, { dirs: new Map(), files: [] });
      node = node.dirs.get(d);
    }
    node.files.push({ name: parts[parts.length - 1], path: p });
  });
  const icon = (name) => {
    const e = name.split('.').pop().toLowerCase();
    // 文件类型角标：不画小方块（方块说明不了任何事），
    // 用「底色 + 一两个字母」的徽标——一眼认得出，也让整列有节奏。
    if (e === 'rs') return ['rs', 'RS'];
    if (e === 'py') return ['py', 'PY'];
    if (['js', 'mjs', 'cjs'].includes(e)) return ['js', 'JS'];
    if (['ts', 'tsx'].includes(e)) return ['js', 'TS'];
    if (['jsx', 'vue'].includes(e)) return ['js', 'UI'];
    if (['java', 'kt'].includes(e)) return ['jv', 'JV'];
    if (['c', 'h'].includes(e)) return ['cc', 'C'];
    if (['cpp', 'hpp', 'cc', 'cs'].includes(e)) return ['cc', 'C+'];
    if (['html', 'htm', 'xml'].includes(e)) return ['ht', '<>'];
    if (['css', 'scss', 'less'].includes(e)) return ['cs', '##'];
    if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'ico', 'webp', 'bmp'].includes(e)) return ['im', '▣'];
    if (['json', 'toml', 'yaml', 'yml', 'ini', 'cfg', 'properties', 'lock'].includes(e)) return ['cf', '{}'];
    if (name.startsWith('.')) return ['cf', '·'];
    if (['md', 'txt'].includes(e)) return ['md', 'MD'];
    return ['fx', '·'];
  };
  const badge = (name) => {
    const [k, t] = icon(name);
    return '<span class="ico ' + k + '">' + esc(t) + '</span>';
  };
  const render = (node, depth) => {
    let html = '';
    // 文件夹在前、文件在后，各自按名字排——IDEA 的次序，扫起来不用找。
    const dirs = Array.from(node.dirs.entries()).sort((a, b) => a[0].localeCompare(b[0]));
    for (const [d, child] of dirs) {
      const key = depth + ':' + d;
      // 默认全展开（IDEA 的项目视图就是这样），用户折叠过哪一层就听用户的。
      // 只记「折叠了哪些」而不是「展开了哪些」——展开的集合会随着目录变化失效。
      const open = !ED.closedDirs.has(key);
      const count = child.files.length + child.dirs.size;
      html += '<div class="ed-node dir' + (open ? ' open' : '') + '" data-dir="' + esc(key) + '"'
        + ' style="--d:' + depth + '">'
        + '<span class="chev">' + (open ? '▾' : '▸') + '</span><span class="ico d"></span>'
        + '<span class="nm">' + esc(d) + '</span><span class="cnt">' + count + '</span></div>';
      if (open) html += render(child, depth + 1);
    }
    for (const file of node.files.slice().sort((a, b) => a.name.localeCompare(b.name))) {
      html += '<div class="ed-node file' + (ED.path === file.path ? ' on' : '') + '"'
        + ' data-file="' + esc(file.path) + '" style="--d:' + depth + '"'
        + ' data-tip="' + esc(file.path) + '">'
        + '<span class="chev"></span>' + badge(file.name)
        + '<span class="nm">' + esc(file.name) + '</span></div>';
    }
    return html;
  };
  box.innerHTML = render(root, 0);
}

// ── 多文件标签：IDEA 的那一条 ──────────────────────────────
// 开着几个文件就摆几个。切文件不用回树里一层层找——写代码时最烦的就是这个。
function edTabsPaint() {
  const box = $('#edTabs');
  if (!box) return;
  box.innerHTML = (ED.tabs || []).map((p) => {
    const name = p.split('/').pop();
    return '<button class="etab' + (p === ED.path ? ' on' : '') + '" data-etab="' + esc(p) + '"'
      + ' data-tip="' + esc(p) + '">'
      + '<span class="nm">' + esc(name) + '</span>'
      + '<span class="x" data-ex="' + esc(p) + '">✕</span></button>';
  }).join('');
}

function edTabAdd(path) {
  if (!ED.tabs) ED.tabs = [];
  if (!ED.tabs.includes(path)) ED.tabs.push(path);
  if (ED.tabs.length > 12) ED.tabs = ED.tabs.slice(-12);
  edTabsPaint();
}

/// 关闭带未应用改动的标签：和切换文件同一套提示（不弹窗、不阻塞）
function edWarnDirtyClose(then) {
  const box = $('#edFiles');
  if (!box) return;
  const old = $('#edDirtyBar');
  if (old) old.remove();
  const bar = document.createElement('div');
  bar.id = 'edDirtyBar';
  bar.className = 'ed-dirtybar';
  bar.innerHTML = '<span>这个文件有没应用的改动。</span>'
    + '<button class="btn sm" data-db="apply">先应用再关</button>'
    + '<button class="btn ghost sm" data-db="drop">丢弃并关掉</button>'
    + '<button class="btn ghost sm" data-db="stay">留在这里</button>';
  box.prepend(bar);
  bar.addEventListener('click', (e) => {
    const a = e.target.closest('button')?.dataset.db;
    if (!a) return;
    bar.remove();
    if (a === 'apply') { edApply().then(() => then && then()); return; }
    if (a === 'drop') {
      ED.dirty = false;
      ED.text = ED.disk;
      $('#edText').value = ED.disk;
      $('#edDirty').hidden = true;
      if (then) then();
    }
  });
}

function edTabClose(path) {
  const go = () => {
    ED.tabs = (ED.tabs || []).filter((p) => p !== path);
    if (ED.path === path) {
      const next = ED.tabs[ED.tabs.length - 1];
      if (next) { edOpen(next); return; }
      ED.path = null; ED.disk = ''; ED.text = ''; ED.dirty = false;
      const t = $('#edText');
      if (t) { t.value = ''; t.disabled = true; }
      if ($('#edPath')) $('#edPath').textContent = '还没有打开文件';
      if ($('#edCode')) $('#edCode').innerHTML = '';
      if ($('#edCrumb')) { $('#edCrumb').hidden = true; $('#edCrumb').innerHTML = ''; }
      if ($('#edHl')) $('#edHl').hidden = true;
      if ($('#edGl')) $('#edGl').hidden = true;
      if ($('#edBrkA')) $('#edBrkA').hidden = true;
      if ($('#edBrkB')) $('#edBrkB').hidden = true;
    }
    edTabsPaint();
  };
  if (ED.path === path && ED.dirty) { edWarnDirtyClose(go); return; }
  go();
}

// ── 找 + 跳行：一个栏两种模式（Ctrl+F / Ctrl+G）───────────
// 高亮用的是「真的选中那段文字」：这是 textarea 上最诚实也最稳的做法，
// 不用去猜字宽、也不用把高亮层再切碎一次。
const FIND = { mode: 'find' };

function edFindShow(mode) {
  const bar = $('#edFind');
  const inp = $('#edFindInput');
  if (!bar || !inp) return;
  FIND.mode = mode || 'find';
  bar.hidden = false;
  inp.placeholder = FIND.mode === 'line' ? '跳到第几行' : '查找…';
  inp.value = '';
  inp.focus();
  if ($('#edFindCnt')) $('#edFindCnt').textContent = '';
}

function edFindHide() {
  const bar = $('#edFind');
  if (bar) bar.hidden = true;
  const inp = $('#edFindInput');
  if (inp) inp.blur();
  $('#edText')?.focus();
}

/// 把光标挪到某个位置，并把那一行滚进视野
function edScrollTo(pos) {
  const t = $('#edText');
  if (!t) return;
  const line = (t.value.slice(0, pos).match(/\n/g) || []).length + 1;
  const want = (line - 3) * ED_LINE_H;
  t.scrollTop = Math.max(0, Math.min(want, t.scrollHeight - t.clientHeight));
  $('#edGutter').scrollTop = t.scrollTop;
}

function edFindGo(dir) {
  const t = $('#edText');
  const inp = $('#edFindInput');
  const cnt = $('#edFindCnt');
  if (!t || !inp) return;
  const q = inp.value;
  if (FIND.mode === 'line') {
    const total = (t.value.match(/\n/g) || []).length + 1;
    const n = Math.max(1, Math.min(parseInt(q, 10) || 1, total));
    const lines = t.value.split('\n');
    let pos = 0;
    for (let i = 0; i < n - 1; i++) pos += lines[i].length + 1;
    t.focus();
    t.selectionStart = t.selectionEnd = pos;
    edScrollTo(pos);
    edPaintCaret();
    if (cnt) cnt.textContent = n + ' / ' + total;
    return;
  }
  if (!q) { if (cnt) cnt.textContent = ''; return; }
  const src = t.value.toLowerCase();
  const needle = q.toLowerCase();
  const hits = [];
  let i = src.indexOf(needle);
  while (i >= 0 && hits.length < 2000) { hits.push(i); i = src.indexOf(needle, i + Math.max(1, needle.length)); }
  if (!hits.length) { if (cnt) cnt.textContent = '0 处'; sfx('bad'); return; }
  const cur = t.selectionStart;
  let at;
  if (dir > 0) at = hits.find((h) => h > cur);
  else at = [...hits].reverse().find((h) => h < cur);
  if (at == null) at = dir > 0 ? hits[0] : hits[hits.length - 1];
  const idx = hits.indexOf(at) + 1;
  t.focus();
  t.selectionStart = at;
  t.selectionEnd = at + q.length;
  edScrollTo(at);
  edPaintCaret();
  if (cnt) cnt.textContent = idx + ' / ' + hits.length;
}

// ── 配对括号：光标贴着括号时，把这一对框出来 ────────────────
const BR_OPEN = '([{';
const BR_CLOSE = ')]}';

function edCharW() {
  if (ED.charW) return ED.charW;
  const el = $('#edCode');
  if (!el) return 7.5;
  const s = document.createElement('span');
  s.textContent = '0'.repeat(50);
  s.style.cssText = 'position:absolute;visibility:hidden;white-space:pre';
  el.appendChild(s);
  ED.charW = s.getBoundingClientRect().width / 50;
  s.remove();
  return ED.charW;
}

function edPartner(src, at, ch) {
  const i = BR_OPEN.indexOf(ch);
  let depth = 0;
  if (i >= 0) {
    for (let k = at; k < src.length; k++) {
      if (src[k] === ch) depth++;
      else if (src[k] === BR_CLOSE[i]) { depth--; if (!depth) return k; }
    }
  } else {
    const o = BR_OPEN[BR_CLOSE.indexOf(ch)];
    for (let k = at; k >= 0; k--) {
      if (src[k] === ch) depth++;
      else if (src[k] === o) { depth--; if (!depth) return k; }
    }
  }
  return -1;
}

function edPaintBrackets() {
  const t = $('#edText');
  const a = $('#edBrkA');
  const b = $('#edBrkB');
  if (!t || !a || !b) return;
  const src = t.value;
  const pos = t.selectionStart;
  const put = (box, at, ch) => {
    const before = src.slice(0, at);
    const line = (before.match(/\n/g) || []).length + 1;
    const ls = before.lastIndexOf('\n') + 1;
    const col = at - ls;
    box.hidden = false;
    box.style.left = (ED_PAD_L + col * edCharW()) + 'px';
    box.style.transform = 'translateY(' + (ED_PAD_T + (line - 1) * ED_LINE_H - t.scrollTop) + 'px)';
  };
  for (const p of [pos, pos - 1]) {
    if (p < 0 || p >= src.length) continue;
    const ch = src[p];
    if (BR_OPEN.indexOf(ch) < 0 && BR_CLOSE.indexOf(ch) < 0) continue;
    const mate = edPartner(src, p, ch);
    if (mate < 0) break;
    put(a, p, ch);
    put(b, mate, src[mate]);
    return;
  }
  a.hidden = true;
  b.hidden = true;
}

// ── 重排缩进 ───────────────────────────────────────────────
function edFormat() {
  const t = $('#edText');
  if (!t || t.disabled || !ED.path) return;
  const before = t.value;
  const after = edReindent(before, ED.path);
  if (after === before) { toast('这一版缩进已经是齐的'); return; }
  t.value = after;
  edMarkDirty();
  edPaintGutter();
  edPaintStat();
  sfx('ok');
  feedback('info', '重排了缩进', esc(ED.path) + '：只按括号层级重排。'
    + '字符串和注释里的括号不参与判断，所以它只是「参考缩进」——不满意点「回滚」，'
    + '应用前那一版留着。');
}

// 边界写在这儿：括号语言按「嵌套层数」重排；Python 只把缩进宽度归一，
// 不重排层级（层级得靠解释器解析，猜错了就是重构事故）。改错能回滚。
function edReindent(text, path) {
  const ind = ' '.repeat(SET.indent || 4);
  const ext = (path || '').split('.').pop().toLowerCase();
  const lines = text.split('\n');
  if (ext === 'py') {
    return lines.map((l) => {
      const m = l.match(/^[ \t]+/);
      if (!m) return l;
      const w = m[0].replace(/\t/g, ind).length;
      return ind.repeat(Math.max(0, Math.round(w / (SET.indent || 4)))) + l.slice(m[0].length);
    }).join('\n');
  }
  let depth = 0;
  return lines.map((raw) => {
    const t = raw.trim();
    if (!t) return '';
    const startsClose = BR_CLOSE.indexOf(t[0]) >= 0;
    const d = Math.max(0, startsClose ? depth - 1 : depth);
    // 数括号：不解析字符串和注释，所以它只是"参考缩进"。留了版本就能回滚。
    for (const ch of t) {
      if (BR_OPEN.indexOf(ch) >= 0) depth++;
      else if (BR_CLOSE.indexOf(ch) >= 0) depth = Math.max(0, depth - 1);
    }
    return ind.repeat(d) + t;
  }).join('\n');
}

/// 面包屑：路径按层切开。新人看着一堆同名文件时，这一条告诉他「你在哪」。
function edPaintCrumb(path) {
  const box = $('#edCrumb');
  if (!box) return;
  if (!path) { box.hidden = true; box.innerHTML = ''; return; }
  box.hidden = false;
  const parts = path.split('/');
  box.innerHTML = parts.map((p, i) => (i ? '<span class="sep">›</span>' : '')
    + '<button class="crumb' + (i === parts.length - 1 ? ' last' : '') + '"'
    + ' data-crumb="' + esc(parts.slice(0, i + 1).join('/')) + '">' + esc(p) + '</button>').join('');
}

async function edOpen(path) {
  if (!S.root || ED.busy) return;
  // 有未应用的改动时不再弹 confirm（WebView 里的 confirm 会卡住整条交互链），
  // 改成就地提示 + 两个按钮，切不切换由用户点。
  if (ED.dirty) { edWarnDirty(path); return; }
  ED.busy = true;
  try {
    const o = await invoke('open_file', { root: S.root, path });
    ED.path = path;
    ED.disk = o.text || '';
    ED.text = o.text || '';
    ED.dirty = false;
    // 记住这个文件本来的编码与换行：写回时要照原样（不然就是悄悄改用户的文件）
    ED.encoding = o.encoding || 'utf-8';
    ED.eol = o.eol || 'lf';
    if (o.note) toast(o.note, !o.readable);
    edPaintFoot();
    const t = $('#edText');
    t.value = o.readable ? ED.text : '';
    t.disabled = !o.readable;
    t.placeholder = o.readable ? '改动先留在内存里，点「应用」才写回磁盘。' : (o.note || '这个文件不能编辑');
    $('#edPath').textContent = path;
    ['#btnEdApply', '#btnEdMore', '#btnEdBuild', '#btnEdTest', '#btnEdRun']
      .forEach((s) => { const el = $(s); if (el) el.disabled = !o.readable; });
    edPaintGutter();
    edPaintStat();
    edPaintFiles();
    edPaintCrumb(path);
    edTabAdd(path);
    if (o.drafts) edDrafts = o.drafts;
  } catch (e) {
    toast(typeof e === 'string' ? e : (e && e.message) || String(e), true);
  } finally {
    ED.busy = false;
  }
}

/// 未应用改动的提示条（不阻塞、不弹窗）
function edWarnDirty(nextPath) {
  const box = $('#edFiles');
  if (!box) return;
  const old = $('#edDirtyBar');
  if (old) old.remove();
  const bar = document.createElement('div');
  bar.id = 'edDirtyBar';
  bar.className = 'ed-dirtybar';
  bar.innerHTML = '<span>这个文件有没应用的改动。</span>'
    + '<button class="btn sm" data-db="apply">先应用</button>'
    + '<button class="btn ghost sm" data-db="drop">丢弃并切换</button>'
    + '<button class="btn ghost sm" data-db="stay">留在这里</button>';
  box.prepend(bar);
  bar.addEventListener('click', (e) => {
    const a = e.target.closest('button')?.dataset.db;
    if (!a) return;
    bar.remove();
    if (a === 'apply') edApply().then(() => { if (nextPath) edOpen(nextPath); });
    else if (a === 'drop') { ED.dirty = false; ED.text = ED.disk; $('#edText').value = ED.disk; $('#edDirty').hidden = true; edOpen(nextPath); }
  });
}

let edDrafts = [];

// ── 语法高亮：按语言给关键词表，一个正则走一遍就行（不引库）──
const SY = {
  rs: { kw: 'fn let mut pub use struct enum impl match if else for while loop return where trait mod crate self super as async await dyn move ref static const unsafe in break continue type', hash: false, typ: 'u8 u16 u32 u64 usize i8 i16 i32 i64 isize f32 f64 bool char str String Vec Option Result Box Self' },
  py: { kw: 'def class return if elif else for while import from as with try except finally raise lambda yield pass break continue global nonlocal assert del in is not and or None True False async await', hash: true, typ: 'int float str bool list dict set tuple bytes object' },
  js: { kw: 'function const let var return if else for while class extends new import from export default async await try catch finally throw typeof instanceof of in this super null undefined true false', hash: false, typ: 'number string boolean object any void Promise Array' },
  ts: { kw: 'function const let var return if else for while class extends implements interface type new import from export default async await try catch finally throw typeof instanceof of in this super null undefined true false readonly public private enum', hash: false, typ: 'number string boolean object any void Promise Array Record' },
  java: { kw: 'class interface enum extends implements public private protected static final void new return if else for while try catch finally throw throws import package this super null true false abstract synchronized', hash: false, typ: 'int long short byte char boolean float double String Object List Map Set' },
  c: { kw: 'int char long short float double void struct union enum typedef static const extern return if else for while do switch case break continue sizeof goto unsigned signed', hash: false, typ: 'size_t uint8_t int32_t bool' },
  sh: { kw: 'if then else fi for do done while case esac function return export local echo cd set source', hash: true, typ: '' },
  toml: { kw: 'true false', hash: true, typ: '' },
  yml: { kw: 'true false null', hash: true, typ: '' },
};

function syLex(ext) {
  const s = SY[ext] || null;
  return s;
}

function highlightCode(src, path) {
  const ext = (path || '').split('.').pop().toLowerCase();
  const lang = syLex(ext);
  const kws = new Set(((lang && lang.kw) || '').split(' ').filter(Boolean));
  const typs = new Set(((lang && lang.typ) || '').split(' ').filter(Boolean));
  const hashComment = !lang || lang.hash !== false;
  const slashComment = !lang || lang.hash === false;
  let out = '';
  let i = 0;
  const push = (cls, text) => { out += cls ? '<span class="' + cls + '">' + esc(text) + '</span>' : esc(text); };
  while (i < src.length) {
    const rest = src.slice(i);
    // 注释
    const block = rest.match(/^\/\*[\s\S]*?\*\//);
    if (block) { push('sy-com', block[0]); i += block[0].length; continue; }
    const line = (slashComment && rest.startsWith('//')) || (hashComment && rest[0] === '#');
    if (line) {
      const nl = rest.indexOf('\n');
      const text = nl < 0 ? rest : rest.slice(0, nl);
      push('sy-com', text);
      i += text.length;
      continue;
    }
    // 字符串
    const str = rest.match(/^("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`)/);
    if (str) { push('sy-str', str[0]); i += str[0].length; continue; }
    // 数字
    const num = rest.match(/^\d[\d_]*(\.\d+)?([eE][+-]?\d+)?/);
    if (num) { push('sy-num', num[0]); i += num[0].length; continue; }
    // 标识符
    const id = rest.match(/^[A-Za-z_$][\w$]*/);
    if (id) {
      const w = id[0];
      const isCall = /^\s*\(/.test(rest.slice(w.length));
      push(kws.has(w) ? 'sy-kw' : (typs.has(w) ? 'sy-typ' : (isCall ? 'sy-fn' : '')), w);
      i += w.length;
      continue;
    }
    push('', src[i]);
    i += 1;
  }
  return out;
}

// 只给「看得见的那几十行」上色。
//
// 为什么要窗口化：3000 行的文件全文上色要 65ms，而这个函数原来挂在
// 每次按键上——打一个字卡一下，就是"用起来变卡"的真正原因。
// 现在只算可见窗口（首行-2 到末行+8），文件多大都是 2–4ms。
// 对齐方式：给 <pre> 一个 (14 - scrollTop) 的位移，再用一个高度 =
// 首行×20 的占位块把第一行推到它该在的位置。
const ED_VIEW = { first: 0, last: 0 };
function edVisibleRange() {
  const t = $('#edText');
  if (!t) return { first: 1, last: 1 };
  const per = ED_LINE_H;
  const first = Math.max(1, Math.floor(Math.max(0, t.scrollTop) / per) + 1 - 2);
  const visible = Math.ceil(t.clientHeight / per) + 4;
  return { first, last: first + visible + 8 };
}
function edPaintCode() {
  const pre = $('#edCode');
  const t = $('#edText');
  if (!pre || !t) return;
  const lines = t.value.split('\n');
  const { first, last } = edVisibleRange();
  ED_VIEW.first = first;
  ED_VIEW.last = last;
  const slice = lines.slice(first - 1, last).join('\n');
  // 占位块负责"上面还有多少行"，这样滚动时文字不会跑位
  pre.style.paddingTop = '0px';
  pre.style.transform = 'translateY(' + (ED_PAD_T - t.scrollTop) + 'px)';
  pre.innerHTML = '<i style="display:block;height:' + ((first - 1) * ED_LINE_H) + 'px"></i>'
    + highlightCode(slice, ED.path || '') + '\n';
}

function edMarkDirty() {
  ED.text = $('#edText').value;
  ED.dirty = ED.text !== ED.disk;
  const d = $('#edDirty');
  if (d) d.hidden = !ED.dirty;
  // 便宜的三件事立刻做：脏标记、状态栏、光标位置。
  // 贵的那件（重画行号 + 重新上色）交给 edPaintHeavySoon 去排队——
  // 这一步就是"打字跟手"和"打一个字卡一下"的分界。
  if (edHeavyTimer === null) edPaintCaret();
  edPaintStat();
  edPaintHeavySoon();
}

function edPaintGutter(force) {
  const g = $('#edNums');
  const wrap = $('#edGutter');
  if (!g || !wrap) return;
  const lines = ($('#edText').value.match(/\n/g) || []).length + 1;
  // 行数没变就不要重画行号：3000 行拼 innerHTML 要 130ms，
  // 而打字时绝大多数按键都不改变行数（只有回车才会）。
  if (force || ED.gutterLines !== lines) {
    let h = '';
    for (let i = 1; i <= lines; i++) h += i + '<br>';
    g.innerHTML = h;
    ED.gutterLines = lines;
  }
  wrap.scrollTop = $('#edText').scrollTop;
  edPaintCode();
  edPaintGuides();
  edPaintCaret();
}

/// 重活：高亮 + 行号 + 缩进线。只在"停下来"之后跑一次。
/// 打字时只更新状态（脏标记、行列、当前行），手感才跟得上手。
let edHeavyTimer = null;
function edPaintHeavySoon() {
  clearTimeout(edHeavyTimer);
  edHeavyTimer = setTimeout(() => {
    edHeavyTimer = null;
    edPaintGutter(false);
  }, 110);
}

/// 光标在第几行第几列。所有坐标类的东西（当前行、缩进线、状态栏）都用它。
function edCaretPos() {
  const t = $('#edText');
  if (!t) return { line: 1, col: 1 };
  const upto = t.value.slice(0, t.selectionStart);
  const nl = upto.lastIndexOf('\n');
  return { line: (upto.match(/\n/g) || []).length + 1, col: upto.length - nl };
}

const ED_LINE_H = 20;   // 与 CSS 的 line-height 一致，改一处要改两处
const ED_PAD_T = 14;    // 与 CSS 的 padding-top 一致
const ED_PAD_L = 16;    // 与 CSS 的 padding-left 一致

function edPaintCaret() {
  const t = $('#edText');
  if (!t) return;
  const { line, col } = edCaretPos();
  const lc = $('#edLnCol');
  if (lc) lc.textContent = '第 ' + line + ' 行，第 ' + col + ' 列';
  const top = (line - 1) * ED_LINE_H;
  const hl = $('#edHl');
  if (hl) {
    hl.hidden = false;
    hl.style.transform = 'translateY(' + (ED_PAD_T + top - t.scrollTop) + 'px)';
  }
  const gl = $('#edGl');
  if (gl) {
    gl.hidden = false;
    gl.style.top = (ED_PAD_T + top) + 'px';
  }
  edPaintBrackets();
}

/// 缩进参考线：有几层缩进画几条竖线。深嵌套的代码靠它对齐，不用数空格。
function edPaintGuides() {
  const box = $('#edGuides');
  const t = $('#edText');
  if (!box || !t) return;
  const ind = SET.indent || 4;
  let max = 0;
  const lines = t.value.split('\n', 4000);
  for (const line of lines) {
    const m = line.match(/^[ \t]*/)[0].replace(/\t/g, ' '.repeat(ind)).length;
    if (m) max = Math.max(max, Math.floor(m / ind));
    if (max >= 12) break;
  }
  const n = Math.min(max, 12);
  if (ED.guideN === n) return;             // 深度没变就不动 DOM
  ED.guideN = n;
  let h = '';
  for (let i = 1; i <= n; i++) h += '<i style="left:calc(16px + ' + (i * ind) + 'ch)"></i>';
  box.innerHTML = h;
}

/// 状态栏右边：这个文件是什么编码、什么换行。
/// 放出来是因为"悄悄改"最伤人——写清楚之后，用户自己就知道该不该动它。
function edPaintFoot() {
  const el = $('#edEnc');
  if (!el) return;
  if (!ED.path) { el.textContent = ''; return; }
  const enc = (ED.encoding || 'utf-8').toUpperCase();
  const eol = (ED.eol || 'lf') === 'crlf' ? 'CRLF' : 'LF';
  el.textContent = enc + ' · ' + eol;
  el.dataset.tip = (ED.encoding === 'gbk'
    ? '这个文件不是 UTF-8（' + enc + '），保存时会按原编码写回，不会改掉编码'
    : '按原编码与原换行保存') + ' · ' + eol;
}

function edPaintStat() {
  const t = $('#edText');
  const el = $('#edStat');
  if (!el) return;
  const lines = (t.value.match(/\n/g) || []).length + 1;
  el.textContent = lines + ' 行 · ' + t.value.length + ' 字符'
    + (ED.dirty ? ' · 与磁盘不一致（点「应用」写回）' : ' · 与磁盘一致')
    + (edDrafts.length ? ' · 有 ' + edDrafts.length + ' 个历史版本' : '');
}

async function edApply() {
  if (!S.root || !ED.path || !ED.dirty) return;
  const t0 = performance.now();
  await guard(async () => {
    const r = await invoke('apply_edit', {
      root: S.root, path: ED.path, text: $('#edText').value,
      encoding: ED.encoding || 'utf-8', eol: ED.eol || 'lf',
    });
    ED.disk = $('#edText').value;
    ED.dirty = false;
    if (r.opened) edDrafts = r.opened.drafts || edDrafts;
    $('#edDirty').hidden = true;
    edPaintStat();
    $('#edPath').animate?.([{ opacity: .4 }, { opacity: 1 }], { duration: 220 });
    sfx('ok');
    feedbackTimed(t0, 'info', '已应用到文件',
      esc(ED.path) + '（应用前那一版留在 <code>.jingwei/drafts/</code>，随时能回滚）');
    autoRecord('改了 ' + ED.path, '在编辑器里应用了一次改动。\n\n应用前的版本已留快照。');
    refreshQuiet(true);
  });
}

async function edSnapshot() {
  if (!S.root || !ED.path) return;
  await guard(async () => {
    await invoke('snapshot_file', { root: S.root, path: ED.path });
    const o = await invoke('open_file', { root: S.root, path: ED.path });
    edDrafts = o.drafts || [];
    edPaintStat();
    sfx('click');
    toast('留了一版');
  });
}

/// 工具条上的「更多」：版本与格式这一类低频动作收在这里。
/// 一行七个按钮是"工程师摆的"——设计师会让主线上只留一个动作。
function edMoreMenu(anchor) {
  document.querySelector(".ed-menu")?.remove();
  const m = document.createElement("div");
  m.className = "ed-menu ed-menu-sm";
  m.innerHTML = '<button data-more="snap" ' + (ED.path ? "" : "disabled") + '>存为版本<small>当前内容留一份，可随时回来</small></button>'
    + '<button data-more="roll" ' + (edDrafts.length ? "" : "disabled") + '>回滚到某个版本<small>'
    + (edDrafts.length ? edDrafts.length + " 个历史版本" : "还没有历史版本") + "</small></button>"
    + '<button data-more="format" ' + (ED.path ? "" : "disabled") + '>重排缩进<small>按括号层级对齐（可回滚）</small></button>';
  const r = anchor.getBoundingClientRect();
  m.style.left = Math.max(8, Math.min(r.left, window.innerWidth - 240)) + "px";
  m.style.top = (r.bottom + 6) + "px";
  document.body.appendChild(m);
  const close = () => m.remove();
  setTimeout(() => document.addEventListener("click", close, { once: true }), 0);
  m.addEventListener("click", (e) => {
    const b = e.target.closest("button[data-more]");
    if (!b || b.disabled) return;
    const k = b.dataset.more;
    close();
    if (k === "snap") edSnapshot();
    else if (k === "roll") edRollMenu(anchor);
    else edFormat();
  });
}

function edRollMenu(anchor) {
  document.querySelector('.ed-menu')?.remove();
  if (!edDrafts.length) { toast('这个文件还没有历史版本'); return; }
  const m = document.createElement('div');
  m.className = 'ed-menu';
  m.innerHTML = edDrafts.slice(0, 20).map((d) => '<button data-draft="' + esc(d.id) + '">'
    + esc(d.id) + ' · ' + Math.round((d.bytes || 0) / 1024) + ' KB</button>').join('');
  const r = anchor.getBoundingClientRect();
  m.style.left = Math.max(8, r.left - 120) + 'px';
  m.style.top = (r.bottom + 6) + 'px';
  document.body.appendChild(m);
  const close = () => m.remove();
  setTimeout(() => document.addEventListener('click', close, { once: true }), 0);
  m.addEventListener('click', (e) => {
    const b = e.target.closest('button[data-draft]');
    if (!b) return;
    close();
    guard(async () => {
      const o = await invoke('restore_draft', { root: S.root, path: ED.path, id: b.dataset.draft });
      ED.disk = o.text || '';
      ED.text = ED.disk;
      ED.dirty = false;
      $('#edText').value = ED.text;
      $('#edDirty').hidden = true;
      edDrafts = o.drafts || [];
      edPaintGutter(); edPaintStat();
      sfx('ok');
      feedback('info', '已回滚到 ' + b.dataset.draft, '回滚前的版本也留着了，再滚回去也行。');
      autoRecord('回滚了 ' + ED.path, '回到版本 ' + b.dataset.draft);
      refreshQuiet(true);
    });
  });
}

/// 只改「哪个是当前项目」的高亮，不重建任何节点。
function paintProjectBanners() {
  ['#projList', '#nbProjects'].forEach((sel) => {
    const box = $(sel);
    if (!box) return;
    for (const el of Array.from(box.children)) {
      const p = el.dataset && (el.dataset.path || el.dataset.npath);
      if (p) el.classList.toggle('on', bannerOn(p));
    }
  });
}

