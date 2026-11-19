"""A campaign map frame of the editor against the game's (docs/plans/campaign_tilemap.md, Phase 3). Python's standard library only.

    python scripts/campaign_compare.py <capture folder> <editor cb_dump file> [editor capture.png] [--top N] [--out folder] [--all]

The capture folder is `scripts/re/rd_campaign_frame.py`'s, named by `wce_terrain_io`'s `campaign_frame` example (shader_names.csv,
screen.png, mask.png). The editor's side: `cb_dump <file>` and `capture <W>x<H> <file.png>` at the capture's camera
(`terrain_cam_game`), the editor in `view_mode game`.

Constants: for each constant buffer, the game's values at the draw of the pass that sets it for the frame (PASS_OF; else the last draw
that reads it) against what the editor's last frame set; the differing variables by their relative difference, the largest first.
Images: the editor's capture against the game's screen before the UI: the mean absolute difference (0 .. 255) and the mean colour of
each region (sky: the game's depth at the far plane; ground: the rest), the out-of-scope mask (armies, agents) left out; a heat map
(`heat.png`, the difference times 4) in the output folder.
"""
import json
import os
import struct
import sys
import zlib

# Compare every buffer (`--all`; those not in PASS_OF at the last draw that reads them), else PASS_OF's alone.
ALL = False

# The pass whose draw gives a buffer's frame values in the game (a part of its pixel or vertex shader's fxc name).
PASS_OF = {
    "camera_VS_PS": "rigidterrain.hlsl_ps_ps30_terrain_to_gbuffer",
    "fog_VS_PS": "battlefog",
    "lighting_VS_PS": "directionallight",
    "directional_light_PS": "directionallight",
    "shadowmap_PS": "directionallight",
    "shroud_PS": "rigidterrain.hlsl_ps_ps30_terrain_to_gbuffer",
    "ps_height_map_cb": "rigidterrain.hlsl_ps_ps30_terrain_to_gbuffer",
    "OceanSharedConstants": "ps30_campaign_high",
    "campaign_CB": "ps30_campaign_high",
    "campaign_ocean_cb": "ps30_campaign_high",
    "hdr_to_screen_PS": "hdr_to_screen",
    "colorimetry_VS_PS": "hdr_to_screen",
    "shroud_plane_buffer": "shroud_plane",
    "weather_shared": "rigidstandard",
}


def flatten(v):
    if isinstance(v, dict):
        return None
    return [float(x) for x in v]


def transposed(v):
    """A 4 x 4's sixteen values the other way round (the editor's buffers hold matrices as the GPU packs them, RenderDoc reads them
    by rows)."""
    return [v[c * 4 + r] for r in range(4) for c in range(4)] if len(v) == 16 else v


def game_constants(folder):
    names = {}
    for line in open(os.path.join(folder, "shader_names.csv")).read().splitlines()[1:]:
        r = line.split(",")
        names[int(r[0])] = r[3]
    c = json.load(open(os.path.join(folder, "constants.json")))
    store = c["cbuffers"]
    chosen = {}
    for g in c["groups"]:
        shaders = names.get(g["ps"], "") + " " + names.get(g["vs"], "")
        for key, h in g["cbuffers"].items():
            buf = key.split(":", 1)[1]
            want = PASS_OF.get(buf)
            if want is None and not ALL:
                continue
            if want is None or want in shaders:
                if want is not None and buf in chosen and chosen[buf][1]:
                    continue
                chosen[buf] = (store.get(h, {}), want is not None and want in shaders, g["event"])
    return {b: (vals, ev) for b, (vals, _, ev) in chosen.items()}


def editor_constants(path):
    out = {}
    for line in open(path).read().splitlines():
        name, _, rest = line.partition(" ")
        buf, _, var = name.partition(".")
        vals = [float(x) for x in rest.split()] if rest.strip() else []
        # An integer variable's bits print as a denormal float: read back as the integer.
        vals = [float(struct.unpack("<i", struct.pack("<f", v))[0]) if v != 0.0 and abs(v) < 1e-30 else v for v in vals]
        out.setdefault(buf, {})[var] = vals
    return out


