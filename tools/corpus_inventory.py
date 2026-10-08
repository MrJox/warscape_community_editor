#!/usr/bin/env python3
"""Inventory of the Assembly Kit corpus used by the tile editor's round-trip and oracle gates.

Walks the raw_data and working_data trees of the battle and campaign tilemap kits and writes a TSV manifest:

    kit  side  path  size  mtime  ext  magic  encoding  eol  lfs  role  scope  [sha256]

- magic: first 8 bytes as hex (identifies FASTBIN0, CHMF, BLMF, RMV2, PNG, TIFF, DDS ...)
- lfs: 'pointer' when the file is a git-LFS pointer stub (the real content is not checked out)
- role: tile_file | tile_db | map_file | art | db | other, plus a 'kind' sub-label for tile files
- scope: in | out. Out of scope (decided 2026-10-08): campaign map files (map.hex, esf/ppd/ptd, trade routes),
  tilemap / Terry map-level data (terrain/battles, terrain/campaigns, campaign_maps, *.terry).
- sha256 only with --hash (slow on 80 GB); gates compute it lazily and cache it in the manifest.

    python corpus_inventory.py --out ..\\manifest\\corpus.tsv [--hash] [--kit PATH ...]
    python corpus_inventory.py --summary ..\\manifest\\corpus.tsv
"""
import argparse
import collections
import hashlib
import os
import sys
import time

GAME_GUESS = r"D:\SteamLibrary\steamapps\common\Total War Attila"
KITS = ["assembly_kit_battle_tilemap", "assembly_kit_campaign_tilemap"]
SIDES = ["raw_data", "working_data"]
SKIP_DIRS = {".git"}

OUT_OF_SCOPE_PREFIXES = (
    "terrain/battles/", "terrain/campaigns/", "campaign_maps/", "empiredesigndata/campaign_maps/",
)
OUT_OF_SCOPE_EXT = {".terry", ".hex", ".esf", ".ppd", ".ptd", ".pbd"}

TILE_FILE_KINDS = {
    "tile.ted": "tile_ted", "definition.xml": "definition", "building_list.xml": "building_list",
    "far_building_list.xml": "far_building_list", "capture_location_list.xml": "capture_locations",
    "deployment_areas.xml": "deployment_areas", "ef_lines.xml": "ef_lines", "zones.xml": "zones",
    "go_outlines.xml": "go_outlines", "non_terrain_outlines.xml": "non_terrain_outlines",
    "terrain_outlines.xml": "terrain_outlines", "sound_emitter_list.xml": "sound_emitters",
    "tree_random_seed.xml": "tree_seed", "height_map_0.png": "height_map", "blend_map.tif": "blend_map_tif",
    "ground_type_map.png": "ground_type_map", "lf_influence_map.png": "lf_influence", "nogo.png": "nogo",
    "protection_map.dds": "protection_map", "tile_normal.tga": "tile_normal", "building_list": "building_list_bin",
    "bmd.tree_list": "tree_list_legacy", "tile.agf": "agf", "hf_height_map.data": "chmf_hf",
    "hf_water_map.data": "chmf_water", "mesh.rigid_model_v2": "tile_mesh",
    "outfield_mesh.rigid_model_v2": "outfield_mesh", "civilian_deployment.bin": "civilian",
    "civilian_shelter.bin": "civilian", "default.grass_list.bin": "grass_list",
    "blend.dds": "blend_dds", "index.dds": "index_dds", "normal.dds": "normal_dds",
    "ground_types.dds": "ground_types_dds", "final_alpha.dds": "final_dds", "final_heights.dds": "final_dds",
    "final_lf_influence.dds": "final_dds", "custom_normal.dds": "custom_normal", "rules.bob": "rules_bob",
}


