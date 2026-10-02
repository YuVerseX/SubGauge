$ErrorActionPreference = 'Stop'
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
# Explicit files avoid carrying stale staging files into a new release.
Compress-Archive -LiteralPath @((Join-Path $stage 'SubGauge.exe'),$guide) -DestinationPath "$stage.zip" -Force
Copy-Item -LiteralPath $installer -Destination $release
foreach ($artifact in @("$stage.zip",(Join-Path $release ([System.IO.Path]::GetFileName($installer))))) {
    $hashAlgorithm = [System.Security.Cryptography.SHA256]::Create()
    $stream = [System.IO.File]::OpenRead($artifact)
    try {
        $hash = [BitConverter]::ToString($hashAlgorithm.ComputeHash($stream)).Replace('-','').ToLowerInvariant()
        [System.IO.File]::WriteAllText("$artifact.sha256", "$hash  $([System.IO.Path]::GetFileName($artifact))`n")
        [pscustomobject]@{ Algorithm='SHA256'; Hash=$hash; Path=$artifact }
    } finally {
        $stream.Dispose()
        $hashAlgorithm.Dispose()
    }
}
