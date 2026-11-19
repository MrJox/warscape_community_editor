r"""Reads a RenderDoc capture of a campaign map frame (Attila's engine: Attila, TDD, Thrones of Britannia) into a folder that the
comparison works from (docs/plans/campaign_tilemap.md, Phase 1). Runs inside RenderDoc's python, through the hidden wrapper:

    sh wce_scratch/py/rd_hidden.sh scripts/re/rd_campaign_frame.py 1500 'RD_CAPTURE=D:\renderdoc\x.rdc' 'RD_OUT=<folder>'

Writes to RD_OUT:
- events.csv: every draw, dispatch and clear: event, kind, indices, instances, its render targets and depth (resource ids), the vertex and
  pixel shaders (resource ids), their constant buffers' names, the textures bound (slot name = resource id), depth and blend states, the
  mesh (index offset, base vertex, vertex buffer 0's offset: the same mesh draws with the same three).
- shaders.csv: each shader once: resource id, stage, the DXBC checksum (the key `campaign_frame.py` names it by from the game's fxc
  files), its constant buffers.
- instances.csv: the per-instance vertex data of every instanced draw (event, instance, the floats of its first per-instance buffer).
- textures.csv: every texture a draw reads: resource id, size, format, mips, array size, cube, and the SHA-1 of its top mip's bytes (the
  key a pack's DDS is matched by).
- constants.json: the constant buffers of the first draw of each group (consecutive draws with the same targets and shaders), stored
  once per distinct content.
- camera.json: `camera_VS_PS` at the terrain resolve (the main view), the viewport, and the key events below.
- targets/ (DDS in the targets' own formats, listed in targets.json): the G-buffer's targets and depth after the G-buffer pass, the lit HDR target before tone mapping, the screen before the UI,
  and the G-buffer before and after the skinned draws (armies, agents: the out-of-scope mask, made by `campaign_frame.py`).

The key events are found by the constant buffers the passes read (stable names across the engine's games): the terrain resolve
(`shroud_PS` and `ps_height_map_cb` into three or more targets), the light (`directional_light_PS`), the tone map (`hdr_to_screen_PS`),
the UI (`sprite_VS` after the tone map), the skinned draws (`variant_animation_size` into the G-buffer).
"""
import hashlib
import json
import os
import struct
import traceback

import renderdoc as rd

OUT = os.environ["RD_OUT"]
MAX_INSTANCES = 8192

log = open(os.path.join(OUT, "rd_campaign_frame.log"), "w")


def p(*a):
    log.write(" ".join(str(x) for x in a) + "\n")
    log.flush()


def rid_int(rid):
    return int(rid)


def var_json(v):
    if len(v.members):
        return {m.name: var_json(m) for m in v.members}
    n = v.rows * v.columns
    if v.type == rd.VarType.SInt:
        return list(v.value.s32v[:n])
    if v.type in (rd.VarType.UInt, rd.VarType.Bool):
        return list(v.value.u32v[:n])
    return [float(x) for x in v.value.f32v[:n]]


