# Build SliceCraft for Windows and put the release files in dist/release: the zip, the installer
# when Inno Setup is installed (set ISCC to ISCC.exe), and their checksums. `cargo xtask dist`
# does the work; this script is the entry point the suite's release workflow calls.
#
# Usage: packaging/windows/package.ps1 [-Arch x64|arm64]
param(
    [ValidateSet('x64', 'arm64')]
    [string]$Arch = 'x64'
)
$ErrorActionPreference = 'Stop'
$Root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
Set-Location $Root

# Windows' own tar writes the zip; Git's GNU tar, often first on PATH, would write a plain tar.
$env:PATH = "$env:SystemRoot\System32;$env:PATH"

$hostArch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }
if ($hostArch -ne $Arch) {
    throw "package.ps1 builds for the machine it runs on ($hostArch), not $Arch"
}

cargo xtask dist
if ($LASTEXITCODE -ne 0) { throw "cargo xtask dist failed ($LASTEXITCODE)" }

$dist = Join-Path $Root 'dist\release'
New-Item -ItemType Directory -Force $dist | Out-Null
# target/dist keeps earlier versions' files: take only this version's.
$toml = Get-Content (Join-Path $Root 'Cargo.toml') -Raw
if ($toml -notmatch '(?ms)^\[workspace\.package\][^\[]*?^version\s*=\s*"([^"]+)"') {
    throw 'could not read [workspace.package] version from Cargo.toml'
}
$version = $Matches[1]
$files = Get-ChildItem (Join-Path $Root 'target\dist') -File -Filter "slicecraft-$version-windows-$Arch*"
if (-not $files) { throw "cargo xtask dist wrote no $version windows-$Arch files" }
$files | Copy-Item -Destination $dist

# Try what was packed: the command-line tool from the zip reads the example cube.
$zip = $files | Where-Object Extension -eq '.zip' | Select-Object -First 1
$work = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
Expand-Archive $zip.FullName -DestinationPath $work
$exe = Get-ChildItem $work -Recurse -Filter 'slicecraft-cli.exe' | Select-Object -First 1
$cube = Get-ChildItem $work -Recurse -Filter 'cube.stl' | Select-Object -First 1
& $exe.FullName info $cube.FullName
if ($LASTEXITCODE -ne 0) { throw "slicecraft-cli info failed ($LASTEXITCODE)" }
Remove-Item -Recurse -Force $work

Get-ChildItem $dist
