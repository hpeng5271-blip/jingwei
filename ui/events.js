// 事件与视图切换：页面之间怎么切、后端推来的事件怎么分发、底部状态栏、
// 详细区（平时不取，点开先摆上次的再刷新）。
//
// 放在页面模块之后：它要把各页的渲染函数接起来。

// **页签清单的唯一真源**（2026-10-16 补回）。
//
// 这几个名字被三处在用，而且**它们必须一致**：
//   ① 这里的 `TABS`（`switchTab` 与 `paintNav` 靠它）
//   ② `index.html` 里每个导航按钮的 `data-tab`
//   ③ 《使用说明.md》里讲的页数
// `tools/check-ui-doc.mjs` 就是拿这三份**互相比对**的——它当年抓到的真错是
// "界面上有图谱这一页，而说明书整整一页没提"。所以这个常量**是判据的锚点**，
// 不是顺手写的数组：它少一项，那道门禁就静默失效（报"读不出 TABS——真源没了"）。
//
// ⚠️ 2026-10-16 记一笔：`ui/` 曾被整体回退过一次，这个常量跟着没了，
// 而门禁当场报「读不出 ui/events.js 里的 TABS —— 真源没了，这条门禁失效」——
// **是门禁自己抓住的**。这就是"新加的东西必须有人守"的意思。
const TABS = ['home', 'run', 'editor', 'note', 'graph', 'machine', 'clean'];

// ── 事件 ───────────────────────────────
// 聚焦按钮（2026-10-22）：停不必要的后台，状态由 shell.js 的 applyFocus 统一管。
if (document.getElementById('btnFocus')) {
  document.getElementById('btnFocus').addEventListener('click', () => toggleFocus());
}

function switchTab(name) {
  PANEL.tab = name;
  panelPolicy();
  document.querySelectorAll('#nav button').forEach((b) => b.classList.toggle('on', b.dataset.tab === name));
  document.querySelectorAll('.view').forEach((v) => v.classList.toggle('on', v.id === 'view-' + name));
  if (name === 'clean' && S.root && !S.clean) scanClean();
  if (name === 'clean' && !S.root && !S.clean) scanClean();
  if (name === 'machine') loadMachine(false);
  if (name === 'note') renderNoteProjects();
  if (name === 'editor' && S.root && !ED.files.length) edLoadFiles();
  if (name === 'editor') edPaintCode();
  // 图谱：切过去才拉数据、才连线——**不切就不做任何事**
  // （一屏画几千个点是有代价的，没在看的时候不该花这份钱）
  if (name === 'graph' && S.root) {
    graphWire();
    graphLoad(true);
  }
}

/// 「这台机器」和「清理」不需要项目；其余三页需要。
/// 没项目时把它们变灰，比点进去看到一个空壳要好。
function paintNav() {
  const has = !!S.root;
  document.querySelectorAll('#nav button[data-needs="project"]').forEach((b) => {
    b.classList.toggle('dim', !has);
    b.title = has ? '' : '先打开一个项目';
  });
}

