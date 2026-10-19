// **账本**（2026-10-19）：每次升级调用 pro 都记一行，落在仓库外。
//
// 为什么放仓库外：账本会因为每次运行而变化，放仓库里会让 `git status` 永远脏。
// 位置：%LOCALAPPDATA%\jingwei\llm-ledger.jsonl（没有就退到临时目录）。
//
// 一行一个 JSON：{ts, model, task, inTok, outTok, cny, peak, escalatedFrom}

import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { CAPS, costOf } from './caps.mjs';

export function ledgerPath() {
  const base = process.env.LOCALAPPDATA || path.join(os.tmpdir(), 'jingwei-local');
  return path.join(base, 'jingwei', 'llm-ledger.jsonl');
}

function readAll() {
  const p = ledgerPath();
  if (!fs.existsSync(p)) return [];
  return fs
    .readFileSync(p, 'utf8')
    .split(/\r?\n/)
    .filter(Boolean)
    .map((l) => {
      try {
        return JSON.parse(l);
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

/** 今天已经用了几次 pro、花了多少钱（按本地日期）。 */
export function todayUsage() {
  const today = new Date().toISOString().slice(0, 10);
  const rows = readAll().filter((r) => String(r.ts || '').slice(0, 10) === today);
  const calls = rows.length;
  const cny = rows.reduce((s, r) => s + (Number(r.cny) || 0), 0);
  return { calls, cny, rows };
}

/**
 * 记一次调用；**超日限就抛错**（抛错发生在调用之后是没用的，
 * 所以调用方必须先 `assertDailyRoom()`）。
 */
export function record({ model, task, inTok, outTok, peak }) {
  const p = ledgerPath();
  const row = {
    ts: new Date().toISOString(),
    model,
    task: task || '',
    inTok,
    outTok,
    cny: Number(costOf(model, inTok, outTok).toFixed(6)),
    peak: typeof peak === 'boolean' ? peak : undefined,
  };
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.appendFileSync(p, JSON.stringify(row) + '\n', 'utf8');
  return row;
}

/** 今天还允不允许再升级一次？不允许就抛错（调用前先问）。 */
export function assertDailyRoom() {
  const { calls, cny } = todayUsage();
  if (calls >= CAPS.proPerDay) {
    throw new Error(`今天已经升级 ${calls} 次，达到上限 ${CAPS.proPerDay}：先把手上的做完`);
  }
  if (cny >= CAPS.cnyPerDay) {
    throw new Error(
      `今天 pro 咨询已花 ¥${cny.toFixed(2)}，达到上限 ¥${CAPS.cnyPerDay}：停`,
    );
  }
  return { calls, cny };
}

/** 打印今天与最近几条，给人看。 */
export function summary(limit = 5) {
  const { calls, cny, rows } = todayUsage();
  const tail = rows.slice(-limit);
  return { todayCalls: calls, todayCny: Number(cny.toFixed(4)), tail };
}
