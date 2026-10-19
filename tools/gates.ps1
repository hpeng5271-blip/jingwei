# One-shot gate runner (design section 12.3).
#
# Why a script: the design has said for a while that "these four gates run in CI",
# yet the repo has no .github at all -- a decision recorded on paper with nothing
# enforcing it. That failure mode already cost this project twice (see the
# iteration rules in 设计.md section 13.4).
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File tools\gates.ps1
#
# ASCII only on purpose. Windows PowerShell 5.1 decodes a BOM-less .ps1 as ANSI,
# so any non-ASCII text in here turns into mojibake and breaks parsing.
# All human-facing text lives in tools\gates.json instead (UTF-8, read explicitly).

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

$spec = Get-Content -Raw -Encoding UTF8 (Join-Path $root 'tools\gates.json') | ConvertFrom-Json

$phase = 0
$total = $spec.steps.Count
$results = @()
$t0 = Get-Date

foreach ($s in $spec.steps) {
    $phase++
    $bar = '-' * 70
    Write-Host ''
    Write-Host $bar -ForegroundColor DarkGray
    Write-Host ("[{0}/{1}] {2}" -f $phase, $total, $s.name) -ForegroundColor Cyan
    Write-Host ("      {0}" -f $s.why) -ForegroundColor DarkGray
    Write-Host ("      `$ {0}" -f $s.cmd) -ForegroundColor DarkGray

    $start = Get-Date
    $out = & cmd /c "$($s.cmd) 2>&1"
    $code = $LASTEXITCODE
    $ms = [int]((Get-Date) - $start).TotalMilliseconds

    if ($code -eq 0) {
        Write-Host ("      OK  {0} ms" -f $ms) -ForegroundColor Green
        # Even on success, print one line of evidence: "passed" alone does not
        # say how much actually ran.
        $evidence = @($out | Select-String -Pattern 'test result:' | ForEach-Object { $_.Line.Trim() })
        if ($evidence.Count -eq 0) {
            $evidence = @($out | Select-String -Pattern 'passed|ok|total' | Select-Object -Last 2 | ForEach-Object { $_.Line.Trim() })
        }
        foreach ($e in $evidence) { Write-Host ("          {0}" -f $e) -ForegroundColor DarkGray }
    } else {
        Write-Host ("      FAILED  exit={0}  {1} ms" -f $code, $ms) -ForegroundColor Red
    }

    $results += [pscustomobject]@{
        Name = $s.name
        Ok   = ($code -eq 0)
        Ms   = $ms
        Tail = (($out | Select-Object -Last 40) -join "`n")
    }
}

$secs = [int]((Get-Date) - $t0).TotalSeconds
$failed = @($results | Where-Object { -not $_.Ok })

Write-Host ''
Write-Host ('-' * 70) -ForegroundColor DarkGray
Write-Host $spec.title -ForegroundColor Cyan
foreach ($r in $results) {
    $mark = 'OK  '
    $color = 'Green'
    if (-not $r.Ok) { $mark = 'FAIL'; $color = 'Red' }
    Write-Host ("  {0}  {1,-20} {2,7} ms" -f $mark, $r.Name, $r.Ms) -ForegroundColor $color
}
Write-Host ("  {0} gates, {1} failed, {2}s total" -f $results.Count, $failed.Count, $secs)

if ($failed.Count -gt 0) {
    Write-Host ''
    Write-Host 'Failure detail (last 40 raw lines per failed gate)' -ForegroundColor Yellow
    foreach ($r in $failed) {
        Write-Host ''
        Write-Host ("== {0}" -f $r.Name) -ForegroundColor Red
        Write-Host $r.Tail
    }
# **失败也算跑过**（债 0.29）：不记的话，日限会低估一半。
& node (Join-Path $PSScriptRoot 'llm\limits.mjs') gate 2>&1 | Out-Null
    exit 1
}

Write-Host ''
# **记账**（债 0.29）：这次门禁真的跑过，记一次。
& node (Join-Path $PSScriptRoot 'llm\limits.mjs') gate 2>&1 | Out-Null
Write-Host 'ALL GATES PASSED' -ForegroundColor Green
