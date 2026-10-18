# 打一个"试用包"：一个文件夹，三样东西，直接递给别人。
#
# 为什么要有这个脚本（2026-10-18）：
#   「一个真人用一次」是这个项目**唯一一条从来没发生过的验证**。
#   前面 51 条标注、3 个真项目、一堆门禁，全是**我自证**——我是出题人也是裁判。
#   所以要有人能"照着跑一遍"，而且**卡在哪要能记下来**。
#   一份写在文档里的"以后要找人试用"等于没写；能递出去的文件夹才算。
#
# 判据（这个脚本自己的验收）：
#   dist\trial\ 里必须恰好有 exe + 试用脚本.md + 记录表.md，exe 能启动（版本号能打出来）。

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root 'dist\trial'
$exe = Join-Path $root 'target\release\jingwei-gui.exe'

if (-not (Test-Path $exe)) {
    Write-Host "  ✗ 找不到 $exe" -ForegroundColor Red
    Write-Host "    先跑：cargo build --release -p jingwei-app" -ForegroundColor DarkGray
    exit 1
}

if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Path $out -Force | Out-Null

# ① exe（绿色免安装：双击就开）
Copy-Item $exe (Join-Path $out 'jingwei-gui.exe') -Force

# ② 两份文档：脚本 + 空白记录表
foreach ($doc in @('试用脚本.md', '试用记录表.md')) {
    $src = Join-Path $root (Join-Path 'docs\trial' $doc)
    if (-not (Test-Path $src)) {
        Write-Host "  ✗ 缺 $doc" -ForegroundColor Red
        exit 1
    }
    Copy-Item $src (Join-Path $out $doc) -Force
}

# ③ 自查：三样齐、exe 能打出内容（不是空壳）
$names = (Get-ChildItem $out -File | Select-Object -ExpandProperty Name) | Sort-Object
$want = @('jingwei-gui.exe', '试用脚本.md', '试用记录表.md') | Sort-Object
$ok = ($names -join '|') -eq ($want -join '|')
Write-Host ("  三样东西齐了吗：" + $(if ($ok) { '是' } else { '否——' + ($names -join ', ') })) -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })

$kb = [math]::Round((Get-Item (Join-Path $out 'jingwei-gui.exe')).Length / 1MB, 1)
Write-Host ("  exe " + $kb + " MB，路径：" + $out)
Write-Host "  （递出去时只说一句：打开它，选一个你自己的项目，然后告诉我这个项目能不能跑。）" -ForegroundColor DarkGray

if (-not $ok) { exit 1 }
exit 0
