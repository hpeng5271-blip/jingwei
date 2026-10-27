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

/// **同一份**：按指纹统计出现次数（纯函数，外壳测试可直接验）。
/// 判据只有一条：指纹字符串相等。**不做相似度**（那是下一步，别混在一起）。
function fingerprintCounts(list) {
  const m = new Map();
  for (const p of list || []) {
    const fp = p.fp;
    if (!fp) continue;
    m.set(fp, (m.get(fp) || 0) + 1);
  }
  return m;
}

/// 这条卡片是不是"同一份"之一（≥2 个相同指纹）。
function duplicateOf(p, counts) {
  if (!p || !p.fp) return 0;
  const n = counts.get(p.fp) || 0;
  return n > 1 ? n : 0;
}

/// 进度行：扫的时候一直在，扫完变终态（done 由后端给）。
function scanProgress(html, done) {
  const prog = $('#scanProg');
  if (!prog) return;
  prog.hidden = false;
  prog.classList.toggle('done', !!done);
  prog.innerHTML = html;
}

