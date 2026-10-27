# Control gate: run the independent B checks on rg + tauri + nushell; exit 1 if any repo fails.
# NOTE: B shares the tree-sitter grammar with jw; traversal/aggregation is independent.
# Checks: (1) raw == edge + dedup  (2) symmetric difference of (src,leaf,line) sets is empty
#         (3) dedup == B_dup - distinct_by_dst_miss
# JSON is read back from a file (control/b51 write it with --out), so PS 5.1 stdout encoding
# never touches the numbers.
param(
    [string[]]$Repos = @("D:\projects\repos\rg", "D:\projects\repos\tauri", "D:\projects\repos\nushell")
)
$py = "C:\Users\cjz\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python\python.exe"
$gate = "D:\projects\jingwei\.pro\exp\b51_gate_control.py"
$outDir = "D:\projects\jingwei\.pro\exp\out"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$hist = Join-Path $outDir "control-history.jsonl"
$fail = 0
$t0 = Get-Date
foreach ($r in $Repos) {
    $name = Split-Path $r -Leaf
    $file = Join-Path $outDir "gate-$name.json"
    if (Test-Path $file) { Remove-Item $file -Force }
    & $py $gate $r --out $file 2>&1 | Out-Null
    $code = $LASTEXITCODE
    $ok = $false
    $obj = $null
    if (Test-Path $file) {
        try {
            $obj = (Get-Content $file -Raw -Encoding UTF8) | ConvertFrom-Json
            $ok = [bool]$obj.ok
        } catch {
            Write-Output ("parse_failed: {0}" -f $_)
            $ok = $false
        }
    }
    if ($code -ne 0 -or -not $ok) { $fail++ }
    if ($null -ne $obj) {
        try {
            $line = [ordered]@{ ts = (Get-Date).ToString("s"); repo = $name; ok = $obj.ok;
                raw = $obj.raw; edge = $obj.edge; dedup = $obj.dedup;
                distinct_by_dst_miss = $obj.distinct_by_dst_miss }
            ($line | ConvertTo-Json -Compress) | Add-Content -Encoding utf8 $hist
        } catch {
            Write-Output ("history_append_failed: {0}" -f $_)
        }
    }
    Write-Output ("{0}: {1}" -f $name, $(if ($ok) { "ok" } else { "FAIL" }))
}
$secs = [int]((Get-Date) - $t0).TotalSeconds
Write-Output ("gate_wall_s={0} fail={1}" -f $secs, $fail)
exit $(if ($fail -eq 0) { 0 } else { 1 })
