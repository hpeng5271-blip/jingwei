# parse scale probe: run one parse, sample peak RSS, record wall time.
param(
    [string]$Repo = "D:\projects\repos\rust",
    [int]$Limit = 20000
)
$jw = "D:\projects\jingwei\target\debug\jw.exe"
$log = Join-Path $env:TEMP "jw-parse-attrib.log"
$err = Join-Path $env:TEMP "jw-parse-attrib.err"
$t0 = Get-Date
$p = Start-Process -FilePath $jw -ArgumentList "parse", "--limit", "$Limit", "." `
    -WorkingDirectory $Repo -RedirectStandardOutput $log -RedirectStandardError $err -PassThru
$peak = 0
while (-not $p.HasExited) {
    try {
        $ws = (Get-Process -Id $p.Id -ErrorAction Stop).WorkingSet64
        if ($ws -gt $peak) { $peak = $ws }
    }
    catch { }
    Start-Sleep -Milliseconds 500
}
$secs = [int]((Get-Date) - $t0).TotalSeconds
"wall_s=$secs peak_rss_mb=$([int]($peak / 1MB))"
Get-Content $log -ErrorAction SilentlyContinue | Select-Object -Last 4
Get-Content $err -ErrorAction SilentlyContinue | Select-Object -Last 3
