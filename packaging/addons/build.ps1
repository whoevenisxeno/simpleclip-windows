[CmdletBinding()]
param(
    [string[]]$Crates = @('sc-events', 'sc-voice', 'sc-roulette'),
    [string]$Out,
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
if (-not $Out) { $Out = Join-Path $root 'dist\addons' }
$prefix = 'https://raw.githubusercontent.com/whoevenisxeno/simpleclip-windows/addons/packages/'
$utf8 = New-Object Text.UTF8Encoding($false)

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:Path += ";$cargoBin" }

$found = @()
foreach ($crate in $Crates) {
    $dir = Join-Path $root "crates\$crate"
    if (-not (Test-Path "$dir\Cargo.toml") -or -not (Test-Path "$dir\manifest.json")) {
        Write-Warning "Skipping $crate, it has no Cargo.toml or manifest.json yet."
        continue
    }
    $found += $crate
}
if (-not $found) { throw 'No add-on crates to package.' }

if (-not $NoBuild) {
    Step "Building $($found -join ', ')"
    $cargoArgs = @('build', '--release')
    foreach ($crate in $found) { $cargoArgs += @('-p', $crate) }
    Push-Location $root
    try {
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
    }
    finally { Pop-Location }
}

$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
$release = Join-Path $target 'release'
$packages = Join-Path $Out 'packages'
$staging = Join-Path $Out 'staging'
Remove-Item $Out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $packages | Out-Null

$entries = @()
foreach ($crate in $found) {
    $dir = Join-Path $root "crates\$crate"
    $toml = Select-String -Path "$dir\Cargo.toml" -Pattern '^version = "(.+)"' | Select-Object -First 1
    if (-not $toml) { throw "$crate has no version of its own in Cargo.toml." }
    $version = $toml.Matches[0].Groups[1].Value
    $text = [IO.File]::ReadAllText("$dir\manifest.json")
    $m = $text | ConvertFrom-Json
    $id = $m.name
    if ($id -notmatch '^[a-z0-9-]{1,32}$') { throw "$crate has a bad add-on name '$id'." }
    $exe = if ($m.exe) { $m.exe } else { "sc-$id.exe" }
    $built = Join-Path $release $exe
    if (-not (Test-Path $built)) { throw "$built was not found. Build it first or drop -NoBuild." }
    $text = ([regex]'"version"\s*:\s*"[^"]*"').Replace($text, "`"version`": `"$version`"", 1)

    $stage = Join-Path $staging $id
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item $built $stage
    [IO.File]::WriteAllText((Join-Path $stage 'manifest.json'), $text, $utf8)
    $file = "$id-$version.zip"
    $zip = Join-Path $packages $file
    Compress-Archive -Path (Join-Path $stage 'manifest.json'), (Join-Path $stage $exe) -DestinationPath $zip
    $item = Get-Item $zip
    $entries += [pscustomobject][ordered]@{
        id      = $id
        title   = [string]$m.title
        about   = [string]$m.about
        version = $version
        min_app = [string]$m.min_app
        url     = $prefix + $file
        size    = [int64]$item.Length
        sha256  = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    }
    Step "Packaged $id $version ($($item.Length) bytes)"
}
Remove-Item $staging -Recurse -Force

$json = ConvertTo-Json -InputObject ([pscustomobject]@{ addons = [object[]]$entries }) -Depth 5
$catalog = Join-Path $Out 'catalog.json'
[IO.File]::WriteAllText($catalog, $json + "`n", $utf8)
Step "Wrote $catalog"
