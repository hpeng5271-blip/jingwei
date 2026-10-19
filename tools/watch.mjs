// **变更监听（MVP）**（2026-10-20，债 0.27 的 Node 版）：
// 用 `fs.watch(..., { recursive: true })` 把变更写进持久 journal，
// 给 `tools/scan-changed.mjs` 用来"只扫变过的目录"。
//
// **边界（不许夸大）**：
// - 这是 Node 的实现，不是 Rust 内核里的 `ReadDirectoryChangesW`（那一步在 D-03 里）；
// - 进程活着才收得到事件；没在跑时"没有记录"≠"没有变化"——所以扫描侧会明说"查不了"；
// - 某些平台 recursive 不可用会直接报错，不静默降级成"什么都收不到"。
//
// 用法：node tools/watch.mjs D:\projects D:\research
//       （Ctrl-C 结束；journal 在 %LOCALAPPDATA%\jingwei\watch.jsonl）

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const roots = process.argv.slice(2);
if (!roots.length) {
  console.error('用法：node tools/watch.mjs <目录> [更多目录...]');
  process.exit(2);
}

const base = process.env.LOCALAPPDATA || path.join(os.tmpdir(), 'jingwei-local');
const journal = path.join(base, 'jingwei', 'watch.jsonl');
fs.mkdirSync(path.dirname(journal), { recursive: true });

let count = 0;
const append = (obj) => {
  fs.appendFileSync(journal, JSON.stringify(obj) + '\n', 'utf8');
  count += 1;
};

append({ ts: Date.now(), event: 'watch-start', roots });

for (const root of roots) {
  if (!fs.existsSync(root)) {
    console.error(`跳过不存在的目录：${root}`);
    continue;
  }
  try {
    const w = fs.watch(root, { recursive: true }, (event, filename) => {
      if (!filename) return;
      append({ ts: Date.now(), event, path: path.join(root, filename.toString()) });
      if (count % 100 === 0) console.log(`  已记 ${count} 条变更…`);
    });
    w.on('error', (e) => {
      console.error(`监听出错（${root}）：${e.message}`);
      append({ ts: Date.now(), event: 'watch-error', root, message: e.message });
    });
    console.log(`监听中：${root}`);
  } catch (e) {
    console.error(`起不了监听（${root}）：${e.message}`);
    process.exit(2);
  }
}

console.log(`journal：${journal}`);
process.on('SIGINT', () => {
  append({ ts: Date.now(), event: 'watch-stop' });
  console.log(`\n已停。本次共记 ${count} 条。`);
  process.exit(0);
});
