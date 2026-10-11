param(
    [Parameter(Mandatory)][string]$NativePath,
    [string]$OverlayPath = (Join-Path $PSScriptRoot '..\target\release\audeze-overlay.exe'),
    [int]$Seconds = 30,
    [int]$Trials = 2,
    [string]$Output = (Join-Path $PSScriptRoot '..\diagnostics\performance\native-comparison.json')
)
$ErrorActionPreference = 'Stop'
# Run with both apps closed; each is measured alone without changing headset values.
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class BenchWindow {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    public delegate bool EnumProc(IntPtr hwnd, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    public static IntPtr FindMainWindow(int pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hwnd, _) => {
            uint owner; GetWindowThreadProcessId(hwnd, out owner);
            if (owner == pid) {
                var title = new StringBuilder(512); GetWindowText(hwnd, title, title.Capacity);
                if (title.ToString().IndexOf("Audeze", StringComparison.OrdinalIgnoreCase) >= 0) found = hwnd;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
}
'@
$NativePath = (Resolve-Path -LiteralPath $NativePath).Path
$OverlayPath = (Resolve-Path -LiteralPath $OverlayPath).Path
function Assert-OnlyMeasuredApp([int]$ExpectedId = 0) {
    $competing = @(Get-Process | Where-Object {
        $_.Path -in @($NativePath, $OverlayPath) -or $_.ProcessName -in @('Audeze', 'Audeze Overlay', 'audeze-overlay')
    } | Where-Object { $_.Id -ne $ExpectedId })
    if ($competing.Count) { throw 'Close other Audeze applications before measuring; competing controllers invalidate the comparison.' }
}
Assert-OnlyMeasuredApp
$results = [System.Collections.Generic.List[object]]::new()
foreach ($trial in 1..$Trials) {
    $apps = if ($trial % 2) { @('native','overlay') } else { @('overlay','native') }
    foreach ($app in $apps) {
        $exe = if ($app -eq 'native') { $NativePath } else { $OverlayPath }
        $start = @{ FilePath=$exe; PassThru=$true; WindowStyle='Hidden' }
        if ($app -eq 'overlay') { $start.ArgumentList = '--open' }
        $process = Start-Process @start
        try {
            Start-Sleep -Seconds 10
            $process.Refresh()
            $hwnd = $process.MainWindowHandle
            if ($hwnd -eq [IntPtr]::Zero) { $hwnd = [BenchWindow]::FindMainWindow($process.Id) }
            if ($hwnd -eq [IntPtr]::Zero) { throw "$app did not expose its main window; cannot compare visibility." }
            foreach ($mode in @('visible','hidden')) {
                [BenchWindow]::ShowWindow($hwnd, $(if ($mode -eq 'visible') { 4 } else { 0 })) | Out-Null
                $inspector = Join-Path $PSScriptRoot '..\target\release\inspect-accessibility.exe'
                if ($app -eq 'native' -and $mode -eq 'visible' -and (Test-Path -LiteralPath $inspector)) {
                    New-Item -ItemType Directory -Force (Split-Path $Output) | Out-Null
                    & $inspector $process.Id | Set-Content (Join-Path (Split-Path $Output) "native-trial-$trial-ui.txt")
                }
                Start-Sleep -Seconds 3
                Assert-OnlyMeasuredApp $process.Id
                $process.Refresh()
                if ($process.HasExited) { throw "$app exited before measurement." }
                $cpuStart = $process.TotalProcessorTime.TotalMilliseconds
                $timer = [Diagnostics.Stopwatch]::StartNew()
                $samples = [System.Collections.Generic.List[object]]::new()
                foreach ($second in 1..$Seconds) {
                    Start-Sleep -Seconds 1
                    $process.Refresh()
                    if ($process.HasExited) { throw "$app exited during measurement." }
                    $samples.Add([pscustomobject]@{
                        workingSetMiB=$process.WorkingSet64/1MB
                        privateCommitMiB=$process.PrivateMemorySize64/1MB
                        handles=$process.HandleCount; threads=$process.Threads.Count
                    })
                }
                $elapsed = $timer.Elapsed.TotalSeconds
                $cpuMs = $process.TotalProcessorTime.TotalMilliseconds - $cpuStart
                Assert-OnlyMeasuredApp $process.Id
                $rect = [BenchWindow+Rect]::new()
                [BenchWindow]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
                if ([BenchWindow]::IsWindowVisible($hwnd) -ne ($mode -eq 'visible')) {
                    throw "$app visibility changed during measurement."
                }
                $result = [pscustomobject]@{
                    app=$app; trial=$trial; mode=$mode; seconds=$elapsed
                    cpuMs=$cpuMs; cpuOneCorePercent=$cpuMs/($elapsed*10)
                    workingSetMiB=($samples.workingSetMiB | Measure-Object -Average).Average
                    privateCommitMiB=($samples.privateCommitMiB | Measure-Object -Average).Average
                    handles=($samples.handles | Measure-Object -Average).Average
                    threads=($samples.threads | Measure-Object -Average).Average
                    visible=[BenchWindow]::IsWindowVisible($hwnd)
                    width=$rect.Right-$rect.Left; height=$rect.Bottom-$rect.Top
                }
                $results.Add($result)
                New-Item -ItemType Directory -Force (Split-Path $Output) | Out-Null
                [pscustomobject]@{
                    capturedAt=(Get-Date).ToString('o'); logicalProcessors=[Environment]::ProcessorCount
                    nativeVersion=(Get-Item -LiteralPath $NativePath).VersionInfo.FileVersion
                    nativeSha256=(Get-FileHash -LiteralPath $NativePath -Algorithm SHA256).Hash
                    overlaySha256=(Get-FileHash -LiteralPath $OverlayPath -Algorithm SHA256).Hash
                    trials=$results
                } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Output -Encoding utf8
                Write-Output ($result | ConvertTo-Json -Compress)
            }
        } finally {
            if (!$process.HasExited) { Stop-Process -Id $process.Id }
        }
    }
}
