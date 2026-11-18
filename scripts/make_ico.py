import struct, sys, pathlib

root = pathlib.Path(__file__).resolve().parent.parent
sizes = [16, 24, 32, 48, 64, 128, 256]
entries = []
for n in sizes:
    data = (root / "target" / "icon_sizes" / f"icon_{n}.png").read_bytes()
    entries.append((n, data))

out = bytearray(struct.pack("<HHH", 0, 1, len(entries)))
offset = 6 + 16 * len(entries)
blobs = bytearray()
for n, data in entries:
    w = 0 if n >= 256 else n
    out += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(data), offset)
    blobs += data
    offset += len(data)
out += blobs
(root / "assets" / "icon.ico").write_bytes(bytes(out))
print("icon.ico", len(out), "bytes,", len(entries), "sizes")