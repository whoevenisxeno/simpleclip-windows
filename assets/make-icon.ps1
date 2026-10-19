<#
  Regenerates the app icon (simpleclip.ico + icon-256.png) from code, so there is
  no binary design asset to keep in sync. Run from the repo root:
    powershell -File assets\make-icon.ps1
#>
Add-Type -AssemblyName System.Drawing
$out = $PSScriptRoot

function Render([int]$n) {
    $bmp = New-Object System.Drawing.Bitmap $n, $n, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)

    # Rounded dark tile.
    $r = $n * 0.22
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc(0, 0, 2 * $r, 2 * $r, 180, 90)
    $path.AddArc($n - 2 * $r - 1, 0, 2 * $r, 2 * $r, 270, 90)
    $path.AddArc($n - 2 * $r - 1, $n - 2 * $r - 1, 2 * $r, 2 * $r, 0, 90)
    $path.AddArc(0, $n - 2 * $r - 1, 2 * $r, 2 * $r, 90, 90)
    $path.CloseFigure()
    $bg = New-Object System.Drawing.Drawing2D.LinearGradientBrush (
        (New-Object System.Drawing.Point 0, 0), (New-Object System.Drawing.Point 0, $n),
        [System.Drawing.Color]::FromArgb(255, 38, 42, 56), [System.Drawing.Color]::FromArgb(255, 18, 20, 28))
    $g.FillPath($bg, $path)

    # Replay ring with a gap, and a record dot.
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 235, 238, 245)), ($n * 0.07)
    $pen.StartCap = 'Round'; $pen.EndCap = 'Round'
    $m = $n * 0.20
    $g.DrawArc($pen, $m, $m, $n - 2 * $m, $n - 2 * $m, -60, 300)
    $dot = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 229, 57, 53))
    $d = $n * 0.30
    $g.FillEllipse($dot, ($n - $d) / 2, ($n - $d) / 2, $d, $d)
    $g.Dispose()
    return $bmp
}

function PngBytes($bmp) {
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    return , $ms.ToArray()
}

$sizes = 16, 24, 32, 48, 64, 128, 256
$pngs = foreach ($s in $sizes) { , (PngBytes (Render $s)) }
[System.IO.File]::WriteAllBytes((Join-Path $out 'icon-256.png'), $pngs[-1])

# ICO container: ICONDIR + one ICONDIRENTRY per image, PNG payloads.
$ms = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $ms
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
    $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
    $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$pngs[$i].Length); $w.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($p in $pngs) { $w.Write($p) }
[System.IO.File]::WriteAllBytes((Join-Path $out 'simpleclip.ico'), $ms.ToArray())
Write-Host "wrote simpleclip.ico and icon-256.png in $out"
