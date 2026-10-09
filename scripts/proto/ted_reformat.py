#!/usr/bin/env python3
"""Does reformat(parse(text)) == text for the numeric attributes of tile.ted?

Rule under test: parse as float32, print with '%.6f', strip trailing zeros and a trailing '.', for the components of
vector(...), euler(...) and plain numbers; '%g' (6 significant digits) for <point position="..."> values.

    python ted_reformat.py <manifest.tsv> <kits_parent_dir> [max_mb]
"""
import collections
import os
import re
import struct
import sys
import xml.etree.ElementTree as ET


def f32(x):
    return struct.unpack("<f", struct.pack("<f", float(x)))[0]


def fmt6(x):
    s = "%.6f" % f32(x)
    s = s.rstrip("0").rstrip(".") if "." in s else s
    return s


def fmtg(x):
    return "%g" % f32(x)


def reformat(text, mode):
    if mode == "point":
        return ",".join(fmtg(v) for v in text.split(","))
    m = re.fullmatch(r"(vector|euler|argb)\((.*)\)", text)
    if m:
        return f"{m.group(1)}(" + ",".join(fmt6(v) for v in m.group(2).split(",")) + ")"
    return fmt6(text)


manifest, root = sys.argv[1], sys.argv[2]
max_mb = float(sys.argv[3]) if len(sys.argv) > 3 else 6
stats = collections.defaultdict(lambda: [0, 0])
bad = {}
for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] != "in" or os.path.basename(c[2]) != "tile.ted" or int(c[3]) > max_mb * 1e6:
        continue
    try:
        r = ET.parse(os.path.join(root, c[0], c[1], c[2])).getroot()
    except ET.ParseError:
        continue
    for e in r.iter():
        for a in ("position", "orientation", "scale", "flag_position", "flag_facing", "area", "radius", "opacity"):
            v = e.get(a)
            if v is None:
                continue
            mode = "point" if (e.tag == "point" and a == "position") else "f6"
            if e.tag == "point" and a == "position" and "," not in v:
                continue
            try:
                ok = reformat(v, mode) == v
            except ValueError:
                ok = False
            k = (e.tag if mode == "point" else "*", a)
            stats[k][0] += 1
            stats[k][1] += ok
            if not ok and k not in bad:
                bad[k] = (v, reformat(v, mode) if True else "")
for k, (n, ok) in sorted(stats.items()):
    print(f"{k!s:28} {ok:9}/{n:9} reproduced", "" if ok == n else f"  e.g. {bad.get(k)}")
