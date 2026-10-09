import os
import struct
import sys
import xml.etree.ElementTree as ET

sys.path.insert(0, os.path.dirname(__file__))
import tdb_proto as t  # noqa: E402

K = "D:/SteamLibrary/steamapps/common/Total War Attila/assembly_kit_battle_tilemap/"
ref = open(K + "working_data/terrain/tiles/battle/tile_database.bin", "rb").read()
d = K + "raw_data/terrain/tiles/battle/_tile_database/TILES/"
ak = K + "working_data/terrain/tiles/battle/_ak_bf_tile_database/TILES/"
sets = [x.get("name") for x in ET.parse(d + "../_settings.xml").getroot().find("TILE_SETS")]
items = []
for base in (d, ak):
    for fn in os.listdir(base):
        if not fn.endswith(".xml"):
            continue
        r = ET.parse(base + fn).getroot()
        bts = t.tile(r)
        n1 = struct.unpack_from("<H", bts, 2)[0]
        n2 = struct.unpack_from("<H", bts, 4 + n1)[0]
        items.append((ref.find(bts[:6 + n1 + n2]), r.get("tile_set"), r.get("location").split(chr(92))[-1], fn, r.get("width"), r.get("height")))
items.sort()
runs = []
for i, ts, loc, fn, w, h in items:
    if not runs or runs[-1][0] != ts:
        runs.append([ts, 1])
    else:
        runs[-1][1] += 1
print(len(runs), "runs of equal tile_set")
print([(a, b, sets.index(a) if a in sets else -1) for a, b in runs][:70])

print([(w, h) for _, _, _, _, w, h in items][:60])
print([(w, h) for _, _, _, _, w, h in items][-60:])
