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
  4. <OutDir>\MistyEngine-Setup-v<version>-windows-x64.exe, the installer, built with Inno Setup 6
     from tools\installer\MistyEngine.iss (skipped with a warning if Inno Setup is not
     installed, or with -NoInstaller), and <setup>.sha256.txt

  The version is the workspace version in Cargo.toml.

  Published releases are built, signed and uploaded by GitHub Actions
  (.github\workflows\release-windows.yml), which runs this script with -Strict, with -ExeDir
  once the exes are signed, and with -HashOnly after the installer is signed. Run by hand it
  makes an unsigned test build.

.PARAMETER OutDir
  Where the package goes. Default: a MistyRelease folder next to the repository.

.PARAMETER TargetDir
  Cargo target folder (use another one if target\release\misty.exe is in use). Default: target.

.PARAMETER SkipBuild
  Package the exes already in <TargetDir>\release.

.PARAMETER ExeDir
  Package MistyEngine.exe and misty_player.exe from this folder instead of building (the
  release workflow passes the folder of signed exes). Nothing is built or renamed.

.PARAMETER NoInstaller
  Leave the installer out.

.PARAMETER Strict
  For the release workflow: stop with an error where a test build only warns (Inno Setup
  missing; an exe without the "Misty Engine" product name and version in its version info,
  which the signing service would refuse).

.PARAMETER HashOnly
  Only write the .sha256.txt files again for the zip and the installer already in <OutDir>
  (the release workflow does this last, after the installer is signed).
#>
param(
    [string]$OutDir,
    [string]$TargetDir,
    [switch]$SkipBuild,
    [string]$ExeDir,
    [switch]$NoInstaller,
    [switch]$Strict,
    [switch]$HashOnly
)

$ErrorActionPreference = 'Stop'
$Repo = Split-Path -Parent $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path (Split-Path -Parent $Repo) 'MistyRelease' }
if (-not $TargetDir) { $TargetDir = Join-Path $Repo 'target' }
# Full paths: Inno Setup reads relative ones from the .iss folder, not from here.
$full = { param($p) $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($p) }
$OutDir = & $full $OutDir
$TargetDir = & $full $TargetDir
if ($ExeDir) { $ExeDir = & $full $ExeDir }

$toml = [IO.File]::ReadAllText((Join-Path $Repo 'Cargo.toml'))
if ($toml -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'No workspace version in Cargo.toml.' }
$Version = $Matches[1]
$Name = "MistyEngine-v$Version-windows-x64"
$SetupName = "MistyEngine-Setup-v$Version-windows-x64.exe"
Write-Host "Misty Engine $Version -> $OutDir\$Name"

# "<hash>  <file name>" next to the file (what the updater checks for the zip).
function Write-Sha256([string]$File) {
    $hash = (Get-FileHash $File -Algorithm SHA256).Hash.ToLower()
    $leaf = Split-Path -Leaf $File
    [IO.File]::WriteAllText("$File.sha256.txt", "$hash  $leaf`n", (New-Object Text.ASCIIEncoding))
    Write-Host ("{0}  {1:N1} MB  sha256 {2}" -f $leaf, ((Get-Item $File).Length / 1MB), $hash)
}

if ($HashOnly) {
    $zipFile = Join-Path $OutDir "$Name.zip"
    if (-not (Test-Path $zipFile)) { throw "$zipFile not found." }
    Write-Sha256 $zipFile
    $setupFile = Join-Path $OutDir $SetupName
    if (Test-Path $setupFile) { Write-Sha256 $setupFile }
    elseif ($Strict -and -not $NoInstaller) { throw "$setupFile not found." }
    return
}

# bsdtar from Windows itself (10 and newer): it writes zips. A GNU tar earlier in PATH (Git for
# Windows has one) would write a tar file named .zip.
$Tar = Join-Path $env:SystemRoot 'System32\tar.exe'
if (-not (Test-Path $Tar)) { $Tar = 'tar' }

$dirty = & git -C $Repo status --porcelain -- crates Cargo.toml Cargo.lock
if ($dirty) { Write-Warning "Uncommitted changes in the engine source; the exes include them:`n$dirty" }

