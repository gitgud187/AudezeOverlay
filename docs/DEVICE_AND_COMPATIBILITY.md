# Device integration and compatibility

[Back to the project](../README.md)

## Performance and compatibility


See the [measured native-app comparison](PERFORMANCE.md) and reproduce it
with `tools/compare-performance.ps1`.

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
value uses a fast 110 ms transition. A newly shown dial starts at the received value.
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
