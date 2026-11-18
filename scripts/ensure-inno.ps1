[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
# Pin the compiler and its official release digest, independently of PATH/winget.
$version = '6.7.4'
$sha256 = 'a549929319f9120716677c685535ca4671e4b937b5fdc87e2585f32d8eb30e8c'
$toolRoot = Join-Path ([System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))) "target\tools\inno-$version"
$compiler = Join-Path $toolRoot 'ISCC.exe'
if (-not (Test-Path -LiteralPath $compiler)) {
    New-Item -ItemType Directory -Path $toolRoot -Force | Out-Null
    $download = Join-Path $toolRoot "innosetup-$version.exe"
    Invoke-WebRequest -Uri "https://github.com/jrsoftware/issrc/releases/download/is-6_7_4/innosetup-$version.exe" -OutFile $download
    if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sha256) { throw 'Inno Setup compiler SHA-256 mismatch.' }
    $setup = Start-Process -FilePath $download -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/CURRENTUSER', '/NOICONS', "/DIR=`"$toolRoot`"") -WindowStyle Hidden -Wait -PassThru
    if ($setup.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $compiler)) { throw 'Could not install the pinned Inno Setup compiler.' }
}
$translation = Join-Path $toolRoot 'Languages\ChineseSimplified.isl'
$translationHash = '7d544b9bb1d142cfa11f2e5d3cc8abe2e55f8e066c5124e3772675aa236e1278'
if (-not (Test-Path -LiteralPath $translation)) {
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/jrsoftware/issrc/is-6_7_4/Files/Languages/Unofficial/ChineseSimplified.isl' -OutFile $translation
}
if ((Get-FileHash -LiteralPath $translation -Algorithm SHA256).Hash.ToLowerInvariant() -ne $translationHash) {
    throw 'The pinned Inno Setup Chinese translation checksum does not match.'
}
Write-Output $compiler
