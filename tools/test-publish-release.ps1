# Contract tests for the real publisher with an in-memory GitHub CLI substitute.
# No network calls, credentials, or device access are used.
$ErrorActionPreference = 'Stop'
$global:AudezeReleaseTestsha = '1234567890abcdef1234567890abcdef12345678'
$global:AudezeReleaseTestbase = 'abcdef1234567890abcdef1234567890abcdef12'
$global:AudezeReleaseTesttag = 'main-1234567890ab'
$global:AudezeReleaseTestnames = @('Audeze-Overlay-Setup.exe', 'Audeze-Overlay-Portable.zip', 'SHA256SUMS.txt')
$testDirectory = Join-Path ([IO.Path]::GetTempPath()) ("audeze-release-test-" + [guid]::NewGuid())
$null = New-Item -ItemType Directory -Path $testDirectory

function Assert-That([bool]$Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
}

function Reset-Fixture {
    $global:AudezeReleaseTeststate = @{
        head = $global:AudezeReleaseTestsha; calls = [Collections.Generic.List[object]]::new()
        failUpload = $false; failList = $false; supersedeAfterUpload = $false
        releases = @(@{
            tag_name = 'development'; name = 'Latest main build'; draft = $false
            prerelease = $false; published_at = '2026-01-01T00:00:00Z'
            body = "Built from commit $global:AudezeReleaseTestbase."; assets = @(@{ name = 'old.zip'; size = 10 })
        })
        latest = 'development'
    }
    foreach ($name in $global:AudezeReleaseTestnames) { Set-Content -LiteralPath (Join-Path $testDirectory $name) -Value 'fixture' }
}

function gh {
    $a = @($args)
    $global:AudezeReleaseTeststate.calls.Add($a)
    $global:LASTEXITCODE = 0
    if ($a[0] -eq 'api') {
        $endpoint = $a | Where-Object { $_ -like 'repos/*' } | Select-Object -First 1
        switch -Wildcard ($endpoint) {
            '*/git/ref/heads/main' { return (@{ object = @{ sha = $global:AudezeReleaseTeststate.head } } | ConvertTo-Json) }
            '*/releases?per_page=100' {
                if ($global:AudezeReleaseTeststate.failList) { $global:LASTEXITCODE = 1; return }
                # Match gh --paginate --slurp: an outer array of pages.
                return ConvertTo-Json -InputObject @(,@($global:AudezeReleaseTeststate.releases)) -Depth 10
            }
            '*/commits/*' {
                $value = if ($endpoint.EndsWith('/development')) { $global:AudezeReleaseTestbase } else { $global:AudezeReleaseTestsha }
                return (@{ sha = $value } | ConvertTo-Json)
            }
            '*/compare/*' {
                return ConvertTo-Json -Depth 10 -InputObject @(
                    @{ status = 'ahead'; html_url = 'https://example.test/compare'; commits = @(@{ commit = @{ message = 'fix: first change' }; html_url = 'https://example.test/one' }) },
                    @{ status = 'ahead'; commits = @(@{ commit = @{ message = 'feat: second change' }; html_url = 'https://example.test/two' }) }
                )
            }
            '*/releases/generate-notes' { return (@{ body = 'Generated version changes' } | ConvertTo-Json) }
            '*/releases/latest' {
                return ($global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $global:AudezeReleaseTeststate.latest | ConvertTo-Json -Depth 10)
            }
            '*/releases/tags/*' {
                $target = $endpoint.Split('/')[-1]
                return ($global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $target | ConvertTo-Json -Depth 10)
            }
            default { throw "Unexpected API request: $endpoint" }
        }
    }
    if ($a[0] -ne 'release') { throw "Unexpected command: $a" }
    $target = $a[2]
    $release = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $target | Select-Object -First 1
    switch ($a[1]) {
        'create' {
            Assert-That (!$release) 'Duplicate release creation'
            Assert-That ('--draft' -in $a) 'New release must start as a draft'
            Assert-That ($a[$a.IndexOf('--target') + 1] -eq $global:AudezeReleaseTestsha) 'Wrong target commit'
            $global:AudezeReleaseTeststate.releases += @{
                tag_name = $target; name = ''; draft = $true; prerelease = $false
                published_at = $null; body = ''; assets = @()
            }
        }
        'upload' {
            Assert-That $release.draft 'Published assets must never be overwritten'
            if ($global:AudezeReleaseTeststate.failUpload) { $global:LASTEXITCODE = 1; return }
            $uploads = @($a[3..($a.IndexOf('--repo') - 1)])
            Assert-That ($uploads.Count -eq 3) 'Unexpected uploaded file count'
            $release.assets = @($uploads | ForEach-Object { @{ name = [IO.Path]::GetFileName($_); size = 7 } })
            if ($global:AudezeReleaseTeststate.supersedeAfterUpload) { $global:AudezeReleaseTeststate.head = $global:AudezeReleaseTestbase }
        }
        'edit' {
            Assert-That ($null -ne $release) 'Editing a nonexistent release'
            if ('--title' -in $a) { $release.name = $a[$a.IndexOf('--title') + 1] }
            if ('--notes-file' -in $a) { $release.body = Get-Content -Raw -LiteralPath $a[$a.IndexOf('--notes-file') + 1] }
            if ('--draft=false' -in $a) {
                Assert-That ($release.assets.Count -eq 3) 'Published before downloads were uploaded'
                $release.draft = $false
                $release.published_at = '2026-02-01T00:00:00Z'
                $release.prerelease = '--prerelease=true' -in $a
            }
            if ('--latest' -in $a) { $global:AudezeReleaseTeststate.latest = $target }
        }
        default { throw "Unexpected release operation: $a" }
    }
}

