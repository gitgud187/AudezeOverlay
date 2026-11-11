#!/bin/sh
# Runs the read-only Bluetooth diagnostic (macOS) and prints its output:
#   cargo build --release --bin probe-bluetooth
#   tools/run-bluetooth-probe.sh [--device NAME] [--idle SECONDS] \
#       [--poll-ms 250,100] [--poll-seconds N]
#
# macOS aborts a bare command-line process at its first Bluetooth call, so the
# binary runs inside a throwaway app bundle launched with `open`. The first run
# of a new signature asks for Bluetooth permission. Ad-hoc signing (the default)
# asks again after every rebuild; set AUDEZE_SIGN_IDENTITY to the name of a
# code-signing identity in your keychain to keep one permission across rebuilds.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
binary=$root/target/release/probe-bluetooth
[ -x "$binary" ] || { echo "Build it first: cargo build --release --bin probe-bluetooth" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/Bluetooth Probe.app"
mkdir -p "$app/Contents/MacOS"
install -m755 "$binary" "$app/Contents/MacOS/probe-bluetooth"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>probe-bluetooth</string>
	<key>CFBundleIdentifier</key>
	<string>io.github.temdah.AudezeOverlay.BluetoothProbe</string>
	<key>CFBundleName</key>
	<string>Bluetooth Probe</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>LSUIElement</key>
	<true/>
	<key>NSBluetoothAlwaysUsageDescription</key>
	<string>Reads status values from the headset's Bluetooth control channel. It never changes a setting.</string>
</dict>
</plist>
PLIST
codesign --force --sign "${AUDEZE_SIGN_IDENTITY:--}" "$app" >/dev/null

output="$work/output.txt"
: > "$output"
open -W -n "$app" --stdout "$output" --stderr "$output" --args "$@"
cat "$output"
