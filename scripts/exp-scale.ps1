# Scale experiment: run the full pipeline on one repo and record wall time / size.
# Usage: powershell -File scripts/exp-scale.ps1 <repo>
param([Parameter(Mandatory = $true)][string]$Repo)

$ErrorActionPreference = "Stop"
$jw = "D:\projects\jingwei\target\debug\jw.exe"
$name = Split-Path $Repo -Leaf

function Step($label, $block) {
    $t = Get-Date
    $out = & $block 2>&1 | Out-String
    $ms = [int]((Get-Date) - $t).TotalMilliseconds
    "{0,-10} {1,8} ms  {2}" -f $label, $ms, ($out.Trim() -split "`n" | Select-Object -First 1)
}

"== $name =="
Push-Location $Repo
try {
    & $jw init . | Out-Null
    Step "ingest" { & $jw ingest --limit 0 . }
    Step "docs" { & $jw docs . }
    Step "parse" { & $jw parse . }
    Step "deps" { & $jw deps . }
    Step "check" { & $jw check . }
    Step "coverage" { & $jw coverage . --limit 1 }
    "--- stat ---"
    & $jw stat . | Select-Object -First 12
    "--- timeline ---"
    & $jw timeline . | Select-Object -First 7
}
finally { Pop-Location }
