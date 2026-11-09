# macOS port review — decisions

This branch, `macos-port-review`, starts at `05f6ae4` on `macos-port`. It reviews
the whole port against `upstream/main` (`777adee`), looking for correct,
minimal implementation rather than new scope. `macos-port` itself is left
untouched.

## Method

- Read the complete `upstream/main...HEAD` diff, including the shared `cfg` edits.
- Compare the AppKit and IOKit code with the Windows and Wayland contracts it
  implements: ownership, error paths, timers, input, modals and shutdown.
- Check that cross-platform changes are necessary and that platform code does
  not duplicate shared logic.
- Verify with `cargo fmt`, `clippy -D warnings`, all-target tests, a packaged
  bundle and focused synthetic input where the change is input-related.

The review only changes something when it removes a bug, a leak, avoidable
allocation, duplicated logic, or an unmet requirement.

## Decisions

### Keep the direct IOKit HID transport

**Decision:** Keep `src/device/hid/mac.rs` on `IOHIDManager`/`IOHIDDevice`
instead of adding a HID crate or a helper daemon.

**Why:** The shared descriptor reader already finds the vendor collection, and
the on-demand GET/SET report model matches the Windows and Linux transports.
The transport has hardware evidence for reads, writes, restore and replug.

**Consequences:** The small amount of IOKit FFI stays, but no new runtime
dependency or privilege is introduced.

### Classify all device-removal IOKit codes

**Decision:** Add `kIOReturnNoDevice` (`0xE00002C0`), `kIOReturnOffline`
(`0xE00002D7`) and `kIOReturnNotAttached` (`0xE00002D9`) to `disconnected`.

**Why:** The SDK header confirms these values, and `kIOReturnNoDevice` is the
common result when a dongle is removed. It previously fell through to
`USB sound command failed` instead of the worker's reconnect path.

**Consequences:** Unplug is reported as a lost connection. A unit test covers
the removal codes and keeps permission/exclusive errors out of that set.

### Translate macOS Delete, select-all and function keys correctly

**Decision:** In `key_input`, treat `U+007F`/`U+0008` as
`text_typed(app, '\u{8}')`, decide Select All from
`charactersIgnoringModifiers` (Control+A arrives as `U+0001` in `characters`),
and drop the `U+F700..=U+F8FF` private-use range used for arrows, Home/End,
forward-delete and function keys. Do not insert text for unhandled Command
shortcuts.

**Why:** AppKit reports Delete as `U+007F`, which `char::is_control()` filters
out. Control+A only looks like `'a'` in the unmodified character, so the first
test passed a combination AppKit never produces. Navigation and function keys
are private-use characters, not control characters, so they reached EQ-name
drafts as junk.

**Consequences:** Numeric and EQ-name editing now matches the other platforms,
including deletion; navigation keys are ignored; Command+A works. Tests cover
DEL, `U+0001` Control+A, Command+A, function/arrow codes and a Command shortcut
that must not insert text.

### Shape the blur with a cap-inset mask image

**Decision:** Give `NSVisualEffectView` a small rounded-corner `NSImage` with
`capInsets` set to the corner radius and `resizingMode` stretch. Do not rely on
a layer corner radius, and do not build a full-size mask.

**Why:** With the display unlocked, an exact window capture showed the effect
layer still opaque outside the overlay's rounded corner; the layer radius did
not clip the behind-window material. Apple documents `maskImage` as the
supported shape and recommends a small mask with cap insets. A full-size mask
also did not apply correctly; the corner image with insets does.

**Consequences:** The mask is rebuilt only when the clip diameter or backing
scale changes, not per frame. The screen check shows the blur corner matching
the overlay curve; the effect still makes the interior opaque, as expected.

### Remove the transient allocation from asset lookup

**Decision:** Keep the existing array/find shape in `draw::asset` and add the
bundle candidate as a cfg-selected `Option<PathBuf>`, instead of building a
`Vec` and pushing into it.

**Why:** The bundle path is resolved the same way on every asset lookup. An
array of three `Option<PathBuf>` values finds the first existing file without a
heap allocation for the container.

### Release the retained HID device if the listener cannot start

**Decision:** Give `SendDevice` ownership of its retained reference and release
it on `Drop`; consuming it for the callback thread disarms that drop. If the
thread does not report ready, set its stop flag before returning the error.

**Why:** `Listener::open` retained the device before spawning, but a failed
`thread::Builder::spawn` dropped the raw pointer without releasing it. A thread
that started but did not report ready also stayed detached with no stop signal.
Both paths are rare, but they are the only extra device owners besides `Node`.

### Frame unnumbered output reports

**Decision:** Split the shared report buffer in `output_report`: numbered
reports keep the ID byte in the payload; report ID 0 strips it and passes zero
as the IOKit report ID. A one-byte unnumbered buffer is rejected.

