// 编辑器那一摊：工作副本、多文件标签、查找与跳行、配对括号、重排缩进、语法高亮。
//
// 规则（写在文件头，因为它决定了这里的每个函数怎么写）：
// **改动留在输入框里，点「应用」才写文件**；应用前自动留一份快照，能回滚。
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

/// **行号旁的诊断标记**（2026-10-22）：编译器检查完的结果按文件挂到行号上，
/// 点一下跳到那一行——不用回「问题」栏里找。
///
/// 数据来自 `DOCK.diags`（`check_project` 的 `diagnostics`），**不另算一份**；
/// 路径可能是绝对也可能是相对，所以按"相等或以 /path 结尾"匹配。
function edDiagsFor(path) {
  const out = new Map();
  const list = (typeof DOCK !== 'undefined' && DOCK.diags) || [];
  if (!path) return out;
  const norm = String(path).replace(/\\/g, '/');
  for (const d of list) {
    if (!d || !d.file || !d.line) continue;
    const f = String(d.file).replace(/\\/g, '/');
    if (f !== norm && !f.endsWith('/' + norm) && !norm.endsWith('/' + f)) continue;
    const prev = out.get(d.line);
    // 同一行有 error 也有 warn：留更严重的那个
    if (!prev || (prev.level !== 'error' && d.level === 'error')) {
      out.set(d.line, { level: d.level || 'warn', message: d.message || '' });
    }
  }
  return out;
}

// 点行号上的诊断标记就跳过去（复用 gotoDiag：切页、定位、提示都在那一处）。
document.addEventListener('click', (e) => {
  const b = e.target && e.target.closest ? e.target.closest('#edNums .ed-diag') : null;
  if (!b || !ED.path) return;
  e.preventDefault();
  gotoDiag(ED.path, parseInt(b.dataset.line, 10) || 1, 1);
});

function edPaintGutter(force) {
  const g = $('#edNums');
  const wrap = $('#edGutter');
  if (!g || !wrap) return;
  const lines = ($('#edText').value.match(/\n/g) || []).length + 1;
  // 行数没变就不要重画行号：3000 行拼 innerHTML 要 130ms，
  // 而打字时绝大多数按键都不改变行数（只有回车才会）。
  if (force || ED.gutterLines !== lines) {
    const diags = edDiagsFor(ED.path);
    let h = '';
    for (let i = 1; i <= lines; i++) {
      const d = diags.get(i);
      const num = d
        ? '<b class="ed-diag" data-line="' + i + '" title="' + esc(d.message) + '" data-lv="'
          + esc(d.level) + '">' + i + '</b>'
        : String(i);
      h += num + '<br>';
    }
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


