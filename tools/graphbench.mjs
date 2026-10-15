// 渲染栈验证：用真实图谱数据测"画得动画不动"。
//
// 为什么要有这个脚本：这是唯一一个"做不到就真的做不到"的瓶颈——
// 别的性能问题可以优化，画不动就是画不动。所以**用真数据量出来**，不靠猜。
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
function findPlaywright() {
  const local = join(process.cwd(), 'node_modules', 'playwright');
  if (existsSync(local)) return local;
  const cache = join(process.env.LOCALAPPDATA || '', 'npm-cache', '_npx');
  if (!existsSync(cache)) return null;
  for (const d of readdirSync(cache)) {
    const p = join(cache, d, 'node_modules', 'playwright');
    if (existsSync(p)) return p;
  }
  return null;
}
const pw = findPlaywright();
if (!pw) { console.error('找不到 playwright'); process.exit(2); }
const { chromium } = require(pw);

const file = 'file:///' + join(process.cwd(), 'tools', 'graphview.html').replace(/\\/g, '/');
// **有头模式**：headless 下 canvas 的栅格化走软件渲染 + rAF 被节流，量出来的帧率不代表真实性能。
const headless = process.env.JW_HEADLESS === '1';
const browser = await chromium.launch({ headless: headless, args: headless ? [] : ['--window-size=1600,960'] });
const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
const errs = [];
page.on('pageerror', (e) => errs.push(String(e)));
await page.goto(file);
await page.waitForTimeout(1500);

async function sample(label) {
  await page.waitForTimeout(2500);   // 让布局跑一阵、帧率稳住
  const r = await page.evaluate(() => {
    const t = document.getElementById('hud').innerText.replace(/\n/g, ' | ');
    return { text: t };
  });
  console.log(`  ${label.padEnd(26)} ${r.text}`);
  return r.text;
}

console.log('渲染栈验证（真数据：经纬索引自己）');
console.log('  规模点                     帧率 / 布局 / 绘制');
await sample('真实图（~1050 点）');

// 密度 3：模拟"有了 calls 之后的网"（边数 ≈ 节点数 x 9）
await page.evaluate(() => {
  const d = document.getElementById('dens');
  d.value = '3';
  d.dispatchEvent(new Event('input', { bubbles: true }));
});
await sample('密度 x3（模拟关系网）');

// 密度 8：压力上限
await page.evaluate(() => {
  const d = document.getElementById('dens');
  d.value = '8';
  d.dispatchEvent(new Event('input', { bubbles: true }));
});
await sample('密度 x8（压力）');

// 节点放大到 3000（模拟大项目）
await page.evaluate(() => {
  const c = document.getElementById('cap');
  c.value = '1000';
  c.dispatchEvent(new Event('input', { bubbles: true }));
});
await sample('1000 点 / 中等边');

await page.evaluate(() => {
  const c = document.getElementById('cap');
  c.value = '800'; c.dispatchEvent(new Event('input', { bubbles: true }));
  const d = document.getElementById('dens');
  d.value = '1'; d.dispatchEvent(new Event('input', { bubbles: true }));
});
await page.screenshot({ path: join(process.cwd(), 'tools', 'graphview.png') });
console.log('  截图：tools/graphview.png');
console.log(errs.length ? '  页面异常：' + errs.join('; ') : '  全程无 JS 异常');
await browser.close();
