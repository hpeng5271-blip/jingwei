// **扫盘与分组**（2026-10-19）：全盘扫描 + 首页按类型/体量分组。
//
// 为什么要单独一个文件：shell.js 已经 900+ 行，而这一块是**自成一件事**——
// 扫描的进度/终态/分组口径都在这里，读的人不用在 shell.js 里翻。
//
// 三个纪律：
//   ① **终态只由 Rust 的 done 决定**，前端不靠"多久没收到"去猜；
//   ② 进度用**不确定态 + 实时计数**（总量未知时不伪造百分比）；
//   ③ 分组是纯函数（`groupFound`），外壳测试可以直接喂数据验它。

/// 体量分档：文件数。阈值取 50 / 500 / 4000（小项目 / 一般 / 大型 / 巨型）。
function sizeOf(n) {
  const v = Number(n) || 0;
  if (v < 50) return '小';
  if (v < 500) return '中';
  if (v < 4000) return '大';
  return '超大';
}

/// 类型：先看**标记文件判出来的 kinds**（引擎给的），映射成首页要的分组名。
/// 兜底顺序照 GitHub linguist 的思路：判不出来就"其他"，**不猜**。
function typeOf(p) {
  const k = (p.kinds && p.kinds[0]) || '未知';
  if (/Rust/.test(k)) return 'Rust';
  if (/前端|Node|TypeScript|JavaScript/.test(k)) return '前端 / Node';
  if (/Java|Kotlin/.test(k)) return 'Java';
  if (/Python/.test(k)) return 'Python';
  if (/C \/ C\+\+|C\+\+/.test(k)) return 'C / C++';
  return '其他';
}

/// **分组**：类型 → 体量。纯函数。
function groupFound(list) {
  const typeOrder = ['Rust', '前端 / Node', 'Java', 'Python', 'C / C++', '其他'];
  const sizeOrder = ['小', '中', '大', '超大'];
  const groups = new Map();
  for (const p of list || []) {
    const t = typeOf(p);
    const s = sizeOf(p.files);
    const key = t + '|' + s;
    if (!groups.has(key)) groups.set(key, { type: t, size: s, items: [] });
    groups.get(key).items.push(p);
  }
  const out = Array.from(groups.values());
  out.sort((a, b) => {
    const ta = typeOrder.indexOf(a.type);
    const tb = typeOrder.indexOf(b.type);
    if (ta !== tb) return ta - tb;
    return sizeOrder.indexOf(a.size) - sizeOrder.indexOf(b.size);
  });
  for (const g of out) g.items.sort((a, b) => (Number(b.files) || 0) - (Number(a.files) || 0));
  return out;
}

function shortPath(p) {
  const parts = String(p).split(/[\\/]/);
  return parts.length <= 2 ? p : '…/' + parts.slice(-2).join('/');
}

/// 画分组列表：每组一个可折叠的头 + 若干横幅。
function renderGrouped(list) {
  const box = $('#projList');
  if (!box) return;
  const groups = groupFound(list);
  if (!groups.length) {
    box.innerHTML = '<div class="hint" style="padding:10px">还没扫到项目——点「全盘扫描」。</div>';
    return;
  }
  const card = (p) => {
    const info = PS.info[p.path];
    const on = bannerOn(p.path);
    const kind = (p.kinds && p.kinds[0]) || '目录';
    return '<div class="banner stagger' + (on ? ' on' : '') + '" data-path="' + esc(p.path) + '">'
      + '<div><div class="b-name">' + esc(p.name)
      + '<span class="b-kind">' + esc(String(kind).toLowerCase()) + '</span></div>'
      + '<div class="b-path" data-tip="' + esc(p.path) + '">' + esc(p.path) + '</div></div>'
      + '<div class="b-facts">' + projFacts(p, info) + '</div>'
      + '<div class="b-acts"><button class="btn ghost sm" data-act="detail">详情</button>'
      + '<button class="btn sm" data-act="open">打开</button></div></div>';
  };
  box.innerHTML = groups.map((g) => {
    const key = esc(g.type + '|' + g.size);
    return '<div class="grp-head" data-grp="' + key + '"><span class="grp-caret">▾</span>'
      + '<b>' + esc(g.type) + '</b><span>·</span><span>' + esc(g.size) + '</span>'
      + '<span class="grp-n">' + g.items.length + ' 个</span></div>'
      + '<div class="grp-body" data-grp="' + key + '">' + g.items.map(card).join('') + '</div>';
  }).join('');
}

/// 进度行：扫的时候一直在，扫完变终态（done 由后端给）。
function scanProgress(html, done) {
  const prog = $('#scanProg');
  if (!prog) return;
  prog.hidden = false;
  prog.classList.toggle('done', !!done);
  prog.innerHTML = html;
}

/// **全盘扫描**：先出结果、后台扫完。
async function scanAll() {
  scanProgress('<span class="sp-bar"></span><span>正在扫…</span>');
  let started;
  try {
    started = await invoke('scan_start', { roots: [], depth: 4 });
  } catch (e) {
    scanProgress('<span>启动失败：' + esc(String(e)) + '</span>', true);
    return;
  }
  if (!started || started.ok === false) {
    scanProgress('<span>' + esc((started && started.why) || '启动失败') + '</span>', true);
    return;
  }
  PS.scanning = true;
  const tick = async () => {
    let st;
    try {
      st = await invoke('scan_status');
    } catch (e) {
      PS.scanning = false;
      scanProgress('<span>读不到扫描状态：' + esc(String(e)) + '</span>', true);
      return;
    }
    const found = (st.found || []).slice();
    PS.all = found;
    renderGrouped(found);
    if (st.running) {
      scanProgress('<span class="sp-bar"></span><span>正在扫：已看 <b>' + st.visited
        + '</b> 个目录 · 发现 <b>' + found.length + '</b> 个项目'
        + (st.current ? ' · ' + esc(shortPath(st.current)) : '')
        + '</span><button class="btn ghost sm" id="btnScanStop">停下</button>');
      const stopBtn = $('#btnScanStop');
      if (stopBtn) stopBtn.addEventListener('click', () => invoke('scan_cancel'));
      setTimeout(tick, 300);
    } else {
      PS.scanning = false;
      scanProgress('<span>' + (st.cancelled ? '已停止' : '✓ 扫完') + '：看了 <b>' + st.visited
        + '</b> 个目录 · 发现 <b>' + found.length + '</b> 个项目 · 用时 '
        + Number(st.seconds || 0).toFixed(1) + ' 秒</span>', true);
      if (typeof renderRailProjs === 'function') renderRailProjs();
      if (SET.analyze && typeof analyzeRange === 'function') {
        analyzeRange(0, Math.min(PS.all.length, PAGE_SIZE));
      }
    }
  };
  tick();
}
