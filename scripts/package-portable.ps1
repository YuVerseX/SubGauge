$ErrorActionPreference = 'Stop'
function Get-DistributionFileHash([string]$FilePath) {
    $hashAlgorithm = [System.Security.Cryptography.SHA256]::Create()
    $stream = $null
    try {
        $stream = [System.IO.File]::OpenRead($FilePath)
        return [BitConverter]::ToString($hashAlgorithm.ComputeHash($stream)).Replace('-','').ToLowerInvariant()
    } finally {
        if ($stream) { $stream.Dispose() }
        $hashAlgorithm.Dispose()
    }
}
$repo = Split-Path $PSScriptRoot -Parent
$manifest = Get-Content -Raw -Encoding UTF8 (Join-Path $repo 'package.json') | ConvertFrom-Json
$tauriManifest = Get-Content -Raw -Encoding UTF8 (Join-Path $repo 'src-tauri\tauri.conf.json') | ConvertFrom-Json
$cargoManifest = Get-Content -Raw -Encoding UTF8 (Join-Path $repo 'src-tauri\Cargo.toml')
$cargoVersion = [regex]::Match($cargoManifest,'(?m)^version\s*=\s*"([^"]+)"\s*$').Groups[1].Value
if ($manifest.version -ne $tauriManifest.version -or $manifest.version -ne $cargoVersion) {
    throw 'npm, Cargo, and Tauri application versions must match before packaging.'
}
$binary = Join-Path $repo 'src-tauri\target\release\subgauge.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw 'Build the Release application before packaging.' }
if ((Get-Item -LiteralPath $binary).VersionInfo.ProductVersion -ne $manifest.version) {
    throw 'The Release binary has a different version. Rebuild before packaging.'
}
$installer = Join-Path $repo "src-tauri\target\release\bundle\nsis\SubGauge_$($manifest.version)_x64-setup.exe"
if (-not (Test-Path -LiteralPath $installer)) { throw 'Build the matching NSIS installer before packaging.' }
if ((Get-Item -LiteralPath $installer).VersionInfo.ProductVersion -ne $manifest.version) {
    throw 'The NSIS installer has a different version. Rebuild before packaging.'
}
$release = Join-Path $repo 'release'
New-Item -ItemType Directory -Force -Path $release | Out-Null
$stage = Join-Path $release "SubGauge-$($manifest.version)-windows-x64"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -LiteralPath $binary -Destination (Join-Path $stage 'SubGauge.exe')
$guide = Join-Path $stage 'README.md'
Copy-Item -LiteralPath (Join-Path $repo 'docs\user-guide.md') -Destination $guide
$legal = Join-Path $release 'legal'
$noticeManifest = Get-Content -Raw -Encoding UTF8 (Join-Path $legal 'notice-manifest.json') | ConvertFrom-Json
if ($noticeManifest.version -ne $manifest.version -or $noticeManifest.target -ne 'x86_64-pc-windows-msvc') {
    throw 'Generate matching Windows distribution notices before packaging.'
}
foreach ($inputCheck in @(
    @{Path='package.json';Expected=$noticeManifest.inputs.packageJsonSha256},
    @{Path='package-lock.json';Expected=$noticeManifest.inputs.packageLockSha256},
    @{Path='src-tauri\Cargo.toml';Expected=$noticeManifest.inputs.cargoManifestSha256},
    @{Path='src-tauri\Cargo.lock';Expected=$noticeManifest.inputs.cargoLockSha256},
    @{Path='licenses\overrides.json';Expected=$noticeManifest.inputs.overridesSha256},
    @{Path='scripts\generate-notices.cjs';Expected=$noticeManifest.inputs.generatorSha256}
)) {
    if ((Get-DistributionFileHash (Join-Path $repo $inputCheck.Path)) -ne $inputCheck.Expected) {
        throw 'Distribution notice inputs changed. Rebuild before packaging.'
    }
}
$notice = Join-Path $legal 'THIRD-PARTY-NOTICES.txt'
$standardLibrary = Join-Path $legal 'RUST-STANDARD-LIBRARY-NOTICES.html'
if ((Get-DistributionFileHash $notice) -ne $noticeManifest.noticeSha256 -or
    (Get-DistributionFileHash $standardLibrary) -ne $noticeManifest.standardLibrarySha256) {
    throw 'Distribution declaration checksum mismatch.'
}
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination (Join-Path $stage 'LICENSE')
Copy-Item -LiteralPath $notice -Destination (Join-Path $stage 'THIRD-PARTY-NOTICES.txt')
Copy-Item -LiteralPath $standardLibrary -Destination (Join-Path $stage 'RUST-STANDARD-LIBRARY-NOTICES.html')
# Explicit files avoid carrying stale staging files into a new release.
Compress-Archive -LiteralPath @((Join-Path $stage 'SubGauge.exe'),$guide,(Join-Path $stage 'LICENSE'),(Join-Path $stage 'THIRD-PARTY-NOTICES.txt'),(Join-Path $stage 'RUST-STANDARD-LIBRARY-NOTICES.html')) -DestinationPath "$stage.zip" -Force
Copy-Item -LiteralPath $installer -Destination $release
foreach ($artifact in @("$stage.zip",(Join-Path $release ([System.IO.Path]::GetFileName($installer))))) {
    $hash = Get-DistributionFileHash $artifact
    [System.IO.File]::WriteAllText("$artifact.sha256", "$hash  $([System.IO.Path]::GetFileName($artifact))`n")
    [pscustomobject]@{ Algorithm='SHA256'; Hash=$hash; Path=$artifact }
}
