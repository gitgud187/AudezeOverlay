# Linux

[Back to documentation](README.md)

> **Status: work in progress.** The Linux build currently provides the device
> transport and the command-line diagnostics. The overlay window, tray icon and
> shortcut are not implemented yet, so there is nothing to launch as an
> application. This page describes what exists and is verified; it will grow as
> the rest lands.

## What works today

The headset protocol, device selection and polling worker are shared with
Windows. Linux adds a transport that talks to the kernel's **hidraw** interface
directly: no driver, no daemon and no libudev. It can read the supported status
fields, send the supported sound commands and notice a dongle being plugged in or
removed.

## Hardware evidence

Only one device has been tested on Linux:

| Device | Firmware | Result |
| --- | --- | --- |
| Audeze Maxwell, Xbox dongle (`3329:4b18`) | headset and dongle v1.0.1.74 | Status reads, game/chat mix, noise filter, sidetone on/off, sidetone level and EQ preset selection were each changed, read back and restored. An unplug and replug was detected and the dongle was found again. |

Tested on Fedora 44 (kernel 7.2) under KDE Plasma, with a person at the headset.
The unit tests use synthetic fixtures and are not hardware evidence. Nothing here
establishes support for other models, wired use, PlayStation dongles, other
firmware, or Maxwell 2 and Maxwell 2 ANC (including ANC and custom EQ editing) on
Linux. The same identities remain experimental on Windows; see the
[validation matrix](DEVICE_PROTOCOL.md).

## Requirements

- **Linux 5.11 or newer.** Input reports are fetched on demand with the
  `HIDIOCGINPUT` request, which older kernels lack. An older kernel is reported as
  such rather than as a lost connection.
- **A udev rule** that lets your user open the headset's `/dev/hidraw` node. Most
  distributions make these nodes root-only.
- A graphical session managed by systemd-logind (the rule grants access to the
  user who is logged in at the seat).

## Allowing access to the headset

Install the rule from this repository once, then reconnect the dongle:

```sh
sudo install -m 0644 packaging/linux/70-audeze-overlay.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=hidraw --action=change
```

The rule lists only the twelve USB product IDs the application recognizes
(`docs/DEVICE_PROTOCOL.md`). It uses `TAG+="uaccess"`, so access goes to the
logged-in user through an ACL and ends with the session; it does not make the
device world-writable. To undo it, delete the file and run
`sudo udevadm control --reload`.

Without the rule the application reports "Permission denied opening the
headset's /dev/hidraw device" rather than behaving as if no headset were
connected.

## Building and checking

```sh
cargo test --locked --lib
cargo build --locked --release --bin verify-direct
```

`verify-direct` is a diagnostic. With no arguments it only **reads**:

```sh
target/release/verify-direct              # one read-only snapshot
target/release/verify-direct --worker     # run the real polling worker for 4 s
```

`--controls-and-restore [--eq-select]` **changes your headset's settings** (mix,
noise filter, sidetone, sidetone level and, with the flag, the EQ preset), reads
each back and restores it, then compares the final state with the original. The
changes are audible, and changing the sidetone level switches sidetone on; the
diagnostic switches it back off. Run it only while you are at the headset.

## Behavior of the original Maxwell worth knowing

These are observations from the hardware above, not documented guarantees:

- **Status replies take about 130 ms**, longer than the worker's 100 ms timeout
  for most queries. Late replies are still parsed, but when the first sidetone
  range query misses its deadline the worker stops asking for the sidetone level,
  so that value is often blank. The identical trace was recorded on Windows. It is
  an existing behavior and has not been changed here.
- **Changing the sidetone level turns sidetone on.** Code that sets the level
  must not assume the on/off state is unchanged.
- Sidetone toggles take about 0.7 s to read back as changed.
