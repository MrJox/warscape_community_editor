r"""Dumps the terrain draws of a RenderDoc capture (Attila's engine: Attila, Thrones of Britannia): every draw whose vertex shader reads
`vs_height_map_cb`, with its shader, instances, the texture bound as `t_low_res_height_map` (saved once per texture as DDS), the
`vs_height_map_cb` values, and the world positions its vertices come out at (`TEXCOORD0` of the vs30_geom family: x, y, z, 1), those in
a box of the battle's frame written to points.csv (event, instance, x, y, z) and the ranges of all of them to dump.txt.

    set RD_CAPTURE=D:\renderdoc\thrones_loch_frame5377.rdc & set RD_OUT=<folder> & set RD_BOX=x0,z0,x1,z1 & qrenderdoc --python scripts\re\rd_terrain_dump.py

RD_BOX is in game units (the battle's frame); without it every point is written.
"""
import os
import struct
import traceback

import renderdoc as rd

D = os.environ["RD_OUT"]
BOX = [float(v) for v in os.environ["RD_BOX"].split(",")] if os.environ.get("RD_BOX") else None
log = open(os.path.join(D, "dump.txt"), "w")
pts = open(os.path.join(D, "points.csv"), "w")


def p(*a):
    log.write(" ".join(str(x) for x in a) + "\n")
    log.flush()


try:
    cap = rd.OpenCaptureFile()
    cap.OpenFile(os.environ["RD_CAPTURE"], "", None)
    st, ctl = cap.OpenCapture(rd.ReplayOptions(), None)
    texs = {t.resourceId: t for t in ctl.GetTextures()}
    draws = []

    def walk(acts):
        for a in acts:
            if a.flags & rd.ActionFlags.Drawcall:
                draws.append(a)
            walk(a.children)

    walk(ctl.GetRootActions())
    p("draws", len(draws))
    saved = set()
    for a in draws:
        ctl.SetFrameEvent(a.eventId, True)
        s = ctl.GetPipelineState()
        vs = s.GetShaderReflection(rd.ShaderStage.Vertex)
        if vs is None:
            continue
        names = [c.name for c in vs.constantBlocks]
        if "vs_height_map_cb" not in names:
            continue
        ps = s.GetShaderReflection(rd.ShaderStage.Pixel)
        p("== event", a.eventId, "vs", vs.entryPoint, "ps", ps.entryPoint if ps else "-", "indices", a.numIndices, "instances",
          a.numInstances, "name", a.customName)
        ci = names.index("vs_height_map_cb")
        d = s.GetConstantBlock(rd.ShaderStage.Vertex, ci, 0).descriptor
        data = ctl.GetBufferData(d.resource, d.byteOffset, d.byteSize)
        for v in vs.constantBlocks[ci].variables:
            n = v.type.rows * v.type.columns
            o = v.byteOffset
            if o + 4 * n <= len(data):
                p("   ", v.name, ["%.6g" % x for x in struct.unpack_from("<%df" % n, data, o)])
        for r in s.GetReadOnlyResources(rd.ShaderStage.Vertex):
            rid = r.descriptor.resource
            t = texs.get(rid)
            if t is None:
                continue
            p("  tex slot", r.access.index, rid, t.width, t.height, t.format.Name(), "mips", t.mips)
            if rid not in saved:
                saved.add(rid)
                ts = rd.TextureSave()
                ts.resourceId = rid
                ts.destType = rd.FileType.DDS
                ts.mip = 0
                p("   saved", ctl.SaveTexture(ts, os.path.join(D, "tex_%d.dds" % int(rid))))
        sig = vs.outputSignature
        offs, at = {}, 0
        for o in sig:
            offs[(o.semanticName.upper(), o.semanticIndex)] = at
            at += 4 * o.compCount
        t0 = offs.get(("TEXCOORD", 0))
        if t0 is None:
            p("  no TEXCOORD0 output", [(o.semanticName, o.semanticIndex, o.compCount) for o in sig])
            continue
        lo, hi, kept = [1e30] * 3, [-1e30] * 3, 0
        for inst in range(max(1, a.numInstances)):
            m = ctl.GetPostVSData(inst, 0, rd.MeshDataStage.VSOut)
            if m.vertexResourceId == rd.ResourceId.Null() or m.vertexByteStride == 0:
                continue
            raw = ctl.GetBufferData(m.vertexResourceId, m.vertexByteOffset, 0)
            stride = m.vertexByteStride
            for k in range(len(raw) // stride):
                x, y, z = struct.unpack_from("<3f", raw, k * stride + t0)
                for i, c in enumerate((x, y, z)):
                    lo[i], hi[i] = min(lo[i], c), max(hi[i], c)
                if BOX is None or (BOX[0] <= x <= BOX[2] and BOX[1] <= z <= BOX[3]):
                    pts.write("%d,%d,%.3f,%.3f,%.3f\n" % (a.eventId, inst, x, y, z))
                    kept += 1
        p("  world min", ["%.1f" % v for v in lo], "max", ["%.1f" % v for v in hi], "points in the box", kept)
    ctl.Shutdown()
    cap.Shutdown()
except Exception:
    p(traceback.format_exc())
log.close()
pts.close()
os._exit(0)
