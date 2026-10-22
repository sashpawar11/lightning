# Builds a shareable installer: dist\Lightning-Setup-<version>.exe (+ .sha256).
#
#   scripts\release.ps1                    # release the version in Cargo.toml
#   scripts\release.ps1 -Version 0.2.0     # bump the version first, then release
#   scripts\release.ps1 -SkipBench         # skip the performance guard (faster)
#
# Steps: version -> tests -> performance guard -> release build -> installer
# -> checksum. Any failing step stops the release.
param(
    [string]$Version,
    [switch]$SkipBench
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)

function Step($name) { Write-Host "`n==> $name" -ForegroundColor Cyan }
function Check($what) { if ($LASTEXITCODE) { throw "$what failed (exit $LASTEXITCODE)" } }

# --- version ----------------------------------------------------------------
$manifest = 'Cargo.toml'
$text = [IO.File]::ReadAllText((Resolve-Path $manifest))
if ($Version) {
    if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.3" }
    $text = [regex]::Replace($text, '(?m)(^\[workspace\.package\][^\[]*?^version\s*=\s*")[^"]+(")', "`${1}$Version`${2}")
    [IO.File]::WriteAllText((Resolve-Path $manifest), $text)   # UTF-8, no BOM
}
$m = [regex]::Match($text, '(?m)^\[workspace\.package\][^\[]*?^version\s*=\s*"([^"]+)"')
if (-not $m.Success) { throw "workspace.package version not found in $manifest" }
$Version = $m.Groups[1].Value
Write-Host "Releasing Lightning $Version" -ForegroundColor Green

# --- checks -----------------------------------------------------------------
Step "Tests"
cargo test --workspace --quiet; Check "tests"

if (-not $SkipBench) {
    Step "Performance guard"
    & "$PSScriptRoot\bench.ps1"; Check "performance guard"
}

# --- build ------------------------------------------------------------------
Step "Release build"
cargo build --release -p lightning; Check "release build"
$exe = Resolve-Path 'target\release\lightning.exe'
$ico = Get-ChildItem 'target\release\build\lightning-*\out\lightning.ico' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $ico) { throw "lightning.ico not produced by build.rs" }

# --- installer --------------------------------------------------------------
Step "Installer"
$iscc = @(
    (Get-Command iscc -ErrorAction SilentlyContinue).Source,
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    Write-Host "Inno Setup not found; installing it for this user via winget..."
    winget install --id JRSoftware.InnoSetup -e --scope user --silent --accept-package-agreements --accept-source-agreements
    $iscc = "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    if (-not (Test-Path $iscc)) { throw "Inno Setup install failed; get it from https://jrsoftware.org/isinfo.php" }
}
& $iscc /Q "/DAppVersion=$Version" "/DSourceExe=$exe" "/DIconFile=$($ico.FullName)" 'installer\lightning.iss'
Check "Inno Setup"

$setup = Resolve-Path "dist\Lightning-Setup-$Version.exe"
$hash = (Get-FileHash $setup -Algorithm SHA256).Hash.ToLower()
"$hash  Lightning-Setup-$Version.exe" | Set-Content -Encoding ascii "$setup.sha256"

# In-app updates read this from the latest GitHub release (see update.rs).
$url = "https://github.com/sashpawar11/lightning/releases/download/v$Version/Lightning-Setup-$Version.exe"
@{ version = $Version; url = $url } | ConvertTo-Json -Compress | Set-Content -Encoding ascii 'dist\latest.json'

Step "Done"
$size = (Get-Item $setup).Length / 1MB
Write-Host ("{0}  ({1:N1} MB)" -f $setup, $size) -ForegroundColor Green
Write-Host "SHA-256 $hash"
Write-Host "Upload with the installer so installed copies update:"
Write-Host "  gh release create v$Version $setup $setup.sha256 dist\latest.json"
