#!/usr/bin/env python3
"""How does TEd format the numbers in tile.ted attributes? Classifies position / orientation / scale / key values.

    python ted_formats.py <manifest.tsv> <kits_parent_dir> [max_mb]
"""
import collections
import os
import re
import sys
import xml.etree.ElementTree as ET

manifest, root = sys.argv[1], sys.argv[2]
max_mb = float(sys.argv[3]) if len(sys.argv) > 3 else 6
pat = collections.defaultdict(collections.Counter)
example = {}


def shape(v):
    s = re.sub(r"-?\d+\.\d{6}(?!\d)", "F6", v)
    s = re.sub(r"-?\d+e[-+]?\d+", "E", s)
    s = re.sub(r"-?\d+\.\d+(e[-+]?\d+)?", "G", s)
    s = re.sub(r"-?\d+", "I", s)
    return s


for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] != "in" or os.path.basename(c[2]) != "tile.ted" or int(c[3]) > max_mb * 1e6:
        continue
    try:
        r = ET.parse(os.path.join(root, c[0], c[1], c[2])).getroot()
    except ET.ParseError:
        continue
    ver = r.get("version")
    for e in r.iter():
        for a in ("position", "orientation", "scale", "bounds_colour", "colour", "line_colour", "area", "radius", "opacity", "flag_position", "flag_facing"):
            v = e.get(a)
            if v is not None:
                k = (e.tag if a in ("position", "orientation") and e.tag == "point" else "*", a, ver)
                sh = shape(v)
                pat[(k[1], k[0])][sh] += 1
                example.setdefault((k[1], k[0], sh), v)
for key, cnt in sorted(pat.items()):
    print(key)
    for sh, n in cnt.most_common(5):
        print(f"   {n:9} {sh:50} e.g. {example[(key[0], key[1], sh)]}")
