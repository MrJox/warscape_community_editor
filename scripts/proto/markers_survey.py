#!/usr/bin/env python3
"""Survey of .markers headers over the corpus: repository / class names, the bytes between them, class tags.

    python markers_survey.py <manifest.tsv> <kits_parent_dir>
"""
import collections
import os
import struct
import sys


def str16(b, i):
    if i + 2 > len(b):
        return None
    n = struct.unpack_from("<H", b, i)[0]
    if not (3 <= n <= 80) or i + 2 + 2 * n > len(b):
        return None
    c = b[i + 2:i + 2 + 2 * n]
    if all(c[k + 1] == 0 and 32 <= c[k] < 127 for k in range(0, len(c), 2)):
        return c[::2].decode(), i + 2 + 2 * n
    return None


manifest, root = sys.argv[1], sys.argv[2]
kinds = collections.defaultdict(collections.Counter)
firstfile = {}
for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] != "in" or not c[2].endswith(".markers"):
        continue
    b = open(os.path.join(root, c[0], c[1], c[2]), "rb").read()
    name = os.path.basename(c[2])
    s1 = str16(b, 0)
    if not s1 or s1[0] != "BASE_MARKER_REPOSITORY":
        kinds[name]["BAD HEADER " + b[:16].hex()] += 1
        continue
    # find the class name string after the repository header
    i = s1[1]
    j = i
    while j < min(len(b) - 2, i + 80) and not str16(b, j):
        j += 1
    s2 = str16(b, j)
    hdr = b[i:j].hex(" ")
    cls = s2[0] if s2 else "?"
    after = b[s2[1]:s2[1] + 40].hex(" ") if s2 else ""
    kinds[name][(cls, hdr, after[:60])] += 1
    firstfile.setdefault((name, cls), c[0] + "/" + c[2])
for name, cnt in sorted(kinds.items()):
    print("==", name, sum(cnt.values()))
    for k, n in cnt.most_common(6):
        print("  ", n, k)
