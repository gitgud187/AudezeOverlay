# Experimental builds

Experimental releases are previews for volunteers, not the recommended download.
They may have bugs, incorrect model names, failed sound controls or regressions.
Use the regular **Latest** release for the verified device path.

The experimental installer and portable ZIP contain the same application. Choose
the installer unless you prefer to extract and run the portable version. Both
are unsigned. Each prerelease links to its exact source commit and includes
SHA256SUMS.txt for verifying the two downloads. GitHub's source archives are for
developers. Experimental prereleases do not replace the normal Latest release.

## Device coverage and evidence

| VID / PID | Provisional label | Evidence |
| --- | --- | --- |
| 3329 / 4BA9 | Maxwell 2 ANC | Existing verified dongle path; prior hardware testing |
| 3329 / 4BA7 | Maxwell | Guessed model assignment and protocol compatibility |
| 3329 / 4BA8 | Maxwell 2 | Guessed model assignment and protocol compatibility |
| 3329 / 4BAA | Maxwell 2 variant | Guessed model assignment and protocol compatibility |

The additional PIDs appeared in local official-app research. That does **not**
prove their model, platform variant, transport mode, feature set or command
compatibility. At the maintainer's request, experimental builds try the existing
sound protocol on these explicitly listed identities, restricted to collection
col02, usage page FF13 and usage 1, with bounded report sizes. Other identities
remain rejected. No new hardware validation is claimed.

The three provisional profiles currently assume no ANC, sidetone range 0–31,
and microphone noise control. These are hypotheses awaiting owner testing. ANC
commands and queries are blocked on those profiles, and unavailable controls
are marked in the UI. The verified ANC profile keeps its existing behavior.
Factory and custom EQ operations retain the existing on-demand protocol.

Live EQ edits immediately write headset settings. Record your original settings
before deliberately testing controls; verify restoration afterward. Report the
model, platform variant, dongle mode and firmware with any results. Do not post
serial numbers, raw private device paths or proprietary app extracts.

## Build and validation

Normal builds omit the provisional identities. Only builds enabling the
`experimental-devices` Cargo feature use them:

```powershell
cargo build --locked --release --features experimental-devices --bin audeze-overlay
```

The experimental workflow runs formatting, Clippy, both normal and all-feature
tests, release compilation and offline preview generation before packaging.
Each check must succeed before publishing. Passing these checks proves software
behavior, not physical compatibility or anti-cheat approval.

The mix polling target remains 50 ms, background sound queries approximately
one second, battery queries 30 seconds, and EQ queries on demand. Waiting-read
experiments remain deferred. Unsupported periodic queries stay disabled for the
whole connection instead of restarting after a one-hour delay.