def main():
    os.makedirs(os.path.join(OUT, "targets"), exist_ok=True)
    cap = rd.OpenCaptureFile()
    cap.OpenFile(os.environ["RD_CAPTURE"], "", None)
    st, ctl = cap.OpenCapture(rd.ReplayOptions(), None)
    sf = ctl.GetStructuredFile()
    texs = {t.resourceId: t for t in ctl.GetTextures()}
    bufs = {b.resourceId: b for b in ctl.GetBuffers()}

    acts = []

    def walk(al):
        for a in al:
            if a.children:
                walk(a.children)
            else:
                acts.append(a)

    walk(ctl.GetRootActions())
    wanted = rd.ActionFlags.Drawcall | rd.ActionFlags.Dispatch | rd.ActionFlags.Clear
    prev_of = {}
    for i, a in enumerate(acts):
        prev_of[a.eventId] = acts[i - 1].eventId if i else a.eventId
    p("actions", len(acts))

    shaders = {}
    events = []
    instances = open(os.path.join(OUT, "instances.csv"), "w")
    instances.write("event,instance,floats\n")
    tex_used = set()
    groups = []
    cb_store = {}
    last_key = None

    def shader_info(s, stg):
        refl = s.GetShaderReflection(stg)
        if refl is None:
            return None, []
        rid = s.GetShader(stg)
        cbs = [c.name for c in refl.constantBlocks]
        k = rid_int(rid)
        if k not in shaders:
            raw = bytes(refl.rawBytes)
            i = raw.find(b"DXBC")
            chk = raw[i + 4:i + 20].hex() if i >= 0 else ""
            shaders[k] = (str(stg).split(".")[-1], chk, refl.entryPoint, cbs)
        return k, cbs

    def dump_cbuffers(s):
        pipe = s.GetGraphicsPipelineObject()
        out = {}
        for stg, sn in ((rd.ShaderStage.Vertex, "VS"), (rd.ShaderStage.Pixel, "PS"), (rd.ShaderStage.Compute, "CS")):
            refl = s.GetShaderReflection(stg)
            if refl is None:
                continue
            sh = s.GetShader(stg)
            for ci, cb in enumerate(refl.constantBlocks):
                try:
                    bd = s.GetConstantBlock(stg, ci, 0).descriptor
                    vs = ctl.GetCBufferVariableContents(pipe, sh, stg, refl.entryPoint, ci, bd.resource, bd.byteOffset, bd.byteSize)
                    content = {v.name: var_json(v) for v in vs}
                except Exception as e:
                    content = {"error": str(e)}
                text = json.dumps(content, sort_keys=True)
                h = hashlib.sha1(text.encode()).hexdigest()[:16]
                cb_store.setdefault(h, content)
                out["%s:%s" % (sn, cb.name)] = h
        return out

    for a in acts:
        if not (a.flags & wanted):
            continue
        ctl.SetFrameEvent(a.eventId, False)
        s = ctl.GetPipelineState()
        rts = [rid_int(o.resource) for o in s.GetOutputTargets() if o.resource != rd.ResourceId.Null()]
        dt = s.GetDepthTarget().resource
        depth = rid_int(dt) if dt != rd.ResourceId.Null() else 0
        kind = "clear" if a.flags & rd.ActionFlags.Clear else ("dispatch" if a.flags & rd.ActionFlags.Dispatch else "draw")
        row = {"event": a.eventId, "kind": kind, "name": a.GetName(sf), "indices": a.numIndices, "instances": a.numInstances,
               "rts": rts, "depth": depth, "vs": 0, "ps": 0, "cs": 0, "vs_cb": [], "ps_cb": [], "cs_cb": [], "tex": [],
               "mesh": "", "depth_state": "", "blend": ""}
        if kind != "clear":
            for stg, k in ((rd.ShaderStage.Vertex, "vs"), (rd.ShaderStage.Pixel, "ps"), (rd.ShaderStage.Compute, "cs")):
                sid, cbs = shader_info(s, stg)
                if sid is not None:
                    row[k] = sid
                    row[k + "_cb"] = cbs
                    refl = s.GetShaderReflection(stg)
                    for r in s.GetReadOnlyResources(stg):
                        rid = r.descriptor.resource
                        if rid in texs:
                            idx = r.access.index
                            nm = refl.readOnlyResources[idx].name if idx < len(refl.readOnlyResources) else "?"
                            row["tex"].append("%s=%d" % (nm, rid_int(rid)))
                            tex_used.add(rid)
        if kind == "draw":
            d3 = ctl.GetD3D11PipelineState()
            ds = d3.outputMerger.depthStencilState
            bl = d3.outputMerger.blendState.blends[0]
            row["depth_state"] = "%d%d%s" % (int(ds.depthEnable), int(ds.depthWrites), str(ds.depthFunction).split(".")[-1])
            row["blend"] = "%d" % int(bl.enabled)
            vbs = s.GetVBuffers()
            vb0 = vbs[0].byteOffset if len(vbs) and vbs[0].resourceId != rd.ResourceId.Null() else 0
            row["mesh"] = "%d:%d:%d" % (a.indexOffset, a.baseVertex, vb0)
            inst_slots = sorted(set(ia.vertexBuffer for ia in s.GetVertexInputs() if ia.perInstance))
            if inst_slots and a.numInstances >= 1:
                vb = vbs[inst_slots[0]]
                if vb.resourceId in bufs and vb.byteStride >= 4:
                    n = min(a.numInstances, MAX_INSTANCES)
                    raw = ctl.GetBufferData(vb.resourceId, vb.byteOffset + a.instanceOffset * vb.byteStride, n * vb.byteStride)
                    nf = vb.byteStride // 4
                    for k in range(min(n, len(raw) // vb.byteStride)):
                        vals = struct.unpack_from("<%df" % nf, raw, k * vb.byteStride)
                        instances.write("%d,%d,%s\n" % (a.eventId, k, " ".join("%.7g" % x for x in vals)))

            key = (tuple(rts), depth, row["vs"], row["ps"])
            if key != last_key:
                groups.append({"event": a.eventId, "rts": rts, "depth": depth, "vs": row["vs"], "ps": row["ps"],
                               "cbuffers": dump_cbuffers(s)})
                last_key = key
        elif kind == "dispatch":
            last_key = None
        events.append(row)

    instances.close()
    p("events", len(events), "shaders", len(shaders), "groups", len(groups), "cbuffer contents", len(cb_store))

    with open(os.path.join(OUT, "events.csv"), "w") as f:
        f.write("event,kind,name,indices,instances,rts,depth,vs,ps,cs,vs_cb,ps_cb,cs_cb,tex,mesh,depth_state,blend\n")
        for r in events:
            f.write(",".join(str(x) for x in (
                r["event"], r["kind"], r["name"].replace(",", ";"), r["indices"], r["instances"], " ".join(map(str, r["rts"])),
                r["depth"], r["vs"], r["ps"], r["cs"], " ".join(r["vs_cb"]), " ".join(r["ps_cb"]), " ".join(r["cs_cb"]),
                " ".join(r["tex"]), r["mesh"], r["depth_state"], r["blend"])) + "\n")

    with open(os.path.join(OUT, "shaders.csv"), "w") as f:
        f.write("id,stage,dxbc,entry,cbuffers\n")
        for k, (stg, chk, entry, cbs) in sorted(shaders.items()):
            f.write("%d,%s,%s,%s,%s\n" % (k, stg, chk, entry, " ".join(cbs)))

    with open(os.path.join(OUT, "constants.json"), "w") as f:
        json.dump({"groups": groups, "cbuffers": cb_store}, f)

    # The textures: their keys (the top mip's bytes).
    with open(os.path.join(OUT, "textures.csv"), "w") as f:
        f.write("id,width,height,format,mips,array,cube,sha1_top\n")
        for rid in sorted(tex_used, key=rid_int):
            t = texs[rid]
            h = ""
            try:
                if t.width * t.height <= 4096 * 4096 and not (t.creationFlags & rd.TextureCategory.ColorTarget) and \
                        not (t.creationFlags & rd.TextureCategory.DepthTarget):
                    h = hashlib.sha1(bytes(ctl.GetTextureData(rid, rd.Subresource(0, 0, 0)))).hexdigest()
            except Exception as e:
                h = "error " + str(e).replace(",", ";")
            f.write("%d,%d,%d,%s,%d,%d,%d,%s\n" % (rid_int(rid), t.width, t.height, t.format.Name(), t.mips, t.arraysize,
                                                   int(t.cubemap), h))

    # The key events.
    def first(pred, after=0):
        for r in events:
            if r["event"] > after and pred(r):
                return r
        return None

    resolve = first(lambda r: r["kind"] == "draw" and "shroud_PS" in r["ps_cb"] and "ps_height_map_cb" in r["ps_cb"]
                    and len(r["rts"]) >= 3)
    light = first(lambda r: r["kind"] == "draw" and "directional_light_PS" in r["ps_cb"])
    tone = first(lambda r: r["kind"] == "draw" and "hdr_to_screen_PS" in r["ps_cb"])
    ui = first(lambda r: r["kind"] == "draw" and "sprite_VS" in r["vs_cb"], tone["event"] if tone else 0)
    gbuf = resolve["rts"] if resolve else []
    skinned = [r for r in events if r["kind"] == "draw" and r["rts"] == gbuf and "variant_animation_size" in r["vs_cb"]]
    keys = {"resolve": resolve and resolve["event"], "light": light and light["event"], "tone_map": tone and tone["event"],
            "ui": ui and ui["event"], "gbuffer_targets": gbuf, "gbuffer_depth": resolve and resolve["depth"],
            "light_target": light and light["rts"], "screen": tone and tone["rts"],
            "skinned": [skinned[0]["event"], skinned[-1]["event"]] if skinned else None,
            "skinned_draws": len(skinned)}
    if skinned:
        between = [r for r in events if skinned[0]["event"] <= r["event"] <= skinned[-1]["event"] and r["kind"] == "draw"
                   and r not in skinned]
        keys["skinned_span_other_draws"] = len(between)
    p("keys", json.dumps(keys))

    # The camera of the main view.
    cam = {"keys": keys}
    if resolve:
        ctl.SetFrameEvent(resolve["event"], True)
        s = ctl.GetPipelineState()
        refl = s.GetShaderReflection(rd.ShaderStage.Pixel)
        pipe = s.GetGraphicsPipelineObject()
        for ci, cb in enumerate(refl.constantBlocks):
            if cb.name == "camera_VS_PS":
                bd = s.GetConstantBlock(rd.ShaderStage.Pixel, ci, 0).descriptor
                vs = ctl.GetCBufferVariableContents(pipe, s.GetShader(rd.ShaderStage.Pixel), rd.ShaderStage.Pixel, refl.entryPoint, ci,
                                                    bd.resource, bd.byteOffset, bd.byteSize)
                cam["camera_VS_PS"] = {v.name: var_json(v) for v in vs}
        vp = s.GetViewport(0)
        cam["viewport"] = [vp.x, vp.y, vp.width, vp.height]
    with open(os.path.join(OUT, "camera.json"), "w") as f:
        json.dump(cam, f, indent=1)

    # The targets, as DDS (their own formats: `campaign_frame` reads them), listed with their formats in targets.json.
    saved = {}

    def save(event, rid, name):
        ctl.SetFrameEvent(event, True)
        t = [t for t in texs if rid_int(t) == rid][0]
        ts = rd.TextureSave()
        ts.resourceId = t
        ts.mip = 0
        ts.alpha = rd.AlphaMapping.Preserve
        ts.destType = rd.FileType.DDS
        path = os.path.join(OUT, "targets", name + ".dds")
        ok = ctl.SaveTexture(ts, path)
        d = texs[t]
        saved[name] = {"event": event, "resource": rid, "width": d.width, "height": d.height, "format": d.format.Name(), "ok": bool(ok)}
        p("saved", name, "at", event, ok)

    if resolve and light:
        done = prev_of[light["event"]]
        for i, rid in enumerate(gbuf):
            save(done, rid, "gbuffer%d" % i)
        if resolve["depth"]:
            save(done, resolve["depth"], "depth")
    if light and tone:
        save(prev_of[tone["event"]], light["rts"][0], "hdr")
    if tone and ui:
        save(prev_of[ui["event"]], tone["rts"][0], "screen")
    if skinned:
        before = prev_of[skinned[0]["event"]]
        for i, rid in enumerate(gbuf):
            save(before, rid, "skinned_before%d" % i)
            save(skinned[-1]["event"], rid, "skinned_after%d" % i)
        if resolve["depth"]:
            save(before, resolve["depth"], "skinned_before_depth")
            save(skinned[-1]["event"], resolve["depth"], "skinned_after_depth")

    with open(os.path.join(OUT, "targets.json"), "w") as f:
        json.dump(saved, f, indent=1)

    ctl.Shutdown()
    cap.Shutdown()
    p("done")


try:
    main()
except Exception:
    p(traceback.format_exc())
log.close()
