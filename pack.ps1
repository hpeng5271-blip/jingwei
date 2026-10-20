# Packaging (2026-10-20, user's call): dist\<yyyy-MM-dd>\ holding the GUI program and the manual.
# Nothing else -- no CLI exe, no run.cmd, no demo trees, no second copy of the same text.
# Why: the old package carried several copies of the same docs plus three demo projects; each
# copy could go stale, and what a user needs to start is one exe and one page.
#
# No version in any artifact name: the folder is the date, files keep their plain names.
# (The binary still reports its own version from `--version`; that is a fact, not a label.)
#
# ASCII ONLY. Windows PowerShell 5.1 reads a BOM-less .ps1 as ANSI, so a non-ASCII byte here
# turns into mojibake and can break parsing. Chinese text lives in tools\demos\pack-msg.txt
# (UTF-8, read explicitly) and is addressed by line number, same as before.
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '4' }

$msgFile = Join-Path $root 'tools\demos\pack-msg.txt'
if (-not (Test-Path -LiteralPath $msgFile)) { throw "missing $msgFile" }
$M = [System.IO.File]::ReadAllLines($msgFile, [System.Text.Encoding]::UTF8)
if ($M.Count -le 5) { throw "pack-msg.txt is missing line 5 (the manual's file name)" }
$manualName = $M[5]

$stamp = Get-Date -Format 'yyyy-MM-dd'
$out = Join-Path $root "dist\$stamp"

Write-Host 'building release binary (jingwei-gui)'
Push-Location $root
try {
    & cargo build --release -p jingwei-app
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release -p jingwei-app failed ($LASTEXITCODE)" }
} finally { Pop-Location }

$gui = Join-Path $root 'target\release\jingwei-gui.exe'
if (-not (Test-Path -LiteralPath $gui)) { throw "missing $gui (did the release build run?)" }
# A release MUST carry the manual. The rule used to be "copy it if present, else skip", and one
# shipped package had no manual and nobody noticed. Stop here instead.
$manual = Join-Path $root $manualName
if (-not (Test-Path -LiteralPath $manual)) { throw "missing $manual" }

if (Test-Path -LiteralPath $out) { [System.IO.Directory]::Delete($out, $true) }
[void](New-Item -ItemType Directory -Path $out -Force)
Copy-Item -LiteralPath $gui -Destination (Join-Path $out 'jingwei-gui.exe')
Copy-Item -LiteralPath $manual -Destination (Join-Path $out $manualName)

Write-Host "done: $out"
Get-ChildItem -LiteralPath $out | ForEach-Object { Write-Host ("  {0}  {1:N0} bytes" -f $_.Name, $_.Length) }
