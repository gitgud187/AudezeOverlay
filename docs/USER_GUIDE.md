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
- Sidetone: switch on/off and drag its level slider (range read from the headset).
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
their corner handles. Growing a card pushes neighbours and expands the panel
within the available screen space. Finishing layout editing closes unused gaps
and trims edges unless **Cards & layout → Keep empty space** is enabled.
**Appearance → Interface scale** is the
separate option for making the cards themselves larger or smaller.

In layout editing, click a card's **… → Remove card** to hide it. Use **+**
beside the grid icon to add it back; the window makes room if needed. Hidden
cards are managed here rather than duplicated in Settings. Choose **… → Minimal
mode — slider** on the mix card for a compact readout. Short mix cards also use
the slider automatically so their controls remain readable. Choose **… → Mini EQ
editor when there is room** to tune a custom ANC preset inside a larger EQ card.
This provides live sliders with Save/Discard, without audio capture or a visualizer.
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
| Cards & layout | Keep empty space, edit layout, reset layout |
| Notifications | Automatic popups, only changed cards, expandable notification types, hold/fade durations, preview |
| Shortcuts | Record the show/hide hotkey |
| Equalizer | Quick-switch pins, factory-preset visibility, custom names, ten-band tuning and headset save |
| Startup & device | Launch with Windows, monitor, edge snapping, position lock, low-battery threshold, reset position, Reset app |

Opacity changes by 5 percentage points; timing by 5 milliseconds (1000 ms =
1 second). Zero fade duration hides immediately. Zero low-battery threshold
disables that alert. Text stays solid when opacity changes. Blur softens the
scene behind transparent areas using Windows composition. Settings pages use
the same outer opacity, card opacity and blur preferences as the sound overlay. Turn it off for
clear transparency with strength 0. The strength slider adjusts the acrylic tint;
Windows controls the blur radius. **Animate controls** enables brief dial, number,
button and card transitions; disabled means immediate updates. The panel and acrylic
use matching native rounded corners; Windows may draw a soft shadow around them.

Record one key or **Alt / Ctrl / Shift + one key**. Escape cancels. Conflicts
preserve the previous shortcut. Click **Save shortcut** to apply the candidate or
**Discard** to cancel. Leaving the page with a recording prompts for Save/Discard
or Cancel; losing focus no longer silently drops it. Windows reserves F12.

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
or both; choose these independently in layout mode. In layout mode,
**+ → Status row under logo** adds the fixed row and its **…** menu removes it.
The row reserves its own space and cannot be dragged or resized. The **+**
button is visible only while editing the layout.

## Equalizer


Add **Equalizer** with the **+** button in layout mode. Its preset button
opens your pinned quick switches; **Edit** opens the same editor as
**Settings → Equalizer**. Six factory presets are read-only. Four custom slots
load their ten existing bands directly from the ANC headset (32 Hz to 16 kHz).
Experimental non-ANC profiles currently support selection but not custom curve editing.

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

When a model connects, default custom labels are imported from the installed Audeze App's
local settings for that model and platform, when available. No native app
process is needed and its files are never modified. Click a custom name to
rename it; Enter or clicking elsewhere saves, Escape cancels. Overlay renames
are preserved. Names and pins are PC-local preferences, not headset data.
Use the pin buttons to choose quick switches, and **Include factory EQs** to
hide/show factory choices in both the menu and Equalizer settings.

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
Disable **Appearance → Animate controls** for immediate transitions.

### Files and saved profiles

**Profiles → Import from file** adds a profile to this PC's library without
changing the headset. It accepts Overlay `.json` files and Audeze `.pnreq` files.
For `.pnreq`, first load a custom slot: the file stores only ten gain values, so
its existing frequencies and Q are retained. Malformed or out-of-range files are
rejected before any device write.

Choose a library profile and **Load into selected custom slot** to preview it.
Confirm the destination: the headset has four custom slots, while the local
library can hold up to 128 independent profiles. **Save** keeps the tuning on the
headset and saves or updates its associated library entry. **Discard** restores
the slot's pre-edit curve. **Save as new** creates a separately named library copy;
it does not overwrite the previous library entry or select another headset slot.
Live edits have already affected the current slot, so Discard is still needed if
you want that slot restored after making a library copy.

**Export current tuning** or a library profile's **Export profile** saves a
portable JSON file including its name, frequency, gain, Q and filter types.
The file can be imported on another PC. Removing a library entry leaves headset
slots and exported files alone. Reset app also clears the local profile library;
export any profiles you want to keep first.

### Cable and battery status

The tested Maxwell 2 ANC PS model can reset mix during a USB mode change. The
overlay restores the recent mix once after a short cable transition, but cannot
prevent firmware's initial reset or guarantee a silent change. Other controls
are refreshed rather than overwritten.

A battery plug icon means a USB cable is connected to this PC. `~` marks the
firmware's estimate while plugged in; it can be biased by charging voltage.
Actual charging current and charge completion are not reported reliably.
Bluetooth-only status reflects Windows' cached connection status for recognized
names; USB is still required for sound controls. It is experimental, does not
cover LE Audio or phone connections, and refreshes when you open the overlay or
Windows reports a device change.

## Connection troubleshooting

Audio and headset controls use different USB interfaces. Working audio does not
necessarily mean the headset is answering battery, mix or EQ requests. **No data**
means the recognized device is present but its controls are not replying; the
overlay retries automatically. Connection reminders clear when live readings return.

For repeated dongle disconnects on Windows, Audeze recommends unplugging the
dongle, restarting **Phone Service** in Windows Services, then reconnecting the
dongle. If a Windows tuning tool disabled this service, it must be enabled before
it can start. See [Audeze's dongle troubleshooting guide](https://support.audeze.com/hc/en-us/articles/25560352180365-Maxwell-Dongle-Issues-Step-By-Step-Troubleshooting-PC-MAC-Xbox).
The overlay does not change Windows services. This troubleshooting step is not
a guarantee that every connection issue has the same cause.
