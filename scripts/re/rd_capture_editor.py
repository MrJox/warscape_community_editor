"""Launches the editor under RenderDoc (on the hidden desktop, through rd_hidden.sh), waits for RD_READY (a file the editor's script
writes when the scene is set), triggers one capture and waits for it. Env: RD_EXE, RD_WORKDIR, RD_ARGS, RD_ENV ("NAME=value;..."),
RD_CAPTURE_TEMPLATE, RD_READY, RD_LOG. The frame captured is the editor window's (its size, its MSAA), not a `capture` command's
offscreen image: a pixel of a 1920 x 1080 game capture is at the same vertical fraction there, the same horizontal one about the
centre (the vertical field of view is fixed). The script should keep drawing (`frames 3000`) after writing RD_READY."""
import os
import time
import traceback

import renderdoc as rd

log = open(os.environ["RD_LOG"], "w")


def p(*a):
    log.write(" ".join(str(x) for x in a) + "\n")
    log.flush()


def main():
    env = []
    for kv in [e for e in os.environ.get("RD_ENV", "").split(";") if e]:
        k, v = kv.split("=", 1)
        env.append(rd.EnvironmentModification(rd.EnvMod.Set, rd.EnvSep.NoSep, k, v))
    opts = rd.CaptureOptions()
    res = rd.ExecuteAndInject(os.environ["RD_EXE"], os.environ["RD_WORKDIR"], os.environ["RD_ARGS"], env,
                              os.environ["RD_CAPTURE_TEMPLATE"], opts, False)
    p("inject", res.result, "ident", res.ident)
    tc = rd.CreateTargetControl("", res.ident, "wce", True)
    if tc is None:
        p("no target control")
        return
    ready = os.environ["RD_READY"]
    t0 = time.time()
    while not os.path.exists(ready) and time.time() - t0 < 1500:
        tc.ReceiveMessage(None)
        time.sleep(0.5)
    p("ready after", round(time.time() - t0), "s:", os.path.exists(ready))
    time.sleep(3)
    tc.TriggerCapture(1)
    t1 = time.time()
    while time.time() - t1 < 300:
        msg = tc.ReceiveMessage(None)
        if msg.type == rd.TargetControlMessageType.NewCapture:
            p("capture", msg.newCapture.path)
            break
        if msg.type == rd.TargetControlMessageType.Disconnected:
            p("disconnected")
            break
        time.sleep(0.2)
    tc.Shutdown()


try:
    main()
except Exception:
    p(traceback.format_exc())
log.close()
