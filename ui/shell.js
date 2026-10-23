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
  if (PS.sel) openInline(PS.sel);   // 重画之后把展开卡挂回去——它是"选中"的一部分
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
        // 错误窗要的是**原始条目**（去重与计数交给 errGroups），这里别先压成数字。
        findings: (d.findings || []).filter((f) => (f.severity === 'error' || f.severity === 'warn') && !f.group),
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

/// **行内展开卡**：点谁就长在谁下面（用户 2026-10-18 的要求）。
/// 详细一点是对的，但**分组要清楚**：一眼能扫到「它是什么 / 怎么跑 / 现在的状态」。
function inlineDetailHtml(p) {
  const info = PS.info[p.path] || {};
  const c = PS.draft && PS.draft.contract;
  const why = (PS.draft && PS.draft.why) || [];
  const run = (c && c.commands && c.commands.run) || info.run;
  const when = p.age_days === 0 ? '今天' : (p.age_days === 1 ? '昨天' : p.age_days + ' 天前');
  const cell = (k, v, mono) => '<div class="ex-sec"><div class="ex-k">' + k + '</div>'
    + '<div class="ex-v' + (mono ? ' mono' : '') + '">' + v + '</div></div>';
  return '<div class="ex-grid">'
    + cell('路径', esc(p.path), true)
    + cell('类型', esc(info.kinds || '…'))
    + cell('最近改动', esc(when))
    + cell('怎么跑', run ? esc(run) : '还没看出来——可以直接填下面那三步', true)
    + cell('契约', info.contract ? '有' : '没有')
    + cell('待处理', info.todo ? info.todo + ' 件' : '没有挡路的')
    + cell('卫生', info.auditAll != null ? info.auditAll + ' 条（' + (info.auditHot || 0) + ' 条值得处理）' : '…')
    + '</div>'
    + (why.length ? '<div class="ex-sec" style="margin-top:10px"><div class="ex-k">为什么是这个结论</div>'
        + '<div class="ex-v">' + why.map(esc).join('<br>') + '</div></div>' : '')
    + '<div class="ex-acts">'
    + '<button class="btn sm" data-pd="open">打开项目</button>'
    + '<button class="btn ghost sm" data-pd="contract">' + (info.contract ? '重新起草契约' : '生成契约') + '</button>'
    + '<button class="btn ghost sm" data-pd="editor">用编辑器打开</button>'
    + '<button class="btn ghost sm" data-pd="clean">去清理</button>'
    + '<button class="btn ghost sm" data-pd="close">收起</button>'
    + '</div>';
}

/// 在**某一条横幅下面**插入/替换展开卡；顺手把旧的收掉（同一时刻只开一张）。
function openInline(p) {
  PS.sel = p;
  paintProjectBanners();
  const list = $('#projList');
  if (!list) return;
  list.querySelectorAll('.b-expand').forEach((el) => el.remove());
  // **不能只看直接子元素**（2026-10-19）：分组之后卡片在 `.grp-body` 里面，
  // 用 children 找会把"点了却没展开"做成静默失败。
  const card = Array.from(list.querySelectorAll('.banner'))
    .find((el) => el.dataset && el.dataset.path === p.path);
  if (!card) return;
  const box = document.createElement('div');
  box.className = 'b-expand';
  box.dataset.for = p.path;
  box.innerHTML = '<div class="hint"><span class="spin"></span> 正在解析项目声明…</div>';
  card.after(box);
  // 声明异步补：先给能立刻说的话，再填细节（不让卡片空着）
  invoke('contract_draft', { root: p.path, save: false })
    .then((d) => { PS.draft = d; })
    .catch(() => { PS.draft = null; })
    .finally(() => {
      if (box.dataset.for !== p.path) return;   // 已经换成别的项目了
      box.innerHTML = inlineDetailHtml(p);
    });
  box.innerHTML = inlineDetailHtml(p);
}

