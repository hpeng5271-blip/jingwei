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

/* ═══ 三种角色（`框架.md` §4.8）═══════════════════════════════════════════
 *
 * ## 为什么不是一个"重要性"数字
 *
 * 入口（`main.rs` 这类）出多入少，公共底层（`util`／`facts`）入多出少。
 * 它们的度数可能差不多——**混成一个数字就把方向抹掉了**，
 * 读图的人会以为"连得多"就是"重要"，然后去找一个什么都不产出的中间层。
 *
 * ## 角色是内核算的，这里只上色
 *
 * 判据（只算 `calls`、聚到文件、跨文件才算、测试文件除外）在
 * `graph/store.rs::dump` 里，界面**不做推断**——这一页只负责让人**看见**。
 *
 * ## 为什么测试要单独一个颜色
 *
 * 测试文件调用一切，出度天然最高。如果不把它们摘出来，
 * 图上最"热闹"的会是 `*_tests.rs`，而它们什么都不产出。
 */
var GROLE = {
  // `why` 是**判据本身**，挂在图例的 title 上：读的人悬停就能问"凭什么"。
  // 三处文字**必须与 `crates/engine/src/graph/roles.rs` 的阈值一致**——
  // 这里写的是给人看的话，那边是唯一的判据（同一个事实只写一处）。
  // **叫「候选」不叫「入口」**（2026-10-18，拿真项目量出来的）：
  // 3 个真应用（fd / cargo-watch / embed-resource）判出 8 个「入口」，
  // 真的是 main.rs 的只有 2 个（**准确率 25%**），其中一个连 main.rs 都没认出来。
  // 原因是跨文件调用太稀（fd：120 条 calls 里 93 条是同文件），入度被系统性低估。
  // **不知道就别装作知道**——所以标签写「候选」，悬停里也只说度数。
  // **入口（结构）与入口候选（度数）是两件事**（2026-10-18）：
  // `src/main.rs` / `src/bin/*.rs` 是**事实**；按度数猜出来的准确率只有 25%。
  entry:           { c: '#cc7832', label: '入口', why: 'Cargo 的 bin 目标（`src/main.rs` / `src/bin/*.rs`）——**这是事实，不看度数**。' },
  entry_candidate: { c: '#e8a33d', label: '入口候选', why: '按跨文件调用度数出来的：出 ≥ 3 且 出 ≥ 2×入。真项目实测只有 25% 对，所以只当线索。' },
  foundation: { c: '#6897bb', label: '公共底层', why: '跨文件调用：入度 ≥ 3，且 入 ≥ 2×出。很多人用它，它自己很少调别人。' },
  // **公共面**（2026-10-18 加）：库的出口 `src/lib.rs`。
  // 它被很多人 use，自己很少调别人——按度数会掉进"公共底层"，
  // 但那是**被人引用的面**，不是**被人当工具用的零件**。两件事，两个名字。
  surface:    { c: '#b48ead', label: '公共面', why: '库的出口（`src/lib.rs`）：别人 `use` 它，它自己不调用谁——不是工具，也不是入口。' },
  middle:     { c: '#9a8f85', label: '中间层', why: '两头都有，但都不极端——它既不是入口也不是公共底层。' },
  // **isolated 不是一种角色**：它说的是「我们读不出它跟谁有关系」。
  // 实测本仓库 53 个「中间层」里有 17 个正是这样——把她们叫中间层就是撒谎。
  isolated:   { c: '#5f5f5f', label: '还没连上', why: '跨文件调用：出度 = 0 且 入度 = 0 —— 我们读不出它跟谁有关系（不是它没有关系）。' },
  test:       { c: '#cfc8bd', label: '测试（不参与判断）', why: '测试文件调用一切，出度天然最高——它们不参与角色判断，连到它们的边也不计。' },
  container:  { c: '#8a7f74', label: '项目', why: '项目节点是容器，不是入口。' }
};

/** 把角色计数画成一行。**没数据就藏起来**；有数据但一个角色都没有，要**明说**。 */
/** **覆盖度不够时，角色图例要加一句提醒**（2026-10-18）。
 *
 * 判据来自 3 个真应用：跨文件调用只接上一部分时，"入口候选/公共底层"会偏多。
 * 真数据：fd 120 条 calls 里 93 条同文件，跨文件只接上 27 条。
 *
 * **拿不到覆盖度就不说**（老库没有这两个数）——"不知道"不许冒充"够用"。
 */
function ghCoverageTip(stats) {
  if (!stats) return "";
  const linked = stats.linked;
  const unresolved = stats.unresolved;
  if (typeof linked !== "number" || typeof unresolved !== "number") return "";
  if (unresolved <= linked) return "";
  return "跨文件调用只接上 " + linked + " 条、" + unresolved + " 条拿不准——角色会比实际偏多";
}