def classify(rel, name):
    """rel: path under raw_data/working_data with forward slashes, lower-cased."""
    ext = os.path.splitext(name)[1].lower()
    if ext in OUT_OF_SCOPE_EXT or rel.startswith(OUT_OF_SCOPE_PREFIXES):
        return "map_file", "map", "out"
    if rel.startswith("terrain/tiles/"):
        if "/_tile_database/" in rel or rel.endswith("/_tile_database") or name == "tile_database.bin" \
                or name == "tile_upgrades.xml":
            kind = "tile_database" if name != "tile_database.bin" else "tile_database_bin"
            return "tile_db", kind, "in"
        if name.endswith(".tree_list.bin") or name == "tree_list.bin":
            return "tile_file", "tree_list", "in"
        if name.endswith("_procedural_bmd_data.bin") or name.endswith("_procedural_bmd_data.xml"):
            return "tile_file", "procedural_bmd", "in"
        if name.startswith("bmd_data_escalation"):
            return "tile_file", "bmd_escalation", "in"
        if name.startswith("river_spline") and ext == ".xml":
            return "tile_file", "river_spline", "in"
        if ext == ".markers":
            return "tile_file", "markers:" + name[:-len(".markers")], "in"
        if "/raw/" in rel or "/wm/" in rel or ext in (".tmd", ".session", ".psd"):
            return "tile_file", "sculpt_intermediate", "in"
        if name in TILE_FILE_KINDS:
            return "tile_file", TILE_FILE_KINDS[name], "in"
        return "tile_file", "other", "in"
    if rel.startswith("db/") or rel.startswith("empiredesigndata/"):
        return "db", "ak_db", "in"
    if rel.startswith(("art/", "variantmeshes/", "rigidmodels/", "animations/")):
        return "art", "art", "out"
    if rel.startswith(("terrain/vegetation/", "battleterrain/", "terrain/textures/", "terrain/deployment/",
                       "lighting/", "warscape_asset_variation_db/")):
        return "other", "tile_support", "in"
    return "other", "other", "in"


def sniff(path):
    try:
        with open(path, "rb") as f:
            head = f.read(4096)
    except OSError:
        return "", "", "", ""
    magic = head[:8].hex()
    lfs = "pointer" if head.startswith(b"version https://git-lfs") else ""
    enc, eol = "", ""
    if head[:3] == b"\xef\xbb\xbf":
        enc = "utf8bom"
    elif head[:2] == b"\xff\xfe":
        enc = "utf16le"
    elif head[:5] in (b"<?xml", b"<proj", b"<TILE", b"<BATT") or head[:1] == b"<":
        enc = "text"
    if enc:
        eol = "crlf" if b"\r\n" in head else ("lf" if b"\n" in head else "")
    return magic, enc, eol, lfs


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def walk(kit_root, do_hash):
    kit = os.path.basename(kit_root)
    for side in SIDES:
        base = os.path.join(kit_root, side)
        if not os.path.isdir(base):
            continue
        for dirpath, dirs, files in os.walk(base):
            dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
            for name in files:
                p = os.path.join(dirpath, name)
                rel = os.path.relpath(p, base).replace("\\", "/").lower()
                try:
                    st = os.stat(p)
                except OSError:
                    continue
                magic, enc, eol, lfs = sniff(p)
                role, kind, scope = classify(rel, name.lower())
                digest = sha256(p) if do_hash and st.st_size < (1 << 31) else ""
                yield [kit, side, rel, str(st.st_size), str(int(st.st_mtime)), os.path.splitext(name)[1].lower(),
                       magic, enc, eol, lfs, role + ":" + kind, scope, digest]


HEADER = ["kit", "side", "path", "size", "mtime", "ext", "magic", "encoding", "eol", "lfs", "role", "scope", "sha256"]


def summary(path):
    rows = [l.rstrip("\n").split("\t") for l in open(path, encoding="utf-8")][1:]
    by = collections.Counter()
    size = collections.Counter()
    lfs = 0
    for r in rows:
        key = (r[0].replace("assembly_kit_", ""), r[1], r[11], r[10].split(":")[0])
        by[key] += 1
        size[key] += int(r[3])
        lfs += r[9] == "pointer"
    print(f"{len(rows)} files, {lfs} LFS pointer stubs")
    for k in sorted(by):
        print(f"  {k[0]:18} {k[1]:13} scope={k[2]:3} {k[3]:10} {by[k]:7} files {size[k] / 1e9:8.2f} GB")
    kinds = collections.Counter(r[10] for r in rows if r[11] == "in" and r[10].startswith(("tile_file", "tile_db")))
    print("tile file kinds (in scope):")
    for k, n in kinds.most_common(60):
        print(f"  {n:7} {k}")
    unk = [r for r in rows if r[11] == "in" and r[10] in ("tile_file:other", "other:other")]
    print(f"unclassified in-scope files: {len(unk)}")
    ext = collections.Counter(r[5] for r in unk)
    print("  by extension:", dict(ext.most_common(15)))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out")
    ap.add_argument("--kit", action="append")
    ap.add_argument("--hash", action="store_true")
    ap.add_argument("--summary")
    a = ap.parse_args()
    if a.summary:
        summary(a.summary)
        return
    kits = a.kit or [os.path.join(GAME_GUESS, k) for k in KITS]
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
    t0, n = time.time(), 0
    with open(a.out, "w", encoding="utf-8", newline="\n") as f:
        f.write("\t".join(HEADER) + "\n")
        for k in kits:
            for row in walk(k, a.hash):
                f.write("\t".join(row) + "\n")
                n += 1
    print(f"{n} files in {time.time() - t0:.0f}s -> {a.out}")


if __name__ == "__main__":
    sys.exit(main())
