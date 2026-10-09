#!/usr/bin/env python3
"""Incremental decoder of the BATTLE_MAP_DEFINITION_DATA FASTBIN files (work in progress).

    python bmd_proto.py <file.bin>            decode what is known and hexdump the undecoded tail
"""
import struct
import sys


class R:
    def __init__(self, b, p=0):
        self.b, self.p = b, p

    def take(self, n):
        if self.p + n > len(self.b):
            raise ValueError(f"truncated at {self.p}")
        v = self.b[self.p:self.p + n]
        self.p += n
        return v

    def u8(self):
        return self.take(1)[0]

    def u16(self):
        return struct.unpack("<H", self.take(2))[0]

    def u32(self):
        return struct.unpack("<I", self.take(4))[0]

    def f32(self):
        return struct.unpack("<f", self.take(4))[0]

    def s(self):
        return self.take(self.u16()).decode("latin1")

    def f(self, n):
        return struct.unpack(f"<{n}f", self.take(4 * n))


def building(r):
    ver = r.u16()
    key, ptype = r.s(), r.s()
    pos, ori, scale = r.f(3), r.f(4), r.f(3)
    dmg = r.f32()
    flags = tuple(r.u8() for _ in range(8))
    return dict(ver=ver, key=key, ptype=ptype, pos=pos, ori=ori, scale=scale, dmg=dmg, flags=flags)


def prop(r):
    key = r.s()
    ver = r.u16()
    tr = r.f(16)
    clamp, typ, allow = r.u8(), r.u32(), r.u8()
    par = r.f(4)
    extra = r.take(4)
    return dict(key=key, ver=ver, tr=tr, clamp=clamp, type=typ, allow=allow, par=par, extra=extra)


def emitter(r):
    key = r.s()
    ver = r.u16()
    rate = r.f32()
    tr = r.f(16)
    name = r.s()
    autoplay = r.u8()
    scale = r.f32()
    flags = tuple(r.u8() for _ in range(5))
    return dict(key=key, ver=ver, rate=rate, tr=tr, name=name, autoplay=autoplay, scale=scale, flags=flags)


def capture(r):
    loc = r.f(2)
    radius = r.f32()
    mn, mx = r.u32(), r.u32()
    ctype = r.s()
    pts = [r.f(2) for _ in range(r.u32())]
    idx = [r.u32() for _ in range(r.u32())]
    key = r.s()
    facing = r.f(2)
    return dict(loc=loc, radius=radius, mn=mn, mx=mx, type=ctype, pts=pts, idx=idx, key=key, facing=facing)


def outlines(r):
    return [[r.f(2) for _ in range(r.u32())] for _ in range(r.u32())]


def parse(b):
    """Returns (model dict, error string or None). model holds what was decoded before the error."""
    r = R(b)
    m = {}
    try:
        assert r.take(8) == b"FASTBIN0"
        m["version"] = r.u16()
        m["props"] = [prop(r) for _ in range(r.u32())]
        m["emitters"] = [emitter(r) for _ in range(r.u32())]
        m["hints"] = (r.u32(), r.u32(), r.u32())
        if any(m["hints"]):
            raise ValueError("AI hint sections non-empty: layout unknown")
        m["deploy"] = (r.u16(), r.u32())
        if m["deploy"][1]:
            raise ValueError("deployment manager non-empty")
        m["buildings_ver"] = r.u16()
        m["buildings"] = [building(r) for _ in range(r.u32())]
        m["far"] = (r.u16(), r.u32())
        if m["far"][1]:
            raise ValueError("far buildings non-empty")
        m["cap_ver"] = r.u16()
        m["captures"] = [[capture(r) for _ in range(r.u32())] for _ in range(r.u32())]
        m["ef"] = r.u32()
        if m["ef"]:
            raise ValueError("EF lines non-empty: layout unknown")
        m["go"] = outlines(r)
        m["non_terrain"] = outlines(r)
        m["zones"] = (r.u16(), r.u32())
        m["prefab"] = (r.u16(), r.u32())
        m["bmd_outline"] = (r.u16(), r.u32())
        m["tail"] = (r.u32(), r.u32(), r.u32(), r.u32())
        if m["zones"][1] or m["prefab"][1] or m["bmd_outline"][1] or any(m["tail"]):
            raise ValueError(f"trailing sections non-empty {m['zones']} {m['prefab']} {m['bmd_outline']} {m['tail']}")
        if r.p != len(b):
            raise ValueError(f"{len(b) - r.p} trailing bytes at {r.p}")
    except Exception as e:  # noqa
        return m, f"{e} (at {r.p} of {len(b)})"
    return m, None


