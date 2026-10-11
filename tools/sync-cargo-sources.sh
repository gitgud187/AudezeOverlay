#!/bin/sh
# Regenerates packaging/flatpak/cargo-sources.json from Cargo.lock, the way
# Flathub builds offline. CI runs this with --check and fails if the committed
# file is stale. Needs python3 with aiohttp and tomlkit (pip install aiohttp tomlkit).
#   tools/sync-cargo-sources.sh [--check]
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
# Pinned so the output is reproducible; update it deliberately.
generator_commit=${GENERATOR_COMMIT:-master}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -sSfL -o "$work/flatpak-cargo-generator.py" \
    "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$generator_commit/cargo/flatpak-cargo-generator.py"
python3 "$work/flatpak-cargo-generator.py" "$root/Cargo.lock" -o "$work/cargo-sources.json"

target=$root/packaging/flatpak/cargo-sources.json
if [ "${1:-}" = "--check" ]; then
    # Compare as sets: the generator's entry order is not guaranteed.
    python3 -I - "$work/cargo-sources.json" "$target" <<'PY' || {
import json, sys
def entries(path):
    items = json.load(open(path))
    for e in items:
        # tomlkit versions differ in whether they write an empty [source] header.
        if "contents" in e:
            e["contents"] = e["contents"].replace("[source]\n", "")
    return sorted(json.dumps(e, sort_keys=True) for e in items)
a, b = entries(sys.argv[1]), entries(sys.argv[2])
if a != b:
    print("only generated:", [e[:160] for e in a if e not in b][:5], file=sys.stderr)
    print("only committed:", [e[:160] for e in b if e not in a][:5], file=sys.stderr)
    sys.exit(1)
PY
        echo "packaging/flatpak/cargo-sources.json is out of date; run tools/sync-cargo-sources.sh." >&2
        exit 1
    }
else
    cp "$work/cargo-sources.json" "$target"
fi
