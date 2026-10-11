# Measured performance: Audeze Overlay vs Audeze App

[Back to the project](../README.md)

Measured on **11 October 2026** using the final Overlay 0.4.1 build at commit
`251fcc8` and the installed Audeze App 1.2.48. Every result below comes from this
single test session. These are application measurements, not a Rust language benchmark.

## Results

Each row averages two approximately 60-second idle samples with the headset
connected. CPU is a percentage of **one logical core**; divide by 16 for this
machine's approximate whole-machine percentage. Memory uses MiB (1,048,576 bytes).

| State / application | CPU, one core | Resident working set | Private committed memory | Threads | Handles |
| --- | ---: | ---: | ---: | ---: | ---: |
| Visible / Audeze Overlay | 0.193% | 21.51 MiB | 7.09 MiB | 5.8 | 207 |
| Visible / Audeze App | 0.965% | 105.95 MiB | 150.53 MiB | 73.9 | 866 |
| Hidden / Audeze Overlay | 0.039% | 21.79 MiB | 6.84 MiB | 3.9 | 207 |
| Hidden / Audeze App | 0.900% | 105.83 MiB | 150.36 MiB | 70.9 | 859 |

In this session, the overlay used **79.7% less resident memory while visible**
and **79.4% less while hidden**. Private committed memory was 95.3% lower while
visible and 95.4% lower while hidden.

Visible CPU ranged from **0.180–0.206%** for the overlay and **0.926–1.004%** for
Audeze App. Hidden CPU ranged from **0.026–0.051%** and **0.823–0.978%**, respectively.
These small totals are sensitive to desktop activity and Windows CPU accounting;
use the absolute values and ranges rather than expecting a fixed CPU reduction.

## Individual samples

| App | Trial | State | Duration | Process CPU time | CPU, one core | Working set | Private commit |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| Audeze App | 1 | visible | 60.75 s | 562.500 ms | 0.926% | 107.47 MiB | 150.52 MiB |
| Audeze App | 1 | hidden | 60.78 s | 500.000 ms | 0.823% | 107.42 MiB | 150.41 MiB |
| Overlay | 1 | visible | 60.73 s | 125.000 ms | 0.206% | 20.98 MiB | 7.25 MiB |
| Overlay | 1 | hidden | 60.73 s | 15.625 ms | 0.026% | 21.53 MiB | 6.81 MiB |
| Overlay | 2 | visible | 60.68 s | 109.375 ms | 0.180% | 22.05 MiB | 6.94 MiB |
| Overlay | 2 | hidden | 60.72 s | 31.250 ms | 0.051% | 22.05 MiB | 6.88 MiB |
| Audeze App | 2 | visible | 60.71 s | 609.375 ms | 1.004% | 104.43 MiB | 150.53 MiB |
| Audeze App | 2 | hidden | 60.72 s | 593.750 ms | 0.978% | 104.24 MiB | 150.31 MiB |

## Conditions and method

- Windows 11 25H2, build 26200.9168; AMD Ryzen 7 9800X3D, 16 logical processors.
- Audeze App 1.2.48, identified by the binary hash below. Its executable does not
  expose a Windows version resource; the version was identified from its startup log.
- Audeze Overlay 0.4.1, optimized LLVM-MinGW release build with all current changes,
  without tracing or allocation instrumentation. The installed main build was
  stopped for the test and restored afterward.
- Maxwell 2 ANC PS connected through its wireless dongle, with the USB charging
  cable absent. A read-only baseline confirmed mix 50/50, sidetone 35%, ANC On,
  battery 85%, and microphone muted. No sound-setting commands were scripted.
- Each app ran alone. Trial one measured Audeze App then Overlay; trial two reversed
  that order. Ten seconds of launch warmup and three seconds of settling preceded
  each visible/hidden sample. The script rejects competing Audeze processes,
  premature process exit, and incorrect end-of-sample visibility.
- Audeze App's connected Sound page was checked through a read-only accessibility
  snapshot before each visible sample. This initializes accessibility support and
  may contribute some native-app overhead.
- Audeze App window: 1216 × 727 pixels. Overlay: 310 × 585 pixels, saved user layout,
  100% text, 40% outer background, 10% card opacity, blur enabled at its migrated
  50% setting, status row enabled, and optional local Visby font. These are real
  application configurations with different window sizes, not a renderer benchmark.
- Hidden means the window was hidden with `ShowWindow`; the process continued
  running and was not suspended or forcibly working-set-trimmed.
- `TotalProcessorTime` measures process CPU. Working set, private commit, threads
  and handles are sampled once per second. Working set includes shared resident
  pages; private commit is allocated private memory, not resident RAM.
- No builds or other headset tools ran during timed samples. Normal desktop
  applications remained open. Raw results are saved locally in ignored
  `diagnostics/performance/final-2026-10-11.json`.

## Scope

These measurements cover connected idle process resources. They do not measure
active dragging, physical wheel-to-popup latency, game FPS, frame times, GPU or
DWM/compositor cost, power use, or anti-cheat compatibility. The official app also
provides firmware management. Window size, fonts, layout and desktop activity can
change results. CPU accounting on this system can advance in roughly 15.625 ms steps.

The overlay retains its 50 ms mix target, approximately 1 s background sound checks,
30 s battery checks and on-demand EQ. These are scheduling targets, not end-to-end
latency guarantees.

## Reproduce

Close both applications, connect the headset through its dongle, and leave the
controls and cable unchanged during timing. From PowerShell in the repository:

```powershell
./build.ps1
if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
cargo build --locked --release --bin inspect-accessibility
if ($LASTEXITCODE -ne 0) { throw 'Inspector build failed' }
./tools/compare-performance.ps1 -NativePath "D:\Audeze\AudezeApp\Audeze.exe" -Seconds 60 -Trials 2
```

Adjust the app path for your installation. Keep window sizes, appearance and desktop
conditions consistent. The script saves raw JSON under ignored diagnostics and stops
only processes it launches. Reopen your usual overlay when it finishes.

### Measured binaries

- Overlay SHA-256: `18c2420bc899de3be714bda39cb867d76f67864c007fbc33db4e99674fbb33a1`
- Audeze App SHA-256: `88299c36990f44de4ce17a5b9c65b5521fd68ee649fdb83e80ccf530ad604ca5`
