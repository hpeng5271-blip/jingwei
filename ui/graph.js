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

/* ═══ 最近运行（M5 那条链的出口）═══════════════════════════════════════
 *
 * ## 它回答什么
 *
 * "这个项目跑起来是什么样" —— **图谱自己记着**。
 * 每跑一次就落一个 `Run` 节点，挂在 `Diagnostic` 上，诊断再挂到文件。
 * 所以列表不是日志的副本，是**结构化的事实**：哪次跑了什么命令、
 * 退出码几、出了几条问题、每条问题**落在哪个文件**。
 *
 * ## 三态在落点上的形状（这是这一块最要紧的地方）
 *
 * 每条问题的"落在哪"有三个答案，**必须分得开**：
 *   有文件   → 就说那个文件
 *   没有位置 → 这条问题本来就没说位置（**用户的空白**，他不用找）
 *   没索引到 → 它说了，但我们没索引那个文件（**我们的失败**）
 * 混成一句"没有文件"就是撒谎——那正是三态被压扁的样子。
 *
 * ## 为什么把明细放在点开之后
 *
 * 一屏能装的信息有限。列表只给"哪次失败、失败几条"（选哪条看的依据），
 * 明细点开才出——这是"该小的小、该现的分级现"。
 */

/** 把一次运行的明细渲染成逐条问题。`diag.file` 是**三态**，不是字符串。 */
function runDetailHtml(diags) {
  return (diags || []).map(function (x) {
    var where;
    var f = x.file || {};
    if (f.state === 'yes') {
      where = '<span class="gd-file">' + esc(String(f.data || '')) + '</span>';
    } else if (f.state === 'no') {
      // **它本来就没说位置**：不是"没找到"，是"这条问题没有落点"。
      where = '<span class="gd-no">没有位置</span>';
    } else {
      // **它说了，但我们没索引到那个文件**：这是我们的失败，要说清。
      where = '<span class="gd-no">没索引到</span>';
    }
    return '<div class="gd">' + where
      + '<span class="gd-title">' + esc(x.title || '') + '</span></div>';
  }).join('');
}

/** 画列表。`SEL`（点开的那一条）放在 `GS.runSel` 里，重画时保持展开。 */
function ghRenderRuns(runs) {
  var box = $('#ghRuns');
  var list = $('#ghRunsList');
  if (!box || !list) return;
  if (!runs || !runs.length) {
    // **"从来没跑过"不是"跑了没问题"**：这句话要说清。
    box.hidden = true;
    return;
  }
  box.hidden = false;
  var sub = $('#ghRunsSub');
  if (sub) sub.textContent = runs.length + ' 次';
  list.innerHTML = runs.map(function (r) {
    var ok = (r.exitCode === 0);
    var open = GS.runSel === r.runId;
    return '<div class="gr' + (open ? ' on' : '') + '" data-rid="' + esc(r.runId) + '">'
      + '<span class="gr-code' + (ok ? '' : ' bad') + '">' + (ok ? '0' : esc(String(r.exitCode))) + '</span>'
      + '<span class="gr-cmd">' + esc(r.command || '（没记下命令）') + '</span>'
      + '<span class="gr-n">' + (r.diagnostics || 0) + '</span>'
      // 明细就挂在行下面**同一个容器里**：点开之后 `.gd` 是这一行的孩子，
      // 测试读的是 `#ghRunsList .gd`，所以位置必须在这里。
      + (open ? runDetailHtml(r.detail) : '')
      + '</div>';
  }).join('');
}

/** 拉一次列表。**点开某一条时再单独拉它的明细**（列表不带明细，省流量）。 */
async function ghLoadRuns() {
  if (!S.root) return;
  var st = unpack(await invoke('graph_runs', { root: S.root, limit: 8 }));
  if (st.state !== 'yes') { ghRenderRuns([]); return; }
  GS.runs = st.data.runs || [];
  // 已经点开的那一条，重画时要把明细一起带上（否则展开会自己收回去）
  if (GS.runSel) {
    var a = unpack(await invoke('graph_runs', { root: S.root, runId: GS.runSel }));
    if (a.state === 'yes') {
      var hit = GS.runs.filter(function (r) { return r.runId === GS.runSel; })[0];
      if (hit) hit.detail = a.data.diagnostics || [];
    }
  }
  ghRenderRuns(GS.runs);
}

/** 绑事件：一次绑好，靠事件委托处理"点开哪一条"。 */
function ghWireRuns() {
  var list = $('#ghRunsList');
  if (!list) return;
  list.addEventListener('click', async function (ev) {
    var row = ev.target.closest ? ev.target.closest('.gr') : null;
    if (!row) return;
    var rid = row.getAttribute('data-rid');
    if (!rid) return;
    // 再点一次就收起来（同一个手势，两个方向）
    GS.runSel = (GS.runSel === rid) ? null : rid;
    await ghLoadRuns();
  });
  ghLoadRuns();
}

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
  // 「最近运行」：索引之后才有得看，所以跟着 HUD 一起绑。
  ghWireRuns();
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