/* **图例卡是否要露出来**（2026-10-18 排版改动）：
   角色与目录两块内容分别在两张卡里，而外层卡片不能一直显示——
   一个空框会让人以为"坏了"。所以两块都空的时候，整张卡收起来。 */
function ghLegendSync() {
  var card = $('#ghLegend');
  if (!card) return;
  var a = $('#ghRoles'), b = $('#ghDirs');
  var any = (a && !a.hidden) || (b && !b.hidden);
  card.hidden = !any;
}

function ghRenderRoles(roles, stats) {
  var box = $('#ghRoles');
  if (!box) return;
  if (!roles) { box.hidden = true; return; }
  var order = ['entry', 'entry_candidate', 'surface', 'foundation', 'middle', 'test', 'isolated'];
  // **数字要有单位**（2026-10-17）：原来只写「入口 18」，读的人不知道是
  // 18 个文件、18 条边、还是 18 个符号。这里是**按文件**数的。
  var unit = ' 个文件';
  var parts = order.filter(function (k) { return (roles[k] || 0) > 0; }).map(function (k) {
    // **凭什么判的**挂在 title 上（悬停才看，不占视觉）——
    // "每条结论要有来源与自查"在这一页的落法。
    return '<span class="gr-role" title="' + GROLE[k].why + '"><i style="background:'
      + GROLE[k].c + '"></i>' + GROLE[k].label + ' <b>' + roles[k] + '</b>' + unit + '</span>';
  });
  if (!parts.length) {
    // **"读不出角色"和"没有角色"是两件事**：一个空框会让人以为是坏的。
    // 这一页的规矩同三态——说不出话的时候要说清"为什么说不出"。
    box.hidden = false;
    box.innerHTML = '<span class="gr-role">这个项目还没有跨文件的调用——'
      + '读不出角色（不是没有模块，是它们之间还没连上）</span>';
    ghLegendSync();
    return;
  }
  box.hidden = false;
  // **覆盖度不够就说一句**（2026-10-18）：图例里的数字是"按我们连上的边算的"，
  // 边只接上一部分时，角色会比实际偏多。不说 = 让人把猜当事实。
  var tip = ghCoverageTip(stats);
  box.innerHTML = parts.join('') + (tip ? '<span class="gr-tip">' + tip + '</span>' : '');
  ghLegendSync();
}

/** 一个点画多大。**颜色说角色，大小说规模**——两件事分开，别挤在一列里。
 *
 * - `role`：定底数（入口最大、测试与"还没连上"最小，因为它们不该抢视线）
 * - `calls`：`out + in`，**只算跨文件 `calls`**（不含 `contains`/`defines`）
 * - `isSym`：符号一律比同角色的文件小，否则 1520 个符号会盖住 95 个文件
 *
 * 为什么要有规模这一维：中间层里既有 `detect/probe.rs`（出13/入13、被 6 个目录用，
 * 那是枢纽），也有 `detect/editors.rs`（出0/入2，局部胶水）。
 * **只按角色给固定大小时，它们长得一模一样**——读的人看不出哪个是枢纽。
 * 而"枢纽"不该是一个新角色（角色说的是方向，不是大小，见 `框架.md` §4.8.3），
 * 所以它落在**大小**上。
 *
 * 上限定 3：再大就会盖住邻点，而这一页首先得看得清。
 */
function ghNodeRadius(role, isSym, calls) {
  const base = role === 'entry' ? 6 : role === 'foundation' ? 5
    : role === 'test' ? 2 : role === 'isolated' ? 2 : 3;
  if (isSym) return Math.max(2, base - 2);
  const bump = Math.min(3, Math.floor((calls || 0) / 6));
  return base + bump;
}

/** 目录级那一行。**没有就不画**，有就只说前三名——不堆。
 *
 * 带标签（「入口」「公共底层」）不是为了好看：**没有标签的一串路径读不懂**，
 * 而这两个词正是 §4.8 要说的事。 */