if ($ExeDir) {
    # Ready exes under their release names (signed ones in the release workflow).
    $EditorExe = Join-Path $ExeDir 'MistyEngine.exe'
    $PlayerExe = Join-Path $ExeDir 'misty_player.exe'
} else {
    if (-not $SkipBuild) {
        $env:CARGO_INCREMENTAL = '0'
        & cargo build --release -p misty_editor -p misty_player `
            --manifest-path (Join-Path $Repo 'Cargo.toml') --target-dir $TargetDir
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
    }
    $Bin = Join-Path $TargetDir 'release'
    $EditorExe = Join-Path $Bin 'misty.exe'
    $PlayerExe = Join-Path $Bin 'misty_player.exe'
}
foreach ($exe in $EditorExe, $PlayerExe) {
    if (-not (Test-Path $exe)) { throw "$exe not found." }
    # The version info comes from crates\misty_editor\build_icon.rs (needs the Windows SDK's
    # rc.exe). The signing service refuses exes whose product name or version differ.
    $info = (Get-Item $exe).VersionInfo
    if ($info.ProductName -ne 'Misty Engine' -or $info.ProductVersion -ne $Version) {
        $msg = "$exe has product name '$($info.ProductName)' and version '$($info.ProductVersion)'; " +
            "expected 'Misty Engine' and '$Version'."
        if ($Strict) { throw $msg } else { Write-Warning $msg }
    }
}

# --- the folder ---------------------------------------------------------------------------
New-Item -ItemType Directory -Force $OutDir | Out-Null
$Pkg = Join-Path $OutDir $Name
if (Test-Path $Pkg) { Remove-Item -Recurse -Force $Pkg }
New-Item -ItemType Directory -Force $Pkg | Out-Null
Copy-Item $EditorExe (Join-Path $Pkg 'MistyEngine.exe')
Copy-Item $PlayerExe (Join-Path $Pkg 'misty_player.exe')

# The example project exactly as committed.
$Examples = Join-Path $Pkg 'Examples'
New-Item -ItemType Directory -Force $Examples | Out-Null
$tarFile = Join-Path $OutDir "$Name-examples.tar"
& git -C $Repo archive --format=tar -o $tarFile HEAD SampleProject
if ($LASTEXITCODE -ne 0) { throw 'git archive failed.' }
& $Tar -x -f $tarFile -C $Examples
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
& $Tar -a -c -f $Zip -C $OutDir $Name
if ($LASTEXITCODE -ne 0) { throw 'tar -a (zip) failed.' }
$magic = New-Object byte[] 2
$zipStream = [IO.File]::OpenRead($Zip)
try { [void]$zipStream.Read($magic, 0, 2) } finally { $zipStream.Dispose() }
if ($magic[0] -ne 0x50 -or $magic[1] -ne 0x4B) { throw "$Zip is not a zip file (wrong tar in use?)." }
Write-Sha256 $Zip

# --- the installer (Inno Setup 6) --------------------------------------------------------
if (-not $NoInstaller) {
    $iscc = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $iscc) {
        $onPath = Get-Command ISCC.exe -ErrorAction SilentlyContinue
        if ($onPath) { $iscc = $onPath.Source }
    }
    if (-not $iscc) {
        $msg = 'Inno Setup 6 not found (https://jrsoftware.org/isinfo.php).'
        if ($Strict) { throw $msg }
        Write-Warning "$msg Skipping the installer."
    } else {
        $iss = Join-Path $Repo 'tools\installer\MistyEngine.iss'
        $ico = Join-Path $Repo 'crates\misty_editor\assets\branding\misty.ico'
        # The installer's own version info wants four numbers (0.3.2 -> 0.3.2.0).
        $numeric = ((($Version -split '[-+]')[0] -split '\.') + '0', '0', '0', '0')[0..3] -join '.'
        & $iscc /Q "/DVersion=$Version" "/DNumericVersion=$numeric" "/DPkgDir=$Pkg" "/DIconFile=$ico" `
            "/O$OutDir" $iss
        if ($LASTEXITCODE -ne 0) { throw 'Inno Setup (ISCC) failed.' }
        Write-Sha256 (Join-Path $OutDir $SetupName)
    }
}
