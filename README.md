# Audeze Overlay

A native Rust / Win32 sound overlay for **Maxwell 2 ANC over its USB dongle**
on Windows 11. It reads and controls the headset directly. **Audeze App can
stay closed.** This build supports the tested dongle (VID 3329, PID 4BA9).
Other headset models, Bluetooth, and wired USB connections are not implemented.

## Use

Download the **windows-x64-setup.exe** installer from GitHub Releases, or
extract the **portable.zip** and run **Audeze Overlay.exe**. A single portable
EXE is also available. The installer adds a Start menu entry and an optional
desktop shortcut, and installs for your Windows account without admin rights.
Uninstall through Windows Settings → Apps; saved preferences are kept.

For a local development build, run `dist/Audeze Overlay.exe`. First launch opens
the controls; later launches stay in the tray. **Alt + Home** opens/hides the
panel. Launching it again opens the existing instance.

- Tray: **Open, Settings, Reset position, Hide, Quit**. Double-click to open.
- Mix: click/drag the circle, scroll in 5-point steps, double-click for 50/50.
- Sidetone: switch on/off and drag its level slider (native 31-step range).
- ANC: select On, Off, or Transparency. FILTER: select Off, Low, or High.
- Device, battery, connection and hardware microphone mute are status displays.
- The **minimize button / Escape** hides to tray. **Quit** exits completely.

Automatic popups preserve keyboard focus and pass mouse clicks through to the
game. Open with the hotkey or tray to interact. Manually opened controls stay
visible until hidden. Use borderless/windowed games: ordinary desktop windows
cannot reliably appear over exclusive fullscreen games.

## Move and resize

Drag the header to move the overlay. Click the **grid icon beside the gear**
to edit the layout. Drag cards to move them, their bottom-right corners to
resize them, or window edges/corners to resize the entire panel. Click the
grid icon again to finish. Cards snap to a grid and cannot overlap.
Drop a card onto another to swap their slots. Larger cards compact to fit and
nearby cards move into the space left behind. Dragging into empty space keeps
the card's size. A layout that
cannot keep every control readable stays at the last valid arrangement.

Drag the window edges outward to create more empty grid space; this does not
stretch cards. Move cards into that space or resize individual cards using
their corner handles. Finishing layout editing closes unused vertical gaps
and trims space after the last card. **Appearance → Interface scale** is the
separate option for making the cards themselves larger or smaller.

In layout editing, click a card's **… → Remove card** to hide it. Use **+**
beside the grid icon to add it back; the window makes room if needed. Hidden
cards also appear in **Settings → Cards & layout**. Minimum dimensions keep
controls readable.
Position, size and card arrangement are saved. **Reset position** recovers an
off-screen panel. **Reset layout** restores the original card arrangement.

## Settings

Numeric settings use sliders that **snap in increments of 5**. Drag for large
changes or scroll over a slider for one five-unit step per notch. The current
value and unit are shown. Drag changes save on release; switches and scrolling
save immediately.

| Page | Options |
| --- | --- |
| Appearance | Interface scale, text size, background opacity, card opacity, background blur, animations |
| Cards & layout | Card visibility, grid rows, edit layout, reset layout |
| Notifications | Master popup switch, mix/sidetone/ANC/FILTER/mute triggers, time before hiding, fade duration, preview |
| Shortcuts | Record the show/hide hotkey |
| Startup & device | Launch with Windows, monitor, edge snapping, position lock, low-battery threshold, reset position, Reset app |

Opacity changes by 5 percentage points; timing by 5 milliseconds (1000 ms =
1 second). Zero fade duration hides immediately. Zero low-battery threshold
disables that alert. Text stays solid when opacity changes. Blur softens the
scene behind transparent areas using Windows composition. Turn it off for
clear transparency. Windows controls the blur radius. The panel and acrylic
use matching native rounded corners; Windows may draw a soft shadow around them.

Record one key or **Alt / Ctrl / Shift + one key**. Escape cancels. Conflicts
preserve the previous shortcut. Click **End recording** to apply the candidate;
recording continues until then. Windows reserves F12.

Settings: `%LOCALAPPDATA%\AudezeOverlay\settings.json`. Startup is off by
default. Enabling it adds the current executable path to the current user's
Run key. After moving the app, toggle startup off/on to update the path.
No Audeze installation files are modified.

