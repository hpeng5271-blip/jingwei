# v1 acceptance: golden layer (fixed counts on tests/fixtures/v1-golden) + self-consistency layer.
# Layer 1: b55_golden.py  (fails if any fixed count changes)
# Layer 2: b53_v1_accept.py (render == SQL; demo structural invariants + tauri cards)
# Layer 3: b79_accept_new.py (external contracts: loc / gap reason codes / capability)
param(
    [string]$Out = "D:\projects\jingwei\.pro\exp\out"
)
$py = "C:\Users\cjz\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python\python.exe"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$t0 = Get-Date
Write-Output "=== golden ==="
& $py "D:\projects\jingwei\.pro\exp\b55_golden.py"
$g = $LASTEXITCODE
Write-Output "=== self-consistency ==="
& $py "D:\projects\jingwei\.pro\exp\b53_v1_accept.py"
$s = $LASTEXITCODE
Write-Output "=== accept-new ==="
& $py "D:\projects\jingwei\.pro\exp\b79_accept_new.py"
$n = $LASTEXITCODE
Write-Output "=== dashboard ==="
$env:JW = "D:\projects\jingwei\target\debug\jw.exe"
$repo = Join-Path $env:TEMP "jw-v1-golden"
$html = Join-Path $Out "dashboard-v1.html"
$d = 1
if (Test-Path $repo) {
    & node "D:\projects\jingwei\tools\dashboard.mjs" $repo $html | Out-Null
    $d = $LASTEXITCODE
} else {
    Write-Output "dashboard: golden fixture repo missing"
}
$dashOk = $false
if ($d -eq 0 -and (Test-Path $html)) {
    $page = Get-Content $html -Raw
    $tl = & $env:JW timeline --json --top 3 $repo | ConvertFrom-Json
    $dashOk = $true
    foreach ($k in @("historical", "current", "deleted")) {
        $n2 = $tl.file_counts.$k
        if (-not $page.Contains([string]$n2)) { $dashOk = $false }
    }
    # ASCII anchor only: the page must be a table-rendering HTML (avoid non-ASCII
    # literals in .ps1: PS 5.1 reads non-BOM scripts as ANSI and mangles them).
    if (-not ($page.Contains("<h1>") -and $page.Contains("<table>"))) { $dashOk = $false }
}
if (-not $dashOk) { $d = 1 }
$secs = [int]((Get-Date) - $t0).TotalSeconds
Write-Output ("v1_wall_s={0} golden={1} self={2} accept_new={3} dashboard={4}" -f $secs, $g, $s, $n, $d)
if ($g -ne 0 -or $s -ne 0 -or $n -ne 0 -or $d -ne 0) { exit 1 } else { exit 0 }
