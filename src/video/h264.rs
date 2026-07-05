use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use openh264_sys2::TagBufferInfo;

use crate::error::{PlayerError, Result};
use crate::video::demux::VideoPacket;
use crate::video::frame::{pack_i420, DecodedFrame};
use crate::video::nal::{extradata_to_annex_b, mp4_sample_to_annex_b};

pub struct H264Decoder {
    decoder: Decoder,
    nal_length_size: u8,
    extradata_annex_b: Vec<u8>,
}

impl H264Decoder {
    pub fn new(extradata: &[u8]) -> Result<Self> {
        let decoder = {
            let mut d = Decoder::new().map_err(|e| PlayerError::VideoDecode(e.to_string()))?;
            // Enable openh264 error concealment (ERROR_CON_SLICE_COPY_CROSS_IDR).
            // Default is disabled (eEcActiveIdc=0, ERROR_CON_DISABLE). Setting to 2
            // enables per-slice copy across IDR boundaries, allowing recovery from
            // corrupt or missing frames instead of returning Native:16 errors.
            #[allow(unsafe_code)]
            unsafe {
                use std::os::raw::c_int;
                // DECODER_OPTION_ERROR_CON_IDC = 8 in openh264_sys2
                let mut val: c_int = 2;
                d.raw_api()
                    .set_option(8 as c_int, std::ptr::addr_of_mut!(val).cast());
            }
            d
        };
        let nal_length_size = crate::video::nal::nal_length_size_from_extradata(extradata);
        let extradata_annex_b = extradata_to_annex_b(extradata);

        log::debug!(
            "H264Decoder::new: extradata_len={} extradata_annex_b={} nal_length_size={} extradata_hex=[{}]",
            extradata.len(),
            extradata_annex_b.len(),
            nal_length_size,
            extradata.iter().take(32).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
        );

        Ok(Self {
            decoder,
            nal_length_size,
            extradata_annex_b,
        })
    }

    pub fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<DecodedFrame>> {
        let sample_annex = mp4_sample_to_annex_b(&packet.data, self.nal_length_size);

        // Prepend SPS/PPS extradata to EVERY keyframe (standard AVCC→Annex B practice).
        // openh264 tolerates repeated parameter sets, and re-sending them on each keyframe
        // makes seek recovery and error resilience robust. If extradata could not be parsed
        // (empty), the parameter sets are expected in-band and we decode the sample as-is.
        let annex = if packet.is_keyframe && !self.extradata_annex_b.is_empty() {
            let mut buf =
                Vec::with_capacity(self.extradata_annex_b.len() + sample_annex.len());
            buf.extend_from_slice(&self.extradata_annex_b);
            buf.extend_from_slice(&sample_annex);
            buf
        } else {
            sample_annex
        };

        if annex.is_empty() {
            log::debug!(
                "h264 decode: empty annex (is_keyframe={}, nal_len={})",
                packet.is_keyframe, self.nal_length_size
            );
            return Ok(Vec::new());
        }

        let mut frames = Vec::new();

        // Use raw decode_frame_no_delay directly because the safe openh264
        // crate's decode() treats any non-zero DECODING_STATE as Err and
        // discards the frame output. dsBitstreamError (Native:16) in
        // particular is common with non-conformant bitstreams but openh264
        // still produces valid output.
        let mut dst = [std::ptr::null_mut::<u8>(); 3];
        let mut buffer_info = TagBufferInfo::default();

        let dec_state = unsafe {
            self.decoder
                .raw_api()
                .decode_frame_no_delay(
                    annex.as_ptr(),
                    annex.len() as std::os::raw::c_int,
                    std::ptr::from_mut(&mut dst).cast(),
                    std::ptr::addr_of_mut!(buffer_info),
                )
        };

        // Log non-zero state for diagnostics but never discard valid output.
        if dec_state != 0 {
            log::trace!(
                "openh264 decode: state={dec_state} pts={:.3} annex_len={}",
                packet.pts_secs,
                annex.len()
            );
        }

        // iBufferStatus == 1 means a decoded frame is ready in dst[0..2].
        if buffer_info.iBufferStatus != 0 {
            let info = unsafe { &buffer_info.UsrData.sSystemBuffer };
            let width = info.iWidth as u32;
            let height = info.iHeight as u32;

            if width > 0
                && height > 0
                && !dst[0].is_null()
                && !dst[1].is_null()
                && !dst[2].is_null()
            {
                unsafe {
                    let y_stride = info.iStride[0] as usize;
                    let uv_stride = info.iStride[1] as usize;
                    let y_h = height as usize;
                    let uv_h = height.div_ceil(2) as usize;

                    let y_src = std::slice::from_raw_parts(dst[0], y_stride * y_h);
                    let u_src = std::slice::from_raw_parts(dst[1], uv_stride * uv_h);
                    let v_src = std::slice::from_raw_parts(dst[2], uv_stride * uv_h);

                    frames.push(pack_i420(
                        packet.pts_secs,
                        width,
                        height,
                        y_src,
                        y_stride,
                        u_src,
                        uv_stride,
                        v_src,
                        uv_stride,
                    ));
                }
            } else {
                log::trace!(
                    "openh264 decode: invalid output dims {}x{} ptrs=[!{} !{} !{}]",
                    width,
                    height,
                    dst[0].is_null() as u8,
                    dst[1].is_null() as u8,
                    dst[2].is_null() as u8,
                );
            }
        } else {
            log::trace!(
                "openh264 decode: no frame (state={dec_state}) pts={:.3}",
                packet.pts_secs,
            );
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