The last Settings page is remembered. Notification durations also accept typed
numbers: click the value, enter milliseconds, and press Enter. Escape cancels.
Typed values can use single-millisecond precision while sliders retain steps of 5.
**Startup & device → Reset app** asks for confirmation, then restores all overlay
preferences, layout and shortcut. It does not change headset sound settings.
Factory defaults use 70% outer background and 70% card opacity; startup is off.

Cards animate between grid positions. The dial expands while held and has an
inner position marker. Selection fills follow the direction of the button change.
Disable **Appearance → Smooth animations** for immediate transitions.

## Performance and compatibility

A separate USB worker alternates due mix reads with background work (50 ms mix target), checks
other sound fields once per second, and reads battery every 30 seconds. Commands
are coalesced during dragging and read back from the headset. Communication
is restricted to recognized sound commands on the supported dongle; firmware
and arbitrary device commands are not exposed. Disconnects clear readings and
trigger reconnection. The worker sleeps until a reply check or command is due; it no longer reads USB on a fixed 10 ms loop. A slow background query times out after 100 ms and cannot queue a sequence of overdue requests ahead of mix updates.
When a reply is split across USB reports, up to two immediate continuation
reads finish it without the normal retry delay. Empty replies use the normal
backoff, so this does not create continuous background reads.

Routine queries use stack buffers and bounded field lists. Repeated state strings
reuse their storage; the worker skips shared-state comparisons when no reply or
command result can have changed it. Trace-only timestamps are collected only with
logging enabled. Already-due work uses a nonblocking channel check instead of a
zero-duration timed wait. These changes leave polling intervals, reply checks,
continuation reads and command confirmation behavior unchanged.

The direct connection removes Audeze App's UI update path. Physical dial
latency still includes firmware, radio, USB responses and Windows scheduling.
The polling target is not an end-to-end latency guarantee. Local diagnostics
verify live reads and control readbacks; physical button-to-popup timing still
needs a hands-on headset check.
Headset-triggered popups appear at full opacity immediately; only manual opening
uses a fade-in. An already visible dial animates for 70 ms while its numeric
value uses a fast 85 ms transition. A newly shown dial starts at the received value.
While dragging the on-screen wheel, the newest target stays visible until its
own command is confirmed; older queued replies cannot rewind the dial. The
current frame is drawn before a hidden window reopens.

Cached fonts and antialiased vectors render at display resolution. No web
runtime or permanent rendering loop is used; painting occurs on changes, with
brief timers for dial/fade animations. Windows performs the blur. Larger
windows and blur can increase desktop-compositor work.

No game injection, graphics hook, game-memory access, custom driver or global
keyboard hook is used. Hotkeys use Windows RegisterHotKey. Anti-cheat vendor
approval and universal compatibility are not claimed.

## Visual assets

Public builds use Windows Segoe UI and an embedded vector wordmark/tray logo.
The logo retains its original ownership; see `THIRD-PARTY-NOTICES.md`. Optional
personal builds can load Visby CF Medium from `assets` beside the executable
or `assets/local` in the repo. These font files are ignored by Git and never
included by the release packager. Only use optional local fonts that you have
permission to use.
The running overlay does not open or launch Audeze.exe.

## Codebase

Start with [Architecture and Rust reading guide](docs/ARCHITECTURE.md).
See [CONTRIBUTING](CONTRIBUTING.md)
for the development checks and commit convention.

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
add `--settings --section 0` (0–4) for settings pages or `--preview-layout stacked`
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
and creates a standalone x64 EXE, portable ZIP, per-user installer and SHA-256
checksums. Open **Actions → Windows build and release → Artifacts → windows-x64**
for development builds. Layout preview images are a separate artifact.

To publish a release, set the version in `Cargo.toml`, run `cargo check` to
update `Cargo.lock`, commit, then push a matching version tag:

```powershell
git tag v0.3.1
git push origin v0.3.1
```

The tag must match the package version. The workflow publishes the tested
downloads to GitHub Releases with generated release notes. Ordinary branches
create build artifacts; successful `main` builds also update the **development**
prerelease with downloadable EXE, ZIP and installer files. Release publishing uses the repository's built-in
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
