#!/usr/bin/env node
// 订阅演示（零依赖）：起 `jw serve`，用 subscribe 等变更，收到后重查一张卡。
// 用法：node tools/watch-panel.mjs <repo> <card-selector> [timeout_ms]
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const [repo, sel, timeoutArg] = process.argv.slice(2);
if (!repo || !sel) {
  console.error("用法：node tools/watch-panel.mjs <repo> <card-selector> [timeout_ms]");
  process.exit(2);
}
const timeout = Number(timeoutArg ?? 2000);

const child = spawn("jw", ["serve", repo], { stdio: ["pipe", "pipe", "inherit"] });
const rl = createInterface({ input: child.stdout });
let seq = 0;

rl.on("line", (line) => {
  const msg = JSON.parse(line);
  if (msg.method === "event" || (msg.result && msg.result.events)) {
    const events = msg.result?.events ?? [];
    const subs = events.flatMap((e) => e.subjects ?? []);
    console.log(`变更 seq=${msg.result.write_seq} gap=${msg.result.gap}：${subs.join(", ")}`);
    child.stdin.write(JSON.stringify({ id: 99, method: "card", selector: sel }) + "\n");
  } else if (msg.id === 99 && msg.ok) {
    console.log(JSON.stringify(msg.result).slice(0, 300));
    child.stdin.write(JSON.stringify({ id: 100, method: "quit" }) + "\n");
  } else if (msg.id === 1) {
    seq = msg.result?.write_seq ?? 0;
    console.log(`订阅中（since=${seq}, ${timeout}ms）… 改一个文件再等等`);
    child.stdin.write(JSON.stringify({ id: 2, method: "subscribe", since: seq, timeout_ms: timeout }) + "\n");
  }
});

child.stdin.write(JSON.stringify({ id: 1, method: "ping" }) + "\n");
setTimeout(() => {
  child.stdin.write(JSON.stringify({ id: 100, method: "quit" }) + "\n");
}, timeout + 3000);
