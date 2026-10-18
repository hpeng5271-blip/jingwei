#!/usr/bin/env node
// **设备健康采样器**（2026-10-18）：挂在后台、每 30 秒写一行，开销约 0.1%。
//
// 为什么不用专门的监控软件：那些"电脑管家"自己就是常驻+推送+自启，比要监视的东西更重。
// 这里只要三件能指导动作的事实：
//   ① CPU 占用（这台机器是不是在满载编译）
//   ② 进程队列（排队说明真的在抢 CPU，而不是瞬时尖峰）
//   ③ **热节流事件**（Windows 自己记的"被降频"，比读温度更硬，而且不需要管理员）
//
// 温度为什么不读：`MSAcpi_ThermalZoneTemperature` 在非管理员下**拒绝访问**；
// 装 LibreHardwareMonitor 能读，但那是"为了监控再装一个常驻程序"——不划算。
// **节流事件是更好的判据**：它由内核记录，代表"机器已经热到要降速"，正好是我们关心的。
//
// 用法：
//   node tools/health.mjs --loop        # 后台常驻（默认 30 秒一次）
//   node tools/health.mjs --once        # 只采一次，打印给人看
// 落盘：%TEMP%\jingwei-health.csv（不写仓库，免得污染 git）

import { appendFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const LOG = join(tmpdir(), 'jingwei-health.csv');
const EVERY = Number(process.env.HEALTH_EVERY_S || 30) * 1000;
const once = process.argv.includes('--once');

function ps(cmd) {
  try {
    return execFileSync('powershell', ['-NoProfile', '-Command', cmd], {
      encoding: 'utf8',
      windowsHide: true,
      timeout: 15000,
    }).trim();
  } catch {
    return '';
  }
}

// **热节流事件计数**（ID 37 = 处理器速度被系统限制）。只看最近 1 小时新增。
function throttleCount() {
  const out = ps(
    "$c=0; try { $c=(Get-WinEvent -FilterHashtable @{LogName='System';ProviderName='Microsoft-Windows-Kernel-Processor-Power';Id=37;StartTime=(Get-Date).AddHours(-1)} -ErrorAction Stop | Measure-Object).Count } catch { $c=0 }; $c",
  );
  return Number(out) || 0;
}

function sample() {
  const cpu = ps(
    "(Get-Counter '\\Processor(_Total)\\% Processor Time' -SampleInterval 1 -MaxSamples 1).CounterSamples.CookedValue",
  );
  const queue = ps(
    "(Get-Counter '\\System\\Processor Queue Length' -SampleInterval 1 -MaxSamples 1).CounterSamples.CookedValue",
  );
  const mem = ps(
    "$o=Get-CimInstance Win32_OperatingSystem; [math]::Round(100*(1-$o.FreePhysicalMemory/$o.TotalVisibleMemorySize),0)",
  );
  return {
    at: new Date().toISOString().replace('T', ' ').slice(0, 19),
    cpu: Number(cpu) || 0,
    queue: Number(queue) || 0,
    mem: Number(mem) || 0,
    throttle1h: throttleCount(),
  };
}

function line(s) {
  return `${s.at},${s.cpu.toFixed(1)},${s.queue},${s.mem},${s.throttle1h}`;
}

if (once) {
  const s = sample();
  console.log('  时间                  CPU%   队列  内存%  近1小时节流');
  console.log(
    `  ${s.at}  ${String(s.cpu.toFixed(1)).padStart(5)}  ${String(s.queue).padStart(4)}  ${String(s.mem).padStart(5)}  ${String(s.throttle1h).padStart(10)}`,
  );
  const verdict =
    s.throttle1h > 0 ? '⚠ 近一小时出现过热降频——该让机器歇一会儿'
      : s.cpu > 70 ? '⚠ CPU 高位（大概率是编译）'
        : '✓ 正常开发负载';
  console.log(`  ${verdict}`);
  process.exit(0);
}

// 常驻：先写表头，再按固定间隔采样
appendFileSync(LOG, 'time,cpu_pct,queue,mem_pct,throttle_1h\n', 'utf8');
const tick = () => {
  try {
    appendFileSync(LOG, line(sample()) + '\n', 'utf8');
  } catch {
    /* 采样失败不退出：下一次再来 */
  }
};
tick();
console.log(`  健康采样已启动（每 ${EVERY / 1000}s 一行）→ ${LOG}`);
setInterval(tick, EVERY);
