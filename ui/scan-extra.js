// **扫盘的两个附加能力**（2026-10-19）：
//   ① 整盘扫描（C:\ D:\，靠已有的跳过规则剪枝，可取消）
//   ② 找同源（Jaccard ≥ 0.8，按需触发——它要重走目录，别拖慢正常扫描）
//
// 两个都遵守同一条纪律：**有终态、可取消、不伪造进度**。

/// 整盘扫描：把盘根交给同一个后端（它已经有"跳过系统目录/符号链接"的规则）。
/// 深度默认 5：够到 `C:\Users\x\source\repos\proj`，又不会把整个用户目录翻穿。
async function scanDrives() {
  const roots = ['C:\\', 'D:\\'];
  scanProgress('<span class="sp-bar"></span><span>正在整盘扫（C:\\ · D:\\）…系统目录会自动跳过</span>');
  let started;
  try {
    started = await invoke('scan_start', { roots: roots, depth: 6 });
  } catch (e) {
    scanProgress('<span>启动失败：' + esc(String(e)) + '</span>', true);
    return;
  }
  if (!started || started.ok === false) {
    scanProgress('<span>' + esc((started && started.why) || '启动失败') + '</span>', true);
    return;
  }
  PS.scanning = true;
  pollScan(true);
}

/// 轮询统一走这里（默认扫与整盘扫共用）：`driveMode` 只影响文案。
function pollScan(driveMode) {
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
      scanProgress('<span class="sp-bar"></span><span>' + (driveMode ? '整盘扫描' : '正在扫')
        + '：已看 <b>' + st.visited + '</b> 个目录 · 发现 <b>' + found.length + '</b> 个项目'
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

/// **找同源**：把结果画到卡片上（`同源 88% · 另一个路径` 挂在 title 上）。
/// 判据与 CLI 完全一致：同名目录 + Jaccard ≥ 0.8。
async function findSimilar() {
  const btn = $('#btnFindSimilar');
  if (btn) { btn.disabled = true; btn.textContent = '正在比…'; }
  try {
    const r = await invoke('scan_similar', { min: 0.8 });
    const pairs = (r && r.pairs) || [];
    // 清掉旧徽章
    document.querySelectorAll('.b-sim').forEach((el) => el.remove());
    const byPath = new Map();
    for (const p of pairs) {
      const add = (path, other, score) => {
        if (!byPath.has(path)) byPath.set(path, []);
        byPath.get(path).push(esc(shortPath(other)) + ' ' + Math.round(score * 100) + '%');
      };
      add(p.a, p.b, p.score);
      add(p.b, p.a, p.score);
    }
    for (const [path, list] of byPath) {
      const card = Array.from(document.querySelectorAll('#projList .banner'))
        .find((el) => el.dataset.path === path);
      if (!card) continue;
      const span = document.createElement('span');
      span.className = 'b-sim';
      span.textContent = '同源 ' + list.length;
      span.title = '与这些目录同源（Jaccard ≥ 0.8）：\n' + list.join('\n');
      card.querySelector('.b-name')?.appendChild(span);
    }
    scanProgress('<span>找同源：<b>' + pairs.length + '</b> 对（同名目录 + Jaccard ≥ 0.8）</span>', true);
  } catch (e) {
    scanProgress('<span>找同源失败：' + esc(String(e)) + '</span>', true);
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = '找同源'; }
  }
}
