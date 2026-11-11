# macOS build

[Back to documentation](README.md)

> **Status: experimental.** Apple silicon only. The overlay, controls, status
> item, native menus and file panels, backdrop blur, launch at login and single
> instance work on the tested setup below. Treat anything outside it as
> unverified.

## Install

The build is **arm64**, ad-hoc signed and not notarized. From a checkout:

```sh
cargo build --release
packaging/macos/package-macos.sh
open dist/Audeze-Overlay-macOS-arm64.dmg
```

Drag “Audeze Overlay” to Applications and eject the disk image. Gatekeeper may
ask you to open it from the context menu, or allow it under System Settings →
Privacy & Security. The app has no Dock icon; quit it from the menu bar icon.

- `--demo` shows sample headset data and never opens the device.
- `--toggle` shows or hides the panel of an already running instance.
- No driver, udev rule or Accessibility permission is needed.
- Launch at login uses `SMAppService` and needs macOS 13 or newer. It requires
  the packaged `.app`; an unbundled binary reports an error.
- The global shortcut is not available yet. Use the menu bar icon or `--toggle`.
- Requires macOS 11 or newer. Supported headset identities are the same as on
  Windows and Linux; see [Device profiles](DEVICE_PROTOCOL.md).

## How it works

- **Device access:** `src/device/hid/mac.rs` uses `IOHIDManager`/`IOHIDDevice`
  directly. macOS publishes each interface's report descriptor, so the shared
  descriptor reader finds the vendor control collection. Reports are read and
  written on demand, as with hidraw and `HidD_GetInputReport`, and hot-plug
  events wake the worker with the same debounce as Linux. The manager stays open
  while a device is open, because closing it closes its devices.
- **Window:** `src/ui/macos.rs` implements the portable `Shell` with a borderless
  `NSWindow` above ordinary and Space-fullscreen apps, a layer showing the
  CPU-rasterized frame, and repeating `CFRunLoop` timers that stop when idle.
  Event positions are converted to the app's logical units at the shell edge.
- **Blur:** an `NSVisualEffectView` behind the overlay, shaped with a small
  cap-inset mask image. It is on or off, like the Linux compositor blur.
- **Single instance:** the socket module shared with Wayland, with the socket in
  `~/Library/Application Support/AudezeOverlay`. A very long home path exceeds
  the 104-byte socket limit and fails startup instead of allowing two instances.
- **Preferences and fonts:** preferences live in Application Support. Fonts are
  tried in order San Francisco, Arial, Verdana; each is parsed before it is
  chosen.

## Hardware and display evidence

An Audeze Maxwell Xbox dongle (`3329:4b18`, headset and dongle firmware
v1.0.1.74) returned a live snapshot (battery, mix, sidetone, EQ index). Mix,
noise filter, sidetone on/off and level each changed, read back and were restored
to the original state, and repeated unplug/replug cycles re-enumerated the
dongle. The overlay was checked on one Retina display above a fullscreen
terminal. This is one device, firmware and display, not a broader claim.

## Limits

- Mission Control, Stage Manager, a second display and other headset models or
  firmware were not tested.
- Mixed-scale multi-display coordinates are not modeled consistently: window
  geometry follows the window's scale while monitor work areas follow each
  screen's. Treat a Retina plus 1× external display as unsupported.
- Games that capture the display exclusively cannot be overlaid, as on any
  platform.
- Global shortcut: Carbon registration worked, but its callback ran while the UI
  state was borrowed and aborted the process, so it is disabled. It needs a
  deferred dispatch or an `NSEvent` monitor (which needs Accessibility).
- No Developer ID signing, notarization or universal binary yet.
- Bluetooth connection status is Windows-only.
- File filters use the deprecated `setAllowedFileTypes`; `UTType` would replace it.
