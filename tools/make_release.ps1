<#
.SYNOPSIS
  Build the Windows release package of Misty Engine: the folder, the zip and its SHA-256 file.

.DESCRIPTION
  One command for a release (see docs/release.md):

    powershell -ExecutionPolicy Bypass -File tools\make_release.ps1

  1. cargo build --release -p misty_editor -p misty_player (skip with -SkipBuild)
  2. <OutDir>\MistyEngine-v<version>-windows-x64\ with MistyEngine.exe (renamed misty.exe),
     misty_player.exe, Examples\SampleProject (from git HEAD, so uncommitted edits are left out;
     no Library\ or Builds\), README.txt, CHANGELOG.txt and LICENSE.txt
  3. <OutDir>\MistyEngine-v<version>-windows-x64.zip (the folder inside, deflate) and
     <zip>.sha256.txt ("<hash>  <zip name>", what the updater checks)

  The version is the workspace version in Cargo.toml.

.PARAMETER OutDir
  Where the package goes. Default: a MistyRelease folder next to the repository.

.PARAMETER TargetDir
  Cargo target folder (use another one if target\release\misty.exe is in use). Default: target.

.PARAMETER SkipBuild
  Package the exes already in <TargetDir>\release.
#>
param(
    [string]$OutDir,
    [string]$TargetDir,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$Repo = Split-Path -Parent $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path (Split-Path -Parent $Repo) 'MistyRelease' }
if (-not $TargetDir) { $TargetDir = Join-Path $Repo 'target' }

$toml = [IO.File]::ReadAllText((Join-Path $Repo 'Cargo.toml'))
if ($toml -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'No workspace version in Cargo.toml.' }
$Version = $Matches[1]
$Name = "MistyEngine-v$Version-windows-x64"
Write-Host "Misty Engine $Version -> $OutDir\$Name"

$dirty = & git -C $Repo status --porcelain -- crates Cargo.toml Cargo.lock
if ($dirty) { Write-Warning "Uncommitted changes in the engine source; the exes include them:`n$dirty" }

if (-not $SkipBuild) {
    $env:CARGO_INCREMENTAL = '0'
    & cargo build --release -p misty_editor -p misty_player `
        --manifest-path (Join-Path $Repo 'Cargo.toml') --target-dir $TargetDir
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
}
$Bin = Join-Path $TargetDir 'release'
foreach ($exe in 'misty.exe', 'misty_player.exe') {
    if (-not (Test-Path (Join-Path $Bin $exe))) { throw "$exe not found in $Bin." }
}

# --- the folder ---------------------------------------------------------------------------
New-Item -ItemType Directory -Force $OutDir | Out-Null
$Pkg = Join-Path $OutDir $Name
if (Test-Path $Pkg) { Remove-Item -Recurse -Force $Pkg }
New-Item -ItemType Directory -Force $Pkg | Out-Null
Copy-Item (Join-Path $Bin 'misty.exe') (Join-Path $Pkg 'MistyEngine.exe')
Copy-Item (Join-Path $Bin 'misty_player.exe') (Join-Path $Pkg 'misty_player.exe')

# The example project exactly as committed.
$Examples = Join-Path $Pkg 'Examples'
New-Item -ItemType Directory -Force $Examples | Out-Null
$tarFile = Join-Path $OutDir "$Name-examples.tar"
& git -C $Repo archive --format=tar -o $tarFile HEAD SampleProject
if ($LASTEXITCODE -ne 0) { throw 'git archive failed.' }
& tar -x -f $tarFile -C $Examples
if ($LASTEXITCODE -ne 0) { throw 'tar -x failed.' }
Remove-Item $tarFile
foreach ($skip in 'Library', 'Builds') {
    $p = Join-Path $Examples "SampleProject\$skip"
    if (Test-Path $p) { Remove-Item -Recurse -Force $p }
}

# Text files with Windows line endings (UTF-8, no BOM).
function Write-Text([string]$Path, [string]$Text) {
    $crlf = ($Text -replace "`r`n", "`n") -replace "`n", "`r`n"
    [IO.File]::WriteAllText($Path, $crlf, (New-Object Text.UTF8Encoding $false))
}
$read = { param($rel) [IO.File]::ReadAllText((Join-Path $Repo $rel)) }
Write-Text (Join-Path $Pkg 'README.txt') ((& $read 'tools\release\README.txt').Replace('{VERSION}', $Version))
Write-Text (Join-Path $Pkg 'CHANGELOG.txt') (& $read 'tools\release\CHANGELOG.txt')
$license = (& $read 'LICENSE') + "`n`n" + ('=' * 77) + "`n`n" + (& $read 'RUNTIME-EXCEPTION.md')
Write-Text (Join-Path $Pkg 'LICENSE.txt') $license

# --- the zip and its checksum ------------------------------------------------------------
$Zip = Join-Path $OutDir "$Name.zip"
if (Test-Path $Zip) { Remove-Item -Force $Zip }
# bsdtar (Windows 10+) writes a deflate zip with forward slashes, which every unzip reads.
& tar -a -c -f $Zip -C $OutDir $Name
if ($LASTEXITCODE -ne 0) { throw 'tar -a (zip) failed.' }
$hash = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
$sumFile = "$Zip.sha256.txt"
[IO.File]::WriteAllText($sumFile, "$hash  $Name.zip`n", (New-Object Text.ASCIIEncoding))

$size = (Get-Item $Zip).Length
Write-Host ("{0}.zip  {1:N1} MB  sha256 {2}" -f $Name, ($size / 1MB), $hash)
