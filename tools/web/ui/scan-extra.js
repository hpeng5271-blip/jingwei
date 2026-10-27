// **扫盘的两个附加能力**（2026-10-19）：
//   ① 整盘扫描（C:\ D:\，靠已有的跳过规则剪枝，可取消）
//   ② 找同源（Jaccard ≥ 0.8，按需触发——它要重走目录，别拖慢正常扫描）
//
// 两个都遵守同一条纪律：**有终态、可取消、不伪造进度**。

/// 整盘扫描：把盘根交给同一个后端（它已经有"跳过系统目录/符号链接"的规则）。
/// 深度默认 5：够到 `C:\Users\x\source\repos\proj`，又不会把整个用户目录翻穿。
async function scanDrives() {
  const roots = ['C:\\', 'D:\\'];
  scanProgress('<span class="sp-bar"></span><span>正在整盘扫（C:\\ · D:\\）…系统目录会自动跳过</span>');
  let started;
  try {

  } catch (e) {
    scanProgress('<span>启动失败：' + esc(String(e)) + '</span>', true);
    return;
  }
  if (!started || started.ok === false) {
    scanProgress('<span>' + esc((started && started.why) || '启动失败') + '</span>', true);
    return;
  }
  PS.scanning = true;
  pollScan(true);
}
