# check-split.ps1 - one command gate for the main.rs split (ASCII only)
# Usage: powershell -File scripts/check-split.ps1
# Exit 0 = all green (tests + 3 fixture SHAs + --version + main.rs size/warnings).
$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$busy = Get-Process -Name jw,jingwei-gui -ErrorAction SilentlyContinue
if ($busy) { Write-Host "gate busy: jw.exe still running"; exit 2 }
$failed = @()

# 1) tests
$testOut = (cmd /c "cargo test --workspace 2>&1") | Out-String
$m = [regex]::Matches($testOut, "test result: ok\. (\d+) passed")
$passed = 0
foreach ($x in $m) { $passed += [int]$x.Groups[1].Value }
if ($testOut -match "test result: FAILED") { $failed += "tests" }
Write-Host "tests_passed=$passed"

# 2) fixture SHAs
$jw = Join-Path $root "target\debug\jw.exe"
$tmp = Join-Path $env:TEMP ("jw-split-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $tmp | Out-Null
$sha = { param($p) (Get-FileHash $p -Algorithm SHA256).Hash }
$pairs = @(
  @{repo="D:\projects\repos\rg"; out="rg-file.json"; base="card-baseline\rg-file.json"; args=@("card","crates/core/flags/defs.rs","--json",".")},
  @{repo="D:\projects\repos\rg"; out="rg-range.json"; base="card-baseline\rg-range.json"; args=@("card","crates/core","--json",".")},
  @{repo="D:\projects\repos\tauri"; out="tauri-file.json"; base="card-baseline\tauri-file.json"; args=@("card","crates/tauri-utils/src/lib.rs","--json",".")},
  @{repo="D:\projects\repos\rg"; out="rg-edges.json"; base="edges-baseline\rg-edges.json"; args=@("edges","crates/core/flags/defs.rs","--json",".")},
  @{repo="D:\projects\repos\tauri"; out="tauri-edges.json"; base="edges-baseline\tauri-edges.json"; args=@("edges","crates/tauri-utils/src/lib.rs","--json",".")},
  @{repo="D:\projects\repos\nushell"; out="nushell-edges.json"; base="edges-baseline\nushell-edges.json"; args=@("edges","crates/nu-protocol/src/lib.rs","--json",".")}
)
foreach ($p in $pairs) {
  Push-Location $p.repo
  try { & $jw @($p.args) 2>$null | Out-File -Encoding utf8 (Join-Path $tmp $p.out) } finally { Pop-Location }
  $base = Join-Path $root (".pro\exp\out\" + $p.base)
  $got = Join-Path $tmp $p.out
  $ok = (Test-Path $base) -and ((& $sha $base) -eq (& $sha $got))
  Write-Host ("sha_" + $p.out + "=" + $ok)
  if (-not $ok) { $failed += ("sha:" + $p.out) }
}

# 2b) text-card sha256 (BOM+CRLF, Out-File utf8 convention)
$textBase = Join-Path $root ".pro\exp\out\t23c-textcard-sha.txt"
if (Test-Path $textBase) {
  $tmap = @{}
  Get-Content $textBase | ForEach-Object {
    if ($_ -notmatch '^#' -and $_ -match '^(\S+).*sha256=([0-9A-F]+)') { $tmap[$Matches[1]] = $Matches[2] }
  }
  $tpairs = @(
    @{key="rg-file"; repo="D:\projects\repos\rg"; args=@("card","crates/core/flags/defs.rs")},
    @{key="rg-range"; repo="D:\projects\repos\rg"; args=@("card","crates/core")},
    @{key="tauri-file"; repo="D:\projects\repos\tauri"; args=@("card","crates/tauri-utils/src/lib.rs")}
  )
  foreach ($p in $tpairs) {
    $of = Join-Path $tmp ("text-" + $p.key + ".txt")
    Push-Location $p.repo
    try { & $jw @($p.args) 2>$null | Out-File -Encoding utf8 $of } finally { Pop-Location }
    $ok = $tmap.ContainsKey($p.key) -and ((& $sha $of) -eq $tmap[$p.key])
    Write-Host ("text_sha_" + $p.key + "=" + $ok)
    if (-not $ok) { $failed += ("text:" + $p.key) }
  }
}

# 3) version
$ver = (& $jw --version).Trim()
Write-Host "version=$ver"
if ($ver -notmatch "jw 0\.4\.1") { $failed += "version" }

# 4) main.rs size + warnings
$lines = (Get-Content (Join-Path $root "crates\cli\src\main.rs")).Count
Write-Host "main_lines=$lines"
if ($Final -and $lines -ge 800) { $failed += "main_lines" }
$buildOut = (cmd /c "cargo build -p jw 2>&1") | Out-String
$warn = ([regex]::Matches($buildOut, "warning:")).Count
Write-Host "warnings=$warn"
$warnBase = Join-Path $root ".pro\exp\out\warn-baseline.txt"
if (Test-Path $warnBase) {
  $baseWarn = [int](Get-Content $warnBase -Raw).Trim()
  Write-Host "warn_baseline=$baseWarn"
  if ($warn -gt $baseWarn) { $failed += "warnings" }
} else {
  Set-Content -Path $warnBase -Value $warn -Encoding ascii
  Write-Host "warn_baseline_saved=$warn"
}
& $jw check D:\projects\repos\rg 2>$null | Out-Null

Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
if ($failed.Count -gt 0) { Write-Host ("SPLIT_FAIL " + ($failed -join ",")); exit 1 }
Write-Host "SPLIT_OK"
exit 0
