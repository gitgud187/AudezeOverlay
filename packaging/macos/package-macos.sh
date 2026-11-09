#!/bin/sh
# Builds the macOS app bundle from an already built binary:
#   packaging/macos/package-macos.sh [BINARY] [OUTPUT_DIR] [ARCH_LABEL]
# Defaults: target/release/audeze-overlay, dist/, and the host architecture.
# The bundle is ad-hoc signed; a public download still needs Developer ID
# signing and notarization.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
binary=${1:-$root/target/release/audeze-overlay}
out=${2:-$root/dist}
arch=${3:-$(uname -m)}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)
app="$out/Audeze Overlay.app"
archive="Audeze-Overlay-macOS-$arch.zip"

[ -x "$binary" ] || { echo "Build the binary first: $binary" >&2; exit 1; }

rm -rf "$app" "$out/$archive" "$out/$archive.sha256"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/docs"
install -m755 "$binary" "$app/Contents/MacOS/audeze-overlay"
cp -R "$root/assets" "$app/Contents/Resources/assets"
install -m644 "$root/README.md" "$root/LICENSE" "$root/THIRD-PARTY-NOTICES.md" \
    "$app/Contents/Resources/"
install -m644 "$root/docs/MACOS.md" "$app/Contents/Resources/docs/"

if command -v sips >/dev/null 2>&1 && command -v iconutil >/dev/null 2>&1; then
    work=$(mktemp -d)
    iconset="$work/AppIcon.iconset"
    mkdir -p "$iconset"
    sips -s format png "$root/assets/audeze.ico" --out "$work/base.png" >/dev/null
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" "$work/base.png" \
            --out "$iconset/icon_${size}x${size}.png" >/dev/null
        double=$((size * 2))
        sips -z "$double" "$double" "$work/base.png" \
            --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
    rm -rf "$work"
fi

sed "s/@VERSION@/$version/g" "$root/packaging/macos/Info.plist" \
    > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

if command -v codesign >/dev/null 2>&1; then
    codesign --force --sign - "$app" >/dev/null 2>&1 || \
        echo "warning: ad-hoc signing failed" >&2
fi

ditto -c -k --sequesterRsrc --keepParent "$app" "$out/$archive"
(cd "$out" && shasum -a 256 "$archive" > "$archive.sha256")
echo "Wrote $out/$archive"
