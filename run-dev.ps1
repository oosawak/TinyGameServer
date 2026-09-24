$ErrorActionPreference = "Stop"
$env:RUST_LOG = "info"

cargo build --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Starting supervisor..."
Start-Process powershell -ArgumentList "-NoExit", "-Command", '$env:RUST_LOG="info"; cargo run -p gameforge-supervisor'
Start-Sleep -Seconds 1

Write-Host "Starting gateway..."
cargo run -p gameforge-gateway
