# 生成图标：SVG 是源文件（给人看/给将来用），PNG/ICO 用本机 .NET 画，不引外部工具。
# 造型：圆角方底 + 几何化的字母 D（jingwei 首字）。笔画粗、结构简单，16px 下仍可辨。
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root 'src-tauri\icons'
[void](New-Item -ItemType Directory -Path $dir -Force)
Add-Type -AssemblyName System.Drawing

$brand = '#C2552F'
$mark  = '#FDFAF6'

# ── SVG 源 ──
$svg = @"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-label="jingwei">
  <rect x="0" y="0" width="512" height="512" rx="112" fill="$brand"/>
  <g stroke="$mark" stroke-linecap="round" fill="none">
    <path d="M173 120 V392" stroke-width="46"/>
    <path d="M173 120 A136 136 0 0 1 173 392" stroke-width="46"/>
  </g>
</svg>
"@
[System.IO.File]::WriteAllText((Join-Path $dir 'icon.svg'), $svg, (New-Object System.Text.UTF8Encoding($false)))

function New-Icon([int]$size, [string]$out) {
  $bmp = New-Object System.Drawing.Bitmap($size, $size)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'
  $g.Clear([System.Drawing.Color]::Transparent)
  $s = $size / 512.0
  # 圆角方底
  $r = [int](112 * $s)
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  $path.AddArc(0, 0, $d, $d, 180, 90)
  $path.AddArc($size - $d - 1, 0, $d, $d, 270, 90)
  $path.AddArc($size - $d - 1, $size - $d - 1, $d, $d, 0, 90)
  $path.AddArc(0, $size - $d - 1, $d, $d, 90, 90)
  $path.CloseFigure()
  $bg = New-Object System.Drawing.SolidBrush([System.Drawing.ColorTranslator]::FromHtml($brand))
  $g.FillPath($bg, $path)
  # 脊线 + 横肋
  $pen = New-Object System.Drawing.Pen([System.Drawing.ColorTranslator]::FromHtml($mark), (46 * $s))
  $pen.StartCap = 'Round'; $pen.EndCap = 'Round'
  $g.DrawLine($pen, 173 * $s, 120 * $s, 173 * $s, 392 * $s)
  $rect = New-Object System.Drawing.Rectangle([int](37 * $s), [int](120 * $s), [int](272 * $s), [int](272 * $s))
  $g.DrawArc($pen, $rect, -90, 180)
  $g.Dispose(); $pen.Dispose(); $bg.Dispose()
  $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}

foreach ($n in 32, 128, 256, 512) {
  New-Icon $n (Join-Path $dir "$n`x$n.png")
}
Copy-Item (Join-Path $dir '256x256.png') (Join-Path $dir '128x128@2x.png') -Force
New-Icon 64 (Join-Path $dir 'icon-64.png')

# ── ICO：把 PNG 塞进 ICO 容器（Vista+ 支持 PNG 压缩条目）──
$sizes = 16, 32, 48, 64, 128, 256
foreach ($s in $sizes) { New-Icon $s (Join-Path $env:TEMP "ico-$s.png") }
$blobs = @{}
foreach ($s in $sizes) { $blobs[$s] = [System.IO.File]::ReadAllBytes((Join-Path $env:TEMP "ico-$s.png")) }
$ms = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($ms)
$bw.Write([UInt16]0); $bw.Write([UInt16]1); $bw.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
foreach ($s in $sizes) {
  $len = $blobs[$s].Length
  $bw.Write([Byte]($(if ($s -ge 256) { 0 } else { $s })))
  $bw.Write([Byte]($(if ($s -ge 256) { 0 } else { $s })))
  $bw.Write([Byte]0); $bw.Write([Byte]0)
  $bw.Write([UInt16]1); $bw.Write([UInt16]32)
  $bw.Write([UInt32]$len); $bw.Write([UInt32]$offset)
  $offset += $len
}
foreach ($s in $sizes) { $bw.Write($blobs[$s]) }
$bw.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $dir 'icon.ico'), $ms.ToArray())
$bw.Dispose(); $ms.Dispose()
foreach ($s in $sizes) { [System.IO.File]::Delete((Join-Path $env:TEMP "ico-$s.png")) }

Get-ChildItem $dir | Select-Object Name, Length | Format-Table -AutoSize
