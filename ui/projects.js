// 项目那一摊：选中之后的概览、卡片下方的小字、变更卡、项目检查、
// 启动（卡片会留下）、一条龙流水线、清理。
//
// 它们是"这个项目现在怎么样、下一步做什么"的答案，所以放在一起。
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
/// **这个目录是不是"装着好几个项目"**（容器）。
///
/// 抽成一个函数，是因为这条判据有**三个消费点**：
///   `renderHome`（首屏）、`renderCardNotes`（小字）、`renderChanges`（变更卡）。
/// 三处各写一遍就会漏——**同一个坑已经漏过一次**：`renderHome` 的提前 return
/// 只挡住了首次渲染，后台刷新绕过去，真窗口里就漏出了「怎么跑：还没有契约」。
function isContainerDir(d) {
  return ((((d || {}).hygiene || {}).items) || [])
    .some((i) => i.id === 'projects-dir-not-a-project');
}

/// **一句话指南**：项目是什么 / 能不能跑 / 卡在哪 / 下一步敲什么。
///
/// ## 为什么它占主位，状态卡退到它下面
///
/// 人打开一个项目时问的是**一个问题**（"我要怎么把它跑起来？"），
/// 而状态卡回答的是**一个统计**（"有 3 件事值得看一眼"）。
/// 统计不出错，但它**把排序工作推回给了用户**——而排序正是工具该替他做的。
///
/// 数据与 `jingwei guide` **同源**（引擎的 `Guide::to_json()`，两处不可能各说各的）。
///
/// ## 三态在这儿的样子
///
/// ✓ 能跑（绿）· ✗ 跑不起来（红）· **? 还查不出来（黄）**
/// **"查不了"不用红色**——红色是"出错了"，而"我们没查过"不是他的错。
/// 第三支带 `whatToDo`（**缺什么、谁来补**）：那是**归属**，
/// 界面只准转述、不准自己判断（SOP §S16）。
function renderGuide() {
  const el = $('#guideCard');
  if (!el) return;
  const g = (S.data || {}).guide;
  if (!g) { el.innerHTML = ''; return; }
  const r = g.ready || {};
  const face = r.state === 'yes' ? { ch: '✓', cls: 'ok' }
    : r.state === 'no' ? { ch: '✗', cls: 'warn' }
      : { ch: '?', cls: 'ask' };
  // **「不适用」不许说成「还查不出来」**（2026-10-18 修）：引擎的 `ready.why` 里
  // 已经有 `not_applicable` 这个值，而界面原来把所有 unknown 都写成同一句——
  // 那就是三态压扁：**「我们没查到」与「这个问题对它不成立」是两件事**，
  // 用户该做的事也完全不同（前者等我们，后者什么都不用做）。
  const na = r.why === 'not_applicable';
  const head = r.state === 'yes' ? ('现在就能跑 · ' + esc(g.what || ''))
    : r.state === 'no' ? ('现在跑不起来 · ' + esc(g.what || ''))
      : na ? ('这个问题对它不适用 · ' + esc(g.what || ''))
        : ('还查不出来 · ' + esc(g.what || ''));
  let html = '<div class="status ' + face.cls + '"><div class="face">' + face.ch + '</div><div>'
    + '<h2>' + head + '</h2>';
  // 结论一句话：有挡路的就说第一条（"先做这个"），否则说下一步。
  if (g.headline) html += '<p>' + esc(g.headline) + '</p>';
  // **"查不了"要说清是谁的问题**——这一句决定用户该不该动手。
  if (r.state === 'unknown' && r.whatToDo) {
    html += '<p class="gd-why">' + esc(r.whatToDo) + '</p>';
  }
  // 下一步：**能敲的才画成命令**（2026-10-18 修）。
  // 未标记但有源码的目录，`nextStep` 是一句话（「新增一个清单文件…」），
  // 拿 `<code>` 画会把建议装成命令——那是**样式在替内容撒谎**。
  if (g.nextStep) {
    const isCmd = !/[\u4e00-\u9fa5]/.test(g.nextStep);
    html += '<div class="gd-next"><b>下一步</b>'
      + (isCmd ? '<code>' + esc(g.nextStep) + '</code>'
        : '<span class="gd-advice">' + esc(g.nextStep) + '</span>')
      + '</div>';
  }
  // 挡路的逐条：第一条就是"先做这个"
  const bs = g.blockers || [];
  if (bs.length) {
    html += '<div class="gd-blockers"><b>卡在哪</b>'
      + bs.map((b, i) => '<div class="gd-b"><span class="gd-n">' + (i + 1) + '</span>'
        + '<div><div>' + esc(b.what || '') + '</div>'
        + (b.fix ? '<div class="gd-fix">' + esc(b.fix) + '</div>' : '')
        + (b.verify ? '<div class="gd-verify">怎么确认：<code>' + esc(b.verify) + '</code></div>' : '')
        + '</div></div>').join('')
      + '</div>';
  }
  // **"锦上添花"CLI 一直在打，界面原来一个字都不说**（2026-10-18 接上）。
  // 它们不进「卡在哪」——不挡路；但"依赖没确认过""没有 .gitignore"这类事，
  // 桌面用户现在也能看到了。**同一份数据，两个壳必须都讲**。
  const nice = g.niceToHave || [];
  if (nice.length) {
    html += '<div class="gd-nice"><b>顺便说一句</b><ul>'
      + nice.map((s) => '<li>' + esc(s) + '</li>').join('') + '</ul></div>';
  }
  html += '</div></div>';
  el.innerHTML = html;
}

