param([Parameter(Mandatory)][string]$OutputPath, [string]$Target = 'x86_64-pc-windows-msvc')
$ErrorActionPreference = 'Stop'
$metadataJson = cargo metadata --locked --format-version 1 --filter-platform $Target
if ($LASTEXITCODE -ne 0) { throw 'Cannot read dependency metadata.' }
$metadata = $metadataJson | ConvertFrom-Json
$resolved = @($metadata.resolve.nodes | ForEach-Object id)
$text = [Text.StringBuilder]::new()
[void]$text.AppendLine('Audeze Overlay — dependency copyright and license notices')
[void]$text.AppendLine('This list also includes build-time dependencies for completeness.')
foreach ($package in ($metadata.packages | Where-Object { $_.source -and $_.id -in $resolved } | Sort-Object name,version)) {
    $directory = Split-Path $package.manifest_path
    $files = @(Get-ChildItem -LiteralPath $directory -File | Where-Object Name -Match '^(LICEN[CS]E|COPYING|NOTICE)([._-].*)?$')
    if ($package.license_file) {
        $files += Get-Item -LiteralPath (Join-Path $directory $package.license_file)
    }
    if (!$files.Count) { throw "No license text found for $($package.name) $($package.version)." }
    [void]$text.AppendLine("`n================================================================")
    [void]$text.AppendLine("$($package.name) $($package.version) — $($package.license)")
    [void]$text.AppendLine("https://crates.io/crates/$($package.name)/$($package.version)")
    foreach ($file in ($files | Sort-Object FullName -Unique)) {
        [void]$text.AppendLine("`n--- $($file.Name) ---")
        [void]$text.AppendLine([IO.File]::ReadAllText($file.FullName))
    }
}
[IO.File]::WriteAllText($OutputPath, $text.ToString(), [Text.UTF8Encoding]::new($false))