def compare_constants(game, editor, top):
    rows = []
    for buf, (vals, event) in sorted(game.items()):
        ed = editor.get(buf)
        if ed is None:
            continue
        for var, gv in vals.items():
            gv = flatten(gv)
            ev = ed.get(var)
            if gv is None or ev is None or not gv:
                continue
            n = min(len(gv), len(ev))
            if n == 16:
                straight = sum(abs(gv[i] - ev[i]) for i in range(16))
                if sum(abs(gv[i] - x) for i, x in enumerate(transposed(ev))) < straight:
                    ev = transposed(ev)
            diffs = [abs(gv[i] - ev[i]) for i in range(n)]
            scale = max(1e-6, max(abs(x) for x in gv[:n] + ev[:n]))
            worst = max(diffs) if diffs else 0.0
            if worst > 1e-4 * scale:
                rows.append((worst / scale, buf, var, event, gv[: min(n, 8)], ev[: min(n, 8)]))
    rows.sort(reverse=True)
    print(f"constants: {len(rows)} variables differ (of the buffers both frames have); the largest {min(top, len(rows))}:")
    for rel, buf, var, event, gv, ev in rows[:top]:
        fmt = lambda a: " ".join(f"{x:.4g}" for x in a)
        print(f"  {rel:7.3f}  {buf}.{var} (game event {event})\n            game   {fmt(gv)}\n            editor {fmt(ev)}")


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", path
    pos, idat, w = 8, b"", 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos : pos + 8])
        body = data[pos + 8 : pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and interlace == 0, f"{path}: 8-bit, not interlaced"
            bpp = {0: 1, 2: 3, 4: 2, 6: 4}[ctype]
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw = zlib.decompress(idat)
    stride = w * bpp
    out = bytearray(h * stride)
    prev = bytearray(stride)
    i = 0
    for y in range(h):
        f = raw[i]
        line = bytearray(raw[i + 1 : i + 1 + stride])
        i += 1 + stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return w, h, bpp, out


def write_png(path, w, h, rgb):
    raw = b"".join(b"\x00" + bytes(rgb[y * w * 3 : (y + 1) * w * 3]) for y in range(h))
    chunk = lambda k, d: struct.pack(">I", len(d)) + k + d + struct.pack(">I", zlib.crc32(k + d) & 0xFFFFFFFF)
    open(path, "wb").write(
        b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
    )


def depth_far(folder, w, h):
    """The game's depth target (targets/depth.dds, D24S8: depth the low 24 bits) at the far plane, per pixel."""
    b = open(os.path.join(folder, "targets", "depth.dds"), "rb").read()
    start = 148 if b[84:88] == b"DX10" else 128
    px = b[start:]
    if len(px) < w * h * 4:
        return None
    return [(struct.unpack_from("<I", px, i * 4)[0] & 0xFFFFFF) >= 0xFFFFF0 for i in range(w * h)]


def compare_images(folder, editor_png, out):
    gw, gh, gb, game = read_png(os.path.join(folder, "screen.png"))
    ew, eh, eb, ed = read_png(editor_png)
    if (gw, gh) != (ew, eh):
        print(f"images: sizes differ ({gw} x {gh} against {ew} x {eh}): not compared")
        return
    mask = None
    if os.path.exists(os.path.join(folder, "mask.png")):
        mw, mh, mb, m = read_png(os.path.join(folder, "mask.png"))
        mask = [m[i * mb] > 127 for i in range(mw * mh)] if (mw, mh) == (gw, gh) else None
    sky = depth_far(folder, gw, gh)
    regions = {"sky": [0, 0.0, [0.0] * 3, [0.0] * 3], "ground": [0, 0.0, [0.0] * 3, [0.0] * 3]}
    heat = bytearray(gw * gh * 3)
    for i in range(gw * gh):
        if mask and mask[i]:
            continue
        g = game[i * gb : i * gb + 3]
        e = ed[i * eb : i * eb + 3]
        d = (abs(g[0] - e[0]) + abs(g[1] - e[1]) + abs(g[2] - e[2])) / 3.0
        r = regions["sky" if sky and sky[i] else "ground"]
        r[0] += 1
        r[1] += d
        for k in range(3):
            r[2][k] += g[k]
            r[3][k] += e[k]
        v = min(255, int(d * 4))
        heat[i * 3 : i * 3 + 3] = bytes((v, v, v))
    print("images (mean |difference| 0 .. 255; mean colour game -> editor):")
    for name, (n, d, gs, es) in regions.items():
        if n:
            gm = [x / n for x in gs]
            em = [x / n for x in es]
            lum = lambda c: 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
            print(
                f"  {name:6} {n:8d} px  difference {d / n:6.2f}  colour ({gm[0]:.0f}, {gm[1]:.0f}, {gm[2]:.0f}) -> ({em[0]:.0f}, {em[1]:.0f}, "
                f"{em[2]:.0f}), the editor {'brighter' if lum(em) > lum(gm) else 'darker'} by {abs(lum(em) - lum(gm)):.1f}"
            )
    os.makedirs(out, exist_ok=True)
    write_png(os.path.join(out, "heat.png"), gw, gh, heat)
    print(f"  heat map: {os.path.join(out, 'heat.png')}")


def main():
    global ALL
    args = [a for a in sys.argv[1:]]
    if "--all" in args:
        ALL = True
        args.remove("--all")
    top = 25
    out = None
    if "--top" in args:
        i = args.index("--top")
        top = int(args[i + 1])
        del args[i : i + 2]
    if "--out" in args:
        i = args.index("--out")
        out = args[i + 1]
        del args[i : i + 2]
    if len(args) < 2:
        print(__doc__)
        sys.exit(2)
    folder, dump = args[0], args[1]
    compare_constants(game_constants(folder), editor_constants(dump), top)
    if len(args) > 2:
        compare_images(folder, args[2], out or os.path.join(folder, "compare"))


if __name__ == "__main__":
    main()
