# macOS port plan

[Back to documentation](README.md)

> **Status: in progress.** The HID transport and the first AppKit window shell
> are implemented; the overlay shows live headset data on a Retina display.
> Pointer/keyboard interaction, desktop integration and packaging remain. The
> Linux port is the reference for scope and validation; see [Linux](LINUX.md).

## Goal

The same overlay and controls on macOS as on Windows and Linux: game/chat mix,
noise cancellation, sidetone, microphone state, battery and EQ. It is its own
window; it does not inject into games, hook graphics or read other programs'
input. The portable core (`model`, `eq`, `layout`, `device::protocol`,
`device::selection`, `device::worker`, views and `ui::app`) stays unchanged.

The port is additive: one `Shell` implementation, one HID transport, small
`cfg`/path/font edits, and a `.app` packaging story.

## Order of work

### 1. Device transport first — done

- Added `src/device/hid/mac.rs` using `IOHIDManager`/`IOHIDDevice`; macOS
  publishes the interface's report descriptor, so the vendor control collection
  (`0xff13`/`1`) is found with the shared descriptor reader.
- Implemented `get_input_report`/`write_report`, endpoint identity and hot-plug
  callbacks with the same debounce as hidraw.
- Validated with `verify-direct` on a real Maxwell: read-only snapshot, then
  `--controls-and-restore` with the headset present, then unplug/replug.

**Exit:** the shared worker controls a headset on macOS. This de-risks the only
part that cannot be validated without hardware.

Hardware evidence: an Audeze Maxwell Xbox dongle (`3329:4b18`) returned a live
snapshot (battery, mix, sidetone, EQ index); the polling worker completed in
about 1.3 s; mix, noise filter, sidetone on/off and sidetone level each changed,
read back, restored, and matched the original state; and repeated unplug/replug
cycles woke the worker and re-enumerated the dongle. This is one device and one
firmware, not a broader compatibility claim.

### 2. Core wiring — done

- Added `target_os = "macos"` arms in `src/main.rs`, `src/ui/mod.rs`,
  `src/device/hid.rs`, `FileSelection` and `Cargo.toml`.
- Preferences moved to `~/Library/Application Support/AudezeOverlay`.
- Added a macOS font lookup (no fontconfig on stock macOS; a variable SF file
  falls through to Arial or Verdana) and the macOS dependencies.
- The bundle-aware `asset()` path moves with the `.app` packaging step.

**Exit:** `cargo test` and `preview-layouts` pass on a clean Mac, with no Windows
or Linux behaviour changed.

### 3. Minimal shell — in progress

Done:

- `src/ui/macos.rs`: accessory `NSApplication`, borderless non-activating window
  above ordinary windows, layer/`CGImage` presentation, `CFRunLoop` timers, a
  run-loop source for redraws and worker wakes, screens and backing scale,
  `--demo` and live headset data.

Remaining:

- Pointer and keyboard events, caption drag and edge resize, pointer capture,
  cursors, click-through toggling and focus loss.
- Native menus, alerts and open/save panels.
- Reuse the Unix-socket single-instance design and the coalescing signal helper.

**Exit:** `--demo` shows and hides a correct overlay on a Retina display and on a
second display, including window movement across screens.

### 4. Desktop integration

- Native menus, alerts and open/save panels; status item with a template image;
  global shortcut via Carbon `RegisterEventHotKey`; launch at login via
  `SMAppService`; autostart error text for unbundled development runs.
- Map the preference format's Windows virtual-key codes to macOS key codes.

**Exit:** parity with the Linux desktop-integration list in [LINUX.md](LINUX.md):
tray, shortcut, autostart, profile import/export and one running instance.

### 5. Fullscreen, Spaces and blur

- Test the overlay above Space-based fullscreen games, Mission Control and
  Stage Manager; document that games taking exclusive display capture cannot be
  overlaid.
- Map blur to `NSVisualEffectView` (on/off, like the compositor blur on Linux).

**Exit:** a written statement of what macOS does and does not cover, with the
same honesty as the GNOME/layer-shell caveat.

### 6. Packaging and release

- `packaging/macos/`: `Info.plist` (`LSUIElement`), `.icns`, `.app` assembly,
  universal binary, zip/dmg and checksum.
- Code signing and notarization for a public download; ad-hoc signing is enough
  for CI artifacts.
- New macOS CI workflow called from the release workflow; add the macOS archive
  to the publish scripts and release notes.
- Turn this page into the support guide, with hardware evidence and install
  instructions, mirroring the structure of [LINUX.md](LINUX.md).

**Exit:** a downloadable macOS build and a documented, reproducible check.

## Non-goals for the first port

- Mac App Store sandboxing.
- Games that capture the display exclusively.
- Bluetooth connection status (currently Windows-only).
- Changing the shared preference schema or the Windows virtual-key numbering.

## Main risks

| Risk | Handling |
| --- | --- |
| `IOHIDDeviceGetReport` report-ID/length conventions | Validate in step 1 with `verify-direct` before building the UI |
| Overlay above fullscreen games | Window level plus `fullScreenAuxiliary`; document exclusive-capture limits |
| Retina and mixed-scale coordinates | Convert AppKit's bottom-left origin at the shell boundary; re-render on backing-property changes |
| Fonts on a clean Mac | Concrete `.ttf` fallbacks; test `Painter::try_new()` without Homebrew fontconfig |
| Nested modal run loops | Keep native modals synchronous, but queue worker notifications instead of re-entering `App` |

## New and changed files

| Path | Change |
| --- | --- |
| `src/ui/macos/` | New shell implementation |
| `src/device/hid/mac.rs` | New IOKit HID transport |
| `src/device/hid.rs`, `src/ui/shell.rs`, `src/ui/app/eq_profiles.rs`, `src/main.rs`, `src/ui/mod.rs` | macOS `cfg` arms and the synchronous `FileSelection::Ready` path |
| `src/config.rs`, `src/ui/draw.rs` | Preferences directory, system font and bundle asset lookup |
| `Cargo.toml` | macOS-only dependencies |
| `packaging/macos/`, `.github/workflows/macos.yml`, `tools/` | Bundle, CI and release plumbing |
