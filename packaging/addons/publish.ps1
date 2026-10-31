[CmdletBinding()]
param(
    [string]$Dist,
    [string]$Work,
    [string]$Remote = 'origin',
    [string]$Branch = 'addons',
    [switch]$Push
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
if (-not $Dist) { $Dist = Join-Path $root 'dist\addons' }
if (-not $Work) { $Work = Join-Path $root 'dist\addons-branch' }
$utf8 = New-Object Text.UTF8Encoding($false)

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Exec {
    $ErrorActionPreference = 'Continue'
    $exe, $rest = $args
    & $exe @rest
    if ($LASTEXITCODE -ne 0) { throw "$($args -join ' ') failed ($LASTEXITCODE)" }
}

$catalogPath = Join-Path $Dist 'catalog.json'
if (-not (Test-Path $catalogPath)) { throw "$catalogPath not found. Run packaging\addons\build.ps1 first." }
$fresh = @(([IO.File]::ReadAllText($catalogPath) | ConvertFrom-Json).addons)
if (-not $fresh) { throw 'The new catalog has no add-ons.' }
foreach ($e in $fresh) {
    $file = $e.url.Substring($e.url.LastIndexOf('/') + 1)
    $zip = Join-Path $Dist "packages\$file"
    if (-not (Test-Path $zip)) { throw "$zip is missing." }
    if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne $e.sha256) { throw "$file does not match its checksum in the catalog." }
}

$ErrorActionPreference = 'Continue'
$url = & git -C $root remote get-url $Remote 2>$null
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0 -or -not $url) { $url = $Remote }
Step "Preparing the $Branch branch from $url in $Work"
if (Test-Path $Work) { Remove-Item $Work -Recurse -Force }
$ErrorActionPreference = 'Continue'
$heads = & git ls-remote --heads $url $Branch
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0) { throw "Could not reach $url." }
if ($heads) {
    Exec git clone --quiet --branch $Branch --single-branch $url $Work
}
else {
    Step "No $Branch branch yet, starting an orphan one"
    Exec git init --quiet $Work
    Exec git -C $Work symbolic-ref HEAD "refs/heads/$Branch"
    Exec git -C $Work remote add origin $url
}

$packages = Join-Path $Work 'packages'
New-Item -ItemType Directory -Force $packages | Out-Null
$old = @()
$oldPath = Join-Path $Work 'catalog.json'
if (Test-Path $oldPath) { $old = @(([IO.File]::ReadAllText($oldPath) | ConvertFrom-Json).addons) }

$merged = [ordered]@{}
foreach ($e in $old) { if ($e.id) { $merged[$e.id] = $e } }
foreach ($e in $fresh) {
    $have = $merged[$e.id]
    if ($have -and ([version]$have.version -gt [version]$e.version)) {
        Write-Warning "$($e.id) $($e.version) is older than the published $($have.version), publishing it anyway."
    }
    $merged[$e.id] = $e
    $file = $e.url.Substring($e.url.LastIndexOf('/') + 1)
    Copy-Item (Join-Path $Dist "packages\$file") $packages -Force
}

$keep = @{}
foreach ($e in @($merged.Values) + @($old)) { if ($e.url) { $keep[$e.url.Substring($e.url.LastIndexOf('/') + 1)] = $true } }
Get-ChildItem $packages -File | Where-Object { -not $keep[$_.Name] } | ForEach-Object {
    Step "Removing old package $($_.Name)"
    Remove-Item $_.FullName -Force
}

$entries = @($merged.Values | Sort-Object id)
$json = ConvertTo-Json -InputObject ([pscustomobject]@{ addons = [object[]]$entries }) -Depth 5
[IO.File]::WriteAllText($oldPath, $json + "`n", $utf8)
[IO.File]::WriteAllText((Join-Path $Work 'README.md'), "# SimpleClip add-ons`n`nAdd-on packages for SimpleClip. The app reads catalog.json. Do not edit by hand.`n", $utf8)
[IO.File]::WriteAllText((Join-Path $Work '.gitattributes'), "* -text`n", $utf8)

Exec git -C $Work add -A
& git -C $Work diff --cached --quiet
if ($LASTEXITCODE -eq 0) {
    Step 'Nothing changed, nothing to commit.'
    return
}
$names = ($fresh | ForEach-Object { "$($_.id) $($_.version)" }) -join ', '
Exec git -C $Work -c user.name=xeno -c user.email=zenomoradeii@gmail.com commit --quiet -m "Add-ons: $names"
Step "Committed 'Add-ons: $names' in $Work"

if ($Push) {
    Exec git -C $Work push origin $Branch
    Step "Pushed $Branch to $url"
}
else {
    Write-Host "Not pushed. Check $Work, then run: git -C `"$Work`" push origin $Branch" -ForegroundColor Yellow
}