function ghRenderDirs(dirs) {
  var box = $('#ghDirs');
  if (!box) return;
  if (!dirs || !dirs.total) { box.hidden = true; return; }
  var groups = [];
  var mk = function (label, color, list, tail, total) {
    if (!list || !list.length) return;
    var items = list.map(function (d) {
      return esc(d.dir) + ' <b>' + (tail === 'out' ? d.out : d.in) + '</b>' + (tail === 'out' ? '→' : '←');
    });
    // **截断不许静默**（2026-10-17 修的）：本仓库入口目录有 4 个，这里只放前三名。
    // 不说出来，读的人会以为"入口就这三个"，而 `total` 是**所有**目录的个数，
    // 根本推不出"这一层还剩一个没显示"。
    var tail2 = (total && total > list.length) ? ' <span class="gr-more">（共 ' + total + ' 个）</span>' : '';
    groups.push('<span class="gr-role"><i style="background:' + color + '"></i>'
      + label + ' ' + items.join(' · ') + tail2 + '</span>');
  };
  mk('入口', GROLE.entry.c, dirs.entry, 'out', dirs.entryTotal);
  mk('公共底层', GROLE.foundation.c, dirs.foundation, 'in', dirs.foundationTotal);
  // **中间层只报个数**（2026-10-17 加）：13 个目录里 7 个是中间层，
  // 原来一个字都不说，读的人会以为"目录只有两类"。
  // 名字那一半会撑爆这一行，所以**只给数**——交代"还有几个"，不交代"是哪几个"。
  if (dirs.middleTotal > 0) {
    groups.push('<span class="gr-role"><i style="background:' + GROLE.middle.c + '"></i>'
      + '中间层 ' + dirs.middleTotal + ' 个目录</span>');
  }
  if (!groups.length) { box.hidden = true; ghLegendSync(); return; }
  box.hidden = false;
  box.innerHTML = groups.join('');
  ghLegendSync();
}