/// **悬停浮卡**：跟随光标、只有 4 行、不挡点击。
function showHover(p, x, y) {
  let el = $('#projHover');
  if (!el) {
    el = document.createElement('div');
    el.id = 'projHover';
    el.className = 'projHover';
    document.body.appendChild(el);
  }
  const info = PS.info[p.path] || {};
  el.innerHTML = '<div class="ph-name">' + esc(p.name) + '</div>'
    + '<div class="ph-row"><b>' + esc(info.kinds || '…') + '</b></div>'
    + '<div class="ph-row">能跑：<b>' + (info.run ? esc(info.run) : (info.ready === 'no' ? '现在不行' : '还没判定')) + '</b></div>'
    + '<div class="ph-row">待处理：<b>' + (info.todo ? info.todo + ' 件' : '无') + '</b></div>';
  el.classList.add('on');
  moveHover(x, y);
}
function moveHover(x, y) {
  const el = $('#projHover');
  if (!el) return;
  const w = el.offsetWidth || 240, h = el.offsetHeight || 90;
  el.style.left = Math.min(x + 14, window.innerWidth - w - 10) + 'px';
  el.style.top = Math.min(y + 16, window.innerHeight - h - 10) + 'px';
}
function hideHover() {
  const el = $('#projHover');
  if (el) el.classList.remove('on');
}

/// **常见位置一把扫**：把几处放项目的地方都扫一遍，结果合并去重。
async function scanCommon() {
  // **GitHub 克隆放哪都要能扫到**（2026-10-18 用户反馈：扫不到拉下来的项目）：
  // 除了常见工作目录，再带上 `D:\research` 与它下面专门放研究对象的那一层。
  const roots = ['D:\\projects', 'D:\\research', 'D:\\research\\jingwei-objects', 'D:\\code', 'D:\\src'];
  const found = new Map();
  for (const r of roots) {
    try {
      const res = await invoke('list_projects', { parent: r, depth: 2 });
      for (const p of (res.projects || [])) found.set(p.path, p);
    } catch (e) { /* 这个位置扫不动就跳过，不算失败 */ }
  }
  PS.all = Array.from(found.values()).sort((a, b) => (b.age_days ?? 0) - (a.age_days ?? 0) === 0
    ? String(a.name).localeCompare(String(b.name)) : (a.age_days ?? 0) - (b.age_days ?? 0));
  PS.page = 0; PS.sel = null; PS.info = {};
  renderProjList(); renderSwitcher(); renderRailProjs();
  feedback(found.size ? 'info' : 'warn', '常见位置扫到 ' + found.size + ' 个项目',
    'D:\\projects · D:\\research · D:\\code · D:\\src（各往下 2 层）');
  if (SET.analyze) analyzeRange(0, PAGE_SIZE);
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

  box.innerHTML = '<div class="pd-metrics" id="pdMetrics">正在读协议（CLI 那边算的）…</div>'
    + '<div class="pd-recent" id="pdRecent">最近：还没有记录</div>'
    + '<div class="pd-bar">'
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
  fillMetrics(p.path);
  // 错误窗与协议条同一个数据源（details.findings），没 error 就不出现。
  renderErrPanel(p.path, info);
}

/// **协议条**（2026-10-21）：三数字 + 三态 + 依据，全部来自 CLI 的 --json。
///
/// 界面这里**不加工**：数字怎么算、生命周期怎么判，都在 CLI 那边（一份真源）。
/// 拿不到就显式说"查不了"，不许显示 0 或空白——那会把"没读到"说成"没有"。
function fmtBytes(n) {
  if (n == null) return '—';
  if (n < 1024) return n + ' B';
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + ' KB';
  return (n / (1024 * 1024)).toFixed(1) + ' MB';
}
function stateText(s) {
  return s === 'yes' ? '可以下结论' : s === 'no' ? '先别下结论' : '查不了';
}
function lifeText(s) {
  const m = {
    active: '活跃（近 30 天有提交且有改动）',
    recent: '近期活跃（近 30 天有提交）',
    maintenance: '维护中（30–180 天）',
    stale: '停更（180 天以上）',
    no_git: '判断不了（没有 .git）',
    unknown: '判断不了',
  };
  return m[s] || s || '判断不了';
}
// **更新窗**（2026-10-21）：项目变了才弹，**弹一次就记住**——不重复抢注意力。
// 比对的两边都是真数：上次看到的手交三数字 vs 这次 handover 的三数字。
// **错误窗的聚合**（2026-10-21）：同一处同一个错只占一行并计数——
// 一次索引失败能刷出几十条一样的行，按条显示等于把用户的眼睛当垃圾桶。
// 纯函数：只吃 findings，不碰 DOM，好在测试里直接喂数据。
function errGroups(findings) {
  const list = (findings || []).filter(
    (f) => (f.severity === 'error' || f.severity === 'warn') && !f.group,
  );
  const rows = new Map();
  for (const f of list) {
    const k = [f.code || f.message || '问题', f.file || '', f.line || 0].join('|');
    const r = rows.get(k) || {
      code: f.code || f.message || '问题',
      message: f.message || '',
      file: f.file || '',
      line: f.line || 0,
      severity: f.severity,
      count: 0,
    };
    r.count += 1;
    if (f.severity === 'error') r.severity = 'error';
    rows.set(k, r);
  }
  const rank = { error: 0, warn: 1 };
  return Array.from(rows.values()).sort(
    (a, b) => (rank[a.severity] ?? 2) - (rank[b.severity] ?? 2) || b.count - a.count,
  );
}

// **弹不弹，问引擎**（2026-10-21）：静音公式只在 Rust 里有一份，界面只负责"记住上次"。
function errGateKey(path) { return 'jw.err.seen.' + path; }

function errSig(rows) {
  return rows.map((r) => r.code + '@' + r.file + ':' + r.line).join(',');
}

/// 上一次同类提示：{ sig, at, repeats }。换了一类错就重置重犯计数。
function errSeen(path, sig) {
  let prev = null;
  try { prev = JSON.parse(localStorage.getItem(errGateKey(path)) || 'null'); } catch (_) { prev = null; }
  if (!prev || prev.sig !== sig) return { lastAt: null, repeats: 0 };
  return { lastAt: prev.at || null, repeats: prev.repeats || 0 };
}

/// **错误窗**：有 error 才自己出来（warn 已经在"待办"里，不抢注意力）。
/// 同类错误连着出现时会被静音——**内容还在**，只是不再自己跳出来抢注意力。

async function renderErrPanel(path, d) {
  let box = document.getElementById('errPanel');
  const rows = errGroups(d && d.findings);
  const errs = rows.filter((r) => r.severity === 'error');
  if (!errs.length) {
    if (box) box.remove();
    return;
  }
  // 同一个（或同一类）错误：**静音期内不自己跳出来**；用户点开详细里仍能看到全文。
  const sig = errSig(rows);
  const seen = errSeen(path, sig);
  let allow = true;
  let repeats = seen.repeats;
  try {
    const now = Math.floor(Date.now() / 1000);
    repeats = seen.lastAt ? seen.repeats + 1 : 0;
    const g = await invoke('notify_gate', { lastAt: seen.lastAt, now, freq: 0, repeats });
    allow = !!(g && g.allow);
    if (allow) {
      localStorage.setItem(errGateKey(path), JSON.stringify({ sig, at: now, repeats }));
    }
  } catch (_) {
    /* 闸门问不到就当可以弹：宁可多提示一次，也不许把真问题藏起来 */
    allow = true;
  }
  if (!allow) {
    if (box) box.remove();
    return;
  }
  if (!box) {
    box = document.createElement('div');
    box.id = 'errPanel';
    box.className = 'err-panel';
    document.body.appendChild(box);
  }
  box.innerHTML = '<div class="ep-head">要处理的 ' + rows.length + ' 条'
    + '<span class="ep-x" data-close="1">收起</span></div>'
    + rows.slice(0, 6).map((r) =>
      '<div class="ep-row" data-sev="' + esc(r.severity) + '">'
      + '<b>' + esc(r.code) + '</b>'
      + (r.count > 1 ? '<span class="ep-n">×' + r.count + '</span>' : '')
      + '<span class="ep-msg">' + esc(r.message)
      + (r.file ? ' · ' + esc(r.file) + (r.line ? ':' + r.line : '') : '') + '</span></div>',
    ).join('');
  const x = box.querySelector('[data-close]');
  if (x) x.onclick = () => box.remove();
}

function updateSeenKey(path) { return 'jw.seen.files.' + path; }

function maybeUpdateWindow(path, metrics) {
  if (!metrics || metrics.files == null) return;
  const cur = {
    files: metrics.files,
    bytes: metrics.volume_bytes || 0,
    life: (metrics.lifecycle && metrics.lifecycle.state) || 'unknown',
  };
  let prev = null;
  try { prev = JSON.parse(localStorage.getItem(updateSeenKey(path)) || 'null'); } catch (_) { prev = null; }
  if (prev && updDiffRows(prev, cur).length) {
    showUpdateWindow(path, prev, cur);
  }
  try { localStorage.setItem(updateSeenKey(path), JSON.stringify(cur)); } catch (_) {}
}

/// **只说变了的那些**（2026-10-22）：三数字里哪几项真的变了就列哪几项，
/// 没变的不要摆出来占地方。旧快照可能是两字段的（本功能早期的版本），缺就跳过。
function updDiffRows(prev, cur) {
  const rows = [];
  if (prev.files !== cur.files) rows.push('files');
  if (prev.bytes !== cur.bytes) rows.push('bytes');
  if (prev.life && prev.life !== cur.life) rows.push('life');
  return rows;
}

function showUpdateWindow(path, prev, cur) {
  let box = document.getElementById('updatePanel');
  if (!box) {
    box = document.createElement('div');
    box.id = 'updatePanel';
    box.className = 'upd-panel';
    document.body.appendChild(box);
  }
  const rows = updDiffRows(prev, cur);
  const dFiles = (cur.files || 0) - (prev.files || 0);
  const sign = dFiles > 0 ? '+' + dFiles : String(dFiles);
  const parts = [];
  if (rows.includes('files')) {
    parts.push('<div class="upd-row" data-k="files">文件 <b>' + esc(String(prev.files))
      + ' → ' + esc(String(cur.files)) + '</b><span class="upd-d">' + esc(sign) + '</span></div>');
  }
  if (rows.includes('bytes')) {
    parts.push('<div class="upd-row" data-k="bytes">体积 <b>' + esc(fmtBytes(prev.bytes))
      + ' → ' + esc(fmtBytes(cur.bytes)) + '</b></div>');
  }
  if (rows.includes('life')) {
    parts.push('<div class="upd-row" data-k="life">生命周期 <b>' + esc(lifeText(prev.life))
      + ' → ' + esc(lifeText(cur.life)) + '</b></div>');
  }
  box.innerHTML = '<div class="upd-head">项目有变化<span class="upd-x" data-close="1">知道了</span></div>'
    + parts.join('')
    + '<div class="upd-why">只列变了的那些；要看"到底改了什么"跑一次 jingwei handover。看过就收起，不重复弹。</div>';
  const x = box.querySelector('[data-close]');
  if (x) x.onclick = () => box.remove();
}

/// **首屏"最近"一句**（2026-10-22）：环境动作 + 检查/测试三块拼成一行。
///
/// 三块数据本来就散着：环境在 `tasks_state.latest.env`（renderEnvLast 缓存到 window），
/// 检查/测试在 `DOCK.diags`。这里**只做拼装**，不新增采集、不重算判据。
function recentSummaryHtml() {
  const parts = [];
  const env = (typeof window !== 'undefined' && window.__latestEnv) || null;
  if (env) {
    const when = env.running ? '正在跑 ' + fmtDur(env.elapsed_s) : '用时 ' + fmtDur(env.elapsed_s);
    parts.push('<a data-recent="env">环境 ' + esc((env.name || '').replace(/^jingwei /, '')) + ' ' + esc(when) + '</a>');
  }
  const diags = (typeof DOCK !== 'undefined' && DOCK.diags) || [];
  const check = diags.filter((d) => (d.src || 'check') === 'check').length;
  const test = diags.filter((d) => d.src === 'test').length;
  const errs = diags.filter((d) => d.level === 'error').length;
  if (check) parts.push('<a data-recent="check">检查 ' + check + ' 处（error ' + errs + '）</a>');
  if (test) parts.push('<a data-recent="test">测试 ' + test + ' 处定位</a>');
  else if (typeof DOCK !== 'undefined' && DOCK.lastTest && DOCK.lastTest.ok === false) {
    parts.push('<a data-recent="check">测试失败（没定位到行）</a>');
  }
  return parts.length ? '最近：' + parts.join(' · ') : '最近：还没有记录';
}

/// 兼容旧名（有些地方按 textContent 用）：纯文本版。
function recentSummaryText() {
  const d = document.createElement('div');
  d.innerHTML = recentSummaryHtml();
  return d.textContent;
}

// **"最近"可点**（2026-10-22）：点哪一段就跳到哪一处——环境跳设置页那行，检查/测试跳问题栏。
// 只是跳转，不动任何数据。
document.addEventListener('click', (e) => {
  const a = e.target && e.target.closest ? e.target.closest('[data-recent]') : null;
  if (!a) return;
  e.preventDefault();
  const what = a.dataset.recent;
  if (what === 'env') {
    const panel = document.getElementById('settingsPanel');
    if (panel && panel.hidden) panel.hidden = false;
    const el = document.getElementById('envLast');
    if (el) {
      el.scrollIntoView({ block: 'center' });
      el.style.background = 'rgba(127,127,127,0.18)';
      setTimeout(() => { el.style.background = ''; }, 900);
    }
    if (typeof netRefresh === 'function') netRefresh();
    return;
  }
  if (typeof dockShow === 'function') dockShow('problems');
});

