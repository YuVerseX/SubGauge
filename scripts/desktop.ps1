param([ValidateSet('dev','build','check','test','fmt','lint')][string]$Action = 'dev')
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $repo
$cargoDirectory = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path -LiteralPath (Join-Path $cargoDirectory 'cargo.exe')) {
    $env:PATH = "$cargoDirectory;$env:PATH"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Rust MSVC toolchain is required. See docs/development.md.'
}
switch ($Action) {
    'dev' { & npm.cmd exec tauri -- dev }
    'build' {
        $previousKey = [Environment]::GetEnvironmentVariable('TAURI_SIGNING_PRIVATE_KEY', 'Process')
        $previousPassword = [Environment]::GetEnvironmentVariable('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'Process')
        $passwordBytes = $null
        $buildExitCode = 1
        try {
            if ([string]::IsNullOrWhiteSpace($previousKey)) {
                if (-not [string]::IsNullOrWhiteSpace($env:SUBGAUGE_SIGNING_DIRECTORY)) {
                    $signingDirectory = [System.IO.Path]::GetFullPath($env:SUBGAUGE_SIGNING_DIRECTORY)
                } elseif ($env:CI -eq 'true') {
                    throw 'CI must configure explicit signing credentials or its isolated validation-only signing directory. Production signing fallback is disabled in CI.'
                } else {
                    $signingDirectory = Join-Path $env:USERPROFILE '.subgauge\signing'
                }
                $privateKeyFile = Join-Path $signingDirectory 'updater.key'
                $protectedPasswordFile = Join-Path $signingDirectory 'password.dpapi'
                $localPublicKeyFile = "$privateKeyFile.pub"
                foreach ($signingFile in @($privateKeyFile, $protectedPasswordFile, $localPublicKeyFile)) {
                    if (-not (Test-Path -LiteralPath $signingFile -PathType Leaf)) {
                        throw 'Updater signing setup is missing. Configure explicit TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD, or complete the local signing setup. Unsigned updater packages are not allowed.'
                    }
                }
                $configuredKey = (Get-Content -LiteralPath (Join-Path $repo 'src-tauri\updater-public.key') -Raw -Encoding UTF8).Trim()
                $localPublicKey = (Get-Content -LiteralPath $localPublicKeyFile -Raw -Encoding UTF8).Trim()
                if ($configuredKey -ne $localPublicKey) {
                    throw 'Local updater signing key differs from the application public key. Preserve the existing keys; do not rotate them implicitly.'
                }
                Add-Type -AssemblyName System.Security
                try {
                    $passwordBytes = [System.Security.Cryptography.ProtectedData]::Unprotect(
                        [System.IO.File]::ReadAllBytes($protectedPasswordFile),
                        $null,
                        [System.Security.Cryptography.DataProtectionScope]::CurrentUser
                    )
                    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [System.Text.Encoding]::UTF8.GetString($passwordBytes)
                } catch {
                    throw 'Local updater signing password could not be decrypted for this Windows user. Preserve the recovery files and configure explicit signing credentials.'
                }
                # The CLI reads the encrypted private key itself; never print its content.
                $env:TAURI_SIGNING_PRIVATE_KEY = $privateKeyFile
            }
            # CI mode also prevents an encrypted explicit key from prompting for a password.
            & npm.cmd exec tauri -- build --ci --bundles nsis -- --locked
            $buildExitCode = $LASTEXITCODE
        } finally {
            [Environment]::SetEnvironmentVariable('TAURI_SIGNING_PRIVATE_KEY', $previousKey, 'Process')
            [Environment]::SetEnvironmentVariable('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', $previousPassword, 'Process')
            if ($passwordBytes) { [Array]::Clear($passwordBytes, 0, $passwordBytes.Length) }
            $passwordBytes = $null
        }
        if ($buildExitCode -eq 0) {
            $buildManifest = Get-Content -LiteralPath (Join-Path $repo 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json
            $builtInstaller = Join-Path $repo "src-tauri\target\release\bundle\nsis\SubGauge_$($buildManifest.version)_x64-setup.exe"
            & node (Join-Path $PSScriptRoot 'prepare-update-manifest.cjs') --installer $builtInstaller --verify-only
            $buildExitCode = $LASTEXITCODE
        }
        exit $buildExitCode
    }
    'check' { & cargo check --locked --manifest-path src-tauri/Cargo.toml }
    'test' { & cargo test --locked --manifest-path src-tauri/Cargo.toml }
    'fmt' { & cargo fmt --manifest-path src-tauri/Cargo.toml -- --check }
    'lint' { & cargo clippy --locked --manifest-path src-tauri/Cargo.toml --lib --tests -- -D warnings }
}
exit $LASTEXITCODE