async function graphLoad(drawIt) {
  if (!S.root) return;
  const st = unpack(await invoke('graph_stats', { root: S.root }));
  if (st.state !== 'yes') {
    $('#ghNodes').textContent = '—';
    $('#ghEdges').textContent = '—';
    ghRenderRoles(null);
    ghRenderDirs(null);
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
  // 覆盖度与角色一起给：`ghCoverageTip` 会判断要不要加那句提醒。
  ghRenderRoles(dp.data.roles, dp.data);
  ghRenderDirs(dp.data.dirs);
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
    // `role` 是内核算好带下来的（见 store.rs::dump），**这里只存不算**。
    // `out`/`in` 也带下来：那是**只算跨文件 `calls`** 的度数，
    // 与上面那个 `deg`（连 `contains`/`defines` 一起算）不是一回事——
    // 文件的 `deg` 主要来自"这个文件里有多少符号"，**拿它排大小会把符号多的当重要**。
    GS.nodes.push({
      id: n.id, kind: n.kind, name: n.name, role: n.role,
      out: n.out || 0, in: n.in || 0,
      x: 0, y: 0, vx: 0, vy: 0, deg: 0,
    });
  }
  GS.edges = [];
  for (const e of raw.edges) {
    const a = idx.get(e.src), b = idx.get(e.dst);
    if (a === undefined || b === undefined) continue;
    // **边要带种类**：`calls` 是"谁在用谁"，`contains`/`defines` 只是包含。
    // 两者画一样重，读的人就会把"装着"当成"用着"——这正是 §4.8 说的那个混淆。
    GS.edges.push({ a: a, b: b, kind: e.kind });
    GS.nodes[a].deg++; GS.nodes[b].deg++;
  }
  // **按角色分环铺开**——这是这一页最省字的一件事：
  //
  //   入口（出多入少）    → 外圈
  //   中间层              → 中圈
  //   公共底层（入多出少）→ 核心
  //   测试                → 最外（垫底，不参与判断）
  //   项目节点            → 正中（它是容器）
  //
  // 于是"依赖方向"不用读标签也看得出来：**箭头都是从外往里**。
  // 原来按度数分环，入口和底层混在一起，方向就抹掉了。
  const RING = { container: 0.18, foundation: 0.5, surface: 0.42, middle: 0.66, entry_candidate: 0.86, entry: 1, test: 1.16, isolated: 1.12 };
  const R = 320;
  for (let i = 0; i < GS.nodes.length; i++) {
    const ring = RING[GS.nodes[i].role] || 0.78;
    const a = i * 2.399963;
    const r = R * ring * Math.sqrt((i + 1) / Math.max(1, GS.nodes.length));
    GS.nodes[i].x = Math.cos(a) * r;
    GS.nodes[i].y = Math.sin(a) * r;
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
  // 边：**分两组画，两组的分量不同**。
  //
  //   `calls`（谁在用谁）→ 深一点、实一点：这是这一页要说的事
  //   包含边（contains/defines）→ 很淡：它们是**结构**，不是**使用**
  //
  // 两组画成一样重，图上最"热闹"的会是包含关系，而读的人会把它当成依赖。
  // 仍然一次 path 画完一组（canvas 比 DOM 快的根本原因）。
  ctx.lineWidth = 1 / GS.view.k;
  for (const group of [
    { kinds: ['contains', 'defines'], stroke: 'rgba(120,110,100,.10)' },
    { kinds: ['calls'], stroke: 'rgba(90,80,70,.42)' },
  ]) {
    ctx.strokeStyle = group.stroke;
    ctx.beginPath();
    for (const e of GS.edges) {
      if (!group.kinds.includes(e.kind)) continue;
      const a = GS.nodes[e.a], b = GS.nodes[e.b];
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
    }
    ctx.stroke();
  }
  // **按角色上色**：这是这一页新读出来的东西（§4.8）。
  // 画序也是信息：测试与"还没连上"先画（垫底），入口最后画（压在最上面）——
  // 谁都不该盖住入口。
  //
  // **`isolated` 必须在这张表里**（2026-10-17 修的 bug）：上一轮只把它加进了
  // 颜色表 / 图例 / 环形半径，没加进这张表——于是那 17 个文件**一个都没被画出来**。
  // 图例说「还没连上 17 个文件」，画布上却一个都找不到。
  // 兜底那一段也救不了它们：那里 `if (n.role ...) continue` 会跳过所有有角色的节点。
  for (const role of ['test', 'isolated', 'middle', 'foundation', 'surface', 'entry_candidate', 'entry', 'container']) {
    const paint = GROLE[role];
    if (!paint) continue;
    ctx.fillStyle = paint.c;
    ctx.beginPath();
    for (const n of GS.nodes) {
      if (n.role !== role) continue;
      const r = ghNodeRadius(role, n.kind === 'symbol', (n.out || 0) + (n.in || 0));
      ctx.moveTo(n.x + r, n.y);
      ctx.arc(n.x, n.y, r, 0, 6.283185307179586);
    }
    ctx.fill();
  }
  // 没有角色的节点（理论上不该有）用原来的按种类上色兜底，**别让它们消失**。
  if (GS.nodes.some(function (n) { return !n.role; })) {
    for (const k of ['symbol', 'file', 'project']) {
      ctx.fillStyle = GCOLOR[k] || '#888';
      ctx.beginPath();
      for (const n of GS.nodes) {
        if (n.role || n.kind !== k) continue;
        const r = n.deg > 12 ? (k === 'symbol' ? 4 : 9) : (k === 'symbol' ? 2 : 5);
        ctx.moveTo(n.x + r, n.y);
        ctx.arc(n.x, n.y, r, 0, 6.283185307179586);
      }
      ctx.fill();
    }
  }
  ctx.restore();
  // **画布自证**（2026-10-18）：DOM 只能说明"画布有尺寸"，**说不了里面有没有东西**。
  // 每次画完把"真的画了什么"挂到 `window.__graphDrawn`，由自检读进报告。
  // 判据全是**能失败的量**（不是"我调用过 draw"）：
  //   nodes/edges 真进画布的数量 · roles 有角色的节点数 ·
  //   radiusMin/Max 半径有没有区分度（全相等说明没算） · ms 这一帧的耗时。
  (function ghSelfReport() {
    var radii = (GS.nodes || []).map(function (n) { return n.r != null ? n.r : (n.deg > 12 ? 9 : 5); });
    window.__graphDrawn = {
      nodes: (GS.nodes || []).length,
      edges: (GS.edges || []).length,
      roles: (GS.nodes || []).filter(function (n) { return !!n.role; }).length,
      radiusMin: radii.length ? Math.min.apply(null, radii) : 0,
      radiusMax: radii.length ? Math.max.apply(null, radii) : 0,
      w: W,
      h: H,
      at: Date.now(),
    };
  })();
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
  // **卡住要说出来**（2026-10-18）：`#ghSub` 的初始文案是「正在读取…」，
  // 而 `graphLoad` 里的 `invoke` 若一直不回来（图谱库被占用 / 索引在跑），
  // 那句「正在读取…」就会**永远停着**——人看不出是忙还是坏，门禁也会一直等。
  //
  // 这条看门狗 20 秒后把它改成「读不到」，并说清可能是谁的问题。
  // **三态纪律：不知道也别说"正在读取"，那是拿"忙"冒充"会好"**。
  // 真加载成功时 `graphLoad` 会把文案覆盖掉，所以它不会盖住正常结果。
  (function ghSubWatchdog() {
    setTimeout(function () {
      var el = $('#ghSub');
      if (el && el.textContent === '正在读取…') {
        el.textContent = '读不到（20 秒还没回来）——可能图谱库被占用，或者索引正在跑；点「索引一次」再试，或关掉别的窗口';
      }
    }, 20000);
  })();

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

