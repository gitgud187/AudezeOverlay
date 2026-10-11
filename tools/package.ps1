param(
    [string]$BinaryPath,
    [string]$CompilerPath,
    [string]$Target = 'x86_64-pc-windows-msvc'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $match = Select-String -LiteralPath 'Cargo.toml' -Pattern '^version = "([^"]+)"' | Select-Object -First 1
    $version = $match.Matches[0].Groups[1].Value
    if (!$BinaryPath) {
        cargo build --locked --release --target $Target --bin audeze-overlay
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
        $BinaryPath = Join-Path $root "target\$Target\release\audeze-overlay.exe"
    }
    $binary = (Resolve-Path -LiteralPath $BinaryPath).Path
    if ((Get-Item -LiteralPath $binary).VersionInfo.ProductVersion -ne $version) {
        throw 'Executable version does not match Cargo.toml. Rebuild before packaging.'
    }
    if (!$CompilerPath) { $CompilerPath = & (Join-Path $PSScriptRoot 'install-inno.ps1') }
    $compiler = (Resolve-Path -LiteralPath $CompilerPath).Path
    $out = [IO.Path]::GetFullPath((Join-Path $root 'dist\release'))
    $stage = [IO.Path]::GetFullPath((Join-Path $out 'stage'))
    # Only this dedicated staging folder may be cleared, never dist or user assets.
    if ($stage -ne [IO.Path]::GetFullPath((Join-Path $root 'dist\release\stage')) -or
        !$stage.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Staging path is outside the repository.'
    }
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Copy-Item -LiteralPath $binary -Destination (Join-Path $stage 'Audeze Overlay.exe')
    foreach ($file in @('README.md', 'LICENSE', 'THIRD-PARTY-NOTICES.md', 'CONTRIBUTING.md')) {
        Copy-Item -LiteralPath (Join-Path $root $file) -Destination $stage
    }
    Copy-Item -LiteralPath (Join-Path $root 'docs') -Destination (Join-Path $stage 'docs') -Recurse
    & (Join-Path $PSScriptRoot 'dependency-notices.ps1') -OutputPath (Join-Path $stage 'DEPENDENCY-LICENSES.txt') -Target $Target
    $zip = Join-Path $out 'Audeze-Overlay-Portable.zip'
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
    & $compiler /Qp "/DAppVersion=$version" "/DStageDir=$stage" "/DOutputDir=$out" (Join-Path $root 'installer\audeze-overlay.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
    $installer = Join-Path $out 'Audeze-Overlay-Setup.exe'
    $lines = foreach ($file in @($zip, $installer)) {
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $([IO.Path]::GetFileName($file))"
    }
    [IO.File]::WriteAllLines((Join-Path $out 'SHA256SUMS.txt'), $lines, [Text.UTF8Encoding]::new($false))
    Write-Host "Release files: $out"
    Get-Item -LiteralPath $zip,$installer | Select-Object Name,Length
} finally {
    Pop-Location
}
