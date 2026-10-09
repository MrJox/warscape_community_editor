#!/usr/bin/env python3
"""Prototype of TEd's building_list.xml export from tile.ted, compared with the shipped file.

    python export_buildings.py <tile raw dir> <tile working dir> [unit_scale] [cells_height]
"""
import math
import numpy as np
import re
import sys
import xml.etree.ElementTree as ET


def vec(s):
    return [float(v) for v in re.fullmatch(r"\w+\((.*)\)", s).group(1).split(",")]


def f6(x):
    return "%.6f" % x


def walk(e, chain, out):
    for ch in e:
        if ch.tag == "children":
            walk(ch, chain, out)
        elif ch.tag == "BUILDING_INSTANCE":
            out.append((ch, chain))
        else:
            walk(ch, chain + [ch], out)


def quat_from_euler_y(a):
    h = np.float32(np.float32(a) * np.float32(0.5))
    return (0.0, float(np.sin(h)), 0.0, float(np.cos(h)))


raw, work = sys.argv[1], sys.argv[2]
unit = float(sys.argv[3]) if len(sys.argv) > 3 else 2.0
size = float(sys.argv[4]) if len(sys.argv) > 4 else 1792.0
root = ET.parse(raw + "/tile.ted").getroot()
out = []
walk(root.find("scene"), [], out)
lines = ["<BATTLEFIELD_BUILDING_LIST>\r\n\r\n"]
for e, chain in out:
    p = [np.float32(v) for v in vec(e.get("position"))]
    for c in chain:
        if c.get("position"):
            q = [np.float32(v) for v in vec(c.get("position"))]
            p = [np.float32(p[i] + q[i]) for i in range(3)]
    x, y, z = np.float32(p[0] / np.float32(unit)), p[1], np.float32(np.float32(size) - np.float32(p[2] / np.float32(unit)))
    ori = vec(e.get("orientation"))
    q = quat_from_euler_y(ori[0])
    lines.append(
        "\t\t<BATTLEFIELD_BUILDING key='%s'  position_x='%s'  position_y='%s'  position_z='%s'  position_type='lf_relative'  "
        "orientation_i='%s'  orientation_j='%s'  orientation_k='%s'  orientation_w='%s'  scale_x='1.000000'  scale_y='1.000000'  scale_z='1.000000'  "
        "starting_damage='0.000000'  on_fire='false'  start_disabled='false'  weak_point='false'  ai_breachable='true'  indestructible='%s'  "
        "dockable='true'  toggleable='%s'  lite='false' >\r\n<BATTLEFIELD_BUILDING_INFO></BATTLEFIELD_BUILDING_INFO></BATTLEFIELD_BUILDING>\r\n"
        % (e.get("key"), f6(x), f6(y), f6(z), f6(q[0]), f6(q[1]), f6(q[2]), f6(q[3]), e.get("indestructible"), e.get("toggleable", "false")))
mine = "".join(lines)
CRLF = chr(13) + chr(10)
LF = chr(10)
ref = open(work + "/building_list.xml", "rb").read().decode("latin1").replace(CRLF, LF)
mine = mine.replace(CRLF, LF)
print(len(mine), len(ref))
# compare entry by entry
me = mine.split("\t\t<BATTLEFIELD_BUILDING ")[1:]
re_ = ref.split("\t\t<BATTLEFIELD_BUILDING ")[1:]
same = sum(1 for a, b in zip(me, re_) if a == b)
print("entries identical:", same, "of", len(re_))
shown = 0
for a, b in zip(me, re_):
    if a != b and shown < 4:
        shown += 1
        da = dict(re.findall(r"(\w+)='([^']*)'", a))
        db = dict(re.findall(r"(\w+)='([^']*)'", b))
        print({k: (da[k], db.get(k)) for k in da if da[k] != db.get(k)})
print("tail ref:", repr(ref[-120:]))

# mismatching orientations: show euler vs expected quaternion
shown = 0
for (e, chain), b in zip(out, re_):
    db = dict(re.findall(r"(\w+)='([^']*)'", b))
    ori = vec(e.get("orientation"))
    if (ori[1] != 0 or ori[2] != 0) and shown < 8:
        shown += 1
        print("euler", e.get("orientation"), "-> quat", [db[k] for k in ("orientation_i", "orientation_j", "orientation_k", "orientation_w")])
