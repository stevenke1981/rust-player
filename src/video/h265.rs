use rust_h265::{parse_annex_b, Decoder, NalUnit};

use crate::error::{PlayerError, Result};
use crate::video::demux::VideoPacket;
use crate::video::frame::{pack_i420, DecodedFrame};
use crate::video::nal::{extradata_to_annex_b, mp4_sample_to_annex_b};

pub struct H265Decoder {
    decoder: Decoder,
    nal_length_size: u8,
    config_sent: bool,
    extradata_annex_b: Vec<u8>,
}

impl H265Decoder {
    pub fn new(extradata: &[u8]) -> Result<Self> {
        let nal_length_size = crate::video::nal::nal_length_size_from_extradata(extradata);
        let extradata_annex_b = extradata_to_annex_b(extradata);

        Ok(Self {
            decoder: Decoder::new(),
            nal_length_size,
            config_sent: extradata_annex_b.is_empty(),
            extradata_annex_b,
        })
    }

    pub fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<DecodedFrame>> {
        let annex = if packet.is_keyframe && !self.config_sent {
            self.config_sent = true;
            let mut buf = self.extradata_annex_b.clone();
            buf.extend(mp4_sample_to_annex_b(&packet.data, self.nal_length_size));
            buf
        } else {
            mp4_sample_to_annex_b(&packet.data, self.nal_length_size)
        };

        if annex.is_empty() {
            return Ok(Vec::new());
        }

        let nals: Vec<NalUnit> = parse_annex_b(&annex);
        let mut frames = Vec::new();

        for nal in &nals {
            match self.decoder.decode_nal(nal) {
                Ok(Some(frame)) => {
                    if frame.bit_depth != 8 {
                        log::debug!("h265: skipping {}-bit frame", frame.bit_depth);
                        continue;
                    }
                    frames.push(hevc_frame_to_decoded(&frame, packet.pts_secs)?);
                }
                Ok(None) => {}
                Err(e) => log::debug!("h265 decode_nal: {e:?}"),
            }
        }

        Ok(frames)
    }

    pub fn flush(&mut self) -> Result<Vec<DecodedFrame>> {
        let mut frames = Vec::new();
        while let Some(frame) = self.decoder.flush() {
            if frame.bit_depth == 8 {
                if let Ok(decoded) = hevc_frame_to_decoded(&frame, 0.0) {
                    frames.push(decoded);
                }
            }
        }
        Ok(frames)
    }
}

fn hevc_frame_to_decoded(
    frame: &rust_h265::Frame,
    pts_secs: f64,
) -> Result<DecodedFrame> {
    let y = frame
        .y
        .as_u8()
        .ok_or_else(|| PlayerError::VideoDecode("h265: expected 8-bit Y plane".into()))?;
    let u = frame
        .u
        .as_u8()
        .ok_or_else(|| PlayerError::VideoDecode("h265: expected 8-bit U plane".into()))?;
    let v = frame
        .v
        .as_u8()
        .ok_or_else(|| PlayerError::VideoDecode("h265: expected 8-bit V plane".into()))?;

    let w = frame.width as usize;
    Ok(pack_i420(pts_secs, frame.width, frame.height, y, w, u, w / 2, v, w / 2))
}