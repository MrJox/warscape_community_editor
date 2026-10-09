#!/usr/bin/env python3
"""Prototype of TEd's building_list.xml export with full parent transforms; compares with the shipped file.

    python exporter_proto.py <tile raw dir> <tile working dir> [unit_scale] [tile_size_m]
"""
import math
import re
import sys
import xml.etree.ElementTree as ET

import numpy as np

F = np.float32


def vec(s):
    return [float(v) for v in re.fullmatch(r"\w+\((.*)\)", s).group(1).split(",")]


def qmul(a, b):
    x1, y1, z1, w1 = a
    x2, y2, z2, w2 = b
    return (F(w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2), F(w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2),
            F(w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2), F(w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2))


def qaxis(axis, a):
    h = F(F(a) * F(0.5))
    s, c = F(np.sin(h)), F(np.cos(h))
    q = [F(0), F(0), F(0), c]
    q[axis] = s
    return tuple(q)


def euler_quat(o):
    a, b, c = o
    return qmul(qmul(qaxis(2, c), qaxis(1, a)), qaxis(0, -b))


def to_matrix(q):
    x, y, z, w = [F(v) for v in q]
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]], dtype=np.float32)


def from_matrix(m):
    t = F(m[0, 0] + m[1, 1] + m[2, 2])
    if t > 0:
        s = F(F(0.5) / F(np.sqrt(F(t + 1))))
        return (F((m[2, 1] - m[1, 2]) * s), F((m[0, 2] - m[2, 0]) * s), F((m[1, 0] - m[0, 1]) * s), F(F(0.25) / s))
    if m[0, 0] > m[1, 1] and m[0, 0] > m[2, 2]:
        s = F(2 * F(np.sqrt(F(1 + m[0, 0] - m[1, 1] - m[2, 2]))))
        return (F(F(0.25) * s), F((m[0, 1] + m[1, 0]) / s), F((m[0, 2] + m[2, 0]) / s), F((m[2, 1] - m[1, 2]) / s))
    if m[1, 1] > m[2, 2]:
        s = F(2 * F(np.sqrt(F(1 + m[1, 1] - m[0, 0] - m[2, 2]))))
        return (F((m[0, 1] + m[1, 0]) / s), F(F(0.25) * s), F((m[1, 2] + m[2, 1]) / s), F((m[0, 2] - m[2, 0]) / s))
    s = F(2 * F(np.sqrt(F(1 + m[2, 2] - m[0, 0] - m[1, 1]))))
    return (F((m[0, 2] + m[2, 0]) / s), F((m[1, 2] + m[2, 1]) / s), F(F(0.25) * s), F((m[1, 0] - m[0, 1]) / s))


def rotate(q, v):
    m = to_matrix(q)
    return [F(m[i, 0] * v[0] + m[i, 1] * v[1] + m[i, 2] * v[2]) for i in range(3)]


def walk(e, chain, out):
    for ch in e:
        if ch.tag == "children":
            walk(ch, chain, out)
        elif ch.tag == "BUILDING_INSTANCE":
            out.append((ch, chain))
        else:
            walk(ch, chain + [ch], out)


def f6(x):
    x = float(F(x))
    s = "%.6f" % x
    if s in ("0.000000", "-0.000000"):
        s = "-0.000000" if math.copysign(1, x) < 0 else "0.000000"
    return s


def main():
    raw, work = sys.argv[1], sys.argv[2]
    unit = F(float(sys.argv[3])) if len(sys.argv) > 3 else F(2.0)
    size = F(float(sys.argv[4])) if len(sys.argv) > 4 else F(1792.0)
    out = []
    walk(ET.parse(raw + "/tile.ted").getroot().find("scene"), [], out)
    mine = []
    for e, chain in out:
        pos = [F(0)] * 3
        rot = (F(0), F(0), F(0), F(1))
        for node in chain + [e]:
            lp = [F(v) for v in vec(node.get("position", "vector(0,0,0)"))]
            lo = euler_quat(vec(node.get("orientation", "euler(0,0,0)")))
            r = rotate(rot, lp)
            pos = [F(pos[i] + r[i]) for i in range(3)]
            rot = qmul(rot, lo)
        q = from_matrix(to_matrix(rot))
        x, y, z = F(pos[0] / unit), pos[1], F(size - F(pos[2] / unit))
        mine.append((e.get("key"), [f6(x), f6(y), f6(z)], [f6(v) for v in q]))
    ents = list(ET.parse(work + "/building_list.xml").getroot().iter("BATTLEFIELD_BUILDING"))
    ok_pos = ok_q = 0
    bad = []
    for (k, p, q), ent in zip(mine, ents):
        rp = [ent.get(a) for a in ("position_x", "position_y", "position_z")]
        rq = [ent.get(a) for a in ("orientation_i", "orientation_j", "orientation_k", "orientation_w")]
        ok_pos += p == rp
        ok_q += q == rq
        if (p != rp or q != rq) and len(bad) < 5:
            bad.append((k, p, rp, q, rq))
    print(f"{len(ents)} entries: positions exact {ok_pos}, orientations exact {ok_q}")
    for b in bad:
        print(b)


if __name__ == "__main__":
    main()
