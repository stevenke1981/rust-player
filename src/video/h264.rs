use openh264::decoder::Decoder;
use openh264::formats::YUVSource;

use crate::error::{PlayerError, Result};
use crate::video::demux::VideoPacket;
use crate::video::frame::{pack_i420, DecodedFrame};
use crate::video::nal::{extradata_to_annex_b, mp4_sample_to_annex_b};

pub struct H264Decoder {
    decoder: Decoder,
    nal_length_size: u8,
    config_sent: bool,
    extradata_annex_b: Vec<u8>,
}

impl H264Decoder {
    pub fn new(extradata: &[u8]) -> Result<Self> {
        let decoder = Decoder::new().map_err(|e| PlayerError::VideoDecode(e.to_string()))?;
        let nal_length_size = crate::video::nal::nal_length_size_from_extradata(extradata);
        let extradata_annex_b = extradata_to_annex_b(extradata);

        Ok(Self {
            decoder,
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

        let mut frames = Vec::new();
        match self.decoder.decode(&annex) {
            Ok(Some(yuv)) => {
                frames.push(yuv_to_frame(&yuv, packet.pts_secs)?);
            }
            Ok(None) => {}
            Err(e) => {
                log::debug!("h264 decode: {e}");
            }
        }

        Ok(frames)
    }

    pub fn flush(&mut self) -> Result<Vec<DecodedFrame>> {
        let flushed = self
            .decoder
            .flush_remaining()
            .map_err(|e| PlayerError::VideoDecode(e.to_string()))?;
        Ok(flushed
            .into_iter()
            .filter_map(|yuv| yuv_to_frame(&yuv, 0.0).ok())
            .collect())
    }
}

fn yuv_to_frame(yuv: &openh264::decoder::DecodedYUV<'_>, pts_secs: f64) -> Result<DecodedFrame> {
    let (width, height) = yuv.dimensions();
    let (y_stride, u_stride, v_stride) = yuv.strides();
    Ok(pack_i420(
        pts_secs,
        width as u32,
        height as u32,
        yuv.y(),
        y_stride,
        yuv.u(),
        u_stride,
        yuv.v(),
        v_stride,
    ))
}