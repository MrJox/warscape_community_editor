#!/usr/bin/env python3
"""DDS header survey: group by (flags, caps, pixel format, mips) and check file size against the computed payload.

    python dds_survey.py <manifest.tsv> <kits_parent_dir>
"""
import collections
import os
import struct
import sys


def payload(w, h, mips, fourcc, bitcount, flags_pf):
    total = 0
    for m in range(max(1, mips)):
        mw, mh = max(1, w >> m), max(1, h >> m)
        if fourcc in (b"DXT1",):
            total += max(1, (mw + 3) // 4) * max(1, (mh + 3) // 4) * 8
        elif fourcc in (b"DXT3", b"DXT5", b"ATI2", b"BC5U"):
            total += max(1, (mw + 3) // 4) * max(1, (mh + 3) // 4) * 16
        elif fourcc and fourcc[0] < 200 and fourcc[1:] == b"\x00\x00\x00":      # D3DFMT numeric (e.g. 114 = R32F)
            total += mw * mh * {114: 4, 111: 2, 113: 8, 112: 4, 115: 8, 116: 16}.get(fourcc[0], 4)
        else:
            total += mw * mh * max(1, bitcount // 8)
    return total


def main():
    manifest, root = sys.argv[1], sys.argv[2]
    groups = collections.defaultdict(list)
    for line in list(open(manifest, encoding="utf-8"))[1:]:
        c = line.rstrip("\n").split("\t")
        if c[11] != "in" or not c[2].lower().endswith(".dds"):
            continue
        with open(os.path.join(root, c[0], c[1], c[2]), "rb") as f:
            h = f.read(148)
        if h[:4] != b"DDS ":
            groups[("not DDS",)].append(c[2])
            continue
        (size, flags, hh, ww, pitch, depth, mips) = struct.unpack_from("<7I", h, 4)
        pf = struct.unpack_from("<8I", h, 76)
        caps = struct.unpack_from("<4I", h, 108)
        fourcc = struct.pack("<I", pf[2])
        dx10 = fourcc == b"DX10"
        extra = 20 if dx10 else 0
        expect = 128 + extra + payload(ww, hh, mips, fourcc, pf[3], pf[1])
        key = (hex(flags), hex(pf[1]), fourcc if pf[1] & 4 else "rgb%d" % pf[3], hex(caps[0]), "mips" if mips > 1 else "nomips", "size ok" if expect == int(c[3]) else "size?")
        groups[key].append((c[2], ww, hh, mips))
    for k, v in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        print(len(v), k, v[0])


if __name__ == "__main__":
    main()