function Invoke-Publisher([string]$RefType = 'branch', [string]$RefName = 'main') {
    & "$PSScriptRoot/publish-release.ps1" -Repository 'owner/repo' -CommitSha $global:AudezeReleaseTestsha -RefType $RefType -RefName $RefName -BuildNumber 42 -ArtifactDirectory $testDirectory
}

try {
    Reset-Fixture
    Invoke-Publisher
    $new = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $global:AudezeReleaseTesttag
    $old = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq 'development'
    Assert-That ($global:AudezeReleaseTeststate.latest -eq $global:AudezeReleaseTesttag -and !$new.draft) 'New main release was not published as Latest'
    Assert-That ($new.body.Contains('fix: first change') -and $new.body.Contains('feat: second change')) 'Paginated changes were lost'
    Assert-That ($old.assets[0].name -eq 'old.zip' -and $old.name.StartsWith('Archived')) 'Old release was not preserved in history'
    Assert-That ($new.body.Contains("/download/$global:AudezeReleaseTesttag/Audeze-Overlay-Setup.exe")) 'Installer link points to a different release'
    $callsBefore = $global:AudezeReleaseTeststate.calls.Count
    Invoke-Publisher
    Assert-That (@($global:AudezeReleaseTeststate.calls | Select-Object -Skip $callsBefore | Where-Object { $_[0] -eq 'release' }).Count -eq 0) 'Rerun mutated a published release'

    Reset-Fixture
    $global:AudezeReleaseTeststate.failUpload = $true
    $failed = $false
    try { Invoke-Publisher } catch { $failed = $true }
    Assert-That ($failed -and $global:AudezeReleaseTeststate.latest -eq 'development') 'Failed upload replaced Latest'
    Assert-That (($global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $global:AudezeReleaseTesttag).draft) 'Failed upload exposed an incomplete release'
    $global:AudezeReleaseTeststate.failUpload = $false
    Invoke-Publisher
    Assert-That ($global:AudezeReleaseTeststate.latest -eq $global:AudezeReleaseTesttag -and $global:AudezeReleaseTeststate.releases.Count -eq 2) 'Draft retry failed'

    Reset-Fixture
    $global:AudezeReleaseTeststate.head = $global:AudezeReleaseTestbase
    Invoke-Publisher
    Assert-That ($global:AudezeReleaseTeststate.releases.Count -eq 1) 'Superseded main created a release'

    Reset-Fixture
    $global:AudezeReleaseTeststate.supersedeAfterUpload = $true
    Invoke-Publisher
    Assert-That ($global:AudezeReleaseTeststate.latest -eq 'development') 'Superseded upload became Latest'

    Reset-Fixture
    $global:AudezeReleaseTeststate.failList = $true
    $failed = $false
    try { Invoke-Publisher } catch { $failed = $true }
    Assert-That ($failed -and $global:AudezeReleaseTeststate.releases.Count -eq 1) 'API failure was mistaken for an empty release history'

    Reset-Fixture
    $version = (Select-String -LiteralPath "$PSScriptRoot/../Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    Invoke-Publisher -RefType tag -RefName "v$version"
    $new = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq "v$version"
    Assert-That ($new.body.Contains('Generated version changes')) 'Version release lost generated notes'
    Assert-That (@($global:AudezeReleaseTeststate.calls | Where-Object { $_[1] -eq 'create' -and '--verify-tag' -in $_ }).Count -eq 1) 'Version release did not verify its tag'

    Write-Host 'PASS: release history, changelog pagination, reruns, draft recovery, stale builds, API errors and version tags.'
} finally {
    # Only this test's uniquely named, resolved temporary directory is removed.
    $resolved = [IO.Path]::GetFullPath($testDirectory)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (!$resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or !(Split-Path $resolved -Leaf).StartsWith('audeze-release-test-')) {
        throw 'Refusing to remove a directory outside the test temporary location'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