**Why:** IOKit's `IOHIDDeviceSetReport` takes the report ID separately and its
header says a numbered report also carries the ID in the buffer. For report ID 0
there is no ID byte, as hidapi handles it. The verified Maxwell protocol uses
numbered reports, but the transport should not send an extra byte if an
unnumbered control report appears.

**Consequences:** A unit test covers numbered, unnumbered and empty framing.
The input path still assumes numbered reports, so an unnumbered input report
remains hardware-unverified.

### Reuse `Config::path()` for the macOS instance socket

**Decision:** Derive the macOS socket directory from
`Config::path().parent()` rather than repeating the
`HOME/Library/Application Support/AudezeOverlay` join. Keep the `Instance`
owner in `run`, clone its listener for the accept thread, and drop the owner
after `app.run()` so a normal quit unlinks `overlay.sock`.

**Why:** The settings file and the socket must live in the same per-user support
directory. Moving the owner into the never-ending listener thread meant its
`Drop` never ran, so the socket stayed behind after quit. The stale-socket path
handled it, but cleanup belongs to the owner.

**Consequences:** Normal quit removes the socket; a crash can still leave one.
The comment now names Application Support and its 104-byte `sun_path` limit, so
an extremely long home path fails startup rather than silently allowing two
instances.

### Share launch-argument parsing

**Decision:** Add one `ui::parse_args` helper for `--demo` and `--toggle` and use
it from both the Wayland and macOS entry points.

**Why:** The two `run` functions contained an identical option validator and
request mapping. Keeping one copy guarantees the same accepted options and error
text on both platforms.

### Add the requested DMG installer

**Decision:** Produce a drag-to-Applications `Audeze-Overlay-macOS-arm64.dmg`
from the signed `.app` and publish it instead of the zip. Update the CI artifact,
release preparation, publisher, contract tests and documentation to the DMG
name. The disk image contains the app and an `Applications` symlink; it is
created with `hdiutil` in UDZO format.

**Why:** The original port request explicitly called for a DMG installer. The
zip-only implementation was incomplete for normal macOS installation. The DMG
is the smallest complete installer story and does not require a separate
installer package or signing identity.

**Consequences:** One macOS download instead of two; release assets stay at six
files (five packages plus the combined checksum manifest). The app remains
ad-hoc signed and not notarized.

### Fail packaging if ad-hoc signing fails

**Decision:** Make `codesign --sign -` a hard failure in `package-macos.sh`
instead of printing a warning and continuing.

**Why:** The README and release notes promise an ad-hoc signed bundle. Uploading
a silently unsigned bundle would make the documented artifact and the archive
disagree.

**Consequences:** A CI runner without a working `codesign` fails the macOS
packaging step rather than publishing an unusable archive.

### Correct the documentation, history and review trail

**Decision:** Fix the macOS file table to name `src/ui/macos.rs`, document the
DMG install, add this review file to the documentation index, update `AGENTS.md`
for macOS and six release assets, and restore the original zip-only packaging
entries in `DECISIONS.md` with a dated review follow-up. Keep this review file
tracked as a project design record.

**Why:** The table named a non-existent `src/ui/macos/` directory. `AGENTS.md`
still described a Windows/Linux product with five release assets. Editing the
original packaging entry to mention the DMG made the two decision logs tell
different histories.

**Consequences:** `DECISIONS.md` remains the original port record and
`DECISIONS_REVIEW.md` records the superseding review changes; the follow-up
entry names the DMG. The `.gitignore` entries remain limited to scratch review
notes.

### Clear the callback state pointer on every exit

**Decision:** Replace the manual clear after `app.run()` with a `StateGuard` that
clears the thread-local pointer on normal return, early error and unwinding. The
guard is declared after `State`, so it runs first.

**Why:** The instance-listener setup can fail after the pointer was installed
but before `app.run()`. The early `?` returned while `STATE` still pointed into
the dropped `State` box. The process did exit immediately, but leaving a dangling
callback pointer depended on that accident.

### Defer focus-loss handling out of modal loops

**Decision:** Call `super` from `resignKeyWindow`, then schedule a
`focusLostLater:` selector with `performSelector:withObject:afterDelay:` in the
default run-loop mode. The deferred method returns early if the window is key
again; otherwise it clears the pending resize and notifies the shared app.

**Why:** `confirm`, `choose_file` and `popup_menu` run modal AppKit loops while
the shared app is borrowed as `&mut App`. When a modal takes key status,
`resignKeyWindow` fired and entered `with_state`, creating a second mutable
borrow of `State` while the first was live. Deferring to the post-modal run-loop
turn avoids the alias; timers already use the default mode, so they do not fire
inside these modal loops. Skipping `super` also left NSWindow's key-window
bookkeeping and `NSWindowDidResignKeyNotification` undone. The key check avoids
committing a draft when focus returned before the deferred call.

