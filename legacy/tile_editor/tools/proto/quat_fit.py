import math, re, sys
import xml.etree.ElementTree as ET
import numpy as np
F = np.float32

def vec(s): return [float(v) for v in re.fullmatch(r"\w+\((.*)\)", s).group(1).split(",")]
def walk(e, out):
    for ch in e:
        if ch.tag == "BUILDING_INSTANCE": out.append(ch)
        else: walk(ch, out)

def qmul(a, b):
    x1,y1,z1,w1=a; x2,y2,z2,w2=b
    return (F(w1*x2+x1*w2+y1*z2-z1*y2), F(w1*y2-x1*z2+y1*w2+z1*x2), F(w1*z2+x1*y2-y1*x2+z1*w2), F(w1*w2-x1*x2-y1*y2-z1*z2))
def qaxis(axis, a):
    h = F(F(a) * F(0.5)); s, c = F(np.sin(h)), F(np.cos(h))
    q = [F(0), F(0), F(0), c]; q[axis] = s; return tuple(q)

def to_matrix(q):
    x, y, z, w = [F(v) for v in q]
    return np.array([[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
                     [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
                     [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]], dtype=np.float32)

def from_matrix(m):
    t = F(m[0,0] + m[1,1] + m[2,2])
    if t > 0:
        s = F(F(0.5) / F(np.sqrt(F(t + 1)))); w = F(F(0.25) / s)
        x = F((m[2,1]-m[1,2]) * s); y = F((m[0,2]-m[2,0]) * s); z = F((m[1,0]-m[0,1]) * s)
    elif m[0,0] > m[1,1] and m[0,0] > m[2,2]:
        s = F(2 * F(np.sqrt(F(1 + m[0,0] - m[1,1] - m[2,2]))))
        w = F((m[2,1]-m[1,2]) / s); x = F(F(0.25) * s); y = F((m[0,1]+m[1,0]) / s); z = F((m[0,2]+m[2,0]) / s)
    elif m[1,1] > m[2,2]:
        s = F(2 * F(np.sqrt(F(1 + m[1,1] - m[0,0] - m[2,2]))))
        w = F((m[0,2]-m[2,0]) / s); x = F((m[0,1]+m[1,0]) / s); y = F(F(0.25) * s); z = F((m[1,2]+m[2,1]) / s)
    else:
        s = F(2 * F(np.sqrt(F(1 + m[2,2] - m[0,0] - m[1,1]))))
        w = F((m[1,0]-m[0,1]) / s); x = F((m[0,2]+m[2,0]) / s); y = F((m[1,2]+m[2,1]) / s); z = F(F(0.25) * s)
    return (x, y, z, w)

def euler_to_quat(o):
    a, b, c = o
    r = qmul(qmul(qaxis(2, c), qaxis(1, a)), qaxis(0, -b))
    return from_matrix(to_matrix(r))

raw, work = sys.argv[1], sys.argv[2]
out = []; walk(ET.parse(raw + "/tile.ted").getroot().find("scene"), out)
ents = list(ET.parse(work + "/building_list.xml").getroot().iter("BATTLEFIELD_BUILDING"))
exact = 0; bad = []
for e, ent in zip(out, ents):
    o = vec(e.get("orientation")); q = euler_to_quat(o)
    ref = [ent.get(k) for k in ("orientation_i","orientation_j","orientation_k","orientation_w")]
    mine = ["%.6f" % float(v) for v in q]
    mine = [("-0.000000" if m == "0.000000" and math.copysign(1, float(v)) < 0 else m) for m, v in zip(mine, q)]
    if mine == ref: exact += 1
    elif len(bad) < 8: bad.append((o, mine, ref))
print("exact", exact, "of", len(out))
for b in bad: print(b)
