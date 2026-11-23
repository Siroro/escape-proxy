# escape-proxy deployment script.
#
# Builds the release executable, installs it to %LOCALAPPDATA%\escape-proxy
# (preserving an existing config.toml), registers HKCU autostart and starts
# the application.
#
# Usage:
#   .\deploy.ps1               # deploy + autostart + start
#   .\deploy.ps1 -NoAutostart  # deploy without registering autostart
#   .\deploy.ps1 -NoStart      # deploy without launching
param(
    [switch]$NoAutostart,
    [switch]$NoStart
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSCommandPath
$dst = Join-Path $env:LOCALAPPDATA 'escape-proxy'
$exe = Join-Path $dst 'escape-proxy.exe'

Write-Host "== building release ==" -ForegroundColor Cyan
Push-Location $repo
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}

$built = Join-Path $repo 'target\release\escape-proxy.exe'

Write-Host "== stopping running instance ==" -ForegroundColor Cyan
$running = Get-Process -Name 'escape-proxy' -ErrorAction SilentlyContinue
if ($running) {
    # Closing the settings window only hides it to the tray, so ask the
    # running instance to quit through its single-instance channel. A build
    # that does not know --exit keeps running and is force-killed below.
    Start-Process -FilePath $built -ArgumentList '--exit'
    foreach ($p in $running) {
        $deadline = (Get-Date).AddSeconds(5)
        while ((Get-Process -Id $p.Id -ErrorAction SilentlyContinue) -and ((Get-Date) -lt $deadline)) {
            Start-Sleep -Milliseconds 200
        }
        if (Get-Process -Id $p.Id -ErrorAction SilentlyContinue) { Stop-Process -Id $p.Id -Force }
    }
}

Write-Host "== installing to $dst ==" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $dst | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $dst 'logs') | Out-Null
# The image can stay locked for a moment after the process is gone.
$installed = $false
foreach ($attempt in 1..5) {
    try {
        Copy-Item $built $dst -Force -ErrorAction Stop
        $installed = $true
        break
    } catch {
        Start-Sleep -Milliseconds 500
    }
}
if (-not $installed) { throw "could not replace $exe (is it still running?)" }

# Keep an existing config; create a default one on first install.
if (-not (Test-Path (Join-Path $dst 'config.toml'))) {
    $defaultConfig = @"
listen_addr = "127.0.0.1:10808"
enabled = true
max_connections = 128
connect_timeout_ms = 5000
handshake_timeout_ms = 5000
restrict_domains = false
domains = []

[ui]
show_on_startup = false
theme = "dark"
"@
    Set-Content -Path (Join-Path $dst 'config.toml') -Value $defaultConfig -Encoding ASCII
    Write-Host "   created default config.toml (edit it or use the settings window)"
}

if (-not $NoAutostart) {
    Write-Host "== registering HKCU autostart ==" -ForegroundColor Cyan
    $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    New-Item -Path $runKey -Force | Out-Null
    Set-ItemProperty -Path $runKey -Name 'escape-proxy' -Value "`"$exe`" --background"
}

if (-not $NoStart) {
    Write-Host "== starting escape-proxy ==" -ForegroundColor Cyan
    Start-Process -FilePath $exe
}

Write-Host "done." -ForegroundColor Green
Write-Host "next: configure FoxyProxy/PIA per README.md (section 'PIA + FoxyProxy setup')."
