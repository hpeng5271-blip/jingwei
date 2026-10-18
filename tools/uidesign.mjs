#!/usr/bin/env node
// **设计对照（A/B）**：换一份样式 → 重编译界面 → 用同一套真窗口门禁量 → 打成一张表。
//
// 为什么要有这个脚本（2026-10-18 用户原话）：
//   「对照组也要跟上，做设计不仅要学，还要对照决策。」
//   ——"我看了成熟项目、觉得这样更好"不是决策，**两张数字互为对照**才是。
//
// 判据：
//   · 每个变体跑**同一份夹具、同一套断言**（`tools/uitest-window.mjs`）；
//   · 只比**能失败的量**（画布占窗口的比例、HUD 占画布的比例、是否越界）；
//   · 结果打印成表，**不替人下结论**——选哪个由人拍板，这里只把数字摆出来。
//
// 用法：
//   node tools/uidesign.mjs                       # 当前样式 + ui/design/ 下所有变体
//   node tools/uidesign.mjs baseline-capped       # 只跑指定变体
//
// 边界：每次变体都要**重编译 Tauri**（`ui/` 是编进 exe 的），一次约 90 秒。
//       所以这个脚本**不进全量门禁**，只在做设计决策时手动跑。

import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const CSS = join(ROOT, 'ui', 'style.css');
const DESIGN = join(ROOT, 'ui', 'design');
const BACKUP = join(ROOT, 'ui', '.style.css.bak');

const only = process.argv.slice(2).filter((x) => !x.startsWith('-'));

function variants() {
  const out = [{ name: '当前（工作区）', file: null }];
  if (existsSync(DESIGN)) {
    for (const f of readdirSync(DESIGN).filter((x) => x.endsWith('.css')).sort()) {
      out.push({ name: f.replace(/\.css$/, ''), file: join(DESIGN, f) });
    }
  }
  return only.length ? out.filter((v) => only.some((o) => v.name.includes(o))) : out;
}

function build() {
  const r = spawnSync('cargo', ['build', '--release', '-p', 'jingwei-app'], {
    cwd: ROOT,
    stdio: 'ignore',
    shell: true,
  });
  return r.status === 0;
}

// 从门禁输出里抠"画布实测：1643x900 / 窗口 1707x996（宽 96% · 高 90%）"
function measure() {
  const r = spawnSync('node', [join(ROOT, 'tools', 'uitest-window.mjs')], {
    cwd: ROOT,
    encoding: 'utf8',
    shell: true,
  });
  const text = (r.stdout || '') + (r.stderr || '');
  const m = text.match(/画布实测：(\d+)x(\d+) \/ 窗口 (\d+)x(\d+)（宽 (\d+)% · 高 (\d+)%）/);
  const bad = text.split(/\r?\n/).filter((l) => l.trim().startsWith('×'));
  return {
    ok: r.status === 0,
    cw: m ? Number(m[1]) : null,
    ch: m ? Number(m[2]) : null,
    ww: m ? Number(m[3]) : null,
    wh: m ? Number(m[4]) : null,
    wp: m ? Number(m[5]) : null,
    hp: m ? Number(m[6]) : null,
    fails: bad,
  };
}

const rows = [];
try {
  copyFileSync(CSS, BACKUP);
  for (const v of variants()) {
    // 变体 = "当前 style.css + 一段覆盖"，这样不必复制整份样式（drift 小）
    const base = readFileSync(BACKUP, 'utf8');
    writeFileSync(CSS, v.file ? base + '\n\n/* ── A/B 变体：' + v.name + ' ── */\n' + readFileSync(v.file, 'utf8') : base, 'utf8');
    process.stdout.write(`  · ${v.name} … `);
    if (!build()) {
      console.log('编译失败，跳过');
      rows.push({ name: v.name, note: '编译失败' });
      continue;
    }
    const r = measure();
    console.log(r.ok ? '门禁绿' : `门禁红（${r.fails.length} 条）`);
    rows.push({ name: v.name, ...r });
  }
} finally {
  if (existsSync(BACKUP)) {
    copyFileSync(BACKUP, CSS);
    // 恢复后重编译一次，免得起见"exe 比源码旧"
    build();
  }
}

console.log('\n  对照组（同一夹具、同一套断言）');
console.log('  ' + '变体'.padEnd(22) + '画布'.padEnd(14) + '窗口'.padEnd(14) + '宽%  高%  门禁');
for (const r of rows) {
  if (r.note) {
    console.log('  ' + r.name.padEnd(22) + r.note);
    continue;
  }
  const cv = r.cw ? `${r.cw}x${r.ch}` : '—';
  const wv = r.ww ? `${r.ww}x${r.wh}` : '—';
  console.log(
    '  ' + r.name.padEnd(22) + cv.padEnd(14) + wv.padEnd(14)
    + `${String(r.wp ?? '—').padStart(3)}  ${String(r.hp ?? '—').padStart(3)}  ${r.ok ? '绿' : '红'}`,
  );
}
console.log('\n  判定：**不在这里下**。要选哪个，看"画布/窗口"那两列的差距，并由人拍板。\n');
process.exit(rows.every((r) => r.ok) ? 0 : 1);
