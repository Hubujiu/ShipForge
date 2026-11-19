param([string] $Dist = (Join-Path $PSScriptRoot '../dist'))

$ErrorActionPreference = 'Stop'
$version = (Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../package.json') | ConvertFrom-Json).version
$base = (Resolve-Path -LiteralPath $Dist).Path
$name = "ShipForge-$version-windows-x64"
$zipPath = Join-Path $base "$name.zip"
$installer = Join-Path $base "$name-setup.exe"
foreach ($file in @($zipPath, $installer)) {
  if (!(Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).Length -lt 20MB) {
    throw "Incomplete Windows release file $file"
  }
}

$temp = [IO.Path]::TrimEndingDirectorySeparator((Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path)
$check = Join-Path $temp ('shipforge-electron-zip-' + [guid]::NewGuid())
$null = New-Item -ItemType Directory -Path $check
try {
  Expand-Archive -LiteralPath $zipPath -DestinationPath $check
  $exe = Get-ChildItem -LiteralPath $check -Recurse -File -Filter 'ShipForge.exe' | Select-Object -First 1
  if (!$exe) { throw 'The portable app is missing' }
  $worker = Join-Path $exe.Directory.FullName 'resources/worker/shipforge-worker.exe'
  $asar = Join-Path $exe.Directory.FullName 'resources/app.asar'
  if (!(Test-Path -LiteralPath $asar -PathType Leaf)) { throw 'The Electron application archive is missing' }
  $banner = & $worker --version
  if ($LASTEXITCODE -or $banner -notmatch '^shipforge-worker [0-9]+\.[0-9]+\.[0-9]+') {
    throw 'The independent work machine failed to identify itself'
  }
  foreach ($file in @($zipPath, $installer)) {
    $sum = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    $shortName = [IO.Path]::GetFileName($file)
    [IO.File]::WriteAllText("$file.sha256", "$sum  $shortName" + [char]10)
    Write-Output "$shortName $sum"
  }
  Write-Output "Verified the Windows ZIP, application archive and $banner"
} finally {
  $verified = (Resolve-Path -LiteralPath $check).Path
  if ((Split-Path -Parent $verified) -cne $temp -or !(Split-Path -Leaf $verified).StartsWith('shipforge-electron-zip-')) {
    throw 'Unexpected package verification directory'
  }
  Remove-Item -LiteralPath $verified -Recurse -Force
}
