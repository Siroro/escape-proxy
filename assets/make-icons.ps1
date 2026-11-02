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
# Keep generous padding so the small title-bar rendering never looks crowded.
$fill = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 76, 175, 80))
$ring = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 24, 28, 34)), 2
$g.FillEllipse($fill, 5, 5, 22, 22)
$g.DrawEllipse($ring, 6.5, 6.5, 19, 19)
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
