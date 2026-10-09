"""Tabulate the PROP marker tail bytes against the xml attributes (bmd_data_escalation*.bin with props)."""
import collections
import os
import struct
import sys
import xml.etree.ElementTree as ET

manifest, root = sys.argv[1], sys.argv[2]
table = collections.Counter()
nfiles = 0
for line in list(open(manifest, encoding="utf-8"))[1:]:
    c = line.rstrip("\n").split("\t")
    if c[11] != "in" or "bmd_data_escalation" not in c[2] or not c[2].endswith(".bin"):
        continue
    px = os.path.join(root, c[0], c[1], c[2][:-3] + "xml")
    pb = os.path.join(root, c[0], c[1], c[2])
    try:
        props = ET.parse(px).getroot().find("PROPS")
    except ET.ParseError:
        continue
    if props is None or not len(props):
        continue
    b = open(pb, "rb").read()
    p = 10
    n = struct.unpack_from("<I", b, p)[0]
    p += 4
    if n != len(props):
        print("count mismatch", pb, n, len(props))
        continue
    nfiles += 1
    for el in props:
        m = el.find("marker")
        ln = struct.unpack_from("<H", b, p)[0]
        key = b[p + 2:p + 2 + ln].decode("latin1")
        p += 2 + ln
        assert key == el.get("key"), (key, el.get("key"))
        ver = struct.unpack_from("<H", b, p)[0]
        p += 2 + 64
        clamp, typ, allow = b[p], struct.unpack_from("<I", b, p + 1)[0], b[p + 5]
        par, sx, sy, sz = struct.unpack_from("<4f", b, p + 6)
        tail = b[p + 22:p + 26]
        p += 26
        table[(tail.hex(), key.split(chr(92))[-1][:28], m.get("type"), m.get("clamp_to_surface"), m.get("allow_in_outfield"))] += 1
print(nfiles, "files; (tail bytes, tiled, is_fauna, snow, destruction) counts:")
for k, v in table.most_common():
    print(v, k)
