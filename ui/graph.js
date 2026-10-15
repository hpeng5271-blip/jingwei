// 图谱那一页：这一页只做一件事——**把内核里的关系画出来**。
//
// 数据全部来自内核（`graph_stats` / `graph_dump` / `graph_index`），
// 前端不做任何推断。画布用 canvas：一次 path 画完所有边，这是它比 DOM 快的根本原因。
// ── 图谱：这一页只做一件事——**把内核里的关系画出来** ────────────
//
// 数据全部来自内核（`graph_stats` / `graph_dump` / `graph_index`），
// 这里不自己算任何关系——那是引擎的事，界面只负责画。
//
// 三条命令的分工（和外壳那边一致）：
//   graph_index  干活（扫 → 解析 → 写库 → 全局解析），几百毫秒
//   graph_stats  看规模，毫秒级，可以频繁问
//   graph_dump   拿数据画图，天生带上限、超了会如实说截断
const GS = { data: null, nodes: [], edges: [], view: { x: 0, y: 0, k: 1 }, drag: false, lx: 0, ly: 0, live: false, raf: 0 };

async function graphLoad(drawIt) {
  if (!S.root) return;
  const st = unpack(await invoke('graph_stats', { root: S.root }));
  if (st.state !== 'yes') {
    $('#ghNodes').textContent = '—';
    $('#ghEdges').textContent = '—';
    // **如实说**：没建过图谱 ≠ 建了但为空。这两句话不一样。
    $('#ghSub').textContent = st.state === 'no' ? '还没建过图谱（点「索引一次」）' : (st.reason || '读不到');
    return;
  }
  $('#ghNodes').textContent = st.data.nodes;
  $('#ghEdges').textContent = st.data.edges;
  if (!drawIt) return;
  const dp = unpack(await invoke('graph_dump', { root: S.root, limit: 3000 }));
  if (dp.state !== 'yes') {
    $('#ghSub').textContent = dp.reason || '拿不到数据';
    return;
  }
  GS.data = dp.data;
  graphBuild();
  // 截断要写在界面上——画不全时得能区分"只有这么多"和"被截了"
  $('#ghSub').textContent = dp.data.truncated
    ? '共 ' + dp.data.total_nodes + ' 节点，画了前 ' + dp.data.nodes.length + ' 个（已截断）'
    : '全部画出来了';
}

function graphBuild() {
  const raw = GS.data;
  const idx = new Map();
  GS.nodes = [];
  for (const n of raw.nodes) {
    idx.set(n.id, GS.nodes.length);
    GS.nodes.push({ id: n.id, kind: n.kind, name: n.name, x: 0, y: 0, vx: 0, vy: 0, deg: 0 });
  }
  GS.edges = [];
  for (const e of raw.edges) {
    const a = idx.get(e.src), b = idx.get(e.dst);
    if (a === undefined || b === undefined) continue;
    GS.edges.push({ a: a, b: b });
    GS.nodes[a].deg++; GS.nodes[b].deg++;
  }
  // 按度数分环铺开：连得多的靠里，叶子在外圈
  const R = 320;
  for (let i = 0; i < GS.nodes.length; i++) {
    const ring = GS.nodes[i].deg > 8 ? 0.3 : (GS.nodes[i].deg > 2 ? 0.62 : 1);
    const a = i * 2.399963;
    GS.nodes[i].x = Math.cos(a) * R * ring * Math.sqrt((i + 1) / Math.max(1, GS.nodes.length));
    GS.nodes[i].y = Math.sin(a) * R * ring * Math.sqrt((i + 1) / Math.max(1, GS.nodes.length));
  }
  graphDraw();
}

