# 打包：把两个 exe 和双击入口放进一个目录，再压成 zip。
# 不做安装器——这个工具是绿色的，解压就能跑，删掉就是卸载。
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$ver = (Get-Content "$root\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).version
$name = "jingwei-$ver-win-x64"
$out = Join-Path $root "dist\$name"

Write-Host "打包 $name"
if (Test-Path $out) { [System.IO.Directory]::Delete($out, $true) }
[void](New-Item -ItemType Directory -Path $out -Force)

foreach ($exe in @('jingwei.exe', 'jingwei-gui.exe')) {
    $src = Join-Path $root "target\release\$exe"
    if (-not (Test-Path $src)) { throw "缺 $exe：先跑 cargo build --release -p jingwei -p jingwei-app" }
    Copy-Item $src $out
}

# 双击入口。内容是纯 ASCII——.cmd 里写中文在 GBK/UTF-8 之间很容易变乱码，
# 说明写在 txt 里，那个可以带 BOM。
$cmd = @(
    '@echo off',
    'cd /d "%~dp0"',
    'start "" "jingwei-gui.exe" %*'
) -join "`r`n"
[System.IO.File]::WriteAllText((Join-Path $out 'run.cmd'), $cmd + "`r`n", [System.Text.Encoding]::ASCII)

# 说明文件是发行版的一部分，缺了就该在这里停下——曾经它是"有就拷、没有就算了"，
# 结果打包出来的一份里没有说明，谁都没发现。
$doc = Join-Path $root '使用说明.md'
if (-not (Test-Path -LiteralPath $doc)) {
    throw "缺 使用说明.md——发行版必须带说明书，不能悄悄少一份"
}

$msg = "经纬 $ver" + [Environment]::NewLine + [Environment]::NewLine
$msg += "完整说明见同目录的 使用说明.md。"
$msg += [Environment]::NewLine + [Environment]::NewLine
$msg += "第一次用：双击 run.cmd，选一个装代码的目录，然后选中列出来的项目看「概览」。"
[System.IO.File]::WriteAllText((Join-Path $out '先看这个.txt'), $msg, (New-Object System.Text.UTF8Encoding($true)))
Copy-Item -LiteralPath $doc -Destination $out

$zip = Join-Path $root "dist\$name.zip"
if (Test-Path $zip) { [System.IO.File]::Delete($zip) }
Compress-Archive -Path "$out\*" -DestinationPath $zip -CompressionLevel Optimal

$size = [math]::Round((Get-Item $zip).Length / 1MB, 2)
Write-Host "好了：$zip（$size MB）"
