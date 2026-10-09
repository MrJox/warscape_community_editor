#!/usr/bin/env python3
"""tile_database.bin from _settings.xml + TILES/*.xml (work in progress). Builds candidate bytes and compares them
with the shipped file so the layout can be fixed step by step.

    python tdb_proto.py <_tile_database dir> <tile_database.bin> [--tiles]
"""
import os
import struct
import sys
import xml.etree.ElementTree as ET

ZERO = bytes([0])
ONE = bytes([1])


def f(x):
    return struct.pack("<f", float(x))


def u16(x):
    return struct.pack("<H", int(x))


def u32(x):
    return struct.pack("<I", int(x))


def i32(x):
    return struct.pack("<i", int(x))


def s(x):
    return u16(len(x)) + x.encode("latin1")


def b1(x):
    return ONE if str(x).lower() == "true" else ZERO


RENDER_SLOTS = [
    "lf_height", "hf_height", "blend_pixel_scale", "unit_scale", "mid_distance_detail_scale",
    "mid_distance_detail_strength", "mid_distance_normal_strength", "mid_distance_detail_near", "mid_distance_detail_far",
    "mid_distance_detail_slope_low", "mid_distance_detail_slope_high", "vertical_offset", "normal_lf_scale",
    "normal_tile_scale", "normal_terrain_scale", "blend_contrast", None, None,
    "campaign_sea_transparency_scale", "campaign_sea_uv_scale", "campaign_lake_transparency_scale",
    "near_distance_detail_distance", "near_distance_detail_scale", "near_distance_detail_strength",
    "mid_distance_detail_near_lq",
]


def render(e):
    out = [u16(6)]
    for name in RENDER_SLOTS:
        out.append(f(e.get(name)) if name else f(0))
    return b"".join(out)


def conversion(e):
    g = e.get
    return b"".join([
        u16(g("serialise_version")), u32(g("triangle_density")), u32(g("max_lf_heights_per_pixel")),
        *[f(g(f"triangle_decimation_size_factors{i}")) for i in range(6)],
        *[f(g(f"triangle_decimation_angle_factors{i}")) for i in range(6)],
    ])


def texture(t):
    g = t.get
    return u16(g("serialise_version")) + s(g("name")) + b"".join(
        f(g(k)) for k in ("mid_distance_strength", "lerp_reflectivity0", "lerp_smoothness0", "lerp_reflectivity1",
                          "lerp_smoothness1", "lerp_reflectivity2", "lerp_smoothness2", "lerp_reflectivity3",
                          "lerp_smoothness3", "blend_pixel_scale", "outfield_blend_pixel_scale"))


def climate(c):
    g = c.get
    out = [u16(g("serialise_version")), s(g("name")), s(g("texture_set")), f(g("red")), f(g("green")), f(g("blue"))]
    textures = c.find("TEXTURES")
    out.append(u32(len(textures)))
    out += [texture(t) for t in textures]
    out += [f(g("grass_alpha_add")), f(g("grass_alpha_mul")), s(g("destruction_climate"))]
    return b"".join(out)


# string slots of a TILE_SET after its name: linking_tile first, link_as_set last (from the shipped data);
# the position of shared_geometry and also_place_tile_set among slots 1 and 2 is settled with the campaign database.
TILESET_STR_ORDER = ["linking_tile", "shared_geometry", "also_place_tile_set", "link_as_set"]


def tile_set(t):
    g = t.get
    return (u16(g("serialise_version")) + s(g("name")) + b"".join(s(g(k)) for k in TILESET_STR_ORDER)
            + f(g("red")) + f(g("green")) + f(g("blue")))


def link(l):
    g = l.get
    head = u16(g("serialise_version")) + s(g("link_set")) + i32(g("x")) + i32(g("y")) + i32(g("base_x")) + i32(g("base_y"))
    return head + b1(g("is_entry")) + ZERO * 4 + u32(g("blend_size")) + b1(g("no_offline_blend")) + s(g("test"))


def tile(t):
    g = t.get
    out = [u16(g("serialise_version")), s(g("location")), s(g("tile_set")), s(g("mask")), u32(g("width")), u32(g("height")),
           f(g("red")), f(g("green")), f(g("blue")), b1(g("requires_infield_lodding")), b1(g("random_rotatable")),
           s(g("custom_alpha_blend_texture")), b1(g("scalable")), b1(g("encampable")), s(g("custom_blend_tile"))]
    tg = t.find("TEXTURE_GROUP")
    out += [s(tg.get(k)) for k in ("red", "green", "blue", "alpha")]
    vs = t.find("VARIATIONS")
    out.append(u32(len(vs)))
    for v in vs:
        h = v.get
        out += [u16(h("serialise_version")), s(h("location")), f(h("min_height")), f(h("scale")), f(h("normal_strength")),
                f(h("overlap_border_size")), u32(h("raw_data_tri_density")), s(h("blend_common")), s(h("index_common")),
                s(h("normal_common")), f(h("red")), f(h("green")), f(h("blue")), b1(h("barbarian"))]
    ts = t.find("TILE_LINK_TARGETS")
    out.append(u32(len(ts)))
    for x in ts:
        out += [u16(x.get("serialise_version")), s(x.get("tile_set")), i32(x.get("x")), i32(x.get("y"))]
    ls = t.find("TILE_LINKS")
    out.append(u32(len(ls)))
    out += [link(l) for l in ls]
    return b"".join(out)