window.addEventListener('DOMContentLoaded', async () => {
  applyLook();   // 外观先落地，免得闪一下默认主题
  initTooltips();
  initParallax();
  initDock();
  // 概览页那个「验一下能不能构建」——**只绑一次**（`bindVerify` 里有 dataset 防重）
  bindVerify();
  // 全局点击反馈：任何可点的东西按下去都有涟漪 + 一声轻响
  document.addEventListener('pointerdown', (e) => {
    const hit = e.target.closest('button, .pcard, .diag, .nb-item, .copy, .agroup>summary, .item');
    if (!hit || hit.disabled) return;
    rippleAt(hit, e);
    sfx('click');
  }, true);
  $('#nav').addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (b) switchTab(b.dataset.tab);
  });
  ['#btnPick', '#btnPick2'].forEach((s) => $(s).addEventListener('click', pickProject));
  // 新建项目：独立弹窗卡片（IDEA 的 New Project 也是独立窗口，
  // 不是塞在页面下方的一块面板——那种做法会把"先看哪个"搞乱）。
  const openNew = () => {
    switchTab('home');
    if ($('#newModal')) $('#newModal').hidden = false;
    paintNewLang();
    loadEnvs();
    setTimeout(() => $('#newName') && $('#newName').focus(), 60);
  };
  ['#btnNew', '#btnNew2'].forEach((s) => $(s) && $(s).addEventListener('click', openNew));
  const closeNew = () => { if ($('#newModal')) $('#newModal').hidden = true; };
  if ($('#btnNewX')) $('#btnNewX').addEventListener('click', closeNew);
  if ($('#newModal')) {
    $('#newModal').addEventListener('click', (e) => { if (e.target.id === 'newModal') closeNew(); });
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && !$('#newModal').hidden) closeNew();
    });
  }
  if ($('#newLang')) $('#newLang').addEventListener('change', paintNewLang);
  $('#btnSaveProfile').addEventListener('click', saveProfile);
  $('#newProfile').addEventListener('change', (e) => {
    const p = S.profiles.find((x) => x.name === e.target.value);
    if (!p) return;
    $('#newTpl').value = p.template || 'package';
    if ($('#newLang')) $('#newLang').value = p.lang || 'python';
    $('#newPython').value = p.python_exe || '';
    $('#newVenv').checked = p.make_venv !== false;
    paintNewLang();
    toast('已沿用配置档「' + p.name + '」');
  });
  $('#btnNewCancel').addEventListener('click', () => { $('#newModal').hidden = true; });
  $('#btnCreate').addEventListener('click', createProject);
  // 运行页的旧按钮已经被"一条龙流水线"取代；这几行保留兼容，
  // 但元素可能不存在——不判空就会整天弹「界面出错」红条。
  [['#btnRun', 'run'], ['#btnTest', 'test'], ['#btnInstall', 'install']].forEach(([sel, task]) => {
    const b = $(sel);
    if (b) b.addEventListener('click', () => doRun(task));
  });
  // 流水线的四个步骤：点了就跑对应的那一步
  document.querySelectorAll('.pstep').forEach((s) => {
    s.addEventListener('click', () => {
      if (s.classList.contains('none')) { toast('契约里没有这一步，可以在项目详情里自己填'); return; }
      doRun(s.dataset.task);
    });
  });
  $('#btnRescan').addEventListener('click', scanClean);
  const bs = $('#btnAuditScan');
  if (bs) bs.addEventListener('click', () => runAudit(false));
  const bd = $('#btnAuditDeep');
  if (bd) bd.addEventListener('click', () => runAudit(true));
  // ── 诊断栏 / 项目切换 / 活动带 ──
  const rb = $('#btnRail');
  if (rb) {
    rb.addEventListener('click', () => {
      const off = document.body.classList.toggle('rail-off');
      PANEL.pinned = !off;          // 手点过就以用户为准
      localStorage.setItem('jingwei.railOff', off ? '1' : '0');
      rb.textContent = off ? '⇤' : '⇥';
    });
    if (localStorage.getItem('jingwei.railOff') === '1') {
      document.body.classList.add('rail-off');
      PANEL.pinned = false;
      rb.textContent = '⇤';
    }
  }
  const railBody = $('#railBody');
  if (railBody) {
    railBody.addEventListener('click', (e) => {
      const d = e.target.closest('.diag');
      if (!d) return;
      switchTab('home');
      const target = document.querySelector('.finding[data-id="' + d.dataset.jump + '"]');
      if (target) {
        target.scrollIntoView({ behavior: 'smooth', block: 'center' });
        const det = target.querySelector('details');
        if (det) det.open = true;
      }
    });
  }
  const bsw = $('#btnSwitch');
  const swMenu = $('#switchMenu');
  if (bsw && swMenu) {
    bsw.addEventListener('click', (e) => {
      e.stopPropagation();
      renderSwitcher();
      swMenu.hidden = !swMenu.hidden;
    });
    swMenu.addEventListener('click', (e) => {
      const b = e.target.closest('button[data-path]');
      if (!b) return;
      swMenu.hidden = true;
      load(b.dataset.path).catch((err) => toast(String(err), true));
    });
    document.addEventListener('click', () => { swMenu.hidden = true; });
  }
  const bbOpen = $('#btnBandOpen');
  if (bbOpen) bbOpen.addEventListener('click', () => switchTab('run'));
  const bbClose = $('#btnBandClose');
  if (bbClose) bbClose.addEventListener('click', () => { const b = $('#band'); if (b) b.hidden = true; });

  // ── 编辑器 ──
  if ($('#btnResetSettings')) {
    $('#btnResetSettings').addEventListener('click', () => {
      localStorage.removeItem('jingwei.settings');
      location.reload();
    });
  }
  if ($('#btnEdRefresh')) $('#btnEdRefresh').addEventListener('click', edLoadFiles);
  if ($('#edFilter')) $('#edFilter').addEventListener('input', edPaintFiles);
  if ($('#edFiles')) {
    $('#edFiles').addEventListener('click', (e) => {
      const dir = e.target.closest('.ed-node.dir');
      if (dir) {
        const key = dir.dataset.dir;
        if (ED.closedDirs.has(key)) ED.closedDirs.delete(key);
        else ED.closedDirs.add(key);
        sfx('click');
        edPaintFiles();
        return;
      }
      const f = e.target.closest('.ed-node.file');
      if (f) edOpen(f.dataset.file);
    });
  }
  if ($('#edText')) {
    $('#edText').addEventListener('input', edMarkDirty);
    $('#edText').addEventListener('scroll', () => {
      $('#edGutter').scrollTop = $('#edText').scrollTop;
      edPaintCode();          // 只重画可见窗口，2–4ms
      edPaintCaret();
    }, { passive: true });
    // 光标一动就要更新行列和当前行——「我在第几行第几列」是读代码时的基本坐标
    ['keyup', 'click', 'select', 'focus'].forEach((ev) =>
      $('#edText').addEventListener(ev, edPaintCaret));
    // 写代码的几条基本期待：Tab 缩进、回车继承缩进、Ctrl+S 落盘
    $('#edText').addEventListener('keydown', (e) => {
      const t = e.target;
      const pad = ' '.repeat(SET.indent || 4);
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') { e.preventDefault(); edFindShow('find'); return; }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'g') { e.preventDefault(); edFindShow('line'); return; }
      if ((e.ctrlKey || e.metaKey) && e.altKey && e.key.toLowerCase() === 'l') { e.preventDefault(); edFormat(); return; }
      if (e.key === 'Escape' && $('#edFind') && !$('#edFind').hidden) { e.preventDefault(); edFindHide(); return; }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault();
        if (ED.dirty) edApply(); else toast('没有未应用的改动');
        return;
      }
      if (e.key === 'Tab') {
        e.preventDefault();
        const s = t.selectionStart;
        if (e.shiftKey) {
          // 反缩进：先把这一行开头多余的空格吃掉
          const ls = t.value.lastIndexOf('\n', s - 1) + 1;
          const head = t.value.slice(ls, Math.min(ls + pad.length, s));
          const drop = head.match(/^ +/)[0].length;
          if (drop) {
            t.value = t.value.slice(0, ls) + t.value.slice(ls + drop);
            t.selectionStart = t.selectionEnd = Math.max(ls, s - drop);
          }
        } else {
          t.value = t.value.slice(0, s) + pad + t.value.slice(t.selectionEnd);
          t.selectionStart = t.selectionEnd = s + pad.length;
        }
        edMarkDirty();
        edPaintCaret();
        return;
      }
      if (e.key === 'Enter' && t.selectionStart === t.selectionEnd) {
        // 回车继承上一行缩进；上一行以 { ( [ : 结尾就再多缩一层
        const s = t.selectionStart;
        const ls = t.value.lastIndexOf('\n', s - 1) + 1;
        const cur = t.value.slice(ls, s);
        const head = cur.match(/^[ \t]*/)[0].replace(/\t/g, pad);
        if (!head) return;                    // 顶格的行交给浏览器默认行为
        const extra = /[{\[(:]\s*$/.test(cur) ? pad : '';
        e.preventDefault();
        const ins = '\n' + head + extra;
        t.value = t.value.slice(0, s) + ins + t.value.slice(t.selectionEnd);
        t.selectionStart = t.selectionEnd = s + ins.length;
        edMarkDirty();
        edPaintCaret();
      }
    });
  }
  if ($('#edCrumb')) {
    $('#edCrumb').addEventListener('click', (e) => {
      const b = e.target.closest('button[data-crumb]');
      if (!b || !$('#edFilter')) return;
      // 点面包屑 = 把左边的树筛到这一层，同级文件一眼看全
      const dir = b.dataset.crumb;
      const f = $('#edFilter');
      f.value = dir.includes('/') ? dir.slice(0, dir.lastIndexOf('/') + 1) : '';
      edPaintFiles();
      sfx('click');
    });
  }
  // 标签条：点一下切文件，点 ✕ 关掉
  if ($('#edTabs')) {
    $('#edTabs').addEventListener('click', (e) => {
      const x = e.target.closest('[data-ex]');
      if (x) { e.stopPropagation(); edTabClose(x.dataset.ex); sfx('click'); return; }
      const tab = e.target.closest('[data-etab]');
      if (tab && tab.dataset.etab !== ED.path) { edOpen(tab.dataset.etab); sfx('click'); }
    });
  }
  // 查找栏
  if ($('#edFindInput')) {
    $('#edFindInput').addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); edFindGo(e.shiftKey ? -1 : 1); }
      else if (e.key === 'Escape') { e.preventDefault(); edFindHide(); }
      else if (FIND.mode === 'line' && e.key !== 'Backspace') setTimeout(() => edFindGo(1), 0);
    });
    $('#edFindInput').addEventListener('input', () => { if (FIND.mode === 'line') edFindGo(1); });
    $('#edFindNext').addEventListener('click', () => edFindGo(1));
    $('#edFindPrev').addEventListener('click', () => edFindGo(-1));
    $('#edFindClose').addEventListener('click', edFindHide);
  }
  if ($('#btnEdFormat')) $('#btnEdFormat').addEventListener('click', edFormat);
  if ($('#btnEdApply')) $('#btnEdApply').addEventListener('click', edApply);
  if ($('#btnEdMore')) $('#btnEdMore').addEventListener('click', (e) => edMoreMenu(e.currentTarget));
  if ($('#btnEdBuild')) $('#btnEdBuild').addEventListener('click', () => { switchTab('run'); doRun('build'); });
  // 编辑器里直接跑一次检查（2026-10-22）：复用「问题」栏那条通道，结果按行挂到行号上。
  if ($('#btnEdCheck')) {
    $('#btnEdCheck').addEventListener('click', async () => {
      if (typeof runCheck !== 'function') return;
      await runCheck();
      if (typeof edPaintGutter === 'function' && ED && ED.path) edPaintGutter(true);
    });
  }
  if ($('#btnEdTest')) $('#btnEdTest').addEventListener('click', () => { switchTab('run'); doRun('test'); });
  if ($('#btnEdRun')) $('#btnEdRun').addEventListener('click', () => { switchTab('run'); doRun('run'); });
  // 设置按钮在底部状态栏上：什么时候想改「自动到什么程度」都够得着
  const bs2 = $('#btnSettings');
  if (bs2) {
    bs2.addEventListener('click', () => {
      const p = $('#settingsPanel');
      if (!p) return;
      p.hidden = !p.hidden;
      if (!p.hidden) {
        paintSettingChanges();
        // 打开设置就拉一次网络允许（2026-10-22）：界面只消费 CLI 的 net status。
        if (typeof netRefresh === 'function') netRefresh();
        ['scan', 'analyze', 'contract', 'verify', 'trial'].forEach((k) => {
          const el = $('#set' + k[0].toUpperCase() + k.slice(1));
          if (el) el.checked = !!SET[k];
          if (el) el.onchange = readSettings;
        });
        ['hover', 'autoRecord'].forEach((k) => {
          const el = $('#set' + k[0].toUpperCase() + k.slice(1));
          if (el) { el.checked = !!SET[k]; el.onchange = readSettings; }
        });
        if ($('#setTheme')) $('#setTheme').value = SET.theme;
        if ($('#setDensity')) $('#setDensity').value = SET.density;
        if ($('#setMotion')) $('#setMotion').value = SET.motion;
        if ($('#setFbLevel')) $('#setFbLevel').value = SET.fb;
        if ($('#setScanDepth')) $('#setScanDepth').value = String(SET.scanDepth);
        if ($('#setCleanLevel')) $('#setCleanLevel').value = SET.cleanLevel;
        if ($('#setDelMode')) $('#setDelMode').value = SET.delMode;
        if ($('#setIndent')) $('#setIndent').value = String(SET.indent);
        ['#setTheme', '#setDensity', '#setMotion', '#setFbLevel'].forEach((s) => {
          const el = $(s);
          if (el) el.onchange = readSettings;
        });
        ['#setScanDepth', '#setCleanLevel', '#setDelMode', '#setIndent'].forEach((s) => {
          const el = $(s);
          if (el) el.onchange = readSettings;
        });
        p.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
      }
    });
  }
  const ab = $('#audit');
  if (ab) {
    ab.addEventListener('click', (e) => {
      const b = e.target.closest('button[data-fix]');
      if (b) applyFix(b.dataset.fix, b);
    });
  }
  // 「时不时扫一眼」：切回这个窗口时扫一次，开着不动每 10 分钟扫一次。
  // 都是快扫（毫秒级），结论没变就不重绘，不抢注意力。
  window.addEventListener('focus', () => refreshQuiet(false));
  setInterval(() => { if (!document.hidden) refreshQuiet(false); }, 10 * 60 * 1000);
  // 变更检测要比项目检查勤：它回答的是「我刚才那一下做了什么」，
  // 隔十分钟才说一次就失去意义了。这一趟只 stat 文件，不读内容。
  window.addEventListener('focus', () => loadChanges());
  // 变更检测：在前台 45 秒一次；切到后台就放宽到 3 分钟一次（省电省 CPU）
  setInterval(() => { if (!document.hidden) loadChanges(); }, 45 * 1000);
  setInterval(() => { if (document.hidden) loadChanges(); }, 180 * 1000);
  // 后台时把环境光的动画停掉：没人看还动着，纯浪费 GPU
  const amb = document.querySelector('.ambient');
  document.addEventListener('visibilitychange', () => {
    if (!amb) return;
    amb.style.animationPlayState = document.hidden ? 'paused' : 'running';
    const inner = amb.querySelector('.ambient-i');
    if (inner) inner.style.animationPlayState = document.hidden ? 'paused' : 'running';
  });

  // ── 项目列表：扫描、分页、卡片、附属卡片 ──
  const bscan = $('#btnScan');
  if (bscan) {
    bscan.addEventListener('click', () => scanProjects($('#scanParent').value.trim(), parseInt($('#scanDepth').value, 10)));
  }
  const bsd = $('#btnScanDir');
  if (bsd) {
    bsd.addEventListener('click', () => guard(async () => {
      const dir = await invoke('pick_folder', { title: '选一个放项目的目录' });
      if (dir) await scanProjects(dir, parseInt($('#scanDepth').value, 10));
    }));
  }
  const sp = $('#scanParent');
  if (sp) {
    sp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') scanProjects(sp.value.trim(), parseInt($('#scanDepth').value, 10));
    });
  }
  const pl = $('#projList');
  if (pl) {
    pl.addEventListener('click', (e) => {
      const card = e.target.closest('.banner');
      if (!card) return;
      const p = PS.all.find((x) => x.path === card.dataset.path);
      if (!p) return;
      const act = e.target.closest('button')?.dataset.act;
      if (act === 'detail') { PS.pinned = true; openInline(p); return; }
      // **点横幅 = 全局选中 + 行内展开详情**（用户 2026-10-18 的要求）：
      // 卡片长在**这一条下面**，换一个项目旧卡自动消失、新卡在它下面长出来。
      PS.pinned = false;
      sfx('click');
      openInline(p);
      load(p.path).catch((err) => toast(String(err), true));
    });
    // **悬停 = 跟手的浮卡**（2026-10-18 用户要求，比 IDEA 的提示更克制）：
    // 只有 4 行关键信息、指针事件穿透（不挡点击）、120ms 进 120ms 出。
    // 悬停**不再**展开整块详情——那是点击的事，两者混在一起会"一动就变脸"。
    pl.addEventListener('mouseover', (e) => {
      if (!SET.hover) return;
      const card = e.target.closest('.banner');
      if (!card) return;
      const p = PS.all.find((x) => x.path === card.dataset.path);
      if (!p) return;
      clearTimeout(PS.hoverTimer);
      PS.hoverTimer = setTimeout(() => showHover(p, PS.mx || 0, PS.my || 0), 140);
    });
    pl.addEventListener('mousemove', (e) => {
      PS.mx = e.clientX; PS.my = e.clientY;
      const el = $('#projHover');
      if (el && el.classList.contains('on')) moveHover(e.clientX, e.clientY);
    });
    pl.addEventListener('mouseout', (e) => {
      // 只有真正离开卡片（不是移到卡片内部元素）才收
      const card = e.target.closest('.banner');
      if (!card) return;
      if (card.contains(e.relatedTarget)) return;
      clearTimeout(PS.hoverTimer);
      hideHover();
    });
    pl.addEventListener('mouseleave', () => {
      clearTimeout(PS.hoverTimer);
      hideHover();
    });
  }
  const pg = $('#projPager');
  if (pg) {
    pg.addEventListener('click', (e) => {
      const d = e.target.closest('button')?.dataset.page;
      if (d === 'prev' && PS.page > 0) PS.page--;
      if (d === 'next') PS.page++;
      renderProjList();
      analyzeRange(PS.page * PAGE_SIZE, (PS.page + 1) * PAGE_SIZE);
    });
  }
  // 展开卡的按钮（data-pd）与旧详情卡共用一套动作
  document.addEventListener('click', (e) => {
    const a = e.target.closest('button')?.dataset.pd;
    if (!a || !e.target.closest('.b-expand')) return;
    detailAct(a);
  });

  // **整盘扫描**：C:\ + D:\（跳过规则在引擎里，可随时停）
  const bdrv = $('#btnScanDrives');
  if (bdrv) bdrv.addEventListener('click', () => guard(scanDrives));

  // **找同源**：按需算 Jaccard，不拖慢正常扫描
  const bsim = $('#btnFindSimilar');
  if (bsim) bsim.addEventListener('click', () => guard(findSimilar));

  // **全盘扫描**：默认位置一次扫完（先出结果、后台继续）
  const ball = $('#btnScanAll');
  if (ball) ball.addEventListener('click', () => guard(scanAll));

  // 分组头折叠（扫完的列表是分组的）
  const plist = $('#projList');
  if (plist) plist.addEventListener('click', (e) => {
    const head = e.target.closest('.grp-head');
    if (!head) return;
    head.classList.toggle('closed');
    const body = plist.querySelector('.grp-body[data-grp="' + head.dataset.grp + '"]');
    if (body) body.classList.toggle('closed');
  });

  // 常见位置：一把扫几个放项目的地方
  const bcommon = $('#btnScanCommon');
  if (bcommon) bcommon.addEventListener('click', () => guard(scanCommon));
  // 一进来先把「你手上有哪些项目」摆出来，不用先点任何东西
  if (SET.scan) await guard(scanInit);

  // ── 笔记页：项目卡片（选中/停留）+ 记录与日志 ──
  const nbp = $('#nbProjects');
  if (nbp) {
    nbp.addEventListener('click', (e) => {
      const card = e.target.closest('.banner, .pcard');
      if (!card) return;
      const p = PS.all.find((x) => x.path === card.dataset.npath);
      if (!p) return;
      const act = e.target.closest('button')?.dataset.nact;
      if (act === 'addnote') { NL.sel = p; selectNoteProject(p); newNote('note'); }
      else if (act === 'addrecord') { NL.sel = p; selectNoteProject(p); newNote('record'); }
      else selectNoteProject(p);
    });
    // 停留即选中：默认开，可以在设置里关掉换成点击
    nbp.addEventListener('mouseover', (e) => {
      if (!SET.hover) return;
      const card = e.target.closest('.banner, .pcard');
      if (!card) return;
      const p = PS.all.find((x) => x.path === card.dataset.npath);
      if (!p) return;
      clearTimeout(NL.hoverTimer);
      NL.hoverTimer = setTimeout(() => selectNoteProject(p), 260);
    });
    nbp.addEventListener('mouseleave', () => clearTimeout(NL.hoverTimer));
  }
  const nbi = $('#nbItems');
  if (nbi) {
    nbi.addEventListener('click', (e) => {
      const ref = e.target.closest('[data-ref]');
      if (ref) appendRef(ref.dataset.ref);
    });
  }
  const nbs = $('#btnNbScan');
  if (nbs) {
    nbs.addEventListener('click', () => guard(async () => {
      if (!PS.parent) await scanInit();
      else await scanProjects(PS.parent, PS.depth);
      renderNoteProjects();
    }));
  }
  $('#btnClean').addEventListener('click', async () => {
    if (!S.picked.size) { toast('还没有勾选任何项'); return; }
    await guard(async () => {
      const r = await invoke('clean_items', {
        root: S.root || '',
        paths: Array.from(S.picked),
        permanent: SET.delMode === 'permanent',
      });
      toast('清了 ' + r.removed + ' 项，释放 ' + r.human
        + (r.failed.length ? '，' + r.failed.length + ' 项没删掉' : ''));
      await scanClean();
    });
  });
  if ($('#setSound')) { $('#setSound').checked = !!SET.sound; $('#setSound').onchange = readSettings; }
  $('#btnNewNote').addEventListener('click', () => {
    S.current = null;
    openEditor(null);
    $('#neTitle').focus();
  });
  $('#nbSearch').addEventListener('input', (e) => renderNotes(e.target.value));

  // 打开方式菜单
  $('#btnOpen').addEventListener('click', () => {
    const def = S.openers.find((o) => o.id === S.defaultOpener) || S.openers[0];
    if (def) openIn(def.id);
  });
  $('#btnOpenMenu').addEventListener('click', (e) => {
    e.stopPropagation();
    $('#openMenu').hidden = !$('#openMenu').hidden;
  });
  $('#openMenu').addEventListener('click', async (e) => {
    const star = e.target.closest('[data-star]');
    if (star) {
      e.stopPropagation();
      const id = star.dataset.star;
      const next = S.defaultOpener === id ? null : id;
      await guard(async () => {
        await invoke('set_default_opener', { id: next });
        S.defaultOpener = next;
        paintOpenButton();
        toast(next ? '已设为默认打开方式' : '已取消默认');
      });
      return;
    }
    const btn = e.target.closest('[data-open]');
    $('#openMenu').hidden = true;
    if (btn) return openIn(btn.dataset.open);
    if (e.target.closest('[data-normal]')) return openIn('explorer');
  });
  document.addEventListener('click', () => { $('#openMenu').hidden = true; });

  // 点命令块复制；点卡片头折叠；点链接跳笔记
  document.addEventListener('click', (e) => {
    const cp = e.target.closest('.copy');
    if (cp) {
      copyText(cp.dataset.copy).then(() => {
        cp.classList.add('done');
        cp.querySelector('.cp').textContent = '已复制';
        setTimeout(() => { cp.classList.remove('done'); cp.querySelector('.cp').textContent = '点击复制'; }, 1600);
      });
      return;
    }
    const head = e.target.closest('.rc-head');
    if (head) { head.nextElementSibling.classList.toggle('hide'); return; }
    const nbItem = e.target.closest('.nb-item');
    if (nbItem) {
      const n = (S.data.notes || []).find((x) => x.slug === nbItem.dataset.slug);
      if (n) openEditor(n);
      return;
    }
    const link = e.target.closest('.wikilink');
    if (link) {
      const name = link.dataset.link;
      const found = (S.data.notes || []).find((x) => x.title === name || x.slug === name);
      if (found) openEditor(found);
      else toast('还没有叫《' + name + '》的笔记');
    }
  });
  document.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 's' && $('#neBody')) { e.preventDefault(); saveNote(); }
  });

  await guard(async () => {
    // 兜底：IPC 回来的东西形状不对时不许崩——一次异常就白屏，用户只会以为工具坏了
    const ops = unpack(await invoke('openers'));
    S.openers = ops.state === 'yes' && Array.isArray(ops.data) ? ops.data : [];
    if (ops.state !== 'yes') feedback('warn', '没检测到编辑器', ops.reason);
    const op = unpack(await invoke('default_opener'));
    S.defaultOpener = (op.state === 'yes' && op.data) || null;
    paintOpenButton();
  });
  $('#btnRefreshMachine').addEventListener('click', () => loadMachine(true));
  await listen('run://event', (e) => applyEvent(e.payload));
  // 实时层推来的图谱变化。**失败了要说出来**——静默的实时层比没有还糟：
  // 界面上的数字会一直停在旧值，而用户以为那是新的。
  await listen('graph://changed', (e) => {
    if (typeof window.__onGraphChanged === 'function') window.__onGraphChanged(e.payload);
  });
  // 状态栏：以「主动拉」为主路径（事件推送可能因为权限/时序拿不到，
  // 那样会让状态栏一直空着）。有事件就当加分，没有也照样有数字。
  const pollSample = async () => {
    if (document.hidden) return;
    // 采样拿不到（状态锁被占）时**保持上一次的数字**，别把状态栏清空——
    // 清空会让人以为"机器闲下来了"，那是假的。
    try {
      const sm = unpack(await invoke('sample_now'));
      if (sm.state === 'yes' && sm.data) paintBar(sm.data);
    } catch (e) { /* 拉不到就保持上一次 */ }
  };
  pollSample();
  setInterval(pollSample, 5000);
  await listen('sys://sample', (e) => paintBar(e.payload));
  renderNotes();
  paintNav();
  invoke('initial_project').then((v) => {
    const r = unpack(v);
    if (r.state === 'yes' && r.data) load(r.data).catch((e) => toast(String(e), true));
  });
  // **深链接：`--tab <页名>`**（2026-10-16 补回）。
  //
  // 这条路原来**断过**：`main.rs` 一直在解析 `--tab`、也一直在提供 `initial_tab`
  // 命令，而**界面里没人调它**——于是 `jingwei-gui.exe <目录> --tab graph`
  // 只会停在概览页，而界面门禁（靠 `--tab` 逐页巡查）拿到的**每一张报告都是同一页**。
  //
  // 那是个很不显眼的坏法：命令有、参数解析有、报告也照样生成，
  // 只是每页的报告都是概览页。**所以它只能由真窗口门禁守**——单测和 CLI 发现不了。
  // **深链接要等"项目就位"再切页**（2026-10-16 修，真竞态）。
  //
  // `initial_project` 与 `initial_tab` 是**两条独立的 invoke**，
  // 谁先回来不定。而 `switchTab('graph')` 里那句是
  // `if (name === 'graph' && S.root) { graphWire(); graphLoad(true) }`——
  // **`S.root` 还没设上就跳过了**，于是页面切过去了、数据永远不拉：
  // `#ghSub` 永远停在「正在读取…」。
  //
  // 症状（自检报告里原样可见）：`activeView=view-graph` 而 `#ghSub="正在读取…"`。
  // 判定为"页切过去了但数据没拉"那个竞态——**而根因是切页早于项目加载**。
  //
  // 修法：把"切页"挂到项目加载之后；项目一直没来就 8 秒后再切
  // （那时候切过去至少界面是对的，只是没数据）。
  (async function deepLink() {
    const tabR = unpack(await invoke('initial_tab'));
    const tab = tabR.state === 'yes' ? tabR.data : null;
    if (typeof tab !== 'string' || !tab) return;
    const t0 = Date.now();
    while (!S.root && Date.now() - t0 < 8000) {
      await new Promise((r) => setTimeout(r, 120));
    }
    switchTab(tab);
  })().catch(() => { /* 没有深链接就用默认页，不打扰人 */ });
});

