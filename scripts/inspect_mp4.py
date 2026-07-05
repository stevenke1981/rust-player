import struct
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "assets/test_av1.mp4"
data = open(path, "rb").read()

# Read first sample via stco - simplified: find mdat payload start
mdat = data.find(b"mdat")
print(f"mdat offset: {mdat}")
payload = data[mdat + 8 : mdat + 8 + 128]
print("mdat payload hex:", payload.hex())

# Try parse leb128 / be32 lengths
off = 0
for i in range(5):
    if off + 4 > len(payload):
        break
    be32 = struct.unpack(">I", payload[off:off+4])[0]
    le32 = struct.unpack("<I", payload[off:off+4])[0]
    print(f"@{off}: be32={be32} le32={le32} byte={payload[off]:#x}")
    off += 1