# Build the three demo projects. ASCII ONLY -- see the note below.
#
# WHY THIS SCRIPT IS PURE ASCII:
#   PowerShell 5.1 reads a .ps1 WITHOUT a BOM as ANSI, so Chinese text inside it
#   gets mangled -- quotes included, which turns the script into a syntax error.
#   (This is the same trap the project already recorded in 设计 §11.0, and the
#   reason tools/gates.json says "脚本保持纯 ASCII".)
#   So: every Chinese byte lives in a UTF-8 data file under tools/demos/,
#   and this script only copies them into place. It also means the demo files
#   are readable and diffable on their own, instead of being buried in a here-string.
#
# WHY A SCRIPT AT ALL: demo projects must be reproducible. Hand-building them once
#   means the next packaging round has to rediscover how they were broken.
#
#   demo 1  hello_python    a healthy project (scaffold output, untouched)
#   demo 2  messy_service   how "cannot tell" becomes "can tell" (each defect maps to a known rule id)
#   demo 3  hello_rust      what the graph page draws (multi-file, module tree, cross-file calls)
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File tools\make-demos.ps1

$ErrorActionPreference = 'Stop'

$root    = Split-Path -Parent $PSScriptRoot
$exe     = Join-Path $root 'target\release\jingwei.exe'
$data    = Join-Path $root 'tools\demos'
$demo    = Join-Path (Split-Path -Parent $root) 'jingwei-demo'

if (-not (Test-Path $exe)) {
    throw "missing $exe -- run: cargo build --release -p jingwei -p jingwei-app"
}
if (-not (Test-Path $data)) { throw "missing $data" }

# The demo folder is the PARENT directory: that is what the app scans.
if (Test-Path $demo) { [System.IO.Directory]::Delete($demo, $true) }
[void](New-Item -ItemType Directory -Path $demo -Force)
Write-Host "demo projects -> $demo"

# Purposes live in a UTF-8 file; reading them explicitly avoids any ANSI guesswork.
$purposes = [System.IO.File]::ReadAllLines((Join-Path $data 'purpose.txt'), [System.Text.Encoding]::UTF8)

function New-Demo([string]$name, [string]$tpl, [string]$lang, [string]$purpose) {
    & $exe new $name --template $tpl --lang $lang --dir $demo --no-venv --purpose $purpose | Out-Null
    $p = Join-Path $demo $name
    if (-not (Test-Path $p)) { throw "scaffold produced nothing at $p" }
    return $p
}

# ---------------------------------------------------------------- demo 1
[void](New-Demo 'hello_python' 'package' 'python' $purposes[0])

# ---------------------------------------------------------------- demo 2
$messy = New-Demo 'messy_service' 'package' 'python' $purposes[1]
$messySrc = Join-Path $data 'messy'

# (a) unpinned deps            -> py-lock-missing / py-unpinned
Copy-Item -LiteralPath (Join-Path $messySrc 'requirements.txt') -Destination $messy -Force

# (b) .env with secret-ish keys, not ignored -> env-file-exposed
Copy-Item -LiteralPath (Join-Path $messySrc '.env') -Destination $messy -Force

# (c) no .gitignore at all, but artifacts ARE present -> gitignore-missing
#     The rule needs both halves: no .gitignore AND something worth ignoring.
#     (Without the artifact it stays silent, and rightly so.)
Remove-Item -Force -LiteralPath (Join-Path $messy '.gitignore')
[void](New-Item -ItemType Directory -Path (Join-Path $messy 'node_modules\demo-pkg') -Force)
Set-Content -Path (Join-Path $messy 'node_modules\demo-pkg\index.js') -Encoding ascii -Value '// demo artifact'
[void](New-Item -ItemType Directory -Path (Join-Path $messy '__pycache__') -Force)
Set-Content -Path (Join-Path $messy '__pycache__\stage_1.cpython-315.pyc') -Encoding ascii -Value 'demo artifact'

# (d) system junk files        -> detritus
Set-Content -Path (Join-Path $messy 'Thumbs.db')   -Encoding ascii -Value 'demo'
Set-Content -Path (Join-Path $messy 'desktop.ini') -Encoding ascii -Value 'demo'

# (e) a whole nested .git      -> nested-git
[void](New-Item -ItemType Directory -Path (Join-Path $messy 'vendor\legacy\.git') -Force)

# (f) code flat in the root    -> no-src-test-layout
#     The rule fires only when there is a src-like dir but no tests dir, or
#     neither -- and the scaffold DOES create tests/. A project this messy would
#     not have tests, so drop them: that is what makes the finding true.
Remove-Item -Recurse -Force -LiteralPath (Join-Path $messy 'tests')
foreach ($n in 1..8) {
    Copy-Item -LiteralPath (Join-Path $messySrc "stage_$n.py") -Destination $messy -Force
}

# ---------------------------------------------------------------- demo 3
$rust = New-Demo 'hello_rust' 'package' 'rust' $purposes[2]

# The scaffold writes a single main.rs, which is too few symbols to draw.
# Replace src/ and tests/ with the module tree from tools/demos/hello_rust/,
# so the graph has cross-file calls to show.
# Note: -LiteralPath throws when the path is absent, so test first.
foreach ($sub in @('src', 'tests')) {
    $p = Join-Path $rust $sub
    if (Test-Path -LiteralPath $p) { Remove-Item -Recurse -Force -LiteralPath $p }
}
Copy-Item -Recurse -Force -LiteralPath (Join-Path $data 'hello_rust\src')   -Destination $rust
Copy-Item -Recurse -Force -LiteralPath (Join-Path $data 'hello_rust\tests') -Destination $rust
Copy-Item -Force -LiteralPath (Join-Path $data 'hello_rust\Cargo.toml')     -Destination $rust

# Cargo.lock: zero third-party deps, so this works offline.
# Native stderr is not a failure here, so do not let it abort the script.
$prev = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
& cargo generate-lockfile --manifest-path (Join-Path $rust 'Cargo.toml') --offline 2>&1 | Out-Null
$ErrorActionPreference = $prev
if (-not (Test-Path (Join-Path $rust 'Cargo.lock'))) {
    Write-Warning "Cargo.lock was not generated -- 'executable has no Cargo.lock' will be reported. That is a real finding, not a bug."
}

# ---------------------------------------------------------------- summary
Write-Host ''
Write-Host 'built:'
Get-ChildItem $demo -Directory | ForEach-Object {
    $files = (Get-ChildItem $_.FullName -Recurse -File -Force | Measure-Object).Count
    Write-Host ("  {0,-16} {1,4} files" -f $_.Name, $files)
}
Write-Host ''
Write-Host 'usage: open jingwei -> tab "overview" -> pick parent directory ->'
Write-Host "       $demo"
