#!/usr/bin/env node
// 最小外部组件示例：起一次 `jw serve`，refresh → watch → 把受影响的卡的第一行打出来。
//
// 用法：node tools/watch.mjs D:\projects\demo
// 依赖：只需 PATH 上有 jw（或把第二个参数换成 jw.exe 的绝对路径）。
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const repo = process.argv[2];
if (!repo) {
  console.error("用法：node tools/watch.mjs <repo>");
  process.exit(2);
}

const child = spawn("jw", ["serve", repo], { stdio: ["pipe", "pipe", "inherit"] });
const rl = createInterface({ input: child.stdout });
const waiters = new Map();
let nextId = 1;

rl.on("line", (line) => {
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    console.error("非 JSON 行：", line);
    return;
  }
  const w = waiters.get(msg.id);
  if (w) {
    waiters.delete(msg.id);
    w(msg);
  }
});

function call(method, params = {}) {
  const id = nextId++;
  const req = JSON.stringify({ id, method, ...params });
  return new Promise((resolve) => {
    waiters.set(id, resolve);
    child.stdin.write(req + "\n");
  });
}

// 1) 先跑一遍三条流入
const refreshed = await call("refresh");
console.log("refresh →", JSON.stringify(refreshed.result));

// 2) 从"当前"开始等新变化
let since = refreshed.result.write_seq;
for (;;) {
  const res = await call("watch", { since, timeout_ms: 10000 });
  const r = res.result;
  since = r.write_seq;
  if (!r.changed) continue;
  if (r.gap) {
    console.log("断档：需要全量重取");
    continue;
  }
  const subjects = [...new Set(r.events.flatMap((e) => e.subjects))];
  console.log(`变化：${subjects.length} 个 subject`);
  // 3) 只重查受影响的卡（一次请求多张）
  const cards = await call("card", { path: subjects[0], also: subjects.slice(1) });
  const list = cards.result.cards ?? [cards.result.card];
  for (const c of list.slice(0, 5)) {
    const head = c.subject ?? c.path ?? c.commit ?? "";
    const state = c.state ?? c.mode ?? "";
    console.log(`  ${state.padEnd(10)} ${head}`);
  }
}

// 一直跑，Ctrl+C 结束
process.on("SIGINT", () => child.kill());
