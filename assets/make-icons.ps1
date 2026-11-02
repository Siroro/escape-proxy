# Regenerates assets/app.ico (a 32x32 RGBA icon containing a PNG frame).
# The tray status icons are generated in code (src/platform/windows/tray.rs),
# so this is the only binary asset the repository carries.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$out = Join-Path $PSScriptRoot 'app.ico'
$size = 32

$bmp = New-Object System.Drawing.Bitmap $size, $size
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.Clear([System.Drawing.Color]::Transparent)
$fill = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 76, 175, 80))
$ring = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 24, 28, 34)), 3
$g.FillEllipse($fill, 3, 3, 26, 26)
$g.DrawEllipse($ring, 4.5, 4.5, 23, 23)
$g.Dispose()

$ms = New-Object System.IO.MemoryStream
$bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
$png = $ms.ToArray()

# ICO container with a single PNG frame (valid on Vista and later).
$header = [byte[]](0, 0, 1, 0, 1, 0)
$entry = [byte[]]@(
    [byte]$size, [byte]$size, 0, 0, 1, 0, 32, 0
) + [BitConverter]::GetBytes([Int32]$png.Length) + [BitConverter]::GetBytes([Int32]22)
[System.IO.File]::WriteAllBytes($out, $header + $entry + $png)
Write-Host "wrote $out"
