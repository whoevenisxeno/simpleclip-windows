[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\SimpleClip'),
    [switch]$RemoveData
)
$ErrorActionPreference = 'Continue'

if (Test-Path "$InstallDir\sc.exe") { & "$InstallDir\sc.exe" quit 2>$null | Out-Null }
Get-Process scd, sc-gui -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -like "$InstallDir\*" } | Stop-Process -Force
Start-Sleep -Milliseconds 500

Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'SimpleClip' -ErrorAction SilentlyContinue
Remove-Item "HKCU:\Software\Classes\AppUserModelId\SimpleClip" -Recurse -ErrorAction SilentlyContinue
Remove-Item (Join-Path ([Environment]::GetFolderPath('Programs')) 'SimpleClip.lnk') -ErrorAction SilentlyContinue

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath) {
    $kept = ($userPath -split ';' | Where-Object { $_ -and $_ -ne $InstallDir }) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $kept, 'User')
}

if ($PSScriptRoot -like "$InstallDir*") {
    Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-Command', "Start-Sleep 2; Remove-Item -LiteralPath '$InstallDir' -Recurse -Force"
}
else {
    Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
}

if ($RemoveData) {
    Remove-Item (Join-Path $env:APPDATA 'SimpleClip') -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $env:LOCALAPPDATA 'SimpleClip') -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host 'Removed settings and logs. Clips in your Videos\SimpleClip folder were left alone.'
}
Write-Host 'SimpleClip removed.' -ForegroundColor Green
