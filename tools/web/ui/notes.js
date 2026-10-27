// 笔记那一页：项目卡片（只读那一套）、记录与笔记、打开方式、新建项目。
//
// 笔记页复用了主页的项目卡片，但**不加载检查与运行组件**——看记录的时候
// 不该顺带去扫项目。
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
    S.data.notes = r.all;
    S.current = r.saved.slug;
    renderNotes($('#nbSearch').value);
    toast('已保存');
  });
}
async function deleteNote(slug) {
  await guard(async () => {
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
  await guard(async () => { toast('打开项目未由内核提供（T6）'); });
}

// ── 新建项目 ───────────────────────────
async function createProject() {
  const name = $('#newName').value.trim();
  if (!name) { toast('先给项目起个名字'); return; }
  await guard(async () => {
    const parent = await invoke('engine_pick_root');
    if (!parent) return;
    toast('建项目未由内核提供（T6）');
  });
}

/// 环境那一段：列出这台机器上的解释器，外加配置档
async function loadEnvs() {
  await guard(async () => {
    // 引擎用信封回答：`unknown` = "这些地方都找过了，没找到"，**不等于"没装"**。
    // 原来的写法直接把它当数组用——空数组在界面上就是"一个都没有"，
    // 而那时候我们真正知道的只是"我们没找到"。
    const py = unpack(S.pythons);
    S.pythons = py.state === 'yes' ? py.data : [];
    if (py.state !== 'yes') feedback('warn', '没找到 Python', py.reason);
    const pf = { state: 'unknown', reason: '配置档未由内核提供（T6）' };
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
    S.profiles = [];
    toast('配置档未由内核提供（T6）');
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

