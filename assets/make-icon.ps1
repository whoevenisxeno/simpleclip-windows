<#
  Builds every icon file from assets\icon.svg, the single source for the logo:
    simpleclip.ico   the exe / installer / shortcut icon (16 to 256 px)
    icon-256.png     the window and taskbar icon
    icon-64.png      the logo in the app's sidebar
    tray-32.rgba     raw 32x32 RGBA pixels for the tray icon
  The SVG is rasterised with Microsoft Edge in headless mode (already on Windows
  10 and 11), so no extra tools are needed. Run from the repo root:
    powershell -File assets\make-icon.ps1
#>
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$out = $PSScriptRoot
$svg = Join-Path $out 'icon.svg'
$edge = @(
    "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
    "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { throw 'Microsoft Edge was not found, it is needed to rasterise icon.svg.' }

$work = Join-Path $env:TEMP 'simpleclip-icon'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
Copy-Item $svg (Join-Path $work 'icon.svg')

function Render([int]$n) {
    $html = Join-Path $work "p$n.html"
    "<!doctype html><meta charset=utf-8><style>html,body{margin:0;background:transparent;overflow:hidden}img{display:block;width:${n}px;height:${n}px}</style><img src='icon.svg'>" |
        Set-Content $html -Encoding utf8
    $png = Join-Path $work "icon-$n.png"
    $prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'   # Edge prints harmless notes on stderr
    & $edge --headless=new --disable-gpu --hide-scrollbars --default-background-color=00000000 `
        --force-device-scale-factor=1 --window-size="$n,$n" --screenshot=$png `
        "file:///$(($work -replace '\\','/'))/p$n.html" 2>&1 | Out-Null
    for ($i = 0; $i -lt 40 -and -not (Test-Path $png); $i++) { Start-Sleep -Milliseconds 250 }
    if (-not (Test-Path $png)) { throw "Edge did not produce a ${n}px image." }
    return $png
}

$sizes = 16, 24, 32, 48, 64, 128, 256
$files = @{}
foreach ($s in $sizes) { $files[$s] = Render $s }
$pngs = foreach ($s in $sizes) { , [System.IO.File]::ReadAllBytes($files[$s]) }
Copy-Item $files[256] (Join-Path $out 'icon-256.png') -Force
Copy-Item $files[64] (Join-Path $out 'icon-64.png') -Force

# Tray icon: raw RGBA, 4 bytes per pixel, row by row.
$bmp = [System.Drawing.Bitmap]::FromFile($files[32])
$raw = New-Object byte[] (32 * 32 * 4)
$i = 0
for ($y = 0; $y -lt 32; $y++) {
    for ($x = 0; $x -lt 32; $x++) {
        $c = $bmp.GetPixel($x, $y)
        $raw[$i++] = $c.R; $raw[$i++] = $c.G; $raw[$i++] = $c.B; $raw[$i++] = $c.A
    }
}
$bmp.Dispose()
[System.IO.File]::WriteAllBytes((Join-Path $out 'tray-32.rgba'), $raw)

# ICO container: ICONDIR + one ICONDIRENTRY per image, PNG payloads.
$ms = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $ms
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $dim = if ($s -ge 256) { 0 } else { $s }
    $w.Write([byte]$dim); $w.Write([byte]$dim)
    $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$pngs[$i].Length); $w.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($p in $pngs) { $w.Write($p) }
[System.IO.File]::WriteAllBytes((Join-Path $out 'simpleclip.ico'), $ms.ToArray())
Write-Host "wrote simpleclip.ico, icon-256.png, icon-64.png and tray-32.rgba in $out"
