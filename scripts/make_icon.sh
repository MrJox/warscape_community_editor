#!/usr/bin/env bash
# Makes the Windows icon (packaging/windows/wce.ico, embedded in wce.exe by crates/wce_app/build.rs) from the app's SVG, the one the
# Flatpak installs. Run it again after changing the SVG and commit both.
#
#   scripts/make_icon.sh
#
# Needs rsvg-convert (librsvg), ImageMagick and Python 3. The sizes are the ones Windows asks for at 100-200% scaling (Explorer, the
# taskbar, the title bar, Alt+Tab). The small ones are 32-bit BMPs (ImageMagick's), 256 is stored as PNG, as Windows expects (a BMP of
# that size would be 270 KB).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SVG="$ROOT/packaging/flatpak/io.github.mrjox.WarscapeCommunityEditor.svg"
OUT="$ROOT/packaging/windows/wce.ico"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

images=()
for size in 16 20 24 32 40 48 64 256; do
    rsvg-convert -w "$size" -h "$size" "$SVG" -o "$tmp/$size.png"
    if [ "$size" -lt 256 ]; then
        magick "$tmp/$size.png" "$tmp/$size.ico"
        images+=("$tmp/$size.ico")
    else
        images+=("$tmp/$size.png")
    fi
done

mkdir -p "$(dirname "$OUT")"
python3 -I - "$OUT" "${images[@]}" <<'EOF'
# Joins single-image ICOs (their one image taken as it is) and PNGs into one ICO.
import struct
import sys

out, inputs = sys.argv[1], sys.argv[2:]
entries = []
for path in inputs:
    data = open(path, "rb").read()
    if data.startswith(b"\x89PNG"):
        width, height = struct.unpack(">II", data[16:24])
        entries.append((width, height, 1, 32, data))
    else:
        _, _, _, _, planes, bits, size, offset = struct.unpack("<BBBBHHII", data[6:22])
        width, height = struct.unpack("<ii", data[offset + 4 : offset + 12])
        entries.append((width, height // 2, planes, bits, data[offset : offset + size]))

header = struct.pack("<HHH", 0, 1, len(entries))
offset = 6 + 16 * len(entries)
directory = b""
for width, height, planes, bits, image in entries:
    directory += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, planes, bits, len(image), offset)
    offset += len(image)
with open(out, "wb") as f:
    f.write(header + directory + b"".join(e[4] for e in entries))
EOF
echo "Wrote $OUT"