const GCOLOR = { project: '#cc7832', file: '#6a8759', symbol: '#6897bb' };
function graphDraw() {
  const cv = $('#graphCv');
  if (!cv || !cv.clientWidth) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const ctx = cv.getContext('2d');
  cv.width = Math.round(cv.clientWidth * dpr);
  cv.height = Math.round(cv.clientHeight * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  const W = cv.clientWidth, H = cv.clientHeight;
  const cs = getComputedStyle(document.documentElement);
  const bg = cs.getPropertyValue('--sunken').trim() || '#f4f1ec';
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, W, H);
  ctx.save();
  ctx.translate(W / 2 + GS.view.x, H / 2 + GS.view.y);
  ctx.scale(GS.view.k, GS.view.k);
  // 边：一次 path 画完（这是 canvas 比 DOM 快的根本原因）
  ctx.strokeStyle = 'rgba(120,110,100,.18)';
  ctx.lineWidth = 1 / GS.view.k;
  ctx.beginPath();
  for (const e of GS.edges) {
    const a = GS.nodes[e.a], b = GS.nodes[e.b];
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
  }
  ctx.stroke();
  for (const k of ['symbol', 'file', 'project']) {
    ctx.fillStyle = GCOLOR[k] || '#888';
    ctx.beginPath();
    for (const n of GS.nodes) {
      if (n.kind !== k) continue;
      const r = n.deg > 12 ? (k === 'symbol' ? 4 : 9) : (k === 'symbol' ? 2 : 5);
      ctx.moveTo(n.x + r, n.y);
      ctx.arc(n.x, n.y, r, 0, 6.283185307179586);
    }
    ctx.fill();
  }
  ctx.restore();
}

function graphWire() {
  const cv = $('#graphCv');
  if (!cv || cv.dataset.wired) return;
  cv.dataset.wired = '1';
  cv.addEventListener('pointerdown', function (e) {
    GS.drag = true; GS.lx = e.clientX; GS.ly = e.clientY;
    cv.setPointerCapture(e.pointerId);
  });
  cv.addEventListener('pointerup', function () { GS.drag = false; });
  cv.addEventListener('pointermove', function (e) {
    if (!GS.drag) return;
    GS.view.x += e.clientX - GS.lx; GS.view.y += e.clientY - GS.ly;
    GS.lx = e.clientX; GS.ly = e.clientY;
    graphDraw();
  });
  cv.addEventListener('wheel', function (e) {
    e.preventDefault();
    GS.view.k = Math.max(0.1, Math.min(5, GS.view.k * (e.deltaY < 0 ? 1.12 : 0.89)));
    graphDraw();
  }, { passive: false });
  window.addEventListener('resize', function () { graphDraw(); });
  $('#ghFit') && ($('#ghFit').onclick = function () { GS.view = { x: 0, y: 0, k: 1 }; graphDraw(); });
  $('#ghIndex') && ($('#ghIndex').onclick = async function () {
    $('#ghSub').textContent = '正在索引…';
    const r = unpack(await invoke('graph_index', { root: S.root }));
    if (r.state !== 'yes') { $('#ghSub').textContent = r.reason || '索引失败'; return; }
    feedback('info', '图谱已更新',
      r.data.symbols + ' 个符号 · 连上 ' + r.data.resolved + ' 条 · 拿不准 ' + r.data.ambiguous
      + ' · 外部 ' + r.data.external + '（' + r.data.ms + 'ms）');
    await graphLoad(true);
  });
  // 「跟着变」：链上后端的实时层——文件一改，索引自己重跑，卡片上的数字自己变
  $('#ghLive') && ($('#ghLive').onclick = async function () {
    if (GS.live) {
      await invoke('watch_stop');
      GS.live = false;
      $('#ghLive').classList.remove('on');
      $('#ghSub').textContent = '已停止跟随';
      return;
    }
    const r = unpack(await invoke('watch_start', { root: S.root, intervalMs: 1500 }));
    if (r.state !== 'yes') { feedback('warn', '跟随没起来', r.reason || ''); return; }
    GS.live = true;
    $('#ghLive').classList.add('on');
    $('#ghSub').textContent = '正在跟随（每 1.5 秒看一次）';
  });
}

// 内核推来的变化：**先看它成没成**——失败的实时层比没有还糟，
// 界面上的数字会一直停在旧值，而用户以为那是新的。
window.__onGraphChanged = function (msg) {
  if (!msg || msg.ok === false) {
    if (msg && msg.reason) feedback('warn', '实时索引没跑成', esc(msg.reason));
    return;
  }
  graphLoad(true);
};