def build(db_dir):
    root = ET.parse(os.path.join(db_dir, "_settings.xml")).getroot()
    out = [b"FASTBIN0", u16(1), render(root.find("RENDER_PARAMS")), conversion(root.find("CONVERSION_PARAMS"))]
    climates = root.find("CLIMATES")
    out.append(u32(len(climates)))
    out += [climate(c) for c in climates]
    sets = root.find("TILE_SETS")
    out.append(u32(len(sets)))
    out += [tile_set(t) for t in sets]
    return b"".join(out), root


EXTRA_TILE_SETS = ["assembly_kit", "user"]


def build_full(db_dir, ak_dir, order=None):
    root = ET.parse(os.path.join(db_dir, "_settings.xml")).getroot()
    out = [b"FASTBIN0", u16(1), render(root.find("RENDER_PARAMS")), conversion(root.find("CONVERSION_PARAMS"))]
    climates = root.find("CLIMATES")
    out.append(u32(len(climates)))
    out += [climate(c) for c in climates]
    sets = list(root.find("TILE_SETS"))
    out.append(u32(len(sets) + len(EXTRA_TILE_SETS)))
    out += [tile_set(t) for t in sets]
    for name in EXTRA_TILE_SETS:
        out.append(u16(1) + s(name) + s("") * 4 + f(0) * 3)
    files = [os.path.join(db_dir, "TILES", fn) for fn in sorted(os.listdir(os.path.join(db_dir, "TILES")))]
    if ak_dir:
        files += [os.path.join(ak_dir, fn) for fn in sorted(os.listdir(ak_dir)) if fn.endswith(".xml")]
    roots = [ET.parse(fp).getroot() for fp in files]
    if order is not None:
        rank = {loc: k for k, loc in enumerate(order)}
        roots.sort(key=lambda r: rank[r.get("location")])
    else:
        # default for new data: footprint area descending, then location (the shipped order has no known rule)
        roots.sort(key=lambda r: (-int(r.get("width")) * int(r.get("height")), r.get("location")))
    out.append(u32(len(roots)))
    out += [tile(r) for r in roots]
    return b"".join(out)


def reference_order(ref, db_dir, ak_dir):
    found = []
    for base in (os.path.join(db_dir, "TILES"), ak_dir):
        for fn in os.listdir(base):
            if fn.endswith(".xml"):
                r = ET.parse(os.path.join(base, fn)).getroot()
                bts = tile(r)
                n1 = struct.unpack_from("<H", bts, 2)[0]
                n2 = struct.unpack_from("<H", bts, 4 + n1)[0]
                found.append((ref.find(bts[:6 + n1 + n2]), r.get("location")))
    return [loc for _, loc in sorted(found)]


def tiles(db_dir):
    d = os.path.join(db_dir, "TILES")
    return [(fn, tile(ET.parse(os.path.join(d, fn)).getroot())) for fn in sorted(os.listdir(d))]


def first_diff(a, b):
    n = min(len(a), len(b))
    return next((k for k in range(n) if a[k] != b[k]), None)


def main():
    ref = open(sys.argv[2], "rb").read()
    if "--full" in sys.argv:
        order = None
        if "--order-from-ref" in sys.argv:
            order = reference_order(ref, sys.argv[1], sys.argv[3])
        mine = build_full(sys.argv[1], sys.argv[3], order)
        i = first_diff(mine, ref)
        print("sizes", len(mine), len(ref), "first difference", i)
        if i is not None:
            print("mine", mine[max(0, i - 24):i + 40].hex(" "))
            print("ref ", ref[max(0, i - 24):i + 40].hex(" "))
        return
    mine, root = build(sys.argv[1])
    if "--tiles" not in sys.argv:
        i = first_diff(mine, ref)
        if i is None:
            print(f"candidate prefix of {len(mine)} bytes matches")
        else:
            print(f"first difference at {i}: mine {mine[max(0, i - 8):i + 16].hex(' ')} | ref {ref[max(0, i - 8):i + 16].hex(' ')}")
        return
    ok = bad = 0
    for fn, bts in tiles(sys.argv[1]):
        n1 = struct.unpack_from('<H', bts, 2)[0]
        n2 = struct.unpack_from('<H', bts, 4 + n1)[0]
        i = ref.find(bts[:6 + n1 + n2])
        if i >= 0 and ref[i:i + len(bts)] == bts:
            ok += 1
            continue
        bad += 1
        if bad <= 4:
            j = max(i, 0)
            n = first_diff(bts, ref[j:j + len(bts)])
            print("tile", fn, "found at", i, "first diff", n)
            if n is not None:
                print("   mine", bts[max(0, n - 12):n + 28].hex(" "))
                print("   ref ", ref[j + max(0, n - 12):j + n + 28].hex(" "))
    print("tiles matching exactly:", ok, "not:", bad)


if __name__ == "__main__":
    main()
