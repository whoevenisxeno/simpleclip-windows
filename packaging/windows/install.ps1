<#
.SYNOPSIS
  Install SimpleClip for the current user (no admin needed).

.DESCRIPTION
  Copies the files next to this script into %LOCALAPPDATA%\Programs\SimpleClip,
  puts `sc` on your PATH, adds a Start Menu shortcut, starts the daemon with
  Windows, and launches it. Re-running upgrades in place (the running daemon is
  stopped first).
#>
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\SimpleClip'),
    [switch]$NoAutostart,
    [switch]$NoLaunch,
    [switch]$NoPath
)
$ErrorActionPreference = 'Stop'
$src = $PSScriptRoot
if (-not (Test-Path "$src\scd.exe")) { throw "scd.exe not found next to install.ps1 - run this from the dist\SimpleClip folder." }

# Stop a running daemon/GUI so their files can be replaced.
if (Test-Path "$InstallDir\sc.exe") { & "$InstallDir\sc.exe" quit 2>$null | Out-Null }
Get-Process scd, sc-gui -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -like "$InstallDir\*" } | Stop-Process -Force
Start-Sleep -Milliseconds 500

New-Item -ItemType Directory -Force $InstallDir | Out-Null
Get-ChildItem $src -File | Where-Object { $_.Name -ne 'install.ps1' } | Copy-Item -Destination $InstallDir -Force
Copy-Item "$src\install.ps1" $InstallDir -Force
Write-Host "Installed to $InstallDir"

# Start Menu shortcut for the GUI.
$shell = New-Object -ComObject WScript.Shell
$lnk = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Programs')) 'SimpleClip.lnk'))
$lnk.TargetPath = "$InstallDir\sc-gui.exe"
$lnk.WorkingDirectory = $InstallDir
$lnk.Description = 'SimpleClip instant replay'
$lnk.Save()

# `sc` on the user PATH.
if (-not $NoPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($userPath -split ';') -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ";$InstallDir"), 'User')
        Write-Host "Added $InstallDir to your PATH (open a new terminal to use 'sc')."
    }
}

# Start the daemon at login.
if (-not $NoAutostart) {
    Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'SimpleClip' -Value "`"$InstallDir\scd.exe`""
    Write-Host 'The daemon will start when you sign in (toggle it from the tray icon).'
}

if (-not $NoLaunch) {
    Start-Process "$InstallDir\scd.exe"
    Start-Sleep -Seconds 1
    Start-Process "$InstallDir\sc-gui.exe"
}
Write-Host 'SimpleClip is installed. Default save hotkey: ALT+F10.' -ForegroundColor Green