// ── 底部状态栏：数据由 Rust 推，前端只负责画 ─────────────
const cpuHist = [];
function drawSpark() {
  const c = $('#spark'), g = c.getContext('2d');
  g.clearRect(0, 0, c.width, c.height);
  if (cpuHist.length < 2) return;
  g.beginPath();
  cpuHist.forEach((v, i) => {
    const x = i / (cpuHist.length - 1) * c.width;
    const y = c.height - Math.min(v, 100) / 100 * (c.height - 2) - 1;
    if (i) g.lineTo(x, y); else g.moveTo(x, y);
  });
  g.strokeStyle = '#E09A75';
  g.lineWidth = 1.5;
  g.stroke();
}
function paintBar(s) {
  // 采样出问题时也要说清楚，而不是让状态栏一直挂个「—」
  if (!s || typeof s.cpu_percent !== 'number') {
    $('#barAt').textContent = '采样没拿到数据';
    return;
  }
  $('#barCpu').textContent = s.cpu_percent.toFixed(0) + '%';
  $('#barMem').textContent = (s.mem_used_mb / 1024).toFixed(1) + ' / '
    + (s.mem_total_mb / 1024).toFixed(1) + ' GB';
  $('#barDisk').textContent = s.disk_free_gb.toFixed(0) + ' GB 可用';
  // 解释器进程：一次快照分别数几种。都没有就说清楚是「没有」，而不是挂个 0。
  const dp = (s.dev_procs || []).filter((x) => x[1] > 0);
  const bp = $('#barPy');
  bp.textContent = dp.length ? dp.map((x) => x[0] + ' ' + x[1]).join(' · ') : '没有解释器在跑';
  bp.dataset.tip = '按进程名数的：python / node / java / cargo / rustc / dotnet。'
    + '跑起来的东西在这儿能看见，不用去任务管理器找。';
  $('#barAt').textContent = new Date().toLocaleTimeString('zh-CN', { hour12: false }) + ' 采样';
  // CPU 曲线只在真的有东西在跑的时候画。没在跑就没人看它，白占 GPU。
  const sp = $('#spark');
  if (PANEL.running) {
    if (sp) sp.hidden = false;
    cpuHist.push(s.cpu_percent);
    while (cpuHist.length > 48) cpuHist.shift();
    drawSpark();
  } else if (cpuHist.length) {
    cpuHist.length = 0;
    if (sp) {
      sp.getContext('2d').clearRect(0, 0, sp.width, sp.height);
      sp.hidden = true;
    }
  } else if (sp && !sp.hidden) {
    sp.hidden = true;
  }
}

