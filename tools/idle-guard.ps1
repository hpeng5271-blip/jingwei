# **闲时守则**：MuMu 只有在"你正在玩"的时候才允许运行（2026-10-18 用户要求）。
#
# 判据（能失败、不猜）：
#   · 有没有一个 **可见的** MuMu 主窗口（不是后台服务）——有 = 你在玩，放过；
#   · 没有 = 只留着 MuMuNxService / MuMuRemote* 这些**后台组件** → 杀掉。
#
# 边界（如实写清）：
#   · `MuMuRemoteService` 是**Windows 服务**，`Stop-Service` 需要管理员；
#     非管理员下我们只能杀它的进程，**服务会把它再拉起来**（实测会复活）。
#     要一次性根治（管理员终端）：
#         sc.exe config MuMuRemoteService start= disabled
#         sc.exe stop   MuMuRemoteService
#   · 只对 MuMu 生效，不碰其它程序；不写注册表、不改启动项。
#
# 用法：
#   powershell -NoProfile -File tools\idle-guard.ps1            # 跑一轮（可用任务计划器挂着）
#   powershell -NoProfile -File tools\idle-guard.ps1 -Loop 60   # 每 60 秒一轮

param(
  [int]$Loop = 0
)

$ErrorActionPreference = 'SilentlyContinue'

function Playing-MuMu {
  # "在玩"的判据：有一个标题非空、且不是纯后台服务的 MuMu 窗口。
  $wins = Get-Process | Where-Object {
    $_.ProcessName -match 'MuMu|MuMuPlayer|NxMain' -and
    $_.MainWindowHandle -ne 0 -and
    $_.MainWindowTitle -ne ''
  }
  return @($wins).Count -gt 0
}

function Sweep {
  if (Playing-MuMu) {
    Write-Host '  · MuMu 有可见窗口——当你在玩，放过' -ForegroundColor DarkGray
    return
  }
  $targets = Get-Process | Where-Object { $_.ProcessName -match 'MuMu|MumuRemote|NxMain|NxDevice' }
  if (-not $targets) {
    Write-Host '  · 没有 MuMu 组件在跑' -ForegroundColor DarkGray
    return
  }
  foreach ($p in $targets) {
    Write-Host ("  杀掉后台组件 " + $p.ProcessName + " (PID " + $p.Id + ")") -ForegroundColor DarkYellow
    Stop-Process -Id $p.Id -Force
  }
}

Sweep
if ($Loop -gt 0) {
  Write-Host ("  进入循环：每 " + $Loop + " 秒巡查一次（Ctrl+C 退出）")
  while ($true) {
    Start-Sleep -Seconds $Loop
    Sweep
  }
}
