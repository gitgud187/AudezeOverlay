# Measured performance: Audeze Overlay vs Audeze App

Local measurement on 10 October 2026. These results compare this application build with the installed native app; they do not establish that one programming language is inherently faster.

## Results

Each row averages two approximately 30-second samples. CPU is percentage of **one logical core**; on this 16-thread machine divide by 16 to approximate Task Manager’s whole-machine percentage. Memory uses MiB (1,048,576 bytes).

| State / application | CPU, one core | Resident working set | Private committed memory | Threads | Handles |
| --- | ---: | ---: | ---: | ---: | ---: |
| Visible / Audeze App | 1.314% | 105.85 MiB | 151.49 MiB | 75.1 | 863 |
| Visible / Rust overlay | 0.103% | 19.48 MiB | 7.17 MiB | 4.8 | 171 |
| Hidden / Audeze App | 0.823% | 107.04 MiB | 151.94 MiB | 73.1 | 864 |
| Hidden / Rust overlay | 0.052% | 19.48 MiB | 7.15 MiB | 4.2 | 172 |

The overlay used **81.6% less resident memory while visible** and **81.8% less while hidden**. Private committed memory was lower by 95.3% and 95.3%, respectively.

Hidden-state CPU was 93.7% lower in these samples. Visible-state CPU was lower by 92.2%. The visible result is **not evidence of an across-the-board CPU improvement**. These low CPU measurements vary with scheduling, redraws and desktop activity; prioritize the absolute values and trial spread over a single percentage ratio.

A separate preliminary run of the earlier candidate showed the opposite visible-CPU ordering (native 0.670%, overlay 1.446% of one core). Its hidden CPU was 0.902% vs 0.155%, and resident memory about 104.5 vs 21.3 MiB. The candidate and desktop activity differed, so the reversal cannot be credited to a particular code change. Memory savings were consistent; CPU savings need longer controlled runs before promising a fixed reduction. A recorded 0 ms sample means below the observed accounting resolution, not zero CPU work.

## Individual samples

| App | Trial | State | Duration | Process CPU time | CPU, one core | Working set | Private commit |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| native | 1 | visible | 30.32 s | 312.500 ms | 1.031% | 105.49 MiB | 153.02 MiB |
| native | 1 | hidden | 30.38 s | 265.625 ms | 0.874% | 105.50 MiB | 152.91 MiB |
| overlay | 1 | visible | 30.35 s | 46.875 ms | 0.154% | 19.54 MiB | 7.24 MiB |
| overlay | 1 | hidden | 30.30 s | 0.000 ms | 0.000% | 19.54 MiB | 7.22 MiB |
| overlay | 2 | visible | 30.29 s | 15.625 ms | 0.052% | 19.42 MiB | 7.09 MiB |
| overlay | 2 | hidden | 30.30 s | 31.250 ms | 0.103% | 19.42 MiB | 7.08 MiB |
| native | 2 | visible | 30.32 s | 484.375 ms | 1.598% | 106.21 MiB | 149.96 MiB |
| native | 2 | hidden | 30.35 s | 234.375 ms | 0.772% | 108.58 MiB | 150.97 MiB |

## Conditions and method

- Windows 11 25H2, build 26200; AMD Ryzen 7 9800X3D, 8 cores / 16 logical processors.
- Audeze App 1.2.48, Qt 6.9.3 (version from its local startup log; its executable has no Windows version resource).
- Audeze Overlay 0.4.1, optimized Rust release build using the repository’s LLVM-MinGW toolchain.
- Maxwell 2 ANC PS USB dongle connected, firmware 1.0.5.7. The native Sound page was verified through a read-only accessibility snapshot before timing. That initializes accessibility support and may contribute some native-app overhead.
- Applications ran separately so they did not compete for the headset. Ten seconds of launch warmup, then three seconds settling after showing/hiding. No scripted audio-setting changes during timing. Trial order reversed for the second repetition.
- Native window: 1216 × 727 pixels. Overlay: 440 × 799 pixels, current user layout and transparency/blur settings, optional local Visby font. Different native layouts are part of the measured real-world configuration; this was not a renderer microbenchmark.
- Hidden means the same window was hidden with ShowWindow, not forcibly working-set-trimmed or suspended. Both processes continued running. Visibility flags were checked in recorded samples.
- Process CPU time measured with TotalProcessorTime; memory, handles and threads sampled once per second through Windows process APIs. Working set includes shared resident pages; private commit is allocated private memory and is not the same as resident RAM.
- No Rust builds or other headset tools ran during the timed samples. Normal desktop applications remained open, so this is a local comparison rather than an isolated laboratory benchmark.

## What this does not prove

No physical wheel-to-sound or wheel-to-popup latency was measured against the native app. Nor were game FPS, frame-time percentiles, GPU cost, DWM/compositor cost, power use or anti-cheat compatibility measured. UI automation was used only to inspect the native app before timing, not as the overlay’s input path. The official app has a broader feature set, including firmware management.

The overlay retains the 50 ms mix polling target, 1-second secondary sound checks and 30-second battery checks. EQ reads remain on demand. Selection retries run only during a pending EQ switch. These intervals are scheduling targets, not end-to-end latency guarantees. The small idle CPU totals can be quantized in approximately 15.625 ms steps on this system.

Live EQ functional tests changed a band and verified restoration of the complete original curve. Preset switches completed with a single request in the test; acknowledgement can precede the newly active index. This functional verification is separate from the resource measurements.

## Reproduce

Close both applications first, connect the headset and keep settings unchanged during timing. From the repository root, build the release executable, then run:

```powershell
./build.ps1
cargo build --release --bin inspect-accessibility
./tools/compare-performance.ps1 -NativePath "D:\Audeze\AudezeApp\Audeze.exe"
```

Adjust NativePath for the local installation. The script writes raw JSON to ignored `diagnostics/performance/native-comparison.json`. Use the same font, window sizes and desktop conditions when comparing results. It stops only the processes it launches; reopen the overlay after the standalone script finishes.

### Measured binaries

- Overlay SHA-256: `ee392fe53365ad387d3a3f79290057ddfb57d180d691169a041624d4852e83df`
- Native SHA-256: `88299c36990f44de4ce17a5b9c65b5521fd68ee649fdb83e80ccf530ad604ca5`
