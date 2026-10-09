import itertools, math, re, sys
import xml.etree.ElementTree as ET
import numpy as np

def vec(s): return [float(v) for v in re.fullmatch(r"\w+\((.*)\)", s).group(1).split(",")]
def qmul(a, b):
    x1,y1,z1,w1=a; x2,y2,z2,w2=b
    return (w1*x2+x1*w2+y1*z2-z1*y2, w1*y2-x1*z2+y1*w2+z1*x2, w1*z2+x1*y2-y1*x2+z1*w2, w1*w2-x1*x2-y1*y2-z1*z2)
def qaxis(axis, a):
    s, c = math.sin(a/2), math.cos(a/2)
    q = [0,0,0,c]; q[axis] = s; return tuple(q)

def walk(e, out):
    for ch in e:
        if ch.tag == "BUILDING_INSTANCE": out.append(ch)
        else: walk(ch, out)

raw, work = sys.argv[1], sys.argv[2]
out = []; walk(ET.parse(raw + "/tile.ted").getroot().find("scene"), out)
ents = list(ET.parse(work + "/building_list.xml").getroot().iter("BATTLEFIELD_BUILDING"))
rows = []
for e, ent in zip(out, ents):
    o = vec(e.get("orientation")); q = [float(ent.get(k)) for k in ("orientation_i","orientation_j","orientation_k","orientation_w")]
    rows.append((o, q))
best = []
for perm in itertools.permutations(range(3)):          # euler component k rotates about axis perm[k]
    for signs in itertools.product((1, -1), repeat=3):
        for order in itertools.permutations(range(3)):  # multiplication order of the three rotations
            err = 0; same = 0
            for o, q in rows[:400]:
                parts = [qaxis(perm[k], signs[k]*o[k]) for k in range(3)]
                r = parts[order[0]]
                for k in order[1:]: r = qmul(r, parts[k])
                d = min(max(abs(a-b) for a, b in zip(r, q)), max(abs(a+b) for a, b in zip(r, q)))
                err += d; same += d < 2e-6
            best.append((same, err, perm, signs, order))
best.sort(key=lambda t: (-t[0], t[1]))
for b in best[:6]: print(b)

print("--- sign analysis for the best combination")
perm, signs, order = (1, 0, 2), (1, -1, 1), (2, 0, 1)
plus = minus = 0
bad = []
for o, q in rows:
    parts = [qaxis(perm[k], signs[k]*o[k]) for k in range(3)]
    r = parts[order[0]]
    for k in order[1:]: r = qmul(r, parts[k])
    dp = max(abs(a-b) for a, b in zip(r, q)); dm = max(abs(a+b) for a, b in zip(r, q))
    if dp < 2e-6: plus += 1
    elif dm < 2e-6:
        minus += 1
        if len(bad) < 6: bad.append((o, r, q))
print("same sign", plus, "opposite sign", minus, "of", len(rows))
for o, r, q in bad: print(o, [round(v,6) for v in r], q)
