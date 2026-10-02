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
    'build' { & npm.cmd exec tauri -- build --bundles nsis -- --locked }
    'check' { & cargo check --locked --manifest-path src-tauri/Cargo.toml }
    'test' { & cargo test --locked --manifest-path src-tauri/Cargo.toml }
    'fmt' { & cargo fmt --manifest-path src-tauri/Cargo.toml -- --check }
    'lint' { & cargo clippy --locked --manifest-path src-tauri/Cargo.toml --lib --tests -- -D warnings }
}
exit $LASTEXITCODE