**Consequences:** Focus and pointer-capture loss are delivered one run-loop turn
later on normal deactivation; an edit committed after a modal is not committed
again if the overlay is key. The order change (commit after the modal rather
than before it) remains, but the stale-focus path no longer commits.

### Wake AppKit after stopping the run loop

**Decision:** After `NSApplication::stop`, post an application-defined
`NSEvent` with `postEvent_atStart`.

**Why:** `stop` takes effect after the current event. A timer or run-loop source
is not an event, so AppKit's `nextEvent` can remain blocked until unrelated
input arrives. This path is currently only used by `--render-only`, which the
macOS argument parser does not accept, but the `Shell` implementation should
still behave correctly.

### Add direct tests for the key translation

**Decision:** Put the AppKit character mapping in a small pure `key_input`
function and test DEL, real Control+A (`U+0001`), Command+A, arrow/function-key
private-use characters, and a Command shortcut that must not insert text.

**Why:** It exercises the actual regressions without depending on synthetic
keyboard focus and a running window server.

## Deliberately unchanged

- **Raw IOKit and `STATE` pointer:** the FFI is confined to two modules, and
  removing it would require a much larger event-loop rewrite. The callbacks run
  on the main thread and do not re-enter `with_state`; the pointer is cleared
  before `State` drops.
- **CPU rasterization and per-frame `CGImage`:** this is the portable rendering
  path and is necessary for the layer presentation. A reusable pixel provider
  would be a separate rendering optimization, not a review cleanup.
- **No screen-parameter observer:** `displays_changed` remains allowed dead on
  macOS. Adding an observer is new behavior that needs display hardware evidence.
- **Mixed-DPI coordinate spaces:** `window_rect`/`place` use the window's
  backing scale while `screen_work` uses each screen's scale. On a Retina display
  plus a 1× external display these are not one coordinate space. This is a
  design gap, not just missing evidence; fixing it needs a canonical coordinate
  model and hardware to verify. It remains an open item below.
- **Cursor shape:** diagonal resize uses the crosshair because macOS has no
  diagonal resize cursor, and the app does not install tracking rects; AppKit may
  override the cursor on the window edge. Cosmetic, not a correctness issue.
- **Timers in default run-loop mode:** animation pauses during native window
  drags and menu tracking. That is acceptable and also keeps timers out of the
  modal loops that triggered the focus-loss deferral.
- **No global shortcut:** the Carbon callback re-entered the borrowed UI state
  and aborted. Revisiting it needs a deferred callback design; it is not part of
  this cleanup.
- **No universal binary or notarization:** both need credentials or a rustup
  toolchain that this environment does not have.

## Verification

- `cargo fmt --all --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --all-targets`, including the new key-translation tests
  in the binary target
- `packaging/macos/package-macos.sh`, then mount the DMG and verify the
  checksum, `codesign --verify --deep --strict`, the `Applications` symlink and
  a successful launch.
- Launch the bundled app, confirm the socket at
  `~/Library/Application Support/AudezeOverlay/overlay.sock`, and confirm a
  second `--toggle` exits 0 while the first instance stays alive.
- CI must pass the Windows, Linux, Flatpak and macOS jobs.

Local result: formatting and clippy pass. All **82** lib tests and **42** binary
tests pass, including the new HID removal/framing tests and the key-translation
tests. The packaged `Audeze-Overlay-macOS-arm64.dmg` verifies its SHA-256,
mounts, contains the `Applications` symlink, and its app passes strict signature
verification. The bundled app created the expected socket and forwarded
`--toggle` with exit 0.

Runtime checks: the display was unlocked for this pass. Exact window captures
with blur off and on showed the rounded overlay corner, then the blur still
filling outside it; after switching to the cap-inset mask, the corner image
matched and the blur's outside-corner alpha matched the blur-off value. A
synthetic click inside the overlay, outside it, and back exercised
`resignKeyWindow` (including the `super` call) without a crash. The modal
staleness path was not forced interactively; it is covered by the key check and
code review.

Normal-quit socket unlinking uses the existing `Instance` drop path (covered by
the instance tests) but was not exercised through the UI; the process was
stopped externally after the smoke tests.

CI result: run `42708533527` passed the Windows build, Linux check, Flatpak and
macOS check jobs, including the new HID and key tests and DMG
packaging/upload. The first macOS attempt was cancelled before a runner was
assigned; the rerun passed. The release job skipped as expected on a branch.

## Open items

- Mixed-DPI coordinate modeling across Retina and 1× displays.
- Developer ID signing and notarization.
- Universal (arm64 + x86_64) bundle.
- Edge-resize and cursor end-to-end exercise.
- Second display, Mission Control/Stage Manager and additional headset evidence.
