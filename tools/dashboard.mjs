#!/usr/bin/env node
// 静态 dashboard：零依赖，数据全部来自 `jw --json`。
// 用法：node tools/dashboard.mjs <repoA> [repoB] [out.html]
import { spawnSync } from "node:child_process";
import { writeFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

const JW = resolve(process.env.JW || "target/debug/jw.exe");
const jw = (args, cwd) => {
  const r = spawnSync(JW, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(`jw ${args.join(" ")} 失败（${r.status}）`);
  return JSON.parse(r.stdout);
};
const esc = (s) =>
  String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

const argv = process.argv.slice(2);
const htmlArg = argv.find((x) => x.endsWith(".html"));
const rest = argv.filter((x) => x !== htmlArg);
const [a, b] = rest;
const outArg = htmlArg;
if (!a) {
  console.error("用法：node tools/dashboard.mjs <repoA> [repoB] [out.html]");
  process.exit(2);
}
const out = resolve(outArg || `dashboard-${Date.now()}.html`);
const tl = jw(["timeline", "--json", "--top", "10", "."], a);
const es = jw(["edges", "--stats", "--json", "."], a);
let cov = null;
try {
  cov = jw(["coverage", "--json", "."], a);
} catch {
  cov = null;
}
// 未跑过 parse：显示“未解析”，不许静默 0（H-obs）
const parsed =
  cov && typeof cov.parsed === "number" ? cov.parsed > 0 : (es.use?.total ?? 0) > 0;
const drift = b ? jw(["cross", resolve(a), resolve(b), "--dep", "--json"], a) : null;
const fc = tl.file_counts || {};
const days = (tl.days || []).slice(-30).reverse();
const h = [];

h.push(`<!doctype html><meta charset="utf-8"><title>${esc(tl.repo)} · 经纬 dashboard</title>`);
h.push("<style>body{font:14px/1.5 ui-monospace,Consolas,monospace;margin:24px;color:#111}h1{font-size:18px;margin:0 0 4px}h2{font-size:14px;margin:22px 0 6px;color:#333}table{border-collapse:collapse}td,th{padding:2px 10px 2px 0;text-align:left}th{color:#666;font-weight:normal}td.n{text-align:right}.muted{color:#888}</style>");
h.push(`<h1>${esc(tl.repo)}</h1>`);
h.push(`<div class="muted">${tl.commits_total} 提交 · ${tl.changes} 改动 · 扫描 ${tl.scanned}</div>`);

h.push(`<h2>时间线</h2>`);
h.push(
  `<div>文件 <b>${fc.historical ?? "?"}</b> 历史 · <b>${fc.current ?? "?"}</b> 当前 · <b>${fc.deleted ?? "?"}</b> 已删</div>`
);
h.push(`<table><tr><th>日期</th><th>提交</th><th>新增</th><th>改动</th><th>删除</th><th>累计</th></tr>`);
for (const d of days) {
  h.push(
    `<tr><td>${esc(d.day)}</td><td class="n">${d.commits}</td><td class="n">${d.added}</td>` +
      `<td class="n">${d.modified}</td><td class="n">${d.deleted}</td><td class="n">${d.cumulative_files}</td></tr>`
  );
}
h.push(`</table><div class="muted">仅显示最近 ${days.length} 天</div>`);

h.push(`<h2>最近改动 Top10</h2><table><tr><th>#</th><th>文件</th><th>改动</th></tr>`);
(tl.top || []).forEach((r, i) =>
  h.push(`<tr><td class="n">${i + 1}</td><td>${esc(r.path)}</td><td class="n">${r.n}</td></tr>`)
);
h.push(`</table>`);

const u = es.use || {}, im = es.impl || {};
h.push(`<h2>边层</h2>`);
if (!parsed) {
  h.push(`<div class="muted">未解析（先跑 <code>jw parse</code>）——这里不是 0，是没有数据</div>`);
} else {
  h.push(
    `<div>导入 <b>${u.total ?? 0}</b>（内部 <b>${u.internal ?? 0}</b> / ext <b>${u.ext ?? 0}</b>）· ` +
      `实现 <b>${im.internal ?? 0}</b>（ext ${im.ext ?? 0} / ? ${im.unquestioned ?? 0}）· ` +
      `未解析 ${u.unresolved ?? 0}</div>`
  );
  h.push(`<table><tr><th>use 最多的文件</th><th>use 数</th></tr>`);
  for (const r of es.top_use || []) h.push(`<tr><td>${esc(r.path)}</td><td class="n">${r.n}</td></tr>`);
  h.push(`</table>`);
}

if (drift) {
  const d = drift.drift || {};
  h.push(`<h2>跨仓依赖漂移</h2>`);
  h.push(
    `<div>共有 <b>${drift.shared}</b> · 不同 <b>${drift.diff}</b> · same ${d.same_major ?? 0} / ` +
      `minor ${d.minor ?? 0} / major ${d.major ?? 0} / unknown ${d.unknown ?? 0}</div>`
  );
  h.push(`<table><tr><th>依赖</th><th>A</th><th>B</th><th>漂移</th></tr>`);
  for (const r of (drift.rows || []).slice(0, 20)) {
    h.push(`<tr><td>${esc(r.name)}</td><td>${esc(r.a)}</td><td>${esc(r.b)}</td><td>${esc(r.drift)}</td></tr>`);
  }
  h.push(`</table>`);
}

const html = h.join("\n");
if (!existsSync(dirname(out))) mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, html, "utf8");
console.log(JSON.stringify({ out, bytes: Buffer.byteLength(html), parsed }));
