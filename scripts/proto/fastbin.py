#!/usr/bin/env python3
"""Prototype parsers/writers for the small FASTBIN0 tile files, verified over the whole corpus.

Each format: parse(bytes) -> model, write(model) -> bytes, and a generic check that write(parse(x)) == x.
Layouts (little endian), all start with b'FASTBIN0' then u16 version:

  civilian_deployment.bin u16 ver=1, u32 n, n * { u16 flag(=1), str kind, str size, u32 np, np * { f32 x, f32 z } }
  civilian_shelter.bin    u16 ver=1, u32 n, n * { u16 flag(=1), f32 x1, f32 z1, f32 x2, f32 z2, u32 capacity }
  *.tree_list.bin       u16 ver=3, u32 n_types, n_types * { str name, u32 n, n * { f32 x, f32 y, f32 z, u16 w } }
  default.grass_list.bin u16 ver=1, u32 n_groups, n_groups * { str name, u32 n, n * 6 bytes }
  str = u16 length + bytes (no terminator)

    python fastbin.py <manifest.tsv> <kits_parent_dir>
"""
import collections
import os
import struct
import sys

MAGIC = b"FASTBIN0"


class R:
    def __init__(self, b):
        self.b, self.p = b, 0

    def take(self, n):
        if self.p + n > len(self.b):
            raise ValueError(f"truncated at {self.p}")
        v = self.b[self.p:self.p + n]
        self.p += n
        return v

    def u16(self):
        return struct.unpack("<H", self.take(2))[0]

    def u32(self):
        return struct.unpack("<I", self.take(4))[0]

    def s(self):
        return self.take(self.u16())

    def done(self):
        if self.p != len(self.b):
            raise ValueError(f"{len(self.b) - self.p} trailing bytes at {self.p}")


def header(r, versions):
    if r.take(8) != MAGIC:
        raise ValueError("not FASTBIN0")
    v = r.u16()
    if v not in versions:
        raise ValueError(f"version {v}")
    return v


def p16(v):
    return struct.pack("<H", v)


def p32(v):
    return struct.pack("<I", v)


def pstr(s):
    return p16(len(s)) + s


# ---- civilian
def parse_civilian(b, kind):
    r = R(b)
    v = header(r, (1,))
    items = []
    for _ in range(r.u32()):
        flag = r.u16()
        if kind == "deployment":
            a, c = r.s(), r.s()
            pts = [struct.unpack("<ff", r.take(8)) for _ in range(r.u32())]
            items.append((flag, a, c, pts))
        else:
            items.append((flag, struct.unpack("<ffff", r.take(16)), r.u32()))
    r.done()
    return {"version": v, "kind": kind, "items": items}


def write_civilian(m):
    out = [MAGIC, p16(m["version"]), p32(len(m["items"]))]
    for it in m["items"]:
        if m["kind"] == "deployment":
            flag, a, c, pts = it
            out += [p16(flag), pstr(a), pstr(c), p32(len(pts))] + [struct.pack("<ff", *p) for p in pts]
        else:
            flag, f4, n = it
            out += [p16(flag), struct.pack("<ffff", *f4), p32(n)]
    return b"".join(out)


# ---- tree list
def parse_tree_list(b):
    r = R(b)
    v = header(r, (3,))
    types = []
    for _ in range(r.u32()):
        name = r.s()
        n = r.u32()
        inst = [struct.unpack("<fffH", r.take(14)) for _ in range(n)]
        types.append((name, inst))
    r.done()
    return {"version": v, "types": types}


def write_tree_list(m):
    out = [MAGIC, p16(m["version"]), p32(len(m["types"]))]
    for name, inst in m["types"]:
        out += [pstr(name), p32(len(inst))]
        out += [struct.pack("<fffH", *i) for i in inst]
    return b"".join(out)


# ---- grass list
def parse_grass_list(b):
    r = R(b)
    v = header(r, (1,))
    groups = []
    for _ in range(r.u32()):
        name = r.s()
        n = r.u32()
        groups.append((name, r.take(6 * n)))
    r.done()
    return {"version": v, "groups": groups}


def write_grass_list(m):
    out = [MAGIC, p16(m["version"]), p32(len(m["groups"]))]
    for name, data in m["groups"]:
        out += [pstr(name), p32(len(data) // 6), data]
    return b"".join(out)


FORMATS = {
    "civilian_deployment": (lambda b: parse_civilian(b, "deployment"), write_civilian),
    "civilian_shelter": (lambda b: parse_civilian(b, "shelter"), write_civilian),
    "tree_list": (parse_tree_list, write_tree_list),
    "grass_list": (parse_grass_list, write_grass_list),
}


def kind_of(path, role):
    name = os.path.basename(path)
    if role == "tile_file:civilian":
        return "civilian_deployment" if "deployment" in name else "civilian_shelter"
    if role == "tile_file:tree_list":
        return "tree_list"
    if role == "tile_file:grass_list":
        return "grass_list"
    return None


def main():
    manifest, root = sys.argv[1], sys.argv[2]
    stats = collections.defaultdict(lambda: [0, 0, []])
    for line in list(open(manifest, encoding="utf-8"))[1:]:
        c = line.rstrip("\n").split("\t")
        if c[11] != "in":
            continue
        k = kind_of(c[2], c[10])
        if not k:
            continue
        data = open(os.path.join(root, c[0], c[1], c[2]), "rb").read()
        parse, write = FORMATS[k]
        s = stats[k]
        try:
            ok = write(parse(data)) == data
            err = "write != input"
        except Exception as e:  # noqa
            ok, err = False, str(e)
        s[0] += 1
        if ok:
            s[1] += 1
        elif len(s[2]) < 5:
            s[2].append(f"{c[0]}/{c[1]}/{c[2]}: {err}")
    for k, (n, ok, fails) in stats.items():
        print(f"{k:12} files {n:6} ok {ok:6}")
        for f in fails:
            print("   FAIL", f)


if __name__ == "__main__":
    main()
