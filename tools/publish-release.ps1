param(
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$CommitSha,
    [Parameter(Mandatory)][ValidateSet('branch', 'tag')][string]$RefType,
    [Parameter(Mandatory)][string]$RefName,
    [Parameter(Mandatory)][string]$BuildNumber,
    [string]$ArtifactDirectory = 'release'
)

$ErrorActionPreference = 'Stop'

function Invoke-GitHub {
    param([Parameter(ValueFromRemainingArguments)][string[]]$Arguments)
    $output = & gh @Arguments
    if ($LASTEXITCODE -ne 0) { throw "GitHub command failed: $($Arguments[0..1] -join ' ')" }
    return $output
}

function Test-CurrentMain {
    $head = Invoke-GitHub api "repos/$Repository/git/ref/heads/main" | ConvertFrom-Json
    return $head.object.sha -eq $CommitSha
}

function Assert-ReleaseAssets {
    param($Release)
    $actual = @($Release.assets | ForEach-Object name)
    if ($actual.Count -ne $assetNames.Count -or @(Compare-Object $assetNames $actual).Count) {
        throw 'Release must contain exactly the installer, portable ZIP and checksums'
    }
    if (@($Release.assets | Where-Object { $_.size -le 0 }).Count) {
        throw 'Release contains an empty download'
    }
    if (!$Release.body.Contains("Built from commit $CommitSha.") -or !$Release.body.Contains("## What's new")) {
        throw 'Release notes do not identify this build and its changes'
    }
}

$mainBuild = $RefType -eq 'branch'
if ($mainBuild -and $RefName -ne 'main') { throw 'Only main and version tags publish releases' }
if ($mainBuild -and !(Test-CurrentMain)) {
    Write-Host 'A newer main commit exists; skipping this superseded build.'
    return
}
$versionMatch = Select-String -LiteralPath "$PSScriptRoot/../Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1
$version = $versionMatch.Matches[0].Groups[1].Value
if (!$mainBuild -and $RefName -ne "v$version") { throw 'Release tag must match Cargo.toml' }
$tag = if ($mainBuild) { "main-$($CommitSha.Substring(0, 12))" } else { $RefName }
$title = if ($mainBuild) { "Audeze Overlay $version - build $BuildNumber" } else { "Audeze Overlay $version" }
$prerelease = !$mainBuild -and $tag.Contains('-')
$assetNames = @('Audeze-Overlay-Setup.exe', 'Audeze-Overlay-Portable.zip', 'SHA256SUMS.txt')
$files = @($assetNames | ForEach-Object { (Get-Item -LiteralPath (Join-Path $ArtifactDirectory $_)).FullName })

# Read all pages so rerunning an older tag cannot accidentally create a duplicate.
$pages = Invoke-GitHub api --paginate --slurp "repos/$Repository/releases?per_page=100" | ConvertFrom-Json
$releases = @($pages | ForEach-Object { $_ })
$existing = $releases | Where-Object tag_name -eq $tag | Select-Object -First 1
if ($existing -and !$existing.draft) {
    Assert-ReleaseAssets $existing
    $commit = Invoke-GitHub api "repos/$Repository/commits/$tag" | ConvertFrom-Json
    if ($commit.sha -ne $CommitSha) { throw 'Published tag points to a different commit' }
    Write-Host "Release $tag is already published; preserving its downloads and changelog."
    return
}

$previous = $releases | Where-Object { !$_.draft -and !$_.prerelease -and $_.tag_name -ne $tag } |
    Sort-Object published_at -Descending | Select-Object -First 1
if ($mainBuild -and $previous) {
    $baseCommit = Invoke-GitHub api "repos/$Repository/commits/$($previous.tag_name)" | ConvertFrom-Json
    if ($baseCommit.sha -eq $CommitSha) {
        $changes = 'Published this build as a separate release; application code is unchanged.'
    } else {
        $comparisons = Invoke-GitHub api --paginate --slurp "repos/$Repository/compare/$($baseCommit.sha)...${CommitSha}?per_page=100" | ConvertFrom-Json
        if ($comparisons[0].status -ne 'ahead') { throw 'Previous release is not an ancestor of this main build' }
        $changes = (@($comparisons | ForEach-Object commits | ForEach-Object {
            $subject = ($_.commit.message -split "`n")[0]
            "- $subject ([commit]($($_.html_url)))"
        }) -join "`n") + "`n`n[Full changelog]($($comparisons[0].html_url))"
    }
} elseif ($mainBuild) {
    $changes = 'First downloadable main build. See the README for features and setup.'
} else {
    $generated = Invoke-GitHub api --method POST "repos/$Repository/releases/generate-notes" -f "tag_name=$tag" -f "target_commitish=$CommitSha" | ConvertFrom-Json
    $changes = $generated.body
}

$download = "https://github.com/$Repository/releases/download/$tag"
$notesPath = Join-Path $ArtifactDirectory 'release-notes.txt'
@(
    '## Download for Windows 11 (64-bit)', '',
    "**[Install Audeze Overlay — recommended]($download/Audeze-Overlay-Setup.exe)**", '',
    'Run the installer, then open Audeze Overlay from the Start menu.', '',
    "[Portable ZIP — no installation]($download/Audeze-Overlay-Portable.zip): extract the ZIP and run Audeze Overlay.exe.", '',
    'Both downloads include the app and licenses. Source code archives are for developers. SHA256SUMS.txt is optional for verifying downloads.', '',
    "## What's new", '', $changes, '', '### Build details', '',
    "Built from commit $CommitSha. Packages are unsigned."
) | Set-Content -LiteralPath $notesPath -Encoding utf8

# Keep partial uploads private. A retry may repair a draft but never replace a public release.
if (!$existing) {
    $options = @('--draft', '--title', $title, '--notes-file', $notesPath, '--target', $CommitSha)
    if (!$mainBuild) { $options += '--verify-tag' }
    Invoke-GitHub release create $tag --repo $Repository @options
}
Invoke-GitHub release upload $tag @files --repo $Repository --clobber
Invoke-GitHub release edit $tag --repo $Repository --title $title --notes-file $notesPath
$draft = Invoke-GitHub api "repos/$Repository/releases/tags/$tag" | ConvertFrom-Json
Assert-ReleaseAssets $draft
if ($mainBuild -and !(Test-CurrentMain)) {
    Write-Host 'A newer main commit arrived during upload; leaving this build as a draft.'
    return
}
$publishOptions = @('--draft=false', "--prerelease=$($prerelease.ToString().ToLowerInvariant())")
$publishOptions += if ($prerelease) { '--latest=false' } else { '--latest' }
Invoke-GitHub release edit $tag --repo $Repository @publishOptions
$published = Invoke-GitHub api "repos/$Repository/releases/tags/$tag" | ConvertFrom-Json
Assert-ReleaseAssets $published
if ($published.draft -or $published.prerelease -ne $prerelease) { throw 'Release publication failed' }
if (!$prerelease) {
    $latest = Invoke-GitHub api "repos/$Repository/releases/latest" | ConvertFrom-Json
    if ($latest.tag_name -ne $tag) { throw 'New build was not marked Latest' }
}

# Retain the former rolling release and every download, but remove its misleading title.
$legacy = $releases | Where-Object { $_.tag_name -eq 'development' -and $_.name -eq 'Latest main build' } | Select-Object -First 1
if ($legacy) {
    Invoke-GitHub release edit development --repo $Repository --title 'Archived main build (before release history)'
}
Write-Host "Published https://github.com/$Repository/releases/tag/$tag"
