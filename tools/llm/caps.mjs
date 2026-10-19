// **预算与计费**（2026-10-19）：pro 只在需要时用，用多少、花多少要有数。
//
// 价格来源：DeepSeek 官方定价页（2026-10 抓取）
//   deepseek-v4-pro：输入 4.5（空闲）/ 9.0（高峰）元/M；输出 13.5 / 27.0 元/M
//   deepseek-flash ：输入 1.0 / 2.0；输出 4.0 / 8.0
// 高峰定义：北京时间 周一至周五（不含法定节假日）9:00–12:00、14:00–18:00；其余为空闲。

export const PRICES = {
  'deepseek-v4-pro': { inIdle: 4.5, inPeak: 9.0, outIdle: 13.5, outPeak: 27.0 },
  'deepseek-flash': { inIdle: 1.0, inPeak: 2.0, outIdle: 4.0, outPeak: 8.0 },
};

/** 每轮 pro 咨询的**硬上限**：输入 8K token、输出 2K token。超了拒绝，不截断。 */
export const CAPS = {
  inputTokens: 8000,
  outputTokens: 4000,
  proPerDay: 60, // 一天最多 60 次升级（2026-10-19 放宽：思考该花钱）
  cnyPerDay: 15.0, // 一天最多花 ¥15 在 pro 咨询上（2026-10-19 放宽）
};

/** 粗估 token：中文 1 字≈1 token，其余按 4 字符≈1 token。**只用于闸门，不用于账单**。 */
export function estimateTokens(text) {
  let cjk = 0;
  let other = 0;
  for (const ch of String(text)) {
    const c = ch.codePointAt(0);
    if (c >= 0x2e80) cjk += 1;
    else other += 1;
  }
  return cjk + Math.ceil(other / 4);
}

function beijingNow() {
  // 机器时区就是 Asia/Shanghai；用本地时间即可，显式说明避免误用 UTC。
  return new Date();
}

export function isPeak(now = beijingNow()) {
  const d = now.getDay(); // 0 周日，6 周六
  if (d === 0 || d === 6) return false;
  const h = now.getHours();
  return (h >= 9 && h < 12) || (h >= 14 && h < 18);
}

/** 算一次调用的费用（元）。 */
export function costOf(model, inTok, outTok, now = beijingNow()) {
  const p = PRICES[model];
  if (!p) throw new Error(`不认识的模型：${model}`);
  const peak = isPeak(now);
  const inRate = peak ? p.inPeak : p.inIdle;
  const outRate = peak ? p.outPeak : p.outIdle;
  return (inTok / 1e6) * inRate + (outTok / 1e6) * outRate;
}

/** 闸门：单次输入/输出超限直接抛错（**不降级、不截断**）。 */
export function assertCaps(inTok, outTok) {
  if (inTok > CAPS.inputTokens) {
    throw new Error(
      `输入 ${inTok} tokens 超过单次上限 ${CAPS.inputTokens}：先切片/拆任务，不许硬塞`,
    );
  }
  if (outTok > CAPS.outputTokens) {
    throw new Error(`要求输出 ${outTok} 超过上限 ${CAPS.outputTokens}：把问题问小一点`);
  }
}