// ── 详细区：平时不取；点开先摆上次的，再刷新 ─────────────
const DEEP = { opened: false, at: null, m: null, d: null };
const kv = (k, v) => '<div class="fact"><div class="k">' + esc(k) + '</div><div class="v" title="'
  + esc(v) + '">' + esc(v) + '</div></div>';

function renderDeep(stale) {
  const m = DEEP.m, D = DEEP.d;
  let h = '';
  if (DEEP.at) {
    h += '<div class="hint">上次获取 ' + DEEP.at.toLocaleTimeString('zh-CN', { hour12: false })
      + (stale ? ' · 正在刷新…' : '') + '</div>';
  }
  if (m) {
    const x = m.machine || {};
    // 这一块只放**快路径拿不到的**：换个地方再画一遍 CPU/内存没有意义，
    // 用户要的是"深入一层多看到了什么"。
    const tools = Object.entries(m.tools || {});
    if (tools.length) {
      h += '<h3 class="secTitle">工具链的出处（快读只给版本，这里给路径）</h3><div class="dgrid">'
        + tools.map(([k, t]) => kv(k, t.ok ? (t.version || '（没报版本）') + '  ' + (t.path || '') : '没找到')).join('')
        + '</div>';
    }
    h += '<h3 class="secTitle">注册表原文（和上面显示名不一样是正常的）</h3><div class="dgrid">'
      + kv('ProductName', x.os_product || '—')
      + kv('DisplayVersion', x.os_version || '—')
      + kv('CurrentBuild', x.os_build || '—')
      + kv('盘符', (x.disks || []).map((d) => d.drive).join('  ') || '—')
      + '</div>';
    const env = Object.entries(m.env || {});
    if (env.length) {
      h += '<h3 class="secTitle">会影响行为的环境变量</h3><div class="dgrid">'
        + env.map(([k, v]) => v.length > 90
          ? '<div class="fact"><div class="k">' + esc(k) + '</div><div class="v long">'
            + '<details><summary>' + esc(v.slice(0, 60)) + '…（点击展开全部 ' + v.length + ' 字符）</summary>'
            + '<div class="fullval">' + esc(v) + '</div></details></div></div>'
          : kv(k, v)).join('')
        + '</div>';
    }
  }
  if (D) {
    if (!D.ok) {
      h += '<div class="finding warn"><div class="ftitle">' + esc(D.why) + '</div></div>';
    } else {
      h += '<h3 class="secTitle">解释器底下那一层</h3><div class="dgrid">'
        + kv('解释器', D.exe)
        + kv('位数 / 架构', D.bits + ' 位 · ' + D.arch)
        + kv('ABI 标签', D.abi || '—')
        + kv('site-packages', (D.siteDirs || []).join('\n'))
        + kv('._pth 文件', (D.pthFiles || []).length ? D.pthFiles.join('\n') : '无（不是便携版）')
        + kv('能否编译扩展', (D.build.has_cl ? '有 cl.exe' : '没有 cl.exe（装不了源码包）')
            + ' · setuptools ' + (D.build.has_setuptools ? '有' : '无')
            + ' · wheel ' + (D.build.has_wheel ? '有' : '无'))
        + '</div>';
      // 原生扩展：几十个同类问题只出**一张卡**（总数 + 涉及哪些包 + 几个例子）。
      // 一条文件一张卡就是刷屏——那是这个工具最该避免的事。
      const ns = D.native || { total: (D.issues || []).length, samples: D.issues || [] };
      h += '<h3 class="secTitle">原生扩展（.pyd）</h3>';
      if (!ns.total) {
        h += '<div class="finding info"><div class="ftitle"><span class="chip info">正常</span>'
          + '<span>' + esc(ns.verdict || '原生扩展都对得上') + '</span></div>'
          + '<div class="boundary">扫过的 .pyd 都解析得到依赖，位数也对得上。</div></div>';
      } else {
        const sev = ns.arch_mismatch ? 'error' : 'warn';
        h += '<div class="finding ' + sev + '">'
          + '<div class="ftitle"><span class="chip ' + sev + '">'
          + (ns.arch_mismatch ? '位数不对' : '缺依赖') + '</span>'
          + '<span>' + esc(ns.verdict) + '</span></div>'
          + ((ns.packages || []).length
            ? '<div class="boundary">涉及：' + (ns.packages).map((p) => esc(p[0]) + '（' + p[1] + '）').join('、') + '</div>'
            : '')
          + '<details><summary>看两个例子</summary><ul>'
          + (ns.samples || []).map((i) => '<li>' + esc(i.pyd)
              + (i.missing && i.missing.length ? ' —— 找不到 ' + esc(i.missing.join(', ')) : '')
              + '（它是 ' + esc(i.arch) + '）</li>').join('')
          + '</ul></details>'
          + (ns.hint ? '<div class="next">下一步 · ' + esc(ns.hint) + '</div>' : '')
          + '</div>';
      }
    }
  }
  $('#deepBody').innerHTML = h;
}

