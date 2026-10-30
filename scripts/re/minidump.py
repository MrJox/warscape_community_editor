"""A small reader of Windows minidumps (Task Manager's "Create dump file": a full dump, Memory64ListStream), for dump_cbs.py and the
other scripts that read the game's memory. Memory-mapped, so a dump of several GB opens at once.

    d = Dump(path); base, size = d.module("empire.retail.dll"); d.u32(addr); d.read(addr, n); d.cstr(addr, max_len)

`read` returns None when the range is not entirely inside one dumped memory region.
"""
import bisect
import mmap
import struct

MODULE_LIST = 4
MEMORY_LIST = 5
MEMORY64_LIST = 9


class Dump:
    def __init__(self, path):
        self._file = open(path, "rb")
        self.m = mmap.mmap(self._file.fileno(), 0, access=mmap.ACCESS_READ)
        sig, _version, count, rva = struct.unpack_from("<4sIII", self.m, 0)
        if sig != b"MDMP":
            raise ValueError(f"{path}: not a minidump")
        self.streams = {}
        for i in range(count):
            kind, size, at = struct.unpack_from("<III", self.m, rva + 12 * i)
            self.streams[kind] = (at, size)
        self.regions = []  # (start address, size, file offset), sorted by address
        if MEMORY64_LIST in self.streams:
            at, _ = self.streams[MEMORY64_LIST]
            n, offset = struct.unpack_from("<QQ", self.m, at)
            for i in range(n):
                start, size = struct.unpack_from("<QQ", self.m, at + 16 + 16 * i)
                self.regions.append((start, size, offset))
                offset += size
        elif MEMORY_LIST in self.streams:
            at, _ = self.streams[MEMORY_LIST]
            (n,) = struct.unpack_from("<I", self.m, at)
            for i in range(n):
                start, size, data_rva = struct.unpack_from("<QII", self.m, at + 4 + 16 * i)
                self.regions.append((start, size, data_rva))
        self.regions.sort()
        self._starts = [r[0] for r in self.regions]
        self.modules = []  # (name, base, size)
        if MODULE_LIST in self.streams:
            at, _ = self.streams[MODULE_LIST]
            (n,) = struct.unpack_from("<I", self.m, at)
            for i in range(n):
                o = at + 4 + 108 * i
                base, size = struct.unpack_from("<QI", self.m, o)
                (name_rva,) = struct.unpack_from("<I", self.m, o + 20)
                (length,) = struct.unpack_from("<I", self.m, name_rva)
                name = bytes(self.m[name_rva + 4:name_rva + 4 + length]).decode("utf-16-le", "replace")
                self.modules.append((name, base, size))

    def module(self, name):
        """(base, size) of the module whose file name is `name` (case-insensitive), or None."""
        for full, base, size in self.modules:
            if full.replace("/", "\\").rsplit("\\", 1)[-1].lower() == name.lower():
                return base, size
        return None

    def read(self, addr, n):
        i = bisect.bisect_right(self._starts, addr) - 1
        if i < 0:
            return None
        start, size, offset = self.regions[i]
        if addr + n > start + size:
            return None
        o = offset + addr - start
        return bytes(self.m[o:o + n])

    def u32(self, addr):
        raw = self.read(addr, 4)
        return struct.unpack("<I", raw)[0] if raw else 0

    def f32(self, addr, n=1):
        raw = self.read(addr, 4 * n)
        return struct.unpack(f"<{n}f", raw) if raw else None

    def cstr(self, addr, max_len=256):
        out = bytearray()
        for k in range(max_len):
            raw = self.read(addr + k, 1)
            if not raw or raw == b"\0":
                break
            out += raw
        return out.decode("latin-1") if out else None
