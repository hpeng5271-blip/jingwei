# Package jingwei v0.4.1-rc (ASCII only, idempotent).
# Usage: powershell -ExecutionPolicy Bypass -File scripts\package.ps1 [-NoBuild]
param(
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
$root = "D:\projects\jingwei"
$shell = "D:\projects\jingwei-shell"
Set-Location $root

if (-not $NoBuild) {
    Write-Host "[1/5] build release (jw + gui)"
    cargo build --release -p jw
    if ($LASTEXITCODE -ne 0) { throw "jw release build failed" }
    cargo build --release --manifest-path "$shell\src-tauri\Cargo.toml"
    if ($LASTEXITCODE -ne 0) { throw "gui release build failed" }
}

$versionLine = (& "$root\target\release\jw.exe" --version) -join ""
if ($versionLine -notmatch "^jw ([0-9]+\.[0-9]+\.[0-9]+) \(rules ([0-9]+)\)$") {
    throw "unexpected version string: $versionLine"
}
$ver = $Matches[1]
$rules = $Matches[2]
$name = "jingwei-v$ver"
Write-Host "[2/5] version=$versionLine -> $name"

$dist = Join-Path $root "dist"
$stage = Join-Path $env:TEMP ("jw-pack-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force "$stage\bin", "$stage\docs", "$stage\ui" | Out-Null

Copy-Item "$shell\target\release\jingwei-gui.exe" "$stage\bin\jingwei-gui.exe"
Copy-Item "$root\target\release\jw.exe" "$stage\bin\jw.exe"
if (Test-Path "$root\README.md") { Copy-Item "$root\README.md" "$stage\docs\README.md" }
if (Test-Path "$root\docs\reference\jingwei-final-design.md") { Copy-Item "$root\docs\reference\jingwei-final-design.md" "$stage\docs\jingwei-final-design.md" }
if (Test-Path "$root\docs\MANIFEST-摘要.md") { Copy-Item "$root\docs\MANIFEST-摘要.md" "$stage\docs\MANIFEST.md" }
if (Test-Path "$root\CHANGELOG.md") { Copy-Item "$root\CHANGELOG.md" "$stage\docs\CHANGELOG.md" }
Copy-Item "$root\tools\web\ui\*" "$stage\ui\" -Recurse
Copy-Item "$dist\jingwei-v$ver.src-manifest.txt" "$stage\src-manifest.txt" -ErrorAction SilentlyContinue

Write-Host "[3/5] contents"
Get-ChildItem $stage -Recurse -File | ForEach-Object { "  " + $_.FullName.Substring($stage.Length + 1) }

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path "$stage\*" -DestinationPath $zip -Force
$zipHash = (Get-FileHash $zip -Algorithm SHA256).Hash
"$zipHash  $name.zip" | Out-File -FilePath "$zip.sha256" -Encoding ascii

Write-Host "[4/5] verify"
$z = [System.IO.Compression.ZipFile]::OpenRead($zip)
$entries = @($z.Entries | ForEach-Object { $_.FullName -replace "\\", "/" })
$z.Dispose()
$required = @("bin/jw.exe", "bin/jingwei-gui.exe", "src-manifest.txt", "ui/index.html", "ui/shim.js")
$missing = @()
foreach ($r in $required) { if ($entries -notcontains $r) { $missing += $r } }
if ($missing.Count -gt 0) { throw "zip missing: $($missing -join ', ')" }
$nonAscii = @($entries | Where-Object { $_ -match "[^\x00-\x7F]" })
if ($nonAscii.Count -gt 0) { throw "non-ASCII entries: $($nonAscii -join ', ')" }

Write-Host "[5/5] ok"
Write-Host "zip=$zip"
Write-Host "zip_sha256=$zipHash"
Write-Host "bytes=$((Get-Item $zip).Length)"
Write-Host "version=$versionLine rules=$rules"
Remove-Item $stage -Recurse -Force