def W(fmt, *a):
    return struct.pack("<" + fmt, *a)


def ws(x):
    return W("H", len(x)) + x.encode("latin1")


def write(m):
    o = [b"FASTBIN0", W("H", m["version"]), W("I", len(m["props"]))]
    for p in m["props"]:
        o += [ws(p["key"]), W("H", p["ver"]), W("16f", *p["tr"]), W("B", p["clamp"]), W("I", p["type"]), W("B", p["allow"]), W("4f", *p["par"]), p["extra"]]
    o.append(W("I", len(m["emitters"])))
    for e in m["emitters"]:
        o += [ws(e["key"]), W("H", e["ver"]), W("f", e["rate"]), W("16f", *e["tr"]), ws(e["name"]), W("B", e["autoplay"]), W("f", e["scale"]), W("5B", *e["flags"])]
    o += [W("3I", *m["hints"]), W("HI", *m["deploy"]), W("HI", m["buildings_ver"], len(m["buildings"]))]
    for b in m["buildings"]:
        o += [W("H", b["ver"]), ws(b["key"]), ws(b["ptype"]), W("3f", *b["pos"]), W("4f", *b["ori"]), W("3f", *b["scale"]), W("f", b["dmg"]), W("8B", *b["flags"])]
    o += [W("HI", *m["far"]), W("HI", m["cap_ver"], len(m["captures"]))]
    for lst in m["captures"]:
        o.append(W("I", len(lst)))
        for c in lst:
            o += [W("2f", *c["loc"]), W("f", c["radius"]), W("2I", c["mn"], c["mx"]), ws(c["type"]), W("I", len(c["pts"]))]
            o += [W("2f", *q) for q in c["pts"]]
            o += [W("I", len(c["idx"]))] + [W("I", i) for i in c["idx"]]
            o += [ws(c["key"]), W("2f", *c["facing"])]
    o.append(W("I", m["ef"]))
    for key in ("go", "non_terrain"):
        o.append(W("I", len(m[key])))
        for ol in m[key]:
            o += [W("I", len(ol))] + [W("2f", *q) for q in ol]
    o += [W("HI", *m["zones"]), W("HI", *m["prefab"]), W("HI", *m["bmd_outline"]), W("4I", *m["tail"])]
    return b"".join(o)


def main():
    if sys.argv[1] == "--corpus":
        import collections
        import os
        manifest, root = sys.argv[2], sys.argv[3]
        ok, bad = 0, collections.Counter()
        examples = {}
        for line in list(open(manifest, encoding="utf-8"))[1:]:
            c = line.rstrip(chr(10)).split(chr(9))
            if c[11] != "in" or not (c[2].endswith("_procedural_bmd_data.bin") or ("bmd_data_escalation" in c[2] and c[2].endswith(".bin"))):
                continue
            m, err = parse(open(os.path.join(root, c[0], c[1], c[2]), "rb").read())
            if err is None:
                data = open(os.path.join(root, c[0], c[1], c[2]), "rb").read()
                if write(m) == data:
                    ok += 1
                else:
                    bad["write != input"] += 1
                    examples.setdefault("write != input", c[2])
            else:
                key = err.split(" (at")[0][:70]
                bad[key] += 1
                examples.setdefault(key, c[0] + "/" + c[1] + "/" + c[2])
        print("fully parsed:", ok)
        for k, n in bad.most_common():
            print(n, k, "e.g.", examples[k])
        return
    b = open(sys.argv[1], "rb").read()
    m, err = parse(b)
    print({k: (len(v) if isinstance(v, list) else v) for k, v in m.items()}, err)


if __name__ == "__main__":
    main()
