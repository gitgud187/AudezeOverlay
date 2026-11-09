# Implementation decisions

Decisions made while porting Audeze Overlay to macOS (branch `macos-port`).
Each entry records what was chosen, why, and what it constrains. Dates are the
day the decision was made.

## 2026-11-09 — Direct IOKit HID transport

**Decision:** Talk to `IOHIDManager`/`IOHIDDevice` directly instead of using the
`hidapi` macOS backend.

**Why:** macOS publishes the interface's full report descriptor, so the existing
descriptor parser and vendor-collection matching work unchanged. Direct calls
also give on-demand input reports (`IOHIDDeviceGetReport`), writes with the
report ID in the buffer, and explicit control over manager lifetime.

**Consequences:** The manager must stay open for as long as a device is open;
closing it closes the devices. Unplug teardown produces several IOReturn codes,
which are classified as a lost connection.

## 2026-11-09 — Logical event coordinates

**Decision:** Convert pointer positions with `point × backingScale ÷ ui_scale`,
matching the Wayland shell, rather than sending physical pixels to the app.

**Why:** The app's hit regions and layout are in logical units. Physical
coordinates made the gear fall outside its hit box and be treated as a caption.

**Consequences:** All shell geometry conversions are explicit; `window_rect`
and `place` use physical pixels, event points use logical units.

## 2026-11-09 — Repeating run-loop timers

**Decision:** `CFRunLoopTimerCreate` uses the requested interval as both the
first fire delay and the repeat interval.

**Why:** The app expects a timer to keep firing until it calls `stop_timer`.
One-shot timers rendered a single animation frame, so mix changes and fades
never repainted.

## 2026-11-09 — Window model

**Decision:** Borderless `NSWindow` subclass at `NSScreenSaverWindowLevel`,
`canJoinAllSpaces | fullScreenAuxiliary | stationary | ignoresCycle`, with
key/main allowed and first-mouse accepted.

**Why:** It stays above ordinary and Space-fullscreen apps while remaining
clickable. A dedicated `NSView` subclass handles mouse, wheel and keyboard.

**Consequences:** Launch starts non-key (accessory app); the first click makes
it key. Games that capture the display exclusively still cannot be overlaid.

## 2026-11-09 — Single instance and `--toggle`

**Decision:** Move the Wayland socket module to `ui/instance.rs` and use it on
both platforms. macOS puts the socket under
`~/Library/Application Support/AudezeOverlay`, uses a portable `flock`, and reads
requests with a bounded nonblocking read.

**Why:** `TMPDIR` can differ between a shell and a LaunchServices-launched app,
so a temp-directory socket is not a stable rendezvous. macOS rejects
`SO_RCVTIMEO` on Unix sockets (`EINVAL`), so a read timeout cannot be used.

**Consequences:** Requests are handed to the main run loop through the existing
wake source. Demo mode intentionally skips single-instance.

## 2026-11-09 — Status item, no global shortcut

**Decision:** Ship a status item with a native menu (Open, Settings, Reset
position, Hide, Quit). Remove the Carbon global shortcut for the first macOS
release.

**Why:** The Carbon hotkey registered and fired, but its callback ran inside an
AppKit call while UI state was borrowed, causing `RefCell already mutably
borrowed` and SIGABRT. A correct fix needs the callback to be deferred onto the
main loop; until then the feature is out of scope.

**Consequences:** `register_hotkey`/`rebind_hotkey` return false on macOS. The
shortcut-related app hooks are allowed to be unused on macOS. Revisit with a
deferred dispatch or `NSEvent` global monitor (needs Accessibility).

## 2026-11-09 — Blur via visual effect view

**Decision:** Use `NSVisualEffectView` (`.underWindowBackground`, behind-window
blending) as the window content, with the overlay view as its subview when blur
is on. When blur is off, the overlay view becomes the content view and the
effect view is hidden.

**Why:** Hiding the effect view while it is the content view also hides the
overlay. Sibling containers did not reliably draw the layer-backed overlay.
Switching content views keeps both states working.

