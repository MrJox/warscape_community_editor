"""Phase 2 spike: talks to rpfm_server (MCP over HTTP) the way the editor's RpfmServer backend will. Throwaway probe, not a gate.

    python scripts/rpfm_spike.py <step> [args]
"""
import json
import sys
import urllib.request

URL = "http://127.0.0.1:45127/mcp"


class Mcp:
    def __init__(self):
        self.sid = None
        self.n = 0
        self.rpc("initialize", {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "wce-spike", "version": "0"}})
        self.notify("notifications/initialized")

    def _post(self, body):
        h = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
        if self.sid:
            h["Mcp-Session-Id"] = self.sid
        r = urllib.request.urlopen(urllib.request.Request(URL, json.dumps(body).encode(), h), timeout=600)
        self.sid = r.headers.get("Mcp-Session-Id") or self.sid
        return r.read().decode("utf-8", "replace")

    @staticmethod
    def _events(text):
        out = []
        for block in text.split("\n\n"):
            for line in block.splitlines():
                if line.startswith("data: ") and line[6:].strip():
                    out.append(json.loads(line[6:]))
        return out

    def notify(self, method):
        self._post({"jsonrpc": "2.0", "method": method})

    def rpc(self, method, params):
        self.n += 1
        ev = [e for e in self._events(self._post({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params})) if e.get("id") == self.n]
        return ev[0]

    def call(self, tool, **args):
        r = self.rpc("tools/call", {"name": tool, "arguments": args})
        if "error" in r:
            raise RuntimeError(r["error"])
        res = r["result"]
        text = "".join(c.get("text", "") for c in res.get("content", []))
        if res.get("isError"):
            raise RuntimeError(text)
        return json.loads(text) if text[:1] in "[{\"" else text
