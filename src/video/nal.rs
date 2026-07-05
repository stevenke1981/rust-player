//! MP4 sample (AVCC/HVCC) to Annex B conversion and extradata parsing.

pub const ANNEX_B_START_CODE: &[u8] = &[0, 0, 0, 1];

/// Convert length-prefixed MP4 sample data to Annex B byte stream.
/// `nal_length_bytes` is the number of bytes per length prefix (typically 4).
pub fn mp4_sample_to_annex_b(data: &[u8], nal_length_bytes: u8) -> Vec<u8> {
    let nal_len_bytes = nal_length_bytes.clamp(1, 4) as usize;
    if nal_len_bytes == 0 || data.len() < nal_len_bytes {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(data.len() + 16);
    let mut offset = 0;

    while offset + nal_len_bytes <= data.len() {
        let nal_len = read_be_uint(&data[offset..], nal_len_bytes);
        offset += nal_len_bytes;
        if nal_len == 0 || offset + nal_len > data.len() {
            break;
        }
        out.extend_from_slice(ANNEX_B_START_CODE);
        out.extend_from_slice(&data[offset..offset + nal_len]);
        offset += nal_len;
    }

    out
}

/// Build Annex B VPS/SPS/PPS prefix from an avcC/hvcC configuration blob.
pub fn extradata_to_annex_b(extradata: &[u8]) -> Vec<u8> {
    if extradata.len() < 6 {
        return Vec::new();
    }

    // Standard AVCC (H.264): byte 5 upper 3 bits must be 111 (reserved).
    if (extradata[5] & 0xe0) == 0xe0 {
        let result = parse_avcc(extradata);
        if !result.is_empty() {
            return result;
        }
    }

    // Standard HVCC (H.265): needs at least 23 bytes.
    if extradata.len() >= 23 {
        if let Some(result) = parse_hvcc(extradata) {
            if !result.is_empty() {
                return result;
            }
        }
    }

    // Fallback: some files have non-standard AVCC where byte 5 reserved bits
    // are not set correctly. Try to parse as AVCC directly.
    let result = parse_avcc(extradata);
    if !result.is_empty() {
        log::debug!(
            "extradata_to_annex_b: fallback AVCC parse succeeded (byte5=0x{:02x}, len={})",
            extradata[5], extradata.len()
        );
        return result;
    }

    Vec::new()
}

fn parse_avcc(avcc: &[u8]) -> Vec<u8> {
    if avcc.len() < 7 {
        return Vec::new();
    }

    let nal_length_size = (avcc[4] & 0x03) + 1;
    let mut out = Vec::new();
    let mut offset = 5;

    if offset >= avcc.len() {
        return out;
    }
    let num_sps = avcc[offset] & 0x1f;
    offset += 1;

    for _ in 0..num_sps {
        if offset + 2 > avcc.len() {
            break;
        }
        let len = u16::from_be_bytes([avcc[offset], avcc[offset + 1]]) as usize;
        offset += 2;
        if offset + len > avcc.len() {
            break;
        }
        out.extend_from_slice(ANNEX_B_START_CODE);
        out.extend_from_slice(&avcc[offset..offset + len]);
        offset += len;
    }

    if offset >= avcc.len() {
        return out;
    }
    let num_pps = avcc[offset] as usize;
    offset += 1;

    for _ in 0..num_pps {
        if offset + 2 > avcc.len() {
            break;
        }
        let len = u16::from_be_bytes([avcc[offset], avcc[offset + 1]]) as usize;
        offset += 2;
        if offset + len > avcc.len() {
            break;
        }
        out.extend_from_slice(ANNEX_B_START_CODE);
        out.extend_from_slice(&avcc[offset..offset + len]);
        offset += len;
    }

    let _ = nal_length_size;
    out
}

fn parse_hvcc(hvcc: &[u8]) -> Option<Vec<u8>> {
    if hvcc.len() < 23 {
        return None;
    }

    let num_arrays = hvcc[22] as usize;
    let mut offset = 23;
    let mut out = Vec::new();

    for _ in 0..num_arrays {
        if offset + 3 > hvcc.len() {
            break;
        }
        offset += 1; // array header (completeness + NAL type)
        let num_nalus = u16::from_be_bytes([hvcc[offset], hvcc[offset + 1]]) as usize;
        offset += 2;

        for _ in 0..num_nalus {
            if offset + 2 > hvcc.len() {
                break;
            }
            let len = u16::from_be_bytes([hvcc[offset], hvcc[offset + 1]]) as usize;
            offset += 2;
            if offset + len > hvcc.len() {
                break;
            }
            out.extend_from_slice(ANNEX_B_START_CODE);
            out.extend_from_slice(&hvcc[offset..offset + len]);
            offset += len;
        }
    }

    Some(out)
}

fn read_be_uint(data: &[u8], bytes: usize) -> usize {
    match bytes {
        1 => data[0] as usize,
        2 => u16::from_be_bytes([data[0], data[1]]) as usize,
        4 => u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize,
        _ => 0,
    }
}

pub fn nal_length_size_from_extradata(extradata: &[u8]) -> u8 {
    // Standard AVCC (H.264) / HVCC (H.265) layout:
    //   byte 4 = reserved(6 bits) | lengthSizeMinusOne(2 bits)
    // See ISO 14496-15 §5.3.3.1.1 (AVCDecoderConfigurationRecord) and §8.3.3.1.1.
    if extradata.len() >= 5 {
        let nalu_size = (extradata[4] & 0x03) + 1;
        log::debug!(
            "nal_length_size_from_extradata: len={}, byte4=0x{:02x} → nalu_size={}",
            extradata.len(),
            extradata[4],
            nalu_size
        );
        return nalu_size;
    }
    log::debug!(
        "nal_length_size_from_extradata: short extradata (len={}), defaulting to 4",
        extradata.len()
    );
    4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp4_sample_length_prefix_to_annex_b() {
        let sample = [0, 0, 0, 3, 0xAB, 0xCD, 0xEF];
        let annex = mp4_sample_to_annex_b(&sample, 4);
        assert_eq!(&annex[..4], ANNEX_B_START_CODE);
        assert_eq!(&annex[4..], &[0xAB, 0xCD, 0xEF]);
    }

    #[test]
    fn avcc_extradata_parses_sps_pps() {
        let avcc = [
            1, 0x64, 0x00, 0x1f, 0xff, 0xe1, 0x00, 0x03, 0xAA, 0xBB, 0xCC, 0x01, 0x00, 0x02,
            0x11, 0x22,
        ];
        let annex = parse_avcc(&avcc);
        assert!(annex.windows(4).any(|w| w == ANNEX_B_START_CODE));
        assert!(annex.contains(&0xAA));
        assert!(annex.contains(&0x11));
    }
}