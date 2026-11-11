# Maxwell Bluetooth protocol

**Status:** protocol notes only. The overlay does not use this channel yet; its
Bluetooth support is still limited to Windows connection status.

## Scope

Verified on **Maxwell 1 Xbox**, paired as `Audeze Maxwell BT`, with headset
firmware **v1.0.1.74** reported by the headset itself.

macOS `system_profiler` shows firmware `1.0.0` for the Bluetooth device, which
differs from the headset's own version. We read it as a Bluetooth device field,
not the headset firmware (`I`).

**Maxwell 2 and Maxwell 2 ANC are untested.** Their frame format may differ.
Do not describe them as supported from this document.

Maxwell 1 has no ANC. Earcup balance is not exposed for this model. The
Wireless/BT mix is a USB-dongle feature, not this control channel.

## Evidence key

- **A** — frame or value taken from the decompiled Android app.
- **M** — sent to / received from the headset on macOS and checked.
- **I** — inferred from an app enum or mapping, not independently tested.
- **O** — observed on macOS with a read-only probe (see
  [Observed behavior](#observed-behavior)). Timings are from one unit, one Mac
  and a handful of runs, not a benchmark.

All read frames below are `A/M`: the frame came from the app and was replayed
from macOS. Semantic mappings such as “value 2 means Xbox” are `I` even though
the raw value is `M`.

## Transport

Classic Bluetooth **SPP over RFCOMM**. Not BLE. The control channel works while
the headset is paired and in range; an active A2DP audio connection is not
required.

| Item | Value |
| --- | --- |
| SPP service UUID | `00000000-0000-0000-0099-AABBCCDDEEFF` |
| Service name | `Airoha_APP` |
| RFCOMM channel on this unit | 21 (resolve via SDP; do not hardcode it) |

macOS exposes Classic Bluetooth through **IOBluetooth**, not CoreBluetooth.
The helper/app needs `NSBluetoothAlwaysUsageDescription`; the first access
triggers the Bluetooth permission prompt. A stable build prompts once.

## Framing

SPP is a byte stream. Integers are little-endian.

Request:

```
05 5A  len_lo len_hi  cmd_lo cmd_hi  payload...
```

`len = 2 + payload.len()`; total frame length is `4 + len`.

Responses:

```
05 5B  len_lo len_hi  cmd_lo cmd_hi  payload...
05 5D  len_lo len_hi  cmd_lo cmd_hi  payload...
```

`05 5B` is an immediate response/ACK; `05 5D` is a notification carrying the
value. A query can return both. Parse by scanning for a header, reading `len`,
and consuming `4 + len` bytes.

Battery example:

```
TX 05 5A 03 00 D6 0C 00
RX 05 5B 03 00 D6 0C 00
RX 05 5D 05 00 D6 0C 00 00 3C      # 0x3C = 60%
```

## Read commands

| Field | Request | Response | Evidence |
| --- | --- | --- | --- |
| Game/Chat mix | `05 5A 04 00 83 2C 0B 00` | `05 5B 06 00 83 2C 00 0B 00 <0..20>` | A/M; 0–20 mapping I |
| Sidetone | `05 5A 04 00 83 2C 07 00` | `05 5B 06 00 83 2C 00 07 00 <0|1>` | A/M |
| Sidetone level | `05 5A 04 00 01 09 2C 00` | `05 5B 06 00 01 09 2C 00 00 <level>` | A/M |
| Sidetone max | `05 5A 04 00 01 09 2D 00` | `05 5B 06 00 01 09 2D 00 00 <max>` | A/M; max 31 M |
| Battery | `05 5A 03 00 D6 0C 00` | ACK then `05 5D 05 00 D6 0C 00 00 <percent>` | A/M |
| AI noise removal | `05 5A 04 00 01 09 24 00` | `05 5B 06 00 01 09 24 00 00 <0|1|2>` | A/M; Off/Low/High mapping I |
| Mic mute state | `05 5A 04 00 01 09 22 00` | `05 5B 06 00 01 09 22 00 00 <0|FF>` | A/M; `00` muted I |
| Voice-prompt level | `05 5A 04 00 01 09 20 00` | `05 5B 06 00 01 09 20 00 00 <level>` | A/M |
| Auto-shutdown | `05 5A 04 00 83 2C 01 00` | `05 5B 0D 00 83 2C 00 01 00 01 00 <sec1 LE> 01 00 <sec2 LE>` | A/M; layout I |
| Headset type | `05 5A 04 00 01 09 2B 00` | `... 2B 00 00 <1|2>` | A/M raw; PS/Xbox mapping I |
| Connection | `05 5A 04 00 01 09 26 00` | `... 26 00 00 02` | A/M raw; `02` with Bluetooth alone and `03` with the dongle also connected O; meaning I |
| Firmware version | `05 5A 03 00 07 1C 00` | ACK then `05 5D 0E 00 07 1C 00 00 09 <ASCII version>` | A/M |

This unit’s firmware response is:

```
05 5D 0E 00 07 1C 00 00 09 76 31 2E 30 2E 31 2E 37 34
```

The trailing bytes are ASCII `v1.0.1.74`.

Mix values are 0–20 in 5% steps: 10 = 50/50, 12 = 40% game / 60% chat.
Auto-shutdown values are seconds; the two timers sit at full-frame indices
11–12 and 15–16. The headset type/connection mappings are inferred from the app
enums; only Xbox/Bluetooth was present during testing.

## Write commands

All writes below were replayed from macOS with read-back and restoration
(`A/M`). No mic mute write is documented: it is a physical switch on Maxwell 1,
so only its state is read.

These are the only frames this document considers understood. Do not send other
commands, and do not build a raw "send any bytes" control. Airoha-based devices
generally accept further commands (firmware update among them), none of which
were examined or tested here. An implementation must whitelist the frames above
and bound the `len` field it accepts when parsing.

| Field | Frame | Evidence |
| --- | --- | --- |
| Game/Chat mix | `05 5A 05 00 82 2C 0B 00 <0..20>` | A/M |
| Sidetone on/off | `05 5A 05 00 82 2C 07 00 <0|1>` | A/M |
| Sidetone level | `05 5A 05 00 00 09 2C 00 <0..max>` | A/M |
| AI noise removal | `05 5A 05 00 00 09 24 00 <0|1|2>` | A/M |
| Voice-prompt level | `05 5A 05 00 00 09 20 00 <level>` | A/M |
| Auto-shutdown 60 min | `05 5A 0C 00 82 2C 01 00 01 00 10 0E 01 00 10 0E` | A/M |
| Auto-shutdown 5 min | `05 5A 0C 00 82 2C 01 00 01 00 2C 01 01 00 2C 01` | A/M |

Verified write/read-back examples:

| Action | Write | Read-back |
| --- | --- | --- |
| Sidetone on | `05 5A 05 00 82 2C 07 00 01` | `... 07 00 01` |
| Mix 60/40 | `05 5A 05 00 82 2C 0B 00 0C` | `... 0B 00 0C` |
| Sidetone level 20 | `05 5A 05 00 00 09 2C 00 14` | `... 2C 00 00 14` |
| Noise Low | `05 5A 05 00 00 09 24 00 01` | `... 24 00 00 01` |
| Voice prompt 4 | `05 5A 05 00 00 09 20 00 04` | `... 20 00 00 04` |

## Observed behavior

All `O`. Measured with a read-only probe on macOS that sent only the read
frames above. Nothing was written to the headset.

| Topic | Observation |
| --- | --- |
| Connect | About 400 ms to open the channel once the headset is connected; MTU 667. |
| Reads | A single read is answered in about 40–60 ms. |
| Concurrency | Twelve reads sent back-to-back with no spacing were all answered, in about 240 ms. |
| Chunking | One receive can hold several frames (two were seen in a single chunk), so a parser must buffer the stream and split on `len`. |
| Ordering | `5D` notifications (battery, firmware) can arrive after the `5B` responses of commands sent later. Match responses by command, not by arrival order. |
| Unsolicited traffic | None during normal use. Mic mute switch, both wheels and their buttons, and a power-button tap produced nothing on this channel in a 90 s window. The tap only sent a media key over the audio link. State changes made on the headset therefore have to be polled. |
| Power-off | Powering the headset off closed the channel; macOS reported `CHANNEL CLOSED` within seconds. |
| Power-on | The channel reopened by itself about 15 s later, and a fresh connection worked without re-pairing. One unrequested frame arrived about a second after the reopen: `05 5C 05 00 82 2C 03 00 00`. Its meaning is unknown. |
| Unknown headers | `05 5C` is a third header type beside `5B` and `5D`. A parser must skip unknown frames by `len` and must not treat them as errors. The same header also appears in the USB dongle's report stream, where the existing parser already discards it. |
| Dongle also connected | With the USB dongle plugged in and the headset connected to the Mac over Bluetooth at the same time, both channels answered every read and returned identical values. Bluetooth reads kept their 40–60 ms latency while the dongle was queried three times in parallel. The two links are not mutually exclusive. Only reads were tested: simultaneous writes over both were not. |
| Connect time with dongle | The channel opened in about 700 ms with the dongle connected and about 300 ms without it (one run each, so treat the difference as unconfirmed). |
| `connection` value | The connection read returned `03` while the dongle was also connected and `02` with Bluetooth alone, in two back-to-back runs on the same unit. The numbers are observed; reading `03` as "dongle also connected" is an inference. |

Read-only values seen on this unit: battery 60 % then 59 % (`0x3C`, `0x3B`),
firmware `v1.0.1.74`, mix 10, sidetone off with level 22 of 31, AI noise removal
Low, mic mute `FF`, voice prompt 5, auto-shutdown 3600 s for both timers, headset
type 2 and connection 2.

Not yet observed: any write acknowledgement frame (the `5C` header may be one,
but that is unconfirmed), behavior with two Bluetooth clients connected, writes
sent over Bluetooth and the dongle at once, and behavior when the headset goes
out of range instead of being powered off.

## Triggering frames from this Mac

Use a small AppKit helper around **IOBluetooth**. It must be a bundled app with
`NSBluetoothAlwaysUsageDescription`; a plain command-line process may be killed
by TCC before it reaches the device.

1. Pair the Maxwell in System Settings.
2. Enumerate `IOBluetoothDevice.pairedDevices()` and select the exact device
   name, not a substring match.
3. Resolve the SPP UUID with `getServiceRecord(for:)`, then
   `getRFCOMMChannelID:`.
4. Open with `openRFCOMMChannelSync:withChannelID:delegate:`.
5. Send frame bytes with `writeSync:length:`.
6. Collect `rfcommChannelData:data:length:`, split frames on
   `05 5B`/`05 5D`, and log the response.

Example frames to pass in:

```text
055a0300d60c00          # battery
055a0300071c00          # firmware version
055a0400832c0b00        # mix
055a0500822c070001      # sidetone on
```

If a reusable command-line sender is useful, add it as a Rust diagnostic under
`src/bin/` rather than embedding a full Swift probe here. The doc is limited to
the protocol and its evidence.
