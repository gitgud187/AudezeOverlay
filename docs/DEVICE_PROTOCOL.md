# Device profiles and validation

[Back to documentation](README.md)

## Coverage in this development branch

The current Audeze App targets Maxwell, Maxwell 2 and Maxwell 2 ANC. This branch
recognizes their PlayStation and Xbox USB identities. This does not include older
HQ-only products such as Mobius and Penrose, nor a Bluetooth control transport.

The Maxwell 2 ANC PlayStation dongle (`3329:4ba9`) has been tested on hardware.
On 11 October 2026, wired `4baa` status reads and mix restoration were also checked.
Other identities and wired EQ writes remain experimental.
Compilation, synthetic packets and sample-data previews do not validate hardware.

| Model | Xbox dongle | PS dongle | PS wired | Xbox wired |
| --- | --- | --- | --- | --- |
| Maxwell | `4b18` | `4b19` | `4b1a` | `4b1e` |
| Maxwell 2 | `4b28` | `4b29` | `4b2a` | `4b2e` |
| Maxwell 2 ANC | `4ba8` | `4ba9` | `4baa` | `4bae` |

All use vendor `3329`, HID usage page `ff13`, usage `1`. Product ranges alone
are not accepted: for example, `4ba7` is not in the allowlist. Ambiguous multiple
models/interfaces are rejected. A single wired endpoint takes precedence over
the matching model/platform's dongle. This is a control-transport choice, not
proof of the current Windows audio output or which wireless headset is paired.

Common sound controls and preset selection have model-aware dispatch. ANC is
available only on the ANC family. Custom EQ editing currently remains ANC-only:
the other families use a different coefficient/bank persistence protocol, which
must not be replaced with ANC biquad commands. They can select existing presets.

## Evidence and encoding

These are independently recorded observations from static inspection of the
installed official Windows application. No proprietary code is distributed.
Binary SHA-256:
`88299c36990f44de4ce17a5b9c65b5521fd68ee649fdb83e80ccf530ad604ca5`.

Relevant backend locations in that binary:

- `0x140028120`: model enumeration; `0x14002ffd0` and table `0x140324af0`:
  explicit twelve-product allowlist; `0x14002f9d0`: platform/interface mapping.
- `0x14001e2a0`: transport stores whether the endpoint is a dongle. Output report
  route is `80` for a dongle, `00` for a wired headset; replies must match.
- `0x1400198f0`: common sound initialization and ANC-family-only queries.
- `0x140021650`: sidetone maximum query `05 5a 04 00 01 09 2d 00`.
  The range is read once on connection. Unknown ranges do not become 31 by default;
  only the already verified `4ba9` retains its known 31-step fallback.
- `0x1400189e0`: ANC preset reads use the user-EQ command; other models use PEQ.
  `0x14001ec60` reads PEQ with `05 5a 04 00 01 09 00 00`; the value is byte 9.
  `0x14001ea50` selects PEQ with `05 5a 05 00 00 09 00 00 index`.
  PEQ indices are 1–10; ANC user indices are 0–9. UI bindings and the ANC-to-PEQ
  conversion in the backend independently agree on this offset.
- `0x1400220e0`: battery query; `0x1400460e0`: percentage getter. The observed
  path exposes a percentage, without a substantiated charging flag.
- `0x140022a90`, `0x140014ca0`, `0x140025180`: non-ANC bank reads, conversion and
  persistence differ substantially from ANC. Custom writes remain disabled.

These observations establish implementation evidence, not device validation.
The USB worker preserves bounded replies, coalescing, a 50 ms mix target,
approximately 1 s sound reads, 30 s battery reads, and on-demand EQ. Unsupported
queries are excluded. Windows device-change events trigger re-enumeration rather
than adding a periodic device-discovery loop.

## Cable changes and battery status

A read-only hardware capture reproduced Game/Chat changing from 20/80 to 50/50
when the cable was attached, with the overlay closed. This is a headset mode
transition, not an application command. The overlay now remembers a recent mix
across a short cable transition on the tested PS ANC identities. After reading
50/50 it restores the previous mix once, with ordinary command verification.
A newer local mix command or a different physical value cancels recovery.
Long disconnections and other models do not replay remembered settings.

Cable presence is tracked separately from the open control endpoint: firmware
can leave control traffic on the dongle while a cable is attached. Notifications
trigger enumeration; ordinary polling remains unchanged. A known topology change
uses a 100 ms reconnect delay instead of the ordinary two-second retry. Recovery
cannot prevent the firmware reset or guarantee an inaudible transition.

The status row distinguishes wireless and wired control paths. A plug inside the
battery icon means a matching USB cable is connected to this PC, not that charging
current has been measured. The percentage is marked with `~` while plugged in.
Captured firmware percentages rose from 84% to 88% immediately after attachment;
there is no validated correction formula or charging bit. Failed reads clear the
percentage. A wall charger cannot be detected through PC USB enumeration.

Bluetooth-only status uses Windows' existing Classic Bluetooth connection cache
and exact known display names. It never starts discovery, pairs a device, enables
sound commands based on a name, or claims to identify the active audio route.
Checks happen at startup, device notifications and manual opening, not on every
mix poll. LE Audio, renamed devices, phone connections and Bluetooth battery
telemetry are not covered. This status path has not been tested with a paired
headset on this PC; it is display-only and experimental.

## Portable EQ profiles

Audeze App's `saveEqToFile` at `0x1400458f0` and `loadEqFromFile` at
`0x140045ae0` describe `.pnreq` files: ten signed one-byte graphic gains.
They contain no profile name, frequencies or Q. Import checks the size and gain
range and preserves the loaded destination bank's frequency and Q values.
This file-format observation is static evidence; no original exported file was
available for an end-to-end interoperability test.

Overlay JSON files are versioned and retain names plus all ten validated bands.
Files contain data only, never device commands or a destination slot. Import
only adds to the local library. Applying a saved profile requires choosing a
supported custom slot and confirming live preview. Export and Save as new do
not write headset settings. Save updates the associated local profile and uses
the existing ordered headset readback; a failed headset confirmation is reported
separately from a successful local save.

## Hardware validation still required

For each family/platform/interface, record firmware, identity, stable reads and
disconnect/reconnect behaviour. With the owner's explicit permission, check
controls and readbacks, then restore the original sound settings and EQ selection.
Do not describe all additional models, charging telemetry or Bluetooth control
as verified. The tested cable recovery restored 20/80; a brief firmware reset was
audible before restoration.