function renderHome() {
  // 指南先画：它占主位，回答"我要怎么把它跑起来"。
  renderGuide();
  const d = S.data;
  const { error: err, warn } = d.counts;
  const card = $('#statusCard');

  // -- 先问一句：**这是一个项目吗？** ------------------------------------
  //
  // 这一段的由来（2026-10-16，真窗口里截出来的）：
  // 打开一个"装着好几个项目"的父目录时，这张卡写着「✓ 环境看着没问题」，
  // 而同一屏下面列着两个不同项目的清单——**一个"项目"的体检里混着两个别的项目**。
  // 那句话**在字面上没错、在用途上是误导**：它把"这个目录没毛病"
  // 说成了"你的项目没问题"，而用户会照它去放心。
  //
  // 所以容器目录要占**主位**：这时候不该出现那张 ✓ 卡。
  const isContainer = isContainerDir(d);

  if (isContainer) {
    card.className = 'status warn';
    card.innerHTML = '<div class="face">?</div><div>'
      + '<h2>这里装着好几个项目，不是一个项目</h2>'
      + '<p>下面那些结论会把它们<b>混在一起</b>看（比如同时列出 Rust 的清单和 Python 的）。'
      + '想看某一个项目的真实状态，在项目列表里直接点它那一行。</p></div>';
    // **容器目录：到这儿为止**（2026-10-16，真窗口里抓出来的第二轮缺陷）。
    //
    // 下面那些东西（事实网格、诊断、变更卡、小字）**都是按"一个项目"的口径**
    // 算出来的。放在容器目录这一屏上，每一条都在把**子项目的事算在父目录头上**——
    // 最典型的一句是「怎么跑：还没有契约」，而契约在子项目里明明有。
    //
    // ⚠️ 只在这里 return 是不够的：`refreshQuiet` 会**绕过 renderHome**
    // 直接调 `renderCardNotes` / `renderChanges`——
    // **同一个坑的第二次**。所以那两处也要各自挡一道（见它们自己的注释）。
    $('#facts').innerHTML = '';
    $('#findings').innerHTML = '<div class="finding info"><div class="ftitle">先选一个项目</div>'
      + '<div class="boundary">这个目录里装着好几个项目。上面那些结论只对"一个项目"成立，'
      + '所以这里不给。在项目列表里点一个，再看它的真实状态。</div></div>';
    $('#cardNotes').innerHTML = '';
    var cc = $('#changeCard');
    if (cc) cc.innerHTML = '';
    renderAudit();
    return;
  } else if (d.guide && d.guide.ready && d.guide.ready.state === 'no') {
    // **同一个屏幕上只能有一个"能不能跑"的判断**（2026-10-17，演示项目上抓到的）。
    //
    // 原来这张卡只读 `d.counts`（诊断条目的 error/warn 数）——
    // 而"依赖没装"在体检里是 `info`。于是 `messy_service` 那一屏写着：
    //   `#guideCard`：✗ 现在跑不起来 · 4 个依赖没装
    //   `#statusCard`：✓ 环境看着没问题 —— 解释器、依赖、契约都对得上
    // **两张卡说的是同一件事，答案相反。**
    //
    // 根子是第 26 轮那个病的另一半：那一轮修的是 `doctor` 的**退出码**要问 `make_guide`，
    // 而界面这张卡**还在自己算**。判据只有一处（`make_guide`），**三个消费者都要问它**。
    card.className = 'status warn';
    card.innerHTML = '<div class="face">✗</div><div><h2>跑不起来</h2>'
      + '<p>' + esc(d.guide.headline || d.guide.what || '') + '</p>'
      + '<p>上面那张指南写了卡在哪、下一步敲什么——两处说法以它为准。</p></div>';
  } else if (err || warn) {
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
    // **这一句原来漏了**（2026-10-17，外壳测试用桩打出来的）：
    // 没有**诊断**（`findings` 里 error/warn）的项目走这条提前 return，
    // 于是下面那行 `renderCardNotes(d)` **一次都没执行**——
    // 换项目时小字就停在**上一个项目**的内容上。
    // 典型场景正是"依赖没装"：它在体检里是 `info`，`list` 因此为空，
    // 于是小字写着别处的「项目类型 Rust · cargo run」，而指南写着「✗ 跑不起来」。
    renderCardNotes(d);
    renderChanges();
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
  // **容器目录：这几行小字一律清空**（2026-10-16）。
  //
  // 每一条小字都是按"一个项目"的口径写的，最典型的是「怎么跑：还没有契约」——
  // 而契约在子项目里明明有。放在父目录这一屏上就是**把子项目的事算在它头上**。
  // 这道闸必须在这儿：`refreshQuiet` 这类后台刷新会**直接调这个函数**，
  // 不经过 `renderHome` 的容器判断。**同一个坑的第二次，所以要挡两处。**
  if (isContainerDir(d)) { box.innerHTML = ''; return; }
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

  // **这一行原来读的是不存在的字段**（2026-10-16 修）。
  //
  // 写的是 `if (d.error || d.warn)`，而 payload 把计数放在 **`d.counts.error`** 里
  // （`renderHome` 就是 `const { error: err } = d.counts`）。
  // 于是 `d.error` 永远 `undefined` → **这张卡永远说「没有挡路的问题」**。
  //
  // 症状（真窗口里抓到的，同一个屏幕上下两张卡说法相反）：
  //   `#guideCard`：✗ 现在跑不起来 · 2 个依赖没装
  //   `#cardNotes`：要处理 **没有挡路的问题**
  // **它是最不该出错的那种错**：一个确定的"没有"，而事实是"有"——
  // 正是三态被压扁的第 N 种形态。而且它从 `cbb5e18`（前端拆七个脚本那次）就在了，
  // **跑了十几轮、47 条外壳测试全绿，没人发现**——因为那些测试都在看别处。
  //
  // 修法不是改成 `d.counts.error`，而是**直接用下面那个数组**：
  // 显示的数字（`todo.length`）和"有没有"这句话**必须同一个来源**，
  // 否则下次任一边改了，又会漂。
  if (todo.length) {
    rows.push(['要处理', '<span class="bad">' + todo.length + ' 件</span>'
      + (todo[0] ? '：' + esc(todo[0].title) : '')]);
  } else if (d.guide && d.guide.ready && d.guide.ready.state === 'no') {
    // 同一条判据的第二个消费者（见上面状态卡那段注释）。
    rows.push(['要处理', '<span class="bad">跑不起来</span>——'
      + esc(d.guide.what || '')]);
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
  // **容器目录：变更卡也不出现**（同上，挡后台刷新那条绕过路）。
  // 而且它还有第二层理由：父目录的"最近变化"其实是子项目在动，
  // 那不该算成"这个项目变了"。
  if (isContainerDir(S.data)) { box.innerHTML = ''; return; }
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

/// **这台机器缺什么**（来自引擎的 `missing_tool`）。
///
/// 只有四样：缺什么 / 为什么"光有本体不够" / 装配形态 / **自己能验的命令**。
/// 认不出缺什么时 `m` 是 null——**那时候一个字都不加**（不猜）。
function missingHtml(m) {
  if (!m || !m.what) return '';
  return '<div class="missing">'
    + '<div class="miss-head">这台机器缺 <b>' + esc(m.what) + '</b></div>'
    + '<div class="miss-line"><span>为什么</span>' + esc(m.why || '') + '</div>'
    + '<div class="miss-line"><span>怎么装</span>' + esc(m.shape || '') + '</div>'
    // **官方链接**（2026-10-18）：只放 https 的；别的协议一律不点，宁可只显示文字。
    // 理由：让用户自己去搜，第一个结果经常是第三方打包站——那比不装更危险。
    + (m.url && /^https:\/\//.test(m.url)
      ? '<div class="miss-line"><span>在哪下</span><a href="' + esc(m.url) + '" target="_blank" rel="noreferrer">' + esc(m.url) + '</a></div>'
      : '')
    + '<div class="miss-line"><span>怎么自己验</span><code>' + esc(m.verify || '') + '</code></div>'
    // **预期输出**：不给这一句，"跑了个命令"就不等于"知道装没装上"。
    + (m.expect ? '<div class="miss-line"><span>看着像这样才算装上</span>' + esc(m.expect) + '</div>' : '')
    + '</div>';
}

/// **"这台机器够不够用"的结果**（四态，一态一句，不许压成两态）。
///
/// `passed` / `failed` / `not_applicable` / `unclear` ——
/// 最后那一态是**我们没验成**，界面上不能显示成"失败"（颜色与文案都分开）。
function verifyHtml(v) {
  if (!v || !v.verdict) return '';
  const cls = v.verdict === 'passed' ? 'good'
    : v.verdict === 'failed' ? 'bad'
    : v.verdict === 'not_applicable' ? 'skip' : 'ask';
  const mark = v.verdict === 'passed' ? '✓'
    : v.verdict === 'failed' ? '✗'
    : v.verdict === 'not_applicable' ? '·' : '?';
  let html = '<div class="verdict ' + cls + '"><b>' + mark + ' ' + esc(v.headline || '') + '</b>';
  if (v.cmd) html += '<div class="boundary">跑了 ' + esc(v.cmd) + '</div>';
  if (v.why) html += '<div class="boundary">' + esc(v.why) + '</div>';
  if (v.first_error) html += '<div class="crux">第一条错误 · ' + esc(v.first_error) + '</div>';
  // 缺件时只说缺什么、怎么自己验——**不给一键装的按钮**（不生成修复命令）
  // **同一段标记别再写第二遍**（2026-10-18 收）：这里原来又抄了一份，
  // 而且**漏了空值保护**——`shape` 缺失时会渲染出字面 `undefined`。
  // `missingHtml()` 是唯一一处实现，两边都走它。
  if (v.missing_tool) html += missingHtml(v.missing_tool);
  return html + '</div>';
}

/// 绑定「验一下能不能构建」。**只做一件事：跑一次、把四态画出来。**
function bindVerify() {
  const btn = $('#verifyBtn');
  if (!btn || btn.dataset.bound) return;
  btn.dataset.bound = '1';
  btn.addEventListener('click', async () => {
    if (!S.root) return;
    const box = $('#verifyResult');
    const hint = $('#verifyHint');
    btn.disabled = true;
    if (hint) hint.textContent = '正在跑构建……这一条会真跑一次，慢是正常的';
    try {
      const v = await invoke('verify_project', { root: S.root });
      box.innerHTML = verifyHtml(v);
      box.hidden = false;
    } catch (e) {
      // **我们没验成 ≠ 构建失败**：这里也说清是"没能验成"。
      box.innerHTML = '<div class="verdict ask"><b>? 我们没能验成</b><div class="boundary">'
        + esc((e && e.message) || String(e)) + '</div></div>';
      box.hidden = false;
    } finally {
      btn.disabled = false;
      if (hint) hint.textContent = '会真跑一次构建，可能要一两分钟';
    }
  });
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
      + missingHtml(rec.missing)
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
    // **缺哪个件**：直接来自引擎的 `Outcome.missing_tool`（外壳不许自己再扫一遍输出）。
    final.missing = out.missing_tool || null;
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

