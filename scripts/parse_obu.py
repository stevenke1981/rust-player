import struct, sys

path = sys.argv[1]
# read sample via manual mp4 parse - use ffprobe style
# Just read file and find first large chunk in mdat using stsz from moov is hard without full parser

# Use mp4 crate via subprocess to rust-player - instead parse moov for first stco offset
data = open(path, 'rb').read()

def find_box(data, box_type, start=0):
    i = start
    while i + 8 <= len(data):
        size = struct.unpack('>I', data[i:i+4])[0]
        typ = data[i+4:i+8]
        if size < 8:
            i += 1
            continue
        if typ == box_type:
            return i, size
        i += 1
    return None, None

moov_pos, moov_size = find_box(data, b'moov')
print('moov', moov_pos, moov_size)

def leb128_read(buf, off):
    val = 0
    shift = 0
    i = off
    while i < len(buf):
        b = buf[i]
        val |= (b & 0x7f) << shift
        i += 1
        if (b & 0x80) == 0:
            return val, i - off
        shift += 7
    return None, 0

# find first stco entry
stco_pos, _ = find_box(data, b'stco')
if stco_pos:
    # skip header 12 + version 4 + count 4
    off = stco_pos + 16
    count = struct.unpack('>I', data[off:off+4])[0]
    off += 4
    first_offset = struct.unpack('>I', data[off:off+4])[0]
    print('stco count', count, 'first offset', first_offset)
    sample = data[first_offset:first_offset+128]
    print('sample head', sample[:32].hex())
    size, n = leb128_read(sample, 0)
    print('leb128 size', size, 'bytes', n)
    if size and n:
        obu = sample[n:n+min(size, 32)]
        print('first obu head', obu.hex())