/// **数据来源**（2026-10-22）：协议里每条 evidence 都带 `ref`（命令 / 文件 / 图库）。
/// 协议条只摆第一条——"这条结论凭什么"的最小答案；点一下把它复制出来，能自己复跑。
function srcChip(v) {
  const ev = (v && v.evidence) || [];
  const first = ev.find((e) => e && (e.ref || e.note));
  if (!first) return '';
  const ref = String(first.ref || '');
  const note = String(first.note || '');
  return '<span class="pm-src" data-src="' + esc(ref) + '" title="' + esc(note) + '">来自 '
    + esc(ref || first.kind || '（没说来源）') + '</span>';
}

/// **单飞 + 去抖**（2026-10-22，D-20261022-03 §三 第 3 项）。
///
/// 为什么：每次取数都要 spawn 一次 CLI（实测 380–540 ms），连点三次就把进程起三次。
/// 规矩：同类请求**同时只有一个在飞**；同一 key 在 `ttl` 毫秒内复用上一次结果。
/// 只做这两件事，不缓存跨 session、不做预取——那是常驻通道（债 0.42）的活。
const INFLIGHT = new Map();
const RECENT = new Map();

function fetchOnce(key, ttl, fn) {
  const hit = RECENT.get(key);
  if (hit && Date.now() - hit.at < ttl) {
    if (typeof window !== 'undefined') {
      window.__fetchHits = (window.__fetchHits || 0) + 1;
    }
    return Promise.resolve(hit.value);
  }
  const flying = INFLIGHT.get(key);
  if (flying) return flying;
  const p = Promise.resolve()
    .then(fn)
    .then((value) => {
      RECENT.set(key, { at: Date.now(), value });
      return value;
    })
    .finally(() => INFLIGHT.delete(key));
  INFLIGHT.set(key, p);
  return p;
}
async function fillMetrics(root) {
  const el = document.getElementById('pdMetrics');
  if (!el) return;
  // "最近"那一行用同一块容器：上面是三数字，下面是最近发生了什么（2026-10-22）。

  try {
    const v = await fetchOnce(`metrics:${root}`, 500, () =>
      channelFetch(root, 'metrics'));
    // 两种来源形状不同：通道直接给 metrics，spawn 给 {metrics:…}（2026-10-22）
    const m = (v && v.metrics) || (v && v.files !== undefined ? v : {}) || {};
    const st = (v && v.state) || 'unknown';
    const life = (m.lifecycle && m.lifecycle.state) || 'unknown';
    maybeUpdateWindow(root, m);
    el.innerHTML = '<span class="pm-pill pm-' + esc(st) + '">' + esc(stateText(st)) + '</span>'
      + '<span class="pm">文件 <b>' + (m.files == null ? '—' : esc(String(m.files))) + '</b></span>'
      + '<span class="pm">体积 <b>' + esc(fmtBytes(m.volume_bytes)) + '</b>'
      + (m.capped ? '<i class="pm-cap">只量了一部分</i>' : '') + '</span>'
      + '<span class="pm">生命周期 <b>' + esc(lifeText(life)) + '</b></span>'
      + '<span class="pm-why">' + esc((m.lifecycle && m.lifecycle.basis) || '') + '</span>'
      + '<span class="pm-why">' + esc((v && v.reason) || '') + '</span>'
      + srcChip(v);
    renderProgress(root);
    const recent = document.getElementById('pdRecent');
    if (recent) recent.innerHTML = recentSummaryHtml();
  } catch (e) {
    el.innerHTML = '<span class="pm-pill pm-unknown">查不了</span><span class="pm-why">' + esc(String(e)) + '</span>';
  }
}
async function detailAct(act) {
  const p = PS.sel;
  if (!p) return;
  if (act === 'close') {
    PS.sel = null; PS.draft = null; PS.pinned = false;
    document.querySelectorAll('.b-expand').forEach((el) => el.remove());
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


// **网络允许**（2026-10-22）：界面上"允许一次"走 CLI 的 jingwei net，
// 不在这里另写一份允许表 JSON——一条判据只能有一处实现（失效模式 #17）。
function netRoot() {
  return S.root || '';
}

async function netRefresh() {
  const box = document.getElementById('netList');
  if (!box) return;
  try {
    const v = await invoke('cli_json', { root: netRoot(), args: ['net', 'status'] });
    const allowed = (v && v.allowed) || [];
    const wl = ((v && v.whitelist) || []).length;
    if (!allowed.length) {
      box.textContent = '白名单 ' + wl + ' 个包仓库；没有额外的临时允许。';
      return;
    }
    box.innerHTML = '白名单 ' + wl + ' 个包仓库；临时允许：'
      + allowed.map((a) => '<span class="net-chip" data-host="' + esc(a.host) + '">'
        + esc(a.host) + ' <i>' + esc(netLeftText(a.left)) + '</i>'
        + '<b data-deny="' + esc(a.host) + '" title="收回">×</b></span>').join(' ');
  } catch (e) {
    box.textContent = '读不到允许表（' + (e && e.message ? e.message : e) + '）——这不等于"什么都没允许"。';
  }
}

function netLeftText(secs) {
  const s = Number(secs) || 0;
  if (s <= 0) return '已过期';
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? h + ' 小时后过期' : m + ' 分钟后过期';
}

async function netAllowHost() {
  const inp = document.getElementById('netHost');
  const host = ((inp && inp.value) || '').trim();
  if (!host) {
    toast('先写一个域名，例如 mirrors.aliyun.com', true);
    return;
  }
  try {
    const v = await invoke('cli_json', { root: netRoot(), args: ['net', 'allow', host] });
    if (v && v.state === 'yes') {
      toast('已允许 ' + host + '（24 小时）');
      if (inp) inp.value = '';
      await netRefresh();
    } else {
      toast((v && v.summary) || '没允许成', true);
    }
  } catch (e) {
    toast('没允许成：' + (e && e.message ? e.message : e), true);
  }
}

async function netDenyHost(host) {
  try {
    await invoke('cli_json', { root: netRoot(), args: ['net', 'deny', host] });
    await netRefresh();
  } catch (e) {
    toast('收回失败：' + (e && e.message ? e.message : e), true);
  }
}

document.addEventListener('click', (e) => {
  const t = e.target;
  if (!t || !t.closest) return;
  if (t.closest('#netAllow')) {
    e.preventDefault();
    netAllowHost();
    return;
  }
  const deny = t.closest('[data-deny]');
  if (deny) {
    e.preventDefault();
    netDenyHost(deny.dataset.deny);
  }
});

// **角落任务角标**（2026-10-21）：只认真任务源（engine 的任务日志），不认假状态。
// 没有任务源的种类显示"还没接"——不许假装空闲。
/// 秒数说人话（2026-10-22）：<60s 说"几秒"，再长说"几分"，一天以上说"几小时"。
function fmtDur(secs) {
  const n = Number(secs) || 0;
  if (n < 60) return n + ' 秒';
  if (n < 3600) return Math.floor(n / 60) + ' 分 ' + (n % 60) + ' 秒';
  return Math.floor(n / 3600) + ' 小时 ' + Math.floor((n % 3600) / 60) + ' 分';
}

/// 设置页那行"最近一次环境动作"（2026-10-22）：数据就是 `tasks_state` 里的 `latest.env`，
/// 不新增命令、不另存一份状态。
function renderEnvLast(latest) {
  renderDataDir();
  window.__latestEnv = (latest && latest.env) || null;   // 首屏那句要用（不新增命令）
  const el = document.getElementById('envLast');
  if (!el) return;
  const e = latest && latest.env;
  if (!e) {
    el.textContent = '最近一次环境动作：还没有记录（跑一次 jingwei envs / env use 就有）';
    return;
  }
  const when = e.running ? '正在跑 ' + fmtDur(e.elapsed_s) : '用时 ' + fmtDur(e.elapsed_s);
  const ok = e.state === 'done' ? '' : (e.state === 'failed' ? '（没成功）' : '');
  el.textContent = '最近一次环境动作：' + (e.name || 'env') + ' · ' + when + ok
    + (e.detail ? ' · ' + e.detail : '');
}

let netLastRefresh = 0;   // 设置页开着时，允许表最多每 15 秒刷一次（2026-10-22）

async function renderTaskIcons() {
  let box = document.getElementById('taskIcons');
  if (!box) {
    box = document.createElement('div');
    box.id = 'taskIcons';
    box.className = 'task-icons';
    box.innerHTML = '<span class="ti" data-kind="index" data-state="idle" title="索引">索</span>'
      + '<span class="ti" data-kind="env" data-state="idle" title="环境探测">环</span>'
      + '<span class="ti" data-kind="deep_graph" data-state="idle" title="深度绘制">绘</span>'
      // 错误计数与任务是两条通道：任务说"在不在跑"，这个说"还有几处要处理"。
      + '<span class="ti ti-err" id="tiErr" hidden title="打开问题栏">!</span>';
    document.body.appendChild(box);
  }
  try {
    const r = await invoke('tasks_state');
    const latest = (r && r.latest) || {};
    for (const el of box.querySelectorAll('.ti')) {
      const k = el.dataset.kind;
      const t = latest[k];
      el.dataset.state = t ? (t.state || 'idle') : 'idle';
      if (!t) {
        el.title = k + '：还没接任务源';
      } else {
        const when = t.running
          ? '已跑 ' + fmtDur(t.elapsed_s)
          : (t.state === 'done' ? '用时 ' + fmtDur(t.elapsed_s) : '失败（' + fmtDur(t.elapsed_s) + '后）');
        el.title = k + '：' + when + (t.detail ? ' · ' + t.detail : '');
      }
    }
    renderEnvLast(latest);   // latest 在这个 try 里才在作用域内
    // 设置页开着时，允许表可能已经过期（24 小时到点），最多每 15 秒对一次账（2026-10-22）。
    // 不在设置页就不刷——那是有代价的（起一次 CLI），不能每 3 秒都花。
    {
      const panel = document.getElementById('settingsPanel');
      if (panel && !panel.hidden && Date.now() - netLastRefresh > 15000 && typeof netRefresh === 'function') {
        netLastRefresh = Date.now();
        netRefresh();
      }
    }
  } catch (e) {
    /* 没有任务源不算错：图标保持 idle */
  }
  // **错误角标**（2026-10-22）：数量只来自 `DOCK.diags`（检查 + 测试定位），不另算一份。
  // 有 error 才亮，点它打开问题栏——抬头看一眼角落就知道"还有几处要处理"。
  const chip = box.querySelector('#tiErr');
  if (chip) {
    const diags = (typeof DOCK !== 'undefined' && DOCK.diags) || [];
    const errs = diags.filter((d) => d.level === 'error').length;
    chip.hidden = errs === 0;
    chip.textContent = errs > 9 ? '9+' : String(errs);
    chip.title = errs ? (errs + ' 处 error，点开问题栏') : '没有 error';
    box.dataset.err = String(errs);
  }
}
// **聚焦模式**（2026-10-22，D-20261022-03 Q2-A）：开的时候停掉轮询与对账——
// 默认档只停这些（索引/扫描由用户自己发起，不在轮询里）。按钮上要说清"后台剩几项"，
// 不许"按了但没停"。
const FOCUS = { on: false, timer: null, bg: 0 };

/// 数一下现在还有几项后台在跑：轮询 + 允许表对账（默认档就是这两项）。
function focusBgCount() {
  let n = 0;
  if (FOCUS.timer) n += 1;          // 任务角标轮询
  if (!FOCUS.on && netLastRefresh) n += 0; // 对账不是常驻，只在设置页可见时发生
  return n;
}

function applyFocus() {
  const btn = document.getElementById('btnFocus');
  if (FOCUS.on) {
    if (FOCUS.timer) { clearInterval(FOCUS.timer); FOCUS.timer = null; }
  } else if (!FOCUS.timer) {
    FOCUS.timer = setInterval(renderTaskIcons, 3000);
  }
  window.__focusOn = FOCUS.on;   // 断言与其它模块读它，不重复存状态
  if (btn) {
    btn.textContent = FOCUS.on ? '聚焦中 · 后台 0 项' : '聚焦';
    btn.title = FOCUS.on
      ? '已停：任务角标轮询、允许表对账。再点一次恢复'
      : '停掉不必要后台（轮询/对账），只留当前要看的东西';
    btn.dataset.on = FOCUS.on ? '1' : '0';
  }
}

function toggleFocus() {
  FOCUS.on = !FOCUS.on;
  applyFocus();
  if (typeof toast === 'function') toast(FOCUS.on ? '聚焦：后台已停' : '恢复后台');
}

FOCUS.timer = setInterval(renderTaskIcons, 3000);
window.addEventListener('load', () => { renderTaskIcons(); applyFocus(); });

// 点错误角标 → 打开问题栏（复用 dockShow，不另写一份"打开"）。
document.addEventListener('click', (e) => {
  const c = e.target && e.target.closest ? e.target.closest('#tiErr') : null;
  if (!c) return;
  e.preventDefault();
  if (typeof dockShow === 'function') dockShow('problems');
});

// 点协议条上的"来自 …"→ 把那条命令复制出来（想自己复跑时不用手打）。
document.addEventListener('click', (e) => {
  const c = e.target && e.target.closest ? e.target.closest('[data-src]') : null;
  if (!c) return;
  e.preventDefault();
  const ref = c.dataset.src || '';
  if (!ref) return;
  if (typeof copyText === 'function') copyText(ref);
  else toast('来源：' + ref);
});


/// **数据目录**（2026-10-22，Q1）：显示后端给的真实路径，点按钮让资源管理器打开。
async function renderDataDir() {
  const el = document.getElementById('dataDir');
  if (!el) return;
  try {
    const p = await invoke('data_dir');
    el.textContent = p;
    el.title = '任务日志、性能记录、缓存都在这里';
  } catch (e) {
    el.textContent = '查不了（' + (e && e.message ? e.message : e) + '）';
  }
}

document.addEventListener('click', async (e) => {
  const b = e.target && e.target.closest ? e.target.closest('#btnDataDir') : null;
  if (!b) return;
  e.preventDefault();
  const el = document.getElementById('dataDir');
  const p = el ? el.textContent : '';
  try { await invoke('open_path', { path: p }); }
  catch (err) { if (typeof toast === 'function') toast('打不开：' + (err && err.message ? err.message : err)); }
});

/// **详情页的常驻通道**（2026-10-22，D-20261022-03 Q8：只接这一条路径）。
///
/// 规矩（pro 定）：惰性启动；**崩了自动回退 spawn**；进程只随详情页存活。
/// 回退只提示一次——每次失败都弹，比慢更烦。
let chanWarned = false;

function channelFetch(root, method, params) {
  return invoke('channel_request', { root, method, params: params || null })
    .then((r) => (method !== 'metrics' ? r : {
      metrics: r,
      // **来源要说真话**：这些数字是常驻通道给的，不是 spawn 出来的 handover
      evidence: [{ kind: 'command', ref: 'jingwei serve', note: '常驻通道（惰性启动）' }],
    }))
    .catch(() => {
      // 看门狗：通道不可用 → 退回 spawn（只提示一次，不每次弹）
      if (!chanWarned) {
        chanWarned = true;
        if (typeof toast === 'function') toast('常驻通道没起来，已退回普通取数（只提示这一次）');
      }
      return invoke('cli_json', { root, args: [method === 'metrics' ? 'handover' : method] })
        .then((v) => {
          if (method !== 'metrics') return v;
          return { metrics: (v && v.metrics) || {}, evidence: (v && v.evidence) || [] };
        });
    });
}

/// 离开详情页时把进程退掉（pro 验收第 3 条：进程只随详情页存活）。
function channelStop() {
  if (typeof invoke !== 'function') return;
  invoke('channel_stop').catch(() => { /* 没起来过就算了 */ });
}

/// **学习进度**（2026-10-23，pro：汇总 → 展示 → 可点回文件）。
/// 数据来自 `jingwei events progress`（CLI 已实现），这里只渲染，不重算。
async function renderProgress(root) {
  let el = document.getElementById('pdProgress');
  if (!el) {
    const host = document.getElementById('pdMetrics');
    if (!host || !host.parentElement) return;
    el = document.createElement('div');
    el.className = 'pd-progress'; el.id = 'pdProgress';
    host.parentElement.insertBefore(el, host.nextSibling);
  }
  try {
    const v = await fetchOnce(`progress:${root}`, 5000, () =>
      channelFetch(root, 'progress', { root }));
    const todo = (v && v.todo) || [];
    const learned = (v && v.learned) || [];
    const mine = (v && v.mine) || [];
    const unknown = (v && v.unknown) || [];
    if (!todo.length && !learned.length) { el.textContent = ''; return; }
    const head = [];
    if (mine.length) head.push('本项目 ' + mine.length);
    if (unknown.length) head.push('未知项目 ' + unknown.length);
    el.innerHTML = '<span class="pp-dim">学习进度：' + head.join(' · ') + '</span>'
      + todo.slice(0, 3).map((k) => '<a class="pp-item" data-open-file="' + esc(k) + '">' + esc(k) + '</a>').join('');
  } catch (e) { el.textContent = ''; }
}

document.addEventListener('click', (e) => {
  const a = e.target && e.target.closest ? e.target.closest('[data-open-file]') : null;
  if (!a) return;
  e.preventDefault();
  const p = a.dataset.openFile;
  if (typeof switchTab === 'function') switchTab('editor');
  if (typeof edOpen === 'function') edOpen(p);
});
