// **依赖只降不升**（2026-10-20，pro 决策：把"越小越省事"变成机器闸）。
//
// 为什么：新依赖是**永久成本**（体积、审计面、升级负担、失败模式未知）。
// 想加可以，但要先改 `tools/deps-baseline.json` 并在提交信息里写理由——
// 让"加了一个库"成为一个**看得见的决定**，而不是顺手。
//
// 统计口径：每个 crate 的 `[dependencies]` 与 `[target.'cfg(...)'.dependencies]` 里的键数
// （dev-dependencies 不算：它不进产物）。
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const BASELINE = path.join(ROOT, 'tools', 'deps-baseline.json');

/** 解析一个 Cargo.toml 里"会进产物"的直接依赖数。 */
export function countDeps(file) {
  const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
  let inDeps = false;
  let n = 0;
  const names = [];
  for (const raw of lines) {
    const s = raw.trim();
    if (s.startsWith('[') && s.endsWith(']')) {
      // `[dependencies]` / `[target.'cfg(windows)'.dependencies]` 都算；
      // `[dev-dependencies]` / `[build-dependencies]` 不算（不进产物）。
      inDeps = /\.dependencies\]$/.test(s) || s === '[dependencies]';
      continue;
    }
    if (!inDeps || !s || s.startsWith('#')) continue;
    const m = s.match(/^([A-Za-z0-9_-]+)\s*=/);
    if (m) {
      n += 1;
      names.push(m[1]);
    }
  }
  return { n, names };
}

export function checkDeps() {
  const base = JSON.parse(fs.readFileSync(BASELINE, 'utf8').replace(/^\uFEFF/, ''));
  const problems = [];
  const rows = [];
  for (const [rel, allowed] of Object.entries(base.crates)) {
    const file = path.join(ROOT, rel);
    if (!fs.existsSync(file)) {
      problems.push(`基线里的 crate 不见了：${rel}`);
      continue;
    }
    const { n, names } = countDeps(file);
    rows.push({ rel, n, allowed });
    if (n > allowed) {
      problems.push(`${rel} 直接依赖 ${n} 个 > 基线 ${allowed}（新增：${names.join(', ')}）——要加就改 tools/deps-baseline.json 并写理由`);
    }
  }
  return { rows, problems };
}
