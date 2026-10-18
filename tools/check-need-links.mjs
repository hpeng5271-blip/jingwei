// **核一遍我们给出的官方链接还活着**（2026-10-18）。
//
// ## 为什么需要它
//
// `toolchain_need.rs` 里 12 条缺件规则现在都带一个官方链接（`url`）。
// 链接是会烂的：站点改版、路径改名、仓库归档——**而烂掉的链接比不给更坏**
// （用户照它去下，落到 404 或者更糟：落到一个抢注的域名上）。
//
// ## 为什么**不进常规门禁**
//
// 它要联网。门禁必须离线可跑、可重复——一个因为"今天网断了"就红的门禁，
// 会训练人忽略它（这条教训在 `框架.md` 里写着）。
//
// **什么时候跑**：改了 `toolchain_need.rs` 的链接之后、发版之前、或者哪天怀疑链接烂了。
//
// ## 用法
//
//     node tools/check-need-links.mjs
//
// 退出码：0 = 全部可达；1 = 有不可达（会逐条列出状态码）。
//
// ## 边界
//
// - 只对 `https://` 的地址发 **HEAD**（不发 GET，不下载内容）；
// - 跟随最多 5 次重定向（官方站点常有 http→https、裸域→www 的跳转）；
// - **不判断内容对不对**，只判断"这个地址还回话"。内容对不对要靠人看。
//
// ## 已知局限：这台机器上 Node 连不上 github.com
//
// 实测（2026-10-18）：Node 的 `fetch` 对 github.com **一律 `UND_ERR_CONNECT_TIMEOUT`**，
// 而同一时刻 PowerShell 的 `Invoke-WebRequest -Method Head` 对同样的地址回 **200**。
// 所以工具把这种情况报成「**查不了**」（退出码 2），并提示用 PowerShell 交叉核——
// **不报成"链接坏了"**：那是把"我们问不到"说成"它不存在"。

import fs from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

/// 从某个文件里抓出所有 `url: "https://…"` 与 JSON 里的 `"url": "https://…"`。
function urlsIn(rel) {
  const p = join(ROOT, rel);
  if (!fs.existsSync(p)) return [];
  const src = fs.readFileSync(p, 'utf8');
  const out = new Set();
  for (const m of src.matchAll(/"(https:\/\/[^"]+)"|url:\s*"(https:\/\/[^"]+)"/g)) {
    const u = m[1] || m[2];
    if (u) out.add(u);
  }
  return [...out];
}

const SOURCES = [
  'crates/engine/src/launch/toolchain_need.rs',
  'tools/toolchains.json',
  '工具链-便携与安装.md',
];

const all = new Map(); // url -> [来源…]
for (const s of SOURCES) {
  for (const u of urlsIn(s)) {
    if (!all.has(u)) all.set(u, []);
    all.get(u).push(s);
  }
}

if (!all.size) {
  console.error('没找到任何 https 链接——判据失效（不是"全部通过"）');
  process.exit(1);
}

console.log(`  核 ${all.size} 个链接（来自 ${SOURCES.length} 处）\n`);

// **三态，不许压成两态**（2026-10-18 第一版就栽在这里）：
//   ok      = 地址回 2xx          → 链接活着
//   FAIL    = 地址回了 4xx/5xx    → 链接真的坏了（要么换，要么删）
//   查不了  = 根本连不上（DNS/TLS/代理）→ **这不是"链接坏"，是我们的网络出不去**
//
// 第一版把"连不上"也写成 FAIL，于是 3 个 GitHub 地址被报成坏的——
// 而同一时刻 PowerShell 的 HEAD 对它们是 200。**先怀疑量具，再怀疑被测物。**
let bad = 0;
let unknown = 0;
for (const [u, from] of all) {
  let line;
  try {
    const r = await fetch(u, { method: 'HEAD', redirect: 'follow', signal: AbortSignal.timeout(25_000) });
    if (r.ok) {
      line = `  ok   ${r.status}  ${u}`;
    } else {
      // 有些站点不让 HEAD（405/403），但地址是活的——再试一次 GET，只取状态码不读正文。
      const g = await fetch(u, { method: 'GET', redirect: 'follow', signal: AbortSignal.timeout(25_000) });
      if (g.ok) {
        line = `  ok   ${g.status}（HEAD 不给，GET 可以）  ${u}`;
      } else {
        line = ` FAIL  ${g.status}  ${u}`;
        bad++;
      }
    }
  } catch (e) {
    // 把 cause 也打出来——"fetch failed" 本身不说明任何事。
    const why = e.cause ? (e.cause.code || e.cause.message || String(e.cause)) : e.message;
    line = `  ??  查不了：${String(why).slice(0, 40)}  ${u}`;
    unknown++;
  }
  console.log(line);
  console.log(`       （来自 ${from.join(' / ')}）`);
}

console.log('');
if (bad) {
  console.log(`  ✗ ${bad} 个地址回了错误码——**这在产品里就是"我给了你一个打不开的地址"**，要么换，要么删掉那一条的 url。`);
  process.exit(1);
}
if (unknown) {
  console.log(`  ? ${unknown} 个**查不了**（连不上，不是地址坏）：换一个网络/关掉代理再跑一次，`);
  console.log('    或者用 PowerShell 交叉核一下：');
  console.log('      Invoke-WebRequest -Uri <地址> -Method Head -MaximumRedirection 5');
  console.log('    **在这之前不要说"链接坏了"——我们还没能问它。**');
  process.exit(2);
}
console.log('  ✓ 全部可达（只证明"还回话"，不证明内容对——内容要人看）。');
