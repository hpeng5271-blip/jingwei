#!/usr/bin/env node
// 边层最小前端：起 `jw serve`，请求 edges，生成一张静态页（零依赖、无画布）。
// 用法：node tools/edges-view.mjs <repo> <path> [out.html]
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { writeFileSync } from "node:fs";

const [repo, target, outArg] = process.argv.slice(2);
if (!repo || !target) {
  console.error("用法：node tools/edges-view.mjs <repo> <path> [out.html]");
  process.exit(2);
}
const out = outArg ?? "edges.html";

const child = spawn("jw", ["serve", repo], { stdio: ["pipe", "pipe", "inherit"] });
const rl = createInterface({ input: child.stdout });
const waiters = new Map();
let nextId = 1;

rl.on("line", (line) => {
  const w = waiters.get(JSON.parse(line).id);
  if (w) {
    waiters.delete(JSON.parse(line).id);
    w(JSON.parse(line));
  }
});

function call(method, params = {}) {
  const id = nextId++;
  return new Promise((resolve) => {
    waiters.set(id, resolve);
    child.stdin.write(JSON.stringify({ id, method, ...params }) + "\n");
  });
}

const res = await call("edges", { path: target });
child.stdin.end();
if (!res.ok) {
  console.error("serve 错误：", res.error);
  process.exit(1);
}
const { edges, total } = res.result;
const byKind = { mod: [], use: [], impl: [] };
for (const e of edges) (byKind[e.kind] ??= []).push(e.dst);

const rows = (kind) =>
  byKind[kind]
    .map((dst) => `      <li class="row" data-kind="${kind}">${esc(dst)}</li>`)
    .join("\n") || `      <li class="empty">（无）</li>`;

const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

const html = `<!doctype html>
<meta charset="utf-8">
<title>edges · ${esc(target)}</title>
<style>
  body { font: 14px/1.5 system-ui, sans-serif; margin: 24px; max-width: 760px; }
  h1 { font-size: 16px; margin: 0 0 4px; }
  .meta { color: #666; margin-bottom: 16px; }
  section { margin-bottom: 18px; }
  h2 { font-size: 13px; color: #444; margin: 0 0 6px; }
  ul { list-style: none; padding: 0; margin: 0; }
  .row { padding: 3px 8px; border-left: 3px solid transparent; cursor: pointer; font-family: ui-monospace, monospace; }
  .row:hover { background: #f4f4f4; }
  .row.sel { background: #e8f0fe; border-left-color: #1a73e8; }
  .empty { color: #999; padding: 3px 8px; }
</style>
<h1>${esc(target)}</h1>
<div class="meta">${esc(repo)} · 共 ${total} 条边（mod ${byKind.mod.length} / use ${byKind.use.length} / impl ${byKind.impl.length}）</div>
<section><h2>mod 包含</h2><ul>
${rows("mod")}
</ul></section>
<section><h2>use 引用</h2><ul>
${rows("use")}
</ul></section>
<section><h2>impl 关系（类型 → trait）</h2><ul>
${rows("impl")}
</ul></section>
<script>
  document.querySelectorAll(".row").forEach((el) => {
    el.addEventListener("click", () => {
      document.querySelectorAll(".row.sel").forEach((x) => x.classList.remove("sel"));
      el.classList.add("sel");
    });
  });
</script>
`;

writeFileSync(out, html, "utf8");
console.log(`wrote ${out}（${total} 条边）`);
