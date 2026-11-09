# macOS build and port status

[Back to documentation](README.md)

> **Status: working build, experimental.** The HID transport, window shell,
> pointer input, status item, single instance, launch at login, native menus,
> file panels/alerts and backdrop blur are implemented. The `.app` bundle, DMG
> installer and macOS CI are in place. Remaining work is signing/notarization, a
> universal binary, and more display/hardware evidence. The Linux port is the
> reference for scope and validation; see [Linux](LINUX.md).

## Install and use

The current build is **Apple silicon (arm64)**, ad-hoc signed and not notarized.
From a checkout:

```sh
cargo build --release
packaging/macos/package-macos.sh
open dist/Audeze-Overlay-macOS-arm64.dmg
```

Drag “Audeze Overlay” from the mounted disk image to Applications, then eject
it. To run the checkout build without installing, open
`dist/Audeze Overlay.app`.

Gatekeeper may ask you to open it from the context menu, or allow it under
System Settings → Privacy & Security. The app has no Dock icon; quit it from the
menu bar icon.

- `--demo` shows sample headset data and never opens the device.
- `--toggle` shows or hides the panel of an already running instance.
- No driver, udev rule or Accessibility permission is needed.
- Launch at login uses `SMAppService` and requires macOS 13 or newer.
- The global shortcut is not available on macOS yet; the menu bar icon and
  `--toggle` are the ways to show or hide the panel.

Requirements: macOS 11 or newer on Apple silicon for this build. The supported
headset identities and their validation limits are the same as Windows and
Linux; see [Device profiles](DEVICE_PROTOCOL.md).

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
  above ordinary windows, layer/`CGImage` presentation, repeating `CFRunLoop`
  timers, a run-loop source for redraws and worker wakes, screens and backing
  scale, `--demo` and live headset data.
- Pointer handling: flipped `NSView`, first-mouse acceptance, clicks, double
  clicks, drags, wheel, edge resize and cursors.
- One running instance: a second launch forwards `open`/`toggle` over the shared
  Unix socket; verified that `--toggle` hides and then restores the panel.
- Verified with synthetic CoreGraphics input: gear opens Settings, back returns
  to the dashboard, the Sidetone toggle changes state, and a click or drag on
  the Game/Chat dial updates the mix.

Remaining:

- Edge resize and cursor changes are wired but not yet exercised end to end.
  Keyboard and text entry (2500 → 3000 ms) and focus-loss commit were verified
  with synthetic input.

**Exit:** `--demo` shows and hides a correct overlay on a Retina display and on a
second display, including window movement across screens.

### 4. Desktop integration

Done:

- Status item with the template logo and a menu for Open, Settings, Reset
  position, Hide and Quit; menu actions were verified by keyboard selection.
- Launch at login through `SMAppService` (macOS 13+); verified in the local
  bundle, where macOS added and then removed the login item.
- One running instance and `--toggle`, shared with the Wayland shell.
- Native menus, alerts and profile import/export; verified by opening
  “Profiles…” → “Import from file…”, which presents a real `NSOpenPanel`.

Skipped:

- Global shortcut. Carbon registration succeeded, but its callback ran while
  the UI state was borrowed and aborted the process. The shortcut is disabled on
  macOS rather than shipped half-working; revisit with a deferred event.

**Exit:** tray, autostart, profile import/export and one running instance. A
global shortcut is explicitly out of scope for the first macOS release.

### 5. Fullscreen, Spaces and blur — done, with limits

- Verified above a fullscreen terminal: the overlay stayed visible while the
  menu bar was hidden (`canJoinAllSpaces` and `fullScreenAuxiliary`).
- Blur is an `NSVisualEffectView` behind the overlay (on/off, like the Linux
  compositor blur). Verified with an A/B capture at low background opacity:
  sharp terminal text with blur off, visibly diffused text with blur on.

**Limits:** Mission Control, Stage Manager, mixed-scale multi-display Spaces and
games that take exclusive display capture were not tested. An exclusive-capture
game cannot be overlaid, as on any platform.

### 6. Packaging and release — in progress

Done:

- `packaging/macos/`: `Info.plist` (`LSUIElement`), icon generated from the
  existing vector, `.app` assembly, ad-hoc signature and a drag-to-Applications
  disk image with a checksum.
- `.github/workflows/macos.yml` builds, tests, lints, renders previews, packages
  the disk image and uploads it; the release workflow downloads the macOS
  artifact and the publish scripts include it in the release assets and notes.
- Verified locally: the bundle launches with no Dock icon, renders, and forwards
  `--toggle` to the running instance.
- The macOS job runs in the same workflow as Windows, Linux and Flatpak; the
  CI artifact is the same disk image whose checksum and signature are verified
  during packaging.

Remaining:

- Developer ID signing and notarization for a public download.
- Universal (arm64 + x86_64) binary; CI currently builds thin arm64.

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
| `src/ui/macos.rs` | New shell implementation |
| `src/device/hid/mac.rs` | New IOKit HID transport |
| `src/device/hid.rs`, `src/ui/shell.rs`, `src/ui/app/eq_profiles.rs`, `src/main.rs`, `src/ui/mod.rs` | macOS `cfg` arms and the synchronous `FileSelection::Ready` path |
| `src/config.rs`, `src/ui/draw.rs` | Preferences directory, system font and bundle asset lookup |
| `Cargo.toml` | macOS-only dependencies |
| `packaging/macos/`, `.github/workflows/macos.yml`, `tools/` | Bundle, CI and release plumbing |
