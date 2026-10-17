# Watch a codex exec run to completion -- with a hard cap and stall detection.
#
# WHY THIS EXISTS
#   A delegated `codex exec` gives no receipt: the MCP bridge can answer with an
#   error while the task is still running in the background (that happened once,
#   and a second identical command would have started a second writer in the same
#   repo). So delegating needs two guards:
#     1. never re-issue while a writer is alive  -> this script watches instead
#     2. never wait forever                      -> hard cap + stall detection
#
# ASCII ONLY. PowerShell 5.1 reads a .ps1 without a BOM as ANSI, so Chinese here
# would be mangled -- quotes included. Same trap the repo already recorded twice.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\watch-codex.ps1
#   powershell ... -File tools\watch-codex.ps1 -MaxMinutes 60 -StallMinutes 5
#
# Exit codes: 0 = finished, 2 = timed out, 3 = stalled (no repo writes for a while)

param(
    [int]$MaxMinutes   = 50,
    [int]$StallMinutes = 5,
    [int]$PollSeconds  = 30,
    [string]$Repo      = 'D:\projects\jingwei'
)

$ErrorActionPreference = 'Stop'
$started = Get-Date
$deadline = $started.AddMinutes($MaxMinutes)

function Get-CodexProc {
    Get-CimInstance Win32_Process -Filter "Name='codex.exe'" -ErrorAction SilentlyContinue
}

function Get-NewestRepoWrite {
    Get-ChildItem $Repo -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '\\(target|node_modules|\.git|dist)\\' } |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
}

if (-not (Get-CodexProc)) {
    Write-Host 'no codex process is running -- nothing to watch'
    exit 0
}

Write-Host ("watching codex, cap {0} min, stall threshold {1} min" -f $MaxMinutes, $StallMinutes)
$lastWrite = (Get-NewestRepoWrite)
$lastChange = Get-Date
$tick = 0

while ($true) {
    Start-Sleep -Seconds $PollSeconds
    $tick++
    $elapsed = [math]::Round(((Get-Date) - $started).TotalMinutes, 1)

    $proc = Get-CodexProc
    if (-not $proc) {
        Write-Host ("[{0} min] codex has exited -- it finished (or died); verify its output now" -f $elapsed)
        exit 0
    }

    $newest = Get-NewestRepoWrite
    if ($newest -and (-not $lastWrite -or $newest.LastWriteTime -gt $lastWrite.LastWriteTime)) {
        $lastWrite = $newest
        $lastChange = Get-Date
        Write-Host ("[{0} min] wrote: {1} ({2} KB)" -f $elapsed, $newest.Name, [math]::Round($newest.Length/1KB,1))
    } else {
        $stalled = [math]::Round(((Get-Date) - $lastChange).TotalMinutes, 1)
        Write-Host ("[{0} min] no repo write for {1} min (still alive)" -f $elapsed, $stalled)
        if ($stalled -ge $StallMinutes) {
            Write-Host ("STALLED: no write for {0} min -- it may be looping on a long command" -f $stalled)
            Write-Host 'next step: read its rollout tail, then decide: wait, or kill PID and take over.'
            exit 3
        }
    }

    if ((Get-Date) -ge $deadline) {
        Write-Host ("TIMEOUT after {0} min -- hard cap reached" -f $elapsed)
        Write-Host 'next step: inspect what it produced so far; do NOT wait longer.'
        exit 2
    }
}
