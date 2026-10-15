// 外壳那一圈：右侧诊断栏、四角信息卡策略、底部活动带、顶部项目切换器、项目列表。
//
// 这些是"任何页面都在场"的东西，所以跟具体某一页分开放——它们是壳，不是页。
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
// 这张树是谁给的（Rust 还是 Node），以及"没有树"时的原因。
// **来源必须显示出来**：同样是"下游树"，出自 Cargo.lock 和出自
// package-lock.json 的可信度与含义都不一样。
let TREE_SRC = '';
let TREE_GAP = '';
const RAIL = { collapsed: new Set(), q: '' };

/// 依赖清单：按 Maven 工具窗的读法组织——一个清单、若干分组、一行一项。
/// 每行两列：清单里声明的版本 → 机器上实际的版本。对不上就是要处理的事。
/// 用回来的一堆边建一张邻接表
function treeIndex(d) {
  const t = (d && d.depsTree) || null;
  // Rust 那张优先；没有就退到 Node 那张（`depsTreeWeb`）。
  // **一次只画一张**：一个 Tauri 项目两边都有，混起来画会把两个生态的
  // 名字串成一棵看起来通顺、其实不存在的树。
  const use = (t && t.edges && t.edges.length) ? t : ((d && d.depsTreeWeb && d.depsTreeWeb.edges && d.depsTreeWeb.edges.length) ? d.depsTreeWeb : null);
  TREE_SRC = use ? (use === t ? 'Rust（Cargo.lock）' : (use.source || 'Node')) : '';
  TREE_GAP = (d && d.depsTreeWeb && d.depsTreeWeb.note) || '';
  if (!use) { TREE = { adj: new Map(), cache: new Map(), ready: false, total: 0 }; return; }
  const t2 = use;
  const adj = new Map();
  t2.edges.forEach((e) => {
    if (!adj.has(e.from)) adj.set(e.from, []);
    adj.get(e.from).push(e.to);
  });
  TREE = { adj, cache: new Map(), ready: true, total: t2.total || 0 };
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

// 「运行时事实」的一行：键 → 值，凭据走悬停。
//
// 为什么长这样：这一栏要回答的是同一类问题——**这次到底会用哪个东西，凭什么**。
// Node 那一摊（包管理器、依赖装到哪）和 JVM 那一摊（哪个 JDK、哪个构建工具）
// 是同一类事实的两种形态，所以共用一套行。
function rtRow(k, v, why) {
  return '<div class="rt-row" data-tip="' + esc(why || k) + '">'
    + '<span class="k">' + esc(k) + '</span>'
    + '<span class="v">' + esc(v) + '</span></div>';
}

function rtSection(title, rows, notes) {
  let html = '<div class="rail-sec">' + esc(title) + '</div>' + rows.join('');
  if (notes && notes.length) {
    html += notes.map((n) => '<div class="rt-note">' + esc(n) + '</div>').join('');
  }
  return html;
}

// Node / 前端那一摊：**用什么包管理器、依赖装到哪去了、版本要求写在哪**。
//
// 为什么这几条值得占位置：它们是"为什么依赖那一栏显示未安装"的答案。
// yarn PnP 项目压根没有 node_modules，monorepo 的依赖装在仓库根——
// 不把这层说清楚，用户看到的就是一整列✗，然后去重装依赖，然后问题依旧。
//
// 每条都带凭据（悬停可见）：界面上"你凭什么这么说"必须有答案。
function webSection(w) {
  const row = rtRow;
  const rows = [];
  const pm = w.package_manager
    ? w.package_manager + (w.pm_pinned ? ' ' + w.pm_pinned : '')
    : '还看不出来';
  rows.push(row('包管理器', pm, w.pm_from || '没有锁文件，也没有 packageManager 字段'));
  if (w.link_kind) rows.push(row('依赖位置', w.link_kind, w.link_from || ''));
  if (w.node_pins && w.node_pins.length) {
    rows.push(row('Node 要求',
      w.node_pins.map((p) => p.spec).join('  /  '),
      w.node_pins.map((p) => p.from + '：' + p.spec).join('\n')));
  }
  if (w.workspaces) rows.push(row('工作区', w.workspaces, 'monorepo：依赖可能被提升到仓库根'));
  if (w.frameworks && w.frameworks.length) {
    rows.push(row('框架', w.frameworks.join(' / '), '从 package.json 的依赖里认出来的'));
  }
  if (w.typescript) rows.push(row('TypeScript', '有 tsconfig', '根目录下有 tsconfig*.json'));
  if (w.scripts && w.scripts.length) {
    rows.push(row('脚本', w.scripts.map((s) => s[0]).join(' / '),
      w.scripts.map((s) => s[0] + ' → ' + s[1]).join('\n')));
  }
  return rtSection('Node / 前端', rows, w.notes);
}

// JVM 那一摊：**这次会用哪个 JDK、项目要求哪个、装了哪几个、构建工具是谁**。
//
// 为什么值得占位置：一台机器上装三个 JDK 是常态，而"用的是哪个"正是
// 「我明明装了 21，编译却报 class file version 52」的答案。
// 后端跑不起来的一半原因在这一栏里。
function jvmSection(j, build) {
  const row = rtRow;
  const rows = [];
  const res = j.resolved;
  rows.push(row(
    '会用哪个',
    res ? ('JDK ' + res.major) : '说不清',
    res ? ('凭据：' + res.source + (res.home ? ' · ' + res.home : '')) : 'JAVA_HOME 没设、PATH 上没有 java',
  ));
  if (res && res.home) rows.push(row('JDK 位置', res.home, 'JAVA_HOME 会按这个设，Maven/Gradle 都认它'));
  if (j.required) rows.push(row('项目要求', 'Java ' + j.required, j.required_from || ''));
  const list = j.jdks || [];
  if (list.length) {
    rows.push(row(
      '装了哪些',
      list.map((d) => d.major + (d.is_jdk ? '' : '（只有 JRE）')).join(' / '),
      list.map((d) => 'JDK ' + d.major + (d.is_jdk ? '' : '（没有 javac，只能跑不能编）') + '：' + d.home).join('\n'),
    ));
  }
  if (build) {
    rows.push(row(
      '构建工具',
      build.name + (build.wrapper ? '（项目自带）' : '（PATH 上的全局）'),
      build.wrapper
        ? '项目自带的 wrapper 钉着它自己的版本，比 PATH 上那个全局的更准'
        : '这个项目没有 wrapper，用的是 PATH 上那个',
    ));
  }
  const notes = [];
  if (!res && list.length > 1) {
    notes.push('装了 ' + list.length + ' 个 JDK 却说不清用哪个——把 JAVA_HOME 指到你要的那个，结果就确定了');
  }
  const onlyJre = list.filter((d) => !d.is_jdk);
  if (onlyJre.length && list.length === onlyJre.length) {
    notes.push('这几个都只有 JRE，没有 javac——能跑，但编译不了');
  }
  return rtSection('JVM / 运行时', rows, notes);
}

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
  if (d.web) html += webSection(d.web);
  if (d.jvm) html += jvmSection(d.jvm, d.jvmBuild);
  if (!total) {
    html += '<div class="rail-empty">该项目未声明依赖。'
      + (manifests.length ? '' : '<br>也未找到依赖清单文件。') + '</div>';
  } else {
    html += railDepHtml(detail, deps);
    html += '<div class="rail-empty">左列为清单声明，右列为实际安装版本。</div>';
  }
  // 下游树是谁给的（或为什么没有）——**这一行不能省**：
  // "点开一层看下游"看起来都一样，但出自 Cargo.lock 和出自 package-lock.json
  // 是两份完全不同的凭据；而没有树的时候，更要说明是"没有"还是"还没做"。
  if (TREE.ready && TREE_SRC) {
    html += '<div class="rail-empty">下游树来自 ' + esc(TREE_SRC) + '（' + TREE.total + ' 条边）。</div>';
  } else if (TREE_GAP) {
    html += '<div class="rt-note">传递依赖图：' + esc(TREE_GAP) + '</div>';
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

