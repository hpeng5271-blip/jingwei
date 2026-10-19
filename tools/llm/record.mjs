// 记一次 pro 咨询（工作流/子智能体路径无法自动拿到 usage，所以手动记**估算值**）。
//
// 用法：
//   node tools/llm/record.mjs --model deepseek-v4-pro --task "next-priority" --in 620 --out 400
//
// 为什么允许"估算"：这是**预算闸**用的，不是账单。真实账单以 platform.deepseek.com 为准；
// 这里把量级记住，保证"一天不超过 20 次 / ¥3"。

import { assertDailyRoom, record, summary } from './ledger.mjs';
import { estimateTokens } from './caps.mjs';

const args = process.argv.slice(2);
const get = (k, d) => {
  const i = args.indexOf(`--${k}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : d;
};

const model = get('model', 'deepseek-v4-pro');
const task = get('task', '');
// 允许直接传文本，用 estimateTokens 估；也允许显式给数字
const inText = get('in-text', '');
const outText = get('out-text', '');
const inTok = Number(get('in', inText ? estimateTokens(inText) : 0));
const outTok = Number(get('out', outText ? estimateTokens(outText) : 0));

if (!inTok || !outTok) {
  console.error('要么给 --in/--out，要么给 --in-text/--out-text');
  process.exit(2);
}

try {
  const room = assertDailyRoom();
  const row = record({ model, task, inTok, outTok });
  console.log(
    `已记：${model} in=${inTok} out=${outTok} ≈¥${row.cny.toFixed(4)}（今天第 ${room.calls + 1} 次）`,
  );
  console.log(JSON.stringify(summary(3), null, 1));
} catch (e) {
  console.error(String(e.message || e));
  process.exit(1);
}
