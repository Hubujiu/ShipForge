[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $workspaceRoot 'dist' }
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)

function Remove-PackageTree([string]$Path) {
    $target = [System.IO.Path]::GetFullPath($Path)
    $parent = [System.IO.Path]::GetDirectoryName($target)
    if (-not $parent.TrimEnd([char[]]@('\', '/')).Equals($OutputDirectory.TrimEnd([char[]]@('\', '/')), [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove a path outside the package output directory: $target"
    }
    if (Test-Path -LiteralPath $target) {
        $item = Get-Item -LiteralPath $target -Force
        if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw "Refusing to replace a linked package directory: $target"
        }
        Remove-Item -LiteralPath $target -Recurse -Force
    }
}

$stagingDirectory = $null
Push-Location -LiteralPath $workspaceRoot
try {
    if (-not $SkipBuild) {
        & cargo build --workspace --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    }
    $metadataText = & cargo metadata --no-deps --format-version 1 --locked
    if ($LASTEXITCODE -ne 0) { throw 'Could not read Cargo package metadata.' }
    $metadata = $metadataText | ConvertFrom-Json
    $version = ($metadata.packages | Where-Object name -eq 'shipforge-cli').version
    $desktopVersion = ($metadata.packages | Where-Object name -eq 'shipforge-desktop').version
    if ($desktopVersion -ne $version) { throw 'CLI and desktop versions must match.' }
    $hostLine = & rustc -vV | Where-Object { $_ -like 'host: *' }
    $hostTriple = $hostLine.Substring(6)
    if ($hostTriple -ne 'x86_64-pc-windows-msvc') { throw "This packaging script currently targets Windows x64 MSVC; found $hostTriple" }
    $packageName = "shipforge-$version-windows-x64"
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    $stagingDirectory = Join-Path $OutputDirectory ".shipforge-stage-$([guid]::NewGuid().ToString('N'))"
    $packageDirectory = Join-Path $stagingDirectory $packageName
    New-Item -ItemType Directory -Path $packageDirectory -Force | Out-Null
    foreach ($executable in @('shipforge.exe', 'shipforge-desktop.exe', 'shipforge-server.exe', 'shipforge-worker.exe')) {
        Copy-Item -LiteralPath (Join-Path $metadata.target_directory "release\$executable") -Destination (Join-Path $packageDirectory $executable)
    }
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'README.md') -Destination (Join-Path $packageDirectory 'README.md') -Force
    foreach ($folder in @('docs', 'skills')) {
        $destination = Join-Path $packageDirectory $folder
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        Get-ChildItem -LiteralPath (Join-Path $workspaceRoot $folder) | Copy-Item -Destination $destination -Recurse -Force
    }
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'Cargo.lock') -Destination (Join-Path $packageDirectory 'Cargo.lock') -Force
    foreach ($relative in @('crates/shipforge-worker/README.md', 'crates/shipforge-core/PROVIDERS.md', 'crates/shipforge-core/tests/fixtures/inbound.json')) {
        $destination = Join-Path $packageDirectory $relative
        New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $workspaceRoot $relative) -Destination $destination
    }
    $licenseDirectory = Join-Path $packageDirectory 'licenses'
    New-Item -ItemType Directory -Path $licenseDirectory -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'crates\shipforge-desktop\assets\LUCIDE-LICENSE.txt') -Destination $licenseDirectory
    Copy-Item -LiteralPath (Join-Path $workspaceRoot 'LICENSE') -Destination (Join-Path $packageDirectory 'LICENSE')
    $checksums = foreach ($executable in @('shipforge.exe', 'shipforge-desktop.exe', 'shipforge-server.exe', 'shipforge-worker.exe')) {
        $hash = (Get-FileHash -LiteralPath (Join-Path $packageDirectory $executable) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $executable`n"
    }
    [System.IO.File]::WriteAllText((Join-Path $packageDirectory 'SHA256SUMS.txt'), ($checksums -join ''), [System.Text.UTF8Encoding]::new($false))
    $archivePath = Join-Path $OutputDirectory "$packageName.zip"
    $stagedArchive = Join-Path $stagingDirectory "$packageName.zip"
    Compress-Archive -LiteralPath $packageDirectory -DestinationPath $stagedArchive -CompressionLevel Optimal
    $publishedDirectory = Join-Path $OutputDirectory $packageName
    Remove-PackageTree $publishedDirectory
    Move-Item -LiteralPath $packageDirectory -Destination $publishedDirectory
    Move-Item -LiteralPath $stagedArchive -Destination $archivePath -Force
    $archiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$archivePath.sha256", "$archiveHash  $packageName.zip`n", [System.Text.UTF8Encoding]::new($false))
    $compiler = & (Join-Path $PSScriptRoot 'ensure-inno.ps1')
    & $compiler '/Qp' "/DAppVersion=$version" "/DPackageDir=$publishedDirectory" "/DOutputDir=$OutputDirectory" (Join-Path $workspaceRoot 'packaging\windows\shipforge.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Windows installer build failed.' }
    $installer = Join-Path $OutputDirectory "$packageName-setup.exe"
    $installerHash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$installer.sha256", "$installerHash  $packageName-setup.exe`n", [System.Text.UTF8Encoding]::new($false))
    [pscustomobject]@{ Executable = Join-Path $publishedDirectory 'shipforge.exe'; Desktop = Join-Path $publishedDirectory 'shipforge-desktop.exe'; Archive = $archivePath; SHA256 = $archiveHash }
}
finally {
    if ($stagingDirectory) { Remove-PackageTree $stagingDirectory }
    Pop-Location
}
