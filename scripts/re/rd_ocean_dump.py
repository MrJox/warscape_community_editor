r"""Dumps the ocean draws of a RenderDoc capture of Attila: the vertex and index buffers, the constant buffers, the sea bed info texture
(DDS and PNG) and the post-VS data, with a summary in dump.txt (docs/re/water.md, "Settled by a RenderDoc capture").

    set RD_CAPTURE=D:\renderdoc\coast.rdc & set RD_OUT=<folder> & qrenderdoc --python scripts\re\rd_ocean_dump.py

The ocean events are found by their shaders' constant buffers (`ocean_VS`); the last one is the colour pass.
"""
import os, traceback
import renderdoc as rd
D = os.environ["RD_OUT"]
f = open(os.path.join(D, "dump.txt"), "w")
def p(*a):
    f.write(" ".join(str(x) for x in a) + "\n"); f.flush()
def save(name, b):
    open(os.path.join(D, name), "wb").write(bytes(b)); p("saved", name, len(b))
try:
    cap = rd.OpenCaptureFile(); cap.OpenFile(r"D:\renderdoc\coast.rdc", "", None)
    st, ctl = cap.OpenCapture(rd.ReplayOptions(), None)
    texs = {t.resourceId: t for t in ctl.GetTextures()}
    bufs = {b.resourceId: b for b in ctl.GetBuffers()}
    events = []
    def walk(acts):
        for a in acts:
            if a.flags & rd.ActionFlags.Drawcall:
                ctl.SetFrameEvent(a.eventId, False)
                vs = ctl.GetPipelineState().GetShaderReflection(rd.ShaderStage.Vertex)
                if vs is not None and any(c.name == "ocean_VS" for c in vs.constantBlocks):
                    events.append(a.eventId)
            walk(a.children)
    walk(ctl.GetRootActions())
    p("ocean events", events)
    last = events[-1]
    for ev in events:
        ctl.SetFrameEvent(ev, True)
        s = ctl.GetPipelineState()
        p("== event", ev, "topology", s.GetPrimitiveTopology())
        for i, a in enumerate(s.GetVertexInputs()):
            p(" input", a.name, "vb", a.vertexBuffer, "off", a.byteOffset, "fmt", a.format.Name(), "perinst", a.perInstance)
        for i, vb in enumerate(s.GetVBuffers()):
            if vb.resourceId == rd.ResourceId.Null(): continue
            size = bufs[vb.resourceId].length if vb.resourceId in bufs else 0
            p(" vb", i, "stride", vb.byteStride, "offset", vb.byteOffset, "buffer bytes", size)
            if ev == last: save("ocean_vb%d.bin" % i, ctl.GetBufferData(vb.resourceId, vb.byteOffset, 0))
        ib = s.GetIBuffer()
        p(" ib stride", ib.byteStride, "offset", ib.byteOffset)
        if ev == last: save("ocean_ib.bin", ctl.GetBufferData(ib.resourceId, ib.byteOffset, 0))
        for stage, sn in ((rd.ShaderStage.Vertex, "vs"), (rd.ShaderStage.Pixel, "ps")):
            refl = s.GetShaderReflection(stage)
            if refl is None: continue
            for ci, cb in enumerate(refl.constantBlocks):
                ud = s.GetConstantBlock(stage, ci, 0)
                d = ud.descriptor
                data = ctl.GetBufferData(d.resource, d.byteOffset, d.byteSize)
                p(" cb", sn, cb.name, "bytes", len(data))
                if ev == last: save("ocean_%s_%s.bin" % (sn, cb.name), data)
                vars_ = []
                for v in cb.variables:
                    vars_.append((v.name, v.byteOffset, v.type.rows, v.type.columns, v.type.elements))
                p("   vars", vars_)
            for r in s.GetReadOnlyResources(stage):
                rid = r.descriptor.resource
                t = texs.get(rid)
                if t is not None:
                    p(" tex", sn, "slot", r.access.index, t.width, t.height, t.format.Name(), "mips", t.mips, "name", ctl.GetResourceName(rid) if hasattr(ctl, "GetResourceName") else "")
                    if ev == last and sn == "vs":
                        ts = rd.TextureSave(); ts.resourceId = rid; ts.destType = rd.FileType.DDS; ts.mip = 0
                        p("   save dds", ctl.SaveTexture(ts, os.path.join(D, "ocean_vs_t%d.dds" % r.access.index)))
                        ts.destType = rd.FileType.PNG; ts.alpha = rd.AlphaMapping.Preserve
                        p("   save png", ctl.SaveTexture(ts, os.path.join(D, "ocean_vs_t%d.png" % r.access.index)))
        if ev == last:
            m = ctl.GetPostVSData(0, 0, rd.MeshDataStage.VSOut)
            p(" postvs verts", m.numIndices, "stride", m.vertexByteStride, "offset", m.vertexByteOffset)
            save("ocean_postvs.bin", ctl.GetBufferData(m.vertexResourceId, m.vertexByteOffset, 0))
            sig = s.GetShaderReflection(rd.ShaderStage.Vertex).outputSignature
            p(" vs outputs", [(o.semanticName, o.semanticIndex, o.compCount) for o in sig])
    ctl.Shutdown(); cap.Shutdown()
except Exception:
    p(traceback.format_exc())
f.close(); os._exit(0)
