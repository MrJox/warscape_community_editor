"""For the events in RD_EVENTS (comma separated) of RD_CAPTURE: the texels of their depth target that the draw changed, the viewport,
and the post-VS clip-space ranges of its vertices (x / w, y / w, z / w), into RD_OUT."""
import os
import struct
import traceback

import renderdoc as rd

out = open(os.environ["RD_OUT"], "w")


def p(*a):
    out.write(" ".join(str(x) for x in a) + "\n")
    out.flush()


def main():
    cap = rd.OpenCaptureFile()
    cap.OpenFile(os.environ["RD_CAPTURE"], "", None)
    st, ctl = cap.OpenCapture(rd.ReplayOptions(), None)
    for ev in [int(e) for e in os.environ["RD_EVENTS"].split(",")]:
        ctl.SetFrameEvent(ev, True)
        ps = ctl.GetPipelineState()
        depth = ps.GetDepthTarget().resource
        vps = ps.GetViewport(0)
        p("event", ev, "depth", int(depth), "viewport", vps.x, vps.y, vps.width, vps.height, "minDepth", vps.minDepth, "maxDepth", vps.maxDepth)
        after = bytes(ctl.GetTextureData(depth, rd.Subresource(0, 0, 0)))
        ctl.SetFrameEvent(ev - 1, True)
        before = bytes(ctl.GetTextureData(depth, rd.Subresource(0, 0, 0)))
        ctl.SetFrameEvent(ev, True)
        n = min(len(after), len(before)) // 4
        changed = sum(1 for k in range(0, n * 4, 4) if after[k:k + 4] != before[k:k + 4])
        p("  texels changed", changed, "of", n)
        mesh = ctl.GetPostVSData(0, 0, rd.MeshDataStage.VSOut)
        if mesh.vertexResourceId != rd.ResourceId.Null() and mesh.numIndices > 0:
            data = bytes(ctl.GetBufferData(mesh.vertexResourceId, mesh.vertexByteOffset, mesh.numIndices * mesh.vertexByteStride))
            xs, ys, zs = [], [], []
            for k in range(min(mesh.numIndices, len(data) // max(mesh.vertexByteStride, 1))):
                x, y, z, w = struct.unpack_from("<4f", data, k * mesh.vertexByteStride)
                if w != 0:
                    xs.append(x / w); ys.append(y / w); zs.append(z / w)
            if xs:
                p("  clip x %.3f .. %.3f  y %.3f .. %.3f  z %.3f .. %.3f" % (min(xs), max(xs), min(ys), max(ys), min(zs), max(zs)))
        else:
            p("  no post-VS data")
    ctl.Shutdown()
    cap.Shutdown()


try:
    main()
except Exception:
    p(traceback.format_exc())
