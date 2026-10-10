# Installs the release compiler into this repository's ignored tools directory.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$compilerDir = Join-Path $root '.tools\inno'
$compiler = Join-Path $compilerDir 'ISCC.exe'
if (Test-Path -LiteralPath $compiler) { return $compiler }
$setup = Join-Path $root '.tools\innosetup-6.7.3.exe'
New-Item -ItemType Directory -Force -Path (Split-Path $setup) | Out-Null
Invoke-WebRequest 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' -OutFile $setup
$expected = '9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732'
if ((Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
    throw 'Inno Setup download did not match its pinned SHA-256 checksum.'
}
$process = Start-Process -FilePath $setup -ArgumentList @(
    '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER',
    ('/DIR="' + $compilerDir + '"')
) -WindowStyle Hidden -PassThru -Wait
if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $compiler)) {
    throw "Installing the release compiler failed ($($process.ExitCode))."
}
$compiler
