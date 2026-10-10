# User guide

[Back to the project](../README.md)

## Use


**[Download the Windows installer (recommended)](https://github.com/temdah/AudezeOverlay/releases/latest/download/Audeze-Overlay-Setup.exe)**

Prefer no installation? Download the [portable ZIP](https://github.com/temdah/AudezeOverlay/releases/latest/download/Audeze-Overlay-Portable.zip),
extract it, and run **Audeze Overlay.exe**. Both options include the same app and licenses.
The installer adds a Start menu entry and an optional
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
Hold a card over another for 320 ms to preview an animated swap. Dragging past
a card does not immediately swap it; release after the preview to keep the swap. Larger cards compact to fit and
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
controls readable. Status and the other rectangular cards share the same minimum
width and height; the circular mix card keeps a larger minimum height.
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
| Cards & layout | Card visibility, status icons beneath the logo, grid rows, edit layout, reset layout |
| Notifications | Master popup switch, mix/sidetone/ANC/FILTER/mute triggers, time before hiding, fade duration, preview |
| Shortcuts | Record the show/hide hotkey |
| Equalizer | Quick-switch pins, factory-preset visibility, custom names, ten-band tuning and headset save |
| Startup & device | Launch with Windows, monitor, edge snapping, position lock, low-battery threshold, reset position, Reset app |

Opacity changes by 5 percentage points; timing by 5 milliseconds (1000 ms =
1 second). Zero fade duration hides immediately. Zero low-battery threshold
disables that alert. Text stays solid when opacity changes. Blur softens the
scene behind transparent areas using Windows composition. Settings pages use
the same outer opacity, card opacity and blur preferences as the sound overlay. Turn it off for
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

Device status can appear as a fixed model/status row beneath the logo, a card,
or both; choose these independently in **Cards & layout**. In layout mode,
**+ → Status row under logo** adds the fixed row and its **…** menu removes it.
The row reserves its own space and cannot be dragged or resized. The **+**
button is visible only while editing the layout.

## Equalizer


Add **Equalizer** with the **+** button or in Cards & layout. Its preset button
opens your pinned quick switches; **Edit** opens the same editor as
**Settings → Equalizer**. Six factory presets are read-only. Four custom slots
load their ten existing bands directly from the headset (32 Hz to 16 kHz).

Select a custom slot and drag or scroll band sliders from −12 to +12 dB to
hear changes live. Editing activates that custom slot. **Save** confirms and
keeps the final curve; **Discard** writes the pre-edit curve back and checks
that the headset restored it. **Flat** previews a zero-gain curve. **Use EQ**
activates the selected preset without editing its bands. Pending drag updates
are coalesced per band so old slider positions do not queue up.

Live preview writes the headset's custom bank immediately: it is not a separate
volatile audio effect. Keep the app open until you Save or Discard. Quitting or
losing power before Discard leaves the most recently applied curve in the
headset. Drafts remain available when switching settings pages or hiding to tray.

On startup, default custom labels are imported from the installed Audeze App's
local settings for the supported ANC dongle, when available. No native app
process is needed and its files are never modified. Click a custom name to
rename it; Enter or clicking elsewhere saves, Escape cancels. Overlay renames
are preserved. Names and pins are PC-local preferences, not headset data.
Use the pin buttons to choose quick switches, and **Include factory EQs** to
hide/show factory choices in that menu.

EQ is loaded only when opening or using EQ controls. It adds no continuous
polling; changes made elsewhere are refreshed when those controls reopen.
Writes preserve untouched bands, wait for the device acknowledgment, and verify
changed bands by reading them back. A firmware write can briefly delay other
USB requests while the headset stores the change.
Saving multiple bands yields to overdue mix reads between bands. Routine mix,
other-sound, and battery polling intervals remain unchanged.

Cards animate between grid positions. The dial expands while held. Mix digits
roll up or down in place with a slight motion blur while the slash stays still.
Button selection drains the old fill and reveals the new fill over 280 ms;
the button outline stays stationary.
Disable **Appearance → Smooth animations** for immediate transitions.
