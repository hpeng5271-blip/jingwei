// **用量闸**（2026-10-19）：pro 做决策、flash 做执行，两边的量都卡死。
//
// 为什么要脚本而不是"我自觉"：这轮之前已经证明，靠自觉会多跑 8–10 轮返工。
// 这里把配额写成**可执行检查**：超了就退出码 1，不许继续。
//
// 用法：
//   node tools/llm/limits.mjs            # 看今天用了多少、还剩多少
//   node tools/llm/limits.mjs check      # 超限则退出码 1（每轮开头跑一次）
//   node tools/llm/limits.mjs round --task T1   # 记一轮
//   node tools/llm/limits.mjs gate       # 记一次门禁
//   node tools/llm/limits.mjs decision   # 记一次"决策记录"

import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { summary, todayUsage } from './ledger.mjs';
import { CAPS } from './caps.mjs';

/** 两边的日限（超出即拒绝继续）。 */
export const LIMITS = {
  // 决策层（pro）
  // 一律从 caps.mjs 取，不再在注释里写第二份数字（注释与运行值曾经不一致）
  proCallsPerDay: CAPS.proPerDay,
  proCnyPerDay: CAPS.cnyPerDay,
  // 执行层（flash 主会话）
  roundsPerDay: 12,
  // 2026-10-21：6 → 20。实测今天跑了 41 次四道门禁（每轮 2–3 次），
  // 「每天 6 次」与真实节奏差一个数量级；闸门的意义是防跑飞，不是防正常迭代。
  gateRunsPerDay: 20,
  decisionsPerDay: 4,
  // 总账（估算，含 flash 与 pro）
  totalCnyPerDay: 10,
};

function statePath() {
  const base = process.env.LOCALAPPDATA || path.join(os.tmpdir(), 'jingwei-local');
  return path.join(base, 'jingwei', 'llm-rounds.json');
}

function today() {
  return new Date().toISOString().slice(0, 10);
}

function readState() {
  const p = statePath();
  if (!fs.existsSync(p)) return {};
  try {
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch {
    return {};
  }
}

function writeState(s) {
  const p = statePath();
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, JSON.stringify(s, null, 1), 'utf8');
}

export function usage() {
  const d = today();
  const s = readState();
  const day = s[d] || { rounds: 0, gates: 0, decisions: 0 };
  const pro = todayUsage();
  return {
    date: d,
    rounds: day.rounds,
    gates: day.gates,
    decisions: day.decisions,
    proCalls: pro.calls,
    proCny: Number(pro.cny.toFixed(4)),
  };
}

export function check() {
  const u = usage();
  const bad = [];
  if (u.rounds >= LIMITS.roundsPerDay) bad.push(`执行轮数 ${u.rounds}/${LIMITS.roundsPerDay}`);
  if (u.gates >= LIMITS.gateRunsPerDay) bad.push(`门禁次数 ${u.gates}/${LIMITS.gateRunsPerDay}`);
  if (u.decisions >= LIMITS.decisionsPerDay) bad.push(`决策记录 ${u.decisions}/${LIMITS.decisionsPerDay}`);
  if (u.proCalls >= LIMITS.proCallsPerDay) bad.push(`pro 调用 ${u.proCalls}/${LIMITS.proCallsPerDay}`);
  if (u.proCny >= LIMITS.proCnyPerDay) bad.push(`pro 花费 ¥${u.proCny}/¥${LIMITS.proCnyPerDay}`);
  return { ok: bad.length === 0, bad, usage: u };
}

function bump(key) {
  const d = today();
  const s = readState();
  s[d] = s[d] || { rounds: 0, gates: 0, decisions: 0 };
  s[d][key] += 1;
  writeState(s);
  return s[d][key];
}

const cmd = process.argv[2] || 'status';

if (cmd === 'check') {
  const r = check();
  if (!r.ok) {
    console.error(`用量超限：${r.bad.join('；')}`);
    process.exit(1);
  }
  console.log(`用量正常：轮 ${r.usage.rounds}/${LIMITS.roundsPerDay}，门禁 ${r.usage.gates}/${LIMITS.gateRunsPerDay}，pro ${r.usage.proCalls}/${LIMITS.proCallsPerDay}`);
} else if (cmd === 'round' || cmd === 'gate' || cmd === 'decision') {
  const key = cmd === 'round' ? 'rounds' : cmd === 'gate' ? 'gates' : 'decisions';
  const n = bump(key);
  const taskIdx = process.argv.indexOf('--task');
  const task = taskIdx > 0 ? process.argv[taskIdx + 1] : '';
  console.log(`已记 ${cmd}${task ? ` (${task})` : ''}：今天第 ${n} 次`);
  const r = check();
  if (!r.ok) {
    console.error(`注意：${r.bad.join('；')}——下一次开头会拒绝继续`);
  }
} else {
  const u = usage();
  console.log(JSON.stringify({ usage: u, limits: LIMITS, pro: summary(3) }, null, 1));
}

