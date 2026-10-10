# Build, diagnostics and releases

[Back to the project](../README.md)

Run commands from the repository root. Read [CONTRIBUTING](../CONTRIBUTING.md) and [AGENTS.md](../AGENTS.md) before making changes.

## Build and verify


```powershell
cargo test
cargo build --release --bin audeze-overlay
```

`build.ps1` uses project-local Rust / LLVM-MinGW tools when available:

```powershell
.\build.ps1 -Test
.\build.ps1
target\release\audeze-overlay.exe --smoke-test
cargo run --bin verify-direct -- --worker
```

Smoke checks use sample data, leave headset values and saved settings alone,
and write `diagnostics/smoke-v3.json`. The worker check reads live state without
changing sound. The separate opt-in `verify-direct --controls-and-restore`
diagnostic temporarily changes and restores sound settings.

`--demo` shows sample data. `--snapshot-preview --render-only` writes a PNG;
add `--settings --section 0` (0–5) for settings pages or `--preview-layout stacked`
for a tall layout. PNGs contain window pixels; real compositor blur is checked
by the separate `check-blur` diagnostic against a controlled desktop pattern.
Legacy accessibility diagnostics remain for comparison; the active app uses
`src/device/`. Build the old diagnostics with `--features legacy-uia`.

For intermittent latency diagnostics, launch with `--trace-usb`. A bounded
`usb-timing.csv` beside the executable records scheduled queries, raw replies,
parsed values, timeouts, and receipt-to-presentation time. Sequence numbers link
received values to visible frames, including the actual window opacity. Logging
is off by default and stops after 30,000 records. These timings do not measure
the physical wheel-to-device delay. Keep the panel open for counted single-step
tests so disappearance and fade-out cannot mask a value.

For an input-worker footprint measurement, quit the overlay and run:

```powershell
cargo run --release --features perf-stats --bin bench-worker -- --seconds=30
```

This read-only diagnostic warms up the real headset connection, then reports
worker queries, report reads, loop iterations, allocations and process CPU time.
Run it by itself, without `--trace-usb`, for comparable measurements. CPU time can
be quantized on Windows; a small or zero result does not prove zero CPU work.
Counters and the counting allocator are absent from ordinary app builds. This
measures USB-worker overhead, not physical wheel-to-screen latency.

`check-blur --corners` compares the real composed window with a contrasting
background baseline; a region-only test cannot detect acrylic spilling into
transparent corners.

The waiting-read probe found no sound-query replies on the interrupt input
stream. The observed report 4 is used by the installed app for head-tracking
re-centering, not sound state. `listen-headset` is a diagnostic only; the
production backend retains scheduled sound queries rather than assuming
unsupported push notifications.

## GitHub builds and releases


`.github/workflows/windows.yml` runs on every push, pull request and manual
workflow run. It checks formatting and Clippy (warnings are errors), runs Rust tests, verifies rendered layouts,
and creates an x64 portable ZIP, per-user installer and SHA-256
checksums. Open **Actions → Windows build and release → Artifacts → windows-x64**
for development builds. Layout preview images are a separate artifact.

Successful pushes to `main` publish the portable ZIP and installer as the
[latest release](https://github.com/temdah/AudezeOverlay/releases/latest).
This rolling release uses the `development` tag and is updated on each successful
main build. No manual tag is needed.
Release notes include download instructions and a changelog since the previous
main build. Numbered releases use GitHub's generated release notes.

To additionally publish a numbered release, set the version in `Cargo.toml`, run `cargo check` to
update `Cargo.lock`, commit, then push a matching version tag:

```powershell
git tag v0.4.1
git push origin v0.4.1
```

The tag must match the package version. The workflow publishes the tested
downloads to GitHub Releases with generated release notes. Ordinary branches
create build artifacts. Version tags containing a hyphen publish prereleases;
main builds publish a normal release marked **Latest**. Release publishing uses the repository's built-in
`GITHUB_TOKEN`; no personal token or extra repository secret is required.
Publishing permissions are limited to the release job. Actions are pinned to
commit hashes, and the Inno Setup compiler download has a pinned SHA-256 hash.

Build the same packages locally with Rust/MSVC installed:

```powershell
.\tools\package.ps1
```

With the project's local LLVM tools:

```powershell
.\build.ps1
.\tools\package.ps1 -BinaryPath target\release\audeze-overlay.exe -Target x86_64-pc-windows-gnullvm
```

Output is in `dist/release`. Packages contain only the app, documentation and
license notices; personal fonts, logs and settings are excluded. They are
currently unsigned. A signing certificate would be needed for signed releases.