/// 没有项目也能看。硬件那一段要问 CIM，约一秒，所以只在进这一页时才去取。
const HW = { at: null, data: null };
async function loadMachine(force) {
  if (HW.at && !force) {
    renderHardware(true);
    return;
  }
  if (HW.at) renderHardware(true); // 先把上次的摆出来，别留空窗
  $('#deepWhen').textContent = '读取中…';
  await guard(async () => {
    // 主路径是毫秒级的那条；WMI 慢到不能用，改成按钮让人自己点
    const [hw, m, d] = await Promise.all([
      invoke('hardware_fast'),
      invoke('machine_report'),
      invoke('deep_check', { project: S.root || null }),
    ]);
    // 硬件这两路也会回信封：读不到时**如实说读不到**，
    // 而不是给一个空对象让界面显示成"这台机器没有这些东西"。
    const h = unpack(hw);
    HW.data = h.state === 'yes' ? h.data : {};
    if (h.state !== 'yes') feedback('warn', '硬件信息没读全', h.reason);
    const mm = unpack(m);
    DEEP.m = mm.state === 'yes' ? mm.data : {};
    if (mm.state !== 'yes') feedback('warn', '机器清单没读全', mm.reason);
    HW.at = new Date();
    DEEP.d = d; DEEP.at = HW.at;
    renderHardware(false);
    renderDeep(false);
  });
}

