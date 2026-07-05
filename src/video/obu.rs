/// Convert MP4 AV1 sample to raw OBU byte stream for rav1d.
pub fn mp4_sample_to_obu_stream(sample: &[u8]) -> Vec<u8> {
    if sample.is_empty() {
        return Vec::new();
    }

    // AV1 OBU forbidden bit is 0 at the high bit of the first byte.
    if sample[0] & 0x80 == 0 {
        return sample.to_vec();
    }

    let mut out = Vec::with_capacity(sample.len());
    let mut i = 0usize;
    while i < sample.len() {
        let Some((obu_size, leb_len)) = read_leb128(&sample[i..]) else {
            break;
        };
        i += leb_len;
        if obu_size == 0 || i + obu_size > sample.len() {
            break;
        }
        out.extend_from_slice(&sample[i..i + obu_size]);
        i += obu_size;
    }

    if out.is_empty() {
        sample.to_vec()
    } else {
        out
    }
}

fn read_leb128(data: &[u8]) -> Option<(usize, usize)> {
    let mut value = 0usize;
    let mut shift = 0u32;
    for (i, &byte) in data.iter().enumerate() {
        value |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_raw_obu() {
        let sample = [0x12u8, 0x00, 0x0a, 0x07];
        assert_eq!(mp4_sample_to_obu_stream(&sample), sample.to_vec());
    }

    #[test]
    fn small_size_prefix_as_raw_obu() {
        // Single-byte values with forbidden bit clear are treated as raw OBU headers.
        let sample = [0x02u8, 0x0a, 0x0b];
        assert_eq!(mp4_sample_to_obu_stream(&sample), sample.to_vec());
    }
}