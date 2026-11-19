param(
  [string] $Worker = (Join-Path $PSScriptRoot '../../target/release/shipforge-worker.exe')
)

$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $Worker).Path
if ([IO.Path]::GetFileName($source) -ne 'shipforge-worker.exe') {
  throw 'Select a ShipForge work machine executable.'
}
$version = & $source --version
if ($LASTEXITCODE -or $version -notmatch '^shipforge-worker [0-9]+\.[0-9]+\.[0-9]+') {
  throw 'The selected executable did not identify itself as the ShipForge work machine.'
}
$build = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../build/worker'))
$null = New-Item -ItemType Directory -Force -Path $build
Copy-Item -LiteralPath $source -Destination (Join-Path $build 'shipforge-worker.exe') -Force
Write-Output $version