function renderHardware(stale) {
  const h = HW.data;
  const when = HW.at ? HW.at.toLocaleTimeString('zh-CN', { hour12: false }) : '';
  $('#deepWhen').textContent = when ? ('读取于 ' + when + (stale ? '，正在刷新…' : '')) : '';
  const m = (DEEP.m && DEEP.m.machine) || {};
  const part = (title, rows) => rows.filter(Boolean).length
    ? '<h3 class="secTitle">' + title + '</h3><div class="dgrid">' + rows.filter(Boolean).join('') + '</div>'
    : '';
  let s = '';
  if (h && h.cpu) {
    s += part('硬件（读注册表，毫秒级）', [
      kv('处理器', h.cpu),
      kv('主频 / 核心', (h.cpu_mhz ? (h.cpu_mhz / 1000).toFixed(2) + ' GHz · ' : '')
        + (h.cores || '?') + ' 个逻辑处理器'),
      kv('显卡', (h.gpus || []).join('   ') || '—'),
      kv('整机', h.system_model || '—'),
      kv('主板', h.baseboard || '—'),
      kv('BIOS', h.bios || '—'),
      kv('内存', h.ram_total_gb ? h.ram_total_gb.toFixed(1) + ' GB' : '—'),
      kv('屏幕', h.screen || '—'),
      kv('磁盘', (h.disks || []).map((d) => d.drive + ' 剩余 ' + d.free_gb.toFixed(0)
        + '/' + d.total_gb.toFixed(0) + ' GB').join('   ')),
      ((h.ram || []).length
        ? kv('内存条', h.ram.map((r) => (r.size_mb / 1024).toFixed(0) + 'GB ' + r.kind
            + (r.speed ? ' ' + r.speed + 'MT/s' : '')
            + (r.maker ? ' ' + r.maker : '') + (r.part ? ' ' + r.part : '')).join('   '))
        : ''),
      ((h.disks_model || []).length ? kv('硬盘型号', h.disks_model.join('   ')) : ''),
    ]);
  } else {
    s += part('硬件', [
      kv('处理器', m.cpu || '—'),
      kv('核心', m.cores ? m.cores + ' 个逻辑处理器' : '—'),
      kv('内存', m.ram_total_mb ? (m.ram_total_mb / 1024).toFixed(1) + ' GB' : '—'),
      kv('磁盘', (m.disks || []).map((d) => d.drive + ' 剩余 ' + d.free_gb.toFixed(0)
        + '/' + d.total_gb.toFixed(0) + ' GB').join('   ')),
    ]);
  }
  s += part('系统', [
    kv('版本', (m.os_name || '') + ' ' + (m.os_version || '') + '  build ' + (m.os_build || '')),
    kv('注册表原文', m.os_product || '—'),
  ]);
  const d = HW.deep;
  if (d && d.cpu) {
    const c = d.cpu || {};
    s += part('再深一层（WMI 读到的）', [
      kv('内存条', (d.ram || []).map((r) => (r.gb || '?') + 'GB ' + (r.mhz || '?')
        + 'MHz ' + [r.maker, r.part].filter(Boolean).join(' ')).join('   ') || '—'),
      kv('硬盘型号', (d.disk || []).map((x) => (x.model || '?') + ' ' + (x.gb || '?')
        + 'GB ' + (x.bus || '')).join('   ') || '—'),
      kv('已安装软件', d.programs == null ? '—' : d.programs + ' 个'),
      kv('BIOS 版本', (d.bios && d.bios.ver) || '—'),
    ]);
  }
  $('#hwBody').innerHTML = s;
}

