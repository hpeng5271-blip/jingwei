// 引擎编辑器（T4）：只调 engine_*；旧编辑器（ed-wrap）已隐藏并标 T6。
const ENG = { rel: '', depth: 0, path: null };

async function engLoadDir(rel, depth) {
  const host = document.querySelector('#engFiles');
  if (!host) return;
  if (depth > 8) { host.innerHTML = '<div class="hint">超过 8 层，停止钻取。</div>'; return; }
  ENG.rel = rel; ENG.depth = depth;
  host.innerHTML = '<div class="hint">读取中…</div>';
  try {
    const r = await invoke('engine_files', { root: S.root || '', path: rel });
    if (r && r.ok === false) throw (r.error || '读取失败');
    const items = (r && r.items) || [];
    const parent = rel.indexOf('/') >= 0 ? rel.slice(0, rel.lastIndexOf('/')) : '';
    const bread = document.querySelector('#engBreadcrumb');
    if (bread) bread.textContent = '/' + rel + (depth ? '（第 ' + depth + ' 层）' : '');
    host.innerHTML = (rel !== '' ? '<div class="kv" data-dir="' + esc(parent) + '"><span>..</span><b>返回上级</b></div>' : '')
      + items.map((n) => '<div class="kv" data-open="' + esc(rel === '' ? n : rel + '/' + n) + '"><span>' + esc(n) + '</span><b></b></div>').join('')
      + (r && r.truncated ? '<div class="hint">已截断：仅显示前 500 项。</div>' : '');
  } catch (e) {
    host.innerHTML = '<div class="hint">目录读取失败：' + esc(String(e)) + '</div>';
  }
}

async function engOpen(path) {
  const text = document.querySelector('#engText');
  const meta = document.querySelector('#engMeta');
  if (!text || !meta) return;
  try {
    const rf = await invoke('engine_read_file', { root: S.root || '', path });
    if (rf && rf.ok === false) throw (rf.error || '读取失败');
    ENG.path = path;
    text.textContent = (rf && rf.text) || '';
    meta.textContent = (rf && rf.truncated ? '已截断（仅前 200KB） · ' : '') + '编码 / 行尾：未由内核提供（T6）';
    engCard(path);
    engEdges(path);
    engCite(path);
    engRefs(path);
  } catch (e) {
    const msg = String(e);
    if (msg.indexOf('is a directory') >= 0) { await engLoadDir(path, ENG.depth + 1); return; }
    text.textContent = '';
    meta.textContent = '读取失败：' + msg;
  }
}

async function engCard(path) {
  const host = document.querySelector('#engCard');
  if (!host) return;
  try {
    const r = await invoke('engine_card', { root: S.root || '', input: path, history: false });
    const card = r && r.card ? r.card : r;
    const st = (card && card.state) || 'uncertain';
    host.textContent = '状态：' + st + ' · exit ' + (r && r.exit != null ? r.exit : '?');
  } catch (e) {
    host.textContent = '卡片读取失败：' + String(e);
  }
}

async function engEdges(path) {
  const host = document.querySelector('#engEdges');
  if (!host) return;
  try {
    const r = await invoke('engine_edges', { root: S.root || '', path });
    const edges = (r && r.edges) || [];
    host.textContent = '边 ' + edges.length + ' 条（仅结构边 use/mod/impl；调用/测试边未显示，全量见 jw edges）'
      + (r && r.unresolved_reasons ? ' · 未解析原因 ' + JSON.stringify(r.unresolved_reasons) : '');
  } catch (e) {
    host.textContent = '边读取失败：' + String(e);
  }
}

async function engStart() {
  const text = document.querySelector('#engText');
  if (!text || text.textContent !== '未打开文件') return;
  await engLoadDir('', 0);
  await engOpen('crates/engine/src/lib.rs');
}

document.addEventListener('click', (e) => {
  const el = e.target && e.target.closest ? e.target.closest('[data-open],[data-dir]') : null;
  if (!el) return;
  if (el.dataset.dir !== undefined) engLoadDir(el.dataset.dir, Math.max(0, ENG.depth - 1));
  else if (el.dataset.open) engOpen(el.dataset.open);
});

async function engCite(path) {
  const host = document.querySelector('#engCite');
  if (!host) return;
  try {
    const r = await invoke('engine_cite', { root: S.root || '', subject: 'file:' + path });
    const v = (r && r.cite) ? r.cite : (r || {});
    if (v.skip) { host.textContent = '引用：skip:' + v.skip + '（无可靠 permalink）'; return; }
    const row = (v.rows || [])[0];
    host.textContent = row
      ? '引用：commit:' + String(row.commit || '').slice(0, 12) + ' ' + row.file + ':' + row.line
      : '引用：skip:no_rows（内核没给行）';
  } catch (e) {
    host.textContent = '引用读取失败：' + String(e);
  }
}

async function engRefs(path) {
  const host = document.querySelector('#engRefs');
  if (!host) return;
  try {
    const r = await invoke('engine_impact', { root: S.root || '', target: 'file:' + path });
    const v = (r && r.impact) ? r.impact : (r || {});
    const direct = v.direct || [];
    const importers = v.importers || [];
    const items = direct.map((d) => ({ label: d.from, line: d.line }))
      .concat(importers.map((im) => ({ label: im.file + ' (' + (im.leaf || '') + ')', line: '' })));
    if (!items.length) { host.innerHTML = '<div class="hint">引用：无（' + esc(v.reason || '没有直接调用者/导入者') + '）</div>'; return; }
    host.innerHTML = items.map((it) => '<div class="kv" data-ref="' + esc(it.label) + '"><span>' + esc(it.label) + '</span><b>' + esc(String(it.line || '')) + '</b></div>').join('');
  } catch (e) {
    host.innerHTML = '<div class="hint">引用读取失败：' + esc(String(e)) + '</div>';
  }
}


// T6.5 最小 preview：替换式、origin 单层（Esc 一次回、二次不回退）、不做 pinned 栈。
const PREV = { origin: null };
async function engPreview(label) {
  const host = document.querySelector('#engPreview');
  if (!host) return;
  if (PREV.origin === null) PREV.origin = host.textContent;
  const card = await invoke('engine_card', { root: S.root || '', input: label, history: false }).catch(() => null);
  const c = (card && card.card) ? card.card : (card || {});
  const file = (String(label).match(/symbol:([^#]+)#/) || [])[1] || '';
  let text = 'symbol: ' + label + ' · 状态 ' + ((c && c.state) || '?');
  if (file) {
    const rf = await invoke('engine_read_file', { root: S.root || '', path: file }).catch(() => null);
    const rt = (rf && rf.text) || '';
    const line = (c && c.line) || 1;
    const lines = rt.split('\n').slice(Math.max(0, line - 3), Math.max(0, line - 3) + 20);
    text += '\n' + lines.map((l, i) => (line - 3 + i + 1) + ': ' + l).join('\n');
  }
  host.textContent = text;
}
document.addEventListener('click', (e) => {
  const el = e.target && e.target.closest ? e.target.closest('#engRefs [data-ref]') : null;
  if (!el) return;
  engPreview(el.dataset.ref);
});
document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  const host = document.querySelector('#engPreview');
  if (!host || PREV.origin === null) return;
  host.textContent = PREV.origin;   // 只回一层：回完就清 origin，二次 Esc 不再变
  PREV.origin = null;
});

window.__engOpen = engOpen;
