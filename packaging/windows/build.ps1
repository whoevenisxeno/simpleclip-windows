[CmdletBinding()]
param(
    [string]$FfmpegDir = $env:FFMPEG_DIR,
    [ValidateSet('gpl', 'lgpl')][string]$Variant = 'lgpl',
    [switch]$InstallDeps,
    [switch]$NoZip,
    [switch]$NoInstaller
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$depsDir = Join-Path $env:LOCALAPPDATA 'SimpleClip\build-deps'

function Have($name) { [bool](Get-Command $name -ErrorAction SilentlyContinue) }
function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

function Need($what, $wingetId, $override) {
    if (-not $InstallDeps) {
        $cmd = "winget install --id $wingetId -e"
        if ($override) { $cmd += " --override `"$override`"" }
        throw "$what is missing. Install it with:`n  $cmd`nor re-run this script with -InstallDeps."
    }
    Step "Installing $what"
    $wingetArgs = @('install', '--id', $wingetId, '-e', '--accept-package-agreements', '--accept-source-agreements')
    if ($override) { $wingetArgs += @('--override', $override) }
    & winget @wingetArgs
}

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:Path += ";$cargoBin" }
if (-not (Have cargo)) {
    Need 'Rust (rustup)' 'Rustlang.Rustup'
    $env:Path += ";$cargoBin"
}

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$haveMsvc = (Test-Path $vswhere) -and
    (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if (-not $haveMsvc) {
    Need 'Visual Studio Build Tools (C++ workload)' 'Microsoft.VisualStudio.2022.BuildTools' `
        '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
}

if (-not $env:LIBCLANG_PATH) {
    $llvm = "$env:ProgramFiles\LLVM\bin"
    if (-not (Test-Path "$llvm\libclang.dll")) {
        Need 'LLVM' 'LLVM.LLVM'
    }
    $env:LIBCLANG_PATH = $llvm
}

if (-not $FfmpegDir -or -not (Test-Path "$FfmpegDir\lib\avcodec.lib")) {
    $FfmpegDir = Join-Path $depsDir "ffmpeg-$Variant"
    if (-not (Test-Path "$FfmpegDir\lib\avcodec.lib")) {
        if (-not $InstallDeps) {
            throw "No FFmpeg dev build found. Set FFMPEG_DIR to one (bin, lib, include), or re-run with -InstallDeps to download it."
        }
        Step "Downloading FFmpeg 8.1 ($Variant, shared)"
        New-Item -ItemType Directory -Force $depsDir | Out-Null
        $zip = Join-Path $depsDir "ffmpeg-$Variant.zip"
        $url = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-$Variant-shared-8.1.zip"
        Invoke-WebRequest $url -OutFile $zip
        $tmp = Join-Path $depsDir "extract-$Variant"
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
        Expand-Archive $zip $tmp -Force
        Move-Item (Get-ChildItem $tmp -Directory | Select-Object -First 1).FullName $FfmpegDir
        Remove-Item $tmp, $zip -Recurse -Force
    }
}
$env:FFMPEG_DIR = $FfmpegDir
Step "Using FFmpeg at $FfmpegDir"

Step 'Building release binaries'
Push-Location $root
try {
    & cargo build --release --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
}
finally { Pop-Location }

$version = (Select-String -Path "$root\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$stage = Join-Path $root "dist\SimpleClip"
Step "Staging $stage"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($exe in 'scd.exe', 'sc.exe', 'sc-gui.exe') {
    Copy-Item "$root\target\release\$exe" $stage
}
foreach ($lib in 'avcodec', 'avformat', 'avutil', 'swresample', 'swscale') {
    Copy-Item "$FfmpegDir\bin\$lib-*.dll" $stage
}
Copy-Item "$root\LICENSE" $stage
Copy-Item "$PSScriptRoot\install.ps1", "$PSScriptRoot\uninstall.ps1" $stage
if ($Variant -eq 'gpl') {
    Set-Content (Join-Path $stage 'FFMPEG-LICENSE.txt') `
        "The bundled FFmpeg DLLs are a GPL build (BtbN/FFmpeg-Builds, includes libx264). See https://ffmpeg.org/legal.html."
}
else {
    Set-Content (Join-Path $stage 'FFMPEG-LICENSE.txt') `
        "The bundled FFmpeg DLLs are an LGPL build (BtbN/FFmpeg-Builds). See https://ffmpeg.org/legal.html."
}

if (-not $NoZip) {
    $zipPath = Join-Path $root "dist\SimpleClip-$version-win64.zip"
    Remove-Item $zipPath -ErrorAction SilentlyContinue
    Compress-Archive -Path $stage -DestinationPath $zipPath
    Step "Wrote $zipPath"
}

$appZip = Join-Path $root "dist\SimpleClip-$version-app.zip"
Remove-Item $appZip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\scd.exe", "$stage\sc.exe", "$stage\sc-gui.exe" -DestinationPath $appZip
Step "Wrote $appZip"
$eventsStage = Join-Path $root "dist\events"
Remove-Item $eventsStage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $eventsStage | Out-Null
Copy-Item "$root\target\release\sc-events.exe" $eventsStage
Set-Content (Join-Path $eventsStage 'manifest.json') "{`"name`":`"events`",`"version`":`"$version`"}" -Encoding ascii
$eventsZip = Join-Path $root "dist\SimpleClip-$version-events.zip"
Remove-Item $eventsZip -ErrorAction SilentlyContinue
Compress-Archive -Path "$eventsStage\*" -DestinationPath $eventsZip
Step "Wrote $eventsZip"
$dllList = Join-Path $root "dist\SimpleClip-$version-dlls.sha256"
Get-ChildItem $stage -Filter *.dll | Sort-Object Name | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content $dllList -Encoding ascii
Step "Wrote $dllList"

if (-not $NoInstaller) {
    $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") |
            Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if (-not $iscc -and $InstallDeps) {
        Need 'Inno Setup' 'JRSoftware.InnoSetup'
        $iscc = "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    }
    if ($iscc -and (Test-Path $iscc)) {
        Step 'Building installer'
        & $iscc /Q "/DAppVersion=$version" "/DSourceDir=$stage" "$PSScriptRoot\simpleclip.iss"
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed ($LASTEXITCODE)" }
        Step "Wrote $root\dist\SimpleClip-$version-Setup.exe"
    }
    else {
        Write-Warning 'Inno Setup not found, skipping Setup.exe (winget install JRSoftware.InnoSetup, or pass -InstallDeps).'
    }
}
Write-Host "Done. Run dist\SimpleClip-$version-Setup.exe to install, or start dist\SimpleClip\scd.exe directly." -ForegroundColor Green