**Consequences:** Blur is on/off, like the Linux compositor blur; strength is
not a kernel size. The settings toggle continues to work.

## 2026-11-09 — Native modals

**Decision:** Use `NSAlert` for confirmations and `NSOpenPanel`/`NSSavePanel`
(modally) for profile files. `popup_menu` remains unimplemented for now.

**Why:** The `Shell` contract is synchronous; modal AppKit calls fit it. The
deprecated `setAllowedFileTypes` is used with a local `allow(deprecated)` rather
than adding the UniformTypeIdentifiers crate.

**Consequences:** File selection returns `FileSelection::Ready`. Card/EQ popup
menus still need a native `NSMenu` implementation.

## 2026-11-09 — Launch at login

**Decision:** Use `SMAppService.mainAppService()` and refuse the feature below
macOS 13.

**Why:** It is the supported login-item API for a bundled app and shows the
standard system consent notification. Ad-hoc signing is enough for the local
bundle test.

**Consequences:** The feature requires a real `.app`; an unbundled development
binary reports an error. A public build still needs Developer ID signing.

## 2026-11-09 — Packaging and minimum OS

**Decision:** Assemble a thin host-architecture `.app` with `Info.plist`,
`LSUIElement`, template icon and ad-hoc signature; set
`LSMinimumSystemVersion` to 11.0.

**Why:** It matches the Linux archive workflow and keeps the app out of the Dock.
SMAppService is guarded at runtime, so macOS 11/12 can still run everything else.

**Consequences:** A universal binary, Developer ID signing and notarization are
follow-ups. Launch at login needs macOS 13+.

## 2026-11-09 — Fonts and preferences

**Decision:** Preferences live in `~/Library/Application Support/AudezeOverlay`.
UI fonts are tried in order San Francisco, Arial, Verdana, and each candidate is
parsed before selection.

**Why:** Stock macOS has no fontconfig; a variable SF font or an unreadable file
must not fail startup. Parsing once at startup lets the next candidate win.

## 2026-11-09 — Deferred hooks

**Decision:** Allow four cross-platform app hooks to be unused on macOS:
`displays_changed`, `pointer_capture_lost`, `focus_lost`, `hotkey_pressed`.

**Why:** They are still used by Windows/Linux and are needed when screen-change,
focus-loss and shortcut handling are wired on macOS. Suppressing them locally
keeps the macOS build clean for CI without deleting shared behavior.

## 2026-11-09 — CI and release integration

**Decision:** Add a `macos-14` workflow that runs formatting, clippy, all-target
tests, release builds, layout previews and the bundle script, then uploads the
app. The release job waits for it, downloads
`Audeze-Overlay-macOS-arm64.zip`, and includes it in the published assets and
notes.

**Why:** It mirrors the Linux workflow and keeps the macOS artifact in the same
tested run as the other packages.

**Consequences:** CI builds a thin arm64 bundle. A universal binary and
notarization are release follow-ups. The release scripts were updated but have
not run on GitHub yet.

## 2026-11-09 — Evidence limits

**Decision:** Document the tested display and hardware rather than implying
broader coverage.

**Why:** All work was verified on one Retina display and one Maxwell Xbox dongle
(`3329:4b18`, headset and dongle firmware v1.0.1.74). No second display,
Mission Control, Stage Manager, exclusive-capture game, other headset model or
other firmware was available.

**Consequences:** The support page must say that multi-display coordinates,
other identities and exclusive fullscreen remain unverified. The instance tests
use synthetic sockets, not hardware.

## Open decisions

- Revisit the global shortcut with a deferred Carbon callback or an
  `NSEvent` monitor.
- Implement native card/EQ popup menus.
- Decide whether to ship a universal binary and notarize it.
- Decide whether a second display and Stage Manager/Mission Control need
  additional coordinate handling.
- Consider replacing deprecated file-type filters with `UTType`.
