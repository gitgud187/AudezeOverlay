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

### Translate macOS Delete to the shared Backspace character

**Decision:** In `key_down`, treat `U+007F`/`U+0008` as `text_typed(app, '\u{8}')`,
the same character Windows `WM_CHAR` and Wayland `Keysym::BackSpace` deliver.
Treat Command+A like Control+A for Select All, and do not insert text for
unhandled Command shortcuts.

**Why:** AppKit reports the Delete key as `U+007F`, which `char::is_control()`
filters out. The shared number and profile-name drafts only understand `U+0008`,
so Backspace did nothing and a value could only be replaced by selecting all and
typing over it. Command+A is the macOS convention.

**Consequences:** Numeric and EQ-name editing now behaves like the other
platforms, including deletion. This was not caught by the earlier typed-value
check because it entered a value by replacing the selection.

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

### Reuse `Config::path()` for the macOS instance socket

**Decision:** Derive the macOS socket directory from
`Config::path().parent()` rather than repeating the
`HOME/Library/Application Support/AudezeOverlay` join.

**Why:** The settings file and the socket must live in the same per-user support
directory. One source of truth avoids drift if the preferences location changes.

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

### Correct the documentation and review trail

**Decision:** Fix the macOS file table to name `src/ui/macos.rs`, document the
DMG install, and add this review file to the documentation index.

**Why:** The table named a non-existent `src/ui/macos/` directory. Installation
instructions must match the shipped artifact.

### Clear the callback state pointer on every exit

**Decision:** Replace the manual clear after `app.run()` with a `StateGuard` that
clears the thread-local pointer on normal return, early error and unwinding. The
guard is declared after `State`, so it runs first.

**Why:** The instance-listener setup can fail after the pointer was installed
but before `app.run()`. The early `?` returned while `STATE` still pointed into
the dropped `State` box. The process did exit immediately, but leaving a dangling
callback pointer depended on that accident.

### Clear a pending edge resize when focus is lost

**Decision:** Reset the resize drag in `resignKeyWindow` before notifying the
shared app that focus and pointer capture were lost.

**Why:** A mouse-up that never reaches the view would otherwise leave a stale
frame and edge in `RESIZE`; a later drag could resize from the old frame.

### Add direct tests for the key translation

**Decision:** Put the AppKit character mapping in a small pure `key_input`
function and test DEL, Backspace, Command/Control+A and a Command shortcut that
must not insert text.

**Why:** It exercises the actual regression without depending on synthetic
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
  macOS. Multi-display and mixed-scale behavior are already documented limits;
  adding an observer is new behavior that needs display hardware evidence.
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

Local result: formatting, clippy and all tests pass. The packaged
`Audeze-Overlay-macOS-arm64.dmg` verifies its SHA-256, mounts, contains the
`Applications` symlink, and its app passes strict signature verification. The
bundled app created the expected socket and forwarded `--toggle` with exit 0.
The display was locked, so this pass did not repeat the visual overlay check;
the earlier rendering evidence and the unchanged presentation path still apply.

## Open items

- Developer ID signing and notarization.
- Universal (arm64 + x86_64) bundle.
- Edge-resize and cursor end-to-end exercise.
- Second display, Mission Control/Stage Manager and additional headset evidence.
