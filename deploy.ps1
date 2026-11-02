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

Write-Host "== stopping running instance ==" -ForegroundColor Cyan
$running = Get-Process -Name 'escape-proxy' -ErrorAction SilentlyContinue
if ($running) {
    $running | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep -Milliseconds 800
    $running = Get-Process -Name 'escape-proxy' -ErrorAction SilentlyContinue
    if ($running) { $running | Stop-Process -Force }
}

Write-Host "== installing to $dst ==" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $dst | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $dst 'logs') | Out-Null
Copy-Item (Join-Path $repo 'target\release\escape-proxy.exe') $dst -Force

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
