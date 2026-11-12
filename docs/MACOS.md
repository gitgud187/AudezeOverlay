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

## Bluetooth control (experimental)

With no USB dongle or cable connected, the original Maxwell can be controlled over
its Bluetooth connection. It is off until you turn it on: pair the headset in
System Settings, then choose **Bluetooth control (experimental)** in the tray menu.
macOS asks for Bluetooth permission the first time.

- **What works:** the status readings, sidetone and its level, and noise
  removal. Nothing else is sent: no ANC, EQ or firmware commands, and the
  microphone switch is only read. Other models are not controlled over Bluetooth.
- **No game/chat mix:** the mix balances the two audio devices the dongle and a USB
  cable present. Bluetooth carries one audio stream, so there is nothing to
  balance; the card says it is not available. The headset's game/chat wheel works
  the sidetone over Bluetooth: a double-click toggles it and a press then scroll
  changes its level, as the headset's guide describes. Changing the sidetone level
  turns sidetone on, which is the headset's own behavior.
- **USB wins:** with the dongle or a cable connected, Bluetooth is not opened at
  all. Plugging one in while connected over Bluetooth switches to USB, and turning
  the menu item off disconnects. The dongle drops off and reappears when the headset
  connects to it, so Bluetooth waits four seconds after USB disappears before taking
  over; a brief gap does not cause a hop to Bluetooth and back.
- **Polling:** the headset never reports changes, so they are read: the sound
  settings about once a second and the battery every 30 seconds.
- **Implementation:** `src/device/bluetooth/` opens the headset's serial service
  through IOBluetooth. IOBluetooth reports events on the main thread's run loop, so
  the channel lives there and the worker reaches it through that loop. The frames
  are the same as over USB, so the shared parser is reused through the `Link`
  interface in `src/device/link.rs`. The link refuses any frame outside a short
  allow-list, whatever the caller asks for.

The frames, their evidence and what was observed are in
[the Bluetooth protocol notes](BLUETOOTH_PROTOCOL.md).

If the headset refuses the connection and only a power cycle helps, another
Bluetooth client is probably holding its control channel. While the app is
ad-hoc signed, macOS asks for Bluetooth permission again after each update.

## Hardware and display evidence

An Audeze Maxwell Xbox dongle (`3329:4b18`, headset and dongle firmware
v1.0.1.74) returned a live snapshot (battery, mix, sidetone, EQ index). Mix,
noise filter, sidetone on/off and level each changed, read back and were restored
to the original state, and repeated unplug/replug cycles re-enumerated the
dongle. The overlay was checked on one Retina display above a fullscreen
terminal. This is one device, firmware and display, not a broader claim.

Over Bluetooth, one Maxwell Xbox (firmware v1.0.1.74, paired as "Audeze Maxwell
BT") returned all readings through the app's worker, and sidetone, sidetone level
and noise removal each changed and read back. The mix also read back when written,
but does nothing over Bluetooth and is no longer offered. Sidetone takes about a
second to switch: turning it on and sending off again straight after left it on
until off was sent once more after it had settled. The channel stopped opening
for a while after several aborted test runs and recovered when the headset was
power-cycled. Plugging the dongle in switched the worker from Bluetooth to the
dongle, losing the dongle switched it back, and turning Bluetooth control off and
on disconnected and reconnected it within a fraction of a second. Those switches
were driven through the worker API, not by clicking the tray menu item. This is
one unit; other models are untested over Bluetooth.

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
- Bluetooth control is limited to the original Maxwell and is experimental.
  Windows shows Bluetooth connection status only.
- File filters use the deprecated `setAllowedFileTypes`; `UTType` would replace it.
