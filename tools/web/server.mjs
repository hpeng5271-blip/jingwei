// 前端桥：浏览器 ↔ jw serve（NDJSON over stdio）。零依赖，只用 node 内置模块。
// 用法：node tools/web/server.mjs <repo> [port]
//   POST /rpc   {"action":"find","term":"Log"}  → 原样转发给 jw serve，返回 {ok,result}
//   GET  /health
//   GET  /watch?since=0    → SSE，转发 subscribe 事件
//   GET  /                 → index.html
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..");
const repo = process.argv[2] || ".";
const port = Number(process.argv[3] || 7777);
// 不靠 PATH：环境变量优先，缺省用仓库内的 debug 构建（相对仓库根，不依赖 cwd）
const JW =
  process.env.JW ||
  join(ROOT, "target", "debug", process.platform === "win32" ? "jw.exe" : "jw");

let alive = false;
const child = spawn(JW, ["serve", repo], { stdio: ["pipe", "pipe", "inherit"] });
child.on("error", (e) => {
  console.error(`jw not found or not executable: ${JW} (${e.code || e.message})`);
  console.error("set JW=<path to jw.exe> or build target/debug/jw.exe");
  process.exit(1);
});
child.on("spawn", () => { alive = true; });
let seq = 0;
const pending = new Map();
const subscribers = new Set();
let buf = "";

child.stdout.setEncoding("utf8");
child.stdout.on("data", (chunk) => {
  buf += chunk;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (!line) continue;
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      continue;
    }
    // 订阅事件（没有 id 的回包）广播给 SSE；有 id 的按 pending 解
    if (msg.id != null && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    } else if (msg.result && msg.result.events) {
      for (const s of subscribers) s.write(`data: ${JSON.stringify(msg.result)}\n\n`);
    }
  }
});
child.on("exit", (code) => {
  console.error(`jw serve exited: ${code}`);
  process.exit(1);
});

function rpc(req, timeoutMs = 8000) {
  const id = ++seq;
  // 前端用 action，jw serve 的协议字段是 method（协调点，只在这一处翻译）
  const { action, ...rest } = req;
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => {
      pending.delete(id);
      reject(new Error("timeout"));
    }, timeoutMs);
    pending.set(id, (msg) => {
      clearTimeout(t);
      resolve(msg);
    });
    child.stdin.write(JSON.stringify({ id, method: action, ...rest }) + "\n");
  });
}

const MIME = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css" };

const server = createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  if (url.pathname === "/health") {
    res.writeHead(alive ? 200 : 503, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: alive, repo, jw: JW }));
    return;
  }
  if (url.pathname === "/rpc" && req.method === "POST") {
    let body = "";
    for await (const chunk of req) body += chunk;
    try {
      const payload = JSON.parse(body || "{}");
      const msg = await rpc(payload);
      res.writeHead(200, { "content-type": "application/json; charset=utf-8" });
      res.end(JSON.stringify(msg));
    } catch (e) {
      res.writeHead(500, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: false, error: String(e.message || e) }));
    }
    return;
  }
  if (url.pathname === "/watch") {
    res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
    const since = Number(url.searchParams.get("since") || 0);
    subscribers.add(res);
    rpc({ action: "subscribe", since, timeout_ms: 30000 }).catch(() => {});
    const keep = setInterval(() => res.write(": ping\n\n"), 15000);
    req.on("close", () => {
      clearInterval(keep);
      subscribers.delete(res);
    });
    return;
  }
  // 静态：/ 或 /index.html
  const file = url.pathname === "/" || url.pathname === "/index.html" ? "index.html" : url.pathname.slice(1);
  try {
    const buf = await readFile(join(HERE, file));
    const ext = file.slice(file.lastIndexOf("."));
    res.writeHead(200, { "content-type": MIME[ext] || "application/octet-stream" });
    res.end(buf);
  } catch {
    if (url.pathname.startsWith("/ui/")) {
    const rel = url.pathname.slice(4);
    const file = join(repoRoot, "tools", "web", "ui", rel);
    try {
      const body = readFileSync(file);
      const ext = rel.split(".").pop();
      const ctype = ext === "css" ? "text/css; charset=utf-8" : ext === "js" ? "application/javascript; charset=utf-8" : "text/html; charset=utf-8";
      res.writeHead(200, { "content-type": ctype });
      res.end(body);
      return;
    } catch {
      res.writeHead(404, { "content-type": "text/plain" });
      res.end("ui asset not found");
      return;
    }
  }
  res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`jingwei web on http://127.0.0.1:${port}  (repo=${repo})`);
});
