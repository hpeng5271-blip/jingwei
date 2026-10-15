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

