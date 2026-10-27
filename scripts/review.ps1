# review.ps1 - 一刀一份审查报告（D8）：诊断数 + 关键日志 + 截图清单
#
# 用法：
#   powershell -File scripts/review.ps1 -Step R1
#   powershell -File scripts/review.ps1 -Step R2 -Files crates/engine/src/lib.rs -Run "env_probe"
#
# 三件事：
#   1) jw lsp-check 改过的 .rs（没改 .rs 就写 n/a）
#   2) jw --debug <Run>，只挑固定事件名（scan/card/serve/env/lsp）前 20 行
#   3) 截图清单（-Shots 给了就用，否则列 .pro/exp/out 最近 5 张 png）
#
# 输出 .pro/exp/out/review-<Step>.md；lsp errors > 0 时退出码 1。

param(
  [Parameter(Mandatory = $true)][string]$Step,
  [string[]]$Files = @(),
  [string]$Run = "",
  [string[]]$Shots = @(),
  [int]$Limit = 40
)

$ErrorActionPreference = "Continue"
# -Files 允许 "a,b" 一次传多个（-File 调用时逗号不会被拆成数组）
$Files = @($Files | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' })
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
$outDir = Join-Path $root ".pro\exp\out"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$jw = Join-Path $root "target\debug\jw.exe"
$fence = [string]([char]96) * 3

if ($Files.Count -eq 0) {
  $changed = @()
  $st = git status --porcelain 2>$null
  if ($st) {
    foreach ($line in $st) {
      $p = $line.Substring(3).Trim()
      if ($p -match ' -> ') { $p = ($p -split ' -> ')[-1] }
      $changed += $p
    }
  }
  if ($changed.Count -eq 0) {
    $last = git diff-tree --no-commit-id --name-only -r HEAD 2>$null
    if ($last) { $changed = @($last) }
  }
  $Files = @($changed | Where-Object { $_ -and (Test-Path $_) })
}

$rs = @($Files | Where-Object { ($_ -like "*.rs") -and (Test-Path $_) })
Write-Host ("changed=" + $Files.Count + " rust=" + $rs.Count)

$diagBlock = ""
$errors = 0
$warnings = 0
if (-not (Test-Path $jw)) {
  $diagBlock = "jw.exe 不存在（先 cargo build -p jw）"
} elseif ($rs.Count -eq 0) {
  $diagBlock = "n/a（本刀没有改 .rs）"
} else {
  $jsonPath = Join-Path $outDir ("review-" + $Step + "-lsp.json")
  $errPath = Join-Path $outDir ("review-" + $Step + "-lsp.err.txt")
  & $jw lsp-check @rs --json 1> $jsonPath 2> $errPath
  $code = $LASTEXITCODE
  try {
    $j = Get-Content $jsonPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $errors = [int]$j.errors
    $warnings = [int]$j.warnings
    $diagBlock = "files=" + $rs.Count + " errors=" + $errors + " warnings=" + $warnings + " exit=" + $code
    $rows = @($j.diagnostics | Where-Object { $_.severity -eq "error" -or $_.severity -eq "warning" } | Select-Object -First $Limit)
    if ($rows.Count -gt 0) {
      $diagBlock += ("`n" + $fence + "text`n")
      foreach ($r in $rows) {
        $diagBlock += ("" + $r.file + ":" + $r.line + ":" + $r.col + " " + $r.severity + " " + $r.message + "`n")
      }
      $diagBlock += $fence
    }
  } catch {
    $diagBlock = "lsp-check 输出解析失败（exit=" + $code + "）：" + $_.Exception.Message
  }
}

$logBlock = "n/a（没传 -Run）"
if ($Run -ne "") {
  if (Test-Path $jw) {
    $errFile = Join-Path $outDir ("review-" + $Step + "-debug.err.txt")
    $outFile = Join-Path $outDir ("review-" + $Step + "-debug.out.txt")
    $jargs = ($Run -split '\s+') + @("--debug")
    & $jw @jargs 1> $outFile 2> $errFile
    $code2 = $LASTEXITCODE
    $keep = @(Get-Content $errFile -ErrorAction SilentlyContinue |
      Where-Object { $_ -match '\b(scan\.start|scan\.end|card\.hit|card\.miss|serve\.call|env\.probe|lsp\.spawn|lsp\.req|lsp\.timeout|lsp\.check)\b' } |
      Select-Object -First 20)
    $logBlock = "exit=" + $code2 + " 事件 " + $keep.Count + " 行"
    if ($keep.Count -gt 0) {
      $logBlock += ("`n" + $fence + "text`n" + ($keep -join "`n") + "`n" + $fence)
    }
  } else {
    $logBlock = "jw.exe 不存在"
  }
}

$shotBlock = ""
if ($Shots.Count -gt 0) {
  foreach ($s in $Shots) { $shotBlock += ("- " + $s + "`n") }
} else {
  $pngs = @(Get-ChildItem $outDir -Filter *.png -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 5)
  if ($pngs.Count -gt 0) {
    foreach ($p in $pngs) { $shotBlock += ("- " + $p.FullName + "（" + $p.LastWriteTime.ToString("HH:mm:ss") + "）`n") }
  } else {
    $shotBlock = "（无）"
  }
}

$md = "# review-" + $Step + " · " + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss') + "`n`n"
$md += "## 1) 诊断（jw lsp-check）`n`n" + $diagBlock + "`n`n"
$md += "## 2) 关键日志（jw --debug）`n`n" + $logBlock + "`n`n"
$md += "## 3) 截图`n`n" + $shotBlock + "`n"
$md += "## 改动文件（" + $Files.Count + "）`n`n"
if ($Files.Count -gt 0) { foreach ($f in $Files) { $md += "- " + $f + "`n" } } else { $md += "（无）`n" }

$path = Join-Path $outDir ("review-" + $Step + ".md")
Set-Content -Path $path -Value $md -Encoding UTF8
Write-Host ("report=" + $path)

Pop-Location
if ($errors -gt 0) { exit 1 } else { exit 0 }
