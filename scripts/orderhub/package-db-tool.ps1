param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,
    [ValidateSet('dev', 'release', 'nextest')]
    [string]$Profile = 'release'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$destination = [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $OutputDirectory))
if (Test-Path -LiteralPath $destination) {
    throw "Output path already exists: $destination"
}

cargo build --locked --manifest-path (Join-Path $repositoryRoot 'Cargo.toml') `
    --package orderhub-tools --bin orderhub-db --profile $Profile
if ($LASTEXITCODE -ne 0) {
    throw "Failed to build orderhub-db (exit $LASTEXITCODE)"
}

$targetRoot = if ($env:CARGO_TARGET_DIR) {
    [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $env:CARGO_TARGET_DIR))
} else {
    Join-Path $repositoryRoot 'target'
}
$profileDirectory = if ($Profile -eq 'dev') { 'debug' } else { $Profile }
$sourceBinary = Join-Path (Join-Path $targetRoot $profileDirectory) 'orderhub-db.exe'
$destinationBinary = Join-Path $destination 'bin/orderhub-db.exe'
$destinationSchema = Join-Path $destination 'schema/sql'

New-Item -ItemType Directory -Path (Split-Path $destinationBinary), $destinationSchema | Out-Null
Copy-Item -LiteralPath $sourceBinary -Destination $destinationBinary
foreach ($name in @('types.sql', 'tables.sql', 'functions.sql', 'partitions.sql')) {
    Copy-Item -LiteralPath (Join-Path $repositoryRoot "schema/sql/$name") `
        -Destination (Join-Path $destinationSchema $name)
}

& $destinationBinary --version
if ($LASTEXITCODE -ne 0) {
    throw "Packaged orderhub-db --version failed (exit $LASTEXITCODE)"
}
& $destinationBinary schema-check --schema $destinationSchema
if ($LASTEXITCODE -ne 0) {
    throw "Packaged orderhub-db schema-check failed (exit $LASTEXITCODE)"
}
