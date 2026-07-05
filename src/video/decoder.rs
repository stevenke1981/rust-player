use std::ptr::NonNull;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::DAV1D_PIXEL_LAYOUT_I420;
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture,
    dav1d_open, dav1d_picture_unref, dav1d_send_data,
};

use crate::error::{PlayerError, Result};
use crate::video::demux::{VideoCodec, VideoPacket};
pub use crate::video::frame::DecodedFrame;
use crate::video::h264::H264Decoder;
use crate::video::h265::H265Decoder;
use crate::video::obu::mp4_sample_to_obu_stream;



pub struct Av1Decoder {
    context: Option<Dav1dContext>,
}

impl Av1Decoder {
    pub fn new() -> Result<Self> {
        let mut settings: Dav1dSettings = unsafe { std::mem::zeroed() };
        unsafe {
            dav1d_default_settings(NonNull::new(&mut settings).unwrap());
        }
        settings.n_threads = decode_thread_count();
        settings.max_frame_delay = 2;

        let mut context = None;
        let res = unsafe {
            dav1d_open(
                NonNull::new(&mut context),
                NonNull::new(&mut settings),
            )
        };
        if res.0 != 0 {
            return Err(PlayerError::VideoDecode(format!(
                "dav1d_open failed: {}",
                res.0
            )));
        }

        Ok(Self { context })
    }

    pub fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<DecodedFrame>> {
        let ctx = self
            .context
            .as_ref()
            .ok_or_else(|| PlayerError::VideoDecode("decoder closed".into()))?;

        let owned = mp4_sample_to_obu_stream(&packet.data);
        if owned.is_empty() {
            return Ok(Vec::new());
        }

        let mut data: Dav1dData = unsafe { std::mem::zeroed() };
        let ptr = unsafe { dav1d_data_create(NonNull::new(&mut data), owned.len()) };
        if ptr.is_null() {
            return Err(PlayerError::VideoDecode("dav1d_data_create failed".into()));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(owned.as_ptr(), ptr, owned.len());
        }
        data.m.timestamp = (packet.pts_secs * 1_000_000.0) as i64;
        data.m.size = owned.len();

        let send_res = unsafe { dav1d_send_data(Some(*ctx), NonNull::new(&mut data)) };
        log::debug!(
            "dav1d_send_data res={} obu_len={} head={:02x}{:02x}",
            send_res.0,
            owned.len(),
            owned.first().copied().unwrap_or(0),
            owned.get(1).copied().unwrap_or(0)
        );
        if send_res.0 < 0 && send_res.0 != -11 {
            log::debug!("dav1d_send_data err={} obu_len={}", send_res.0, owned.len());
            unsafe {
                dav1d_data_unref(NonNull::new(&mut data));
            }
            return Ok(Vec::new());
        }

        let mut frames = Vec::new();
        loop {
            let mut picture: Dav1dPicture = unsafe { std::mem::zeroed() };
            let res = unsafe { dav1d_get_picture(Some(*ctx), NonNull::new(&mut picture)) };
            if res.0 == -11 {
                break;
            }
            if res.0 < 0 {
                if res.0 != -11 {
                    log::debug!("dav1d_get_picture err={}", res.0);
                }
                break;
            }

            if let Some(frame) = extract_frame(&picture, packet.pts_secs) {
                frames.push(frame);
            }
            unsafe {
                dav1d_picture_unref(NonNull::new(&mut picture));
            }
        }

        unsafe {
            dav1d_data_unref(NonNull::new(&mut data));
        }

        Ok(frames)
    }

    pub fn flush(&mut self) -> Result<Vec<DecodedFrame>> {
        let ctx = self
            .context
            .as_ref()
            .ok_or_else(|| PlayerError::VideoDecode("decoder closed".into()))?;

        let mut empty: Dav1dData = unsafe { std::mem::zeroed() };
        let _ = unsafe { dav1d_send_data(Some(*ctx), NonNull::new(&mut empty)) };

        let mut frames = Vec::new();
        loop {
            let mut picture: Dav1dPicture = unsafe { std::mem::zeroed() };
            let res = unsafe { dav1d_get_picture(Some(*ctx), NonNull::new(&mut picture)) };
            if res.0 == -11 {
                break;
            }
            if res.0 < 0 {
                break;
            }
            if let Some(frame) = extract_frame(&picture, 0.0) {
                frames.push(frame);
            }
            unsafe {
                dav1d_picture_unref(NonNull::new(&mut picture));
            }
        }
        Ok(frames)
    }
}

impl Drop for Av1Decoder {
    fn drop(&mut self) {
        let mut ctx_slot = self.context.take();
        if ctx_slot.is_some() {
            unsafe {
                dav1d_close(NonNull::new(&mut ctx_slot));
            }
        }
    }
}

fn extract_frame(picture: &Dav1dPicture, pts_secs: f64) -> Option<DecodedFrame> {
    if picture.p.w <= 0 || picture.p.h <= 0 {
        return None;
    }

    let width = picture.p.w as u32;
    let height = picture.p.h as u32;
    let _y_stride = picture.stride[0].unsigned_abs();
    let _uv_stride = picture.stride[1].unsigned_abs();

    let y_ptr = picture.data[0]?.as_ptr() as *const u8;
    let u_ptr = picture.data[1]?.as_ptr() as *const u8;
    let v_ptr = picture.data[2]?.as_ptr() as *const u8;

    let y_h = height as usize;
    let uv_h = height as usize / 2;
    let uv_w = width as usize / 2;

    let mut y_plane = vec![0u8; width as usize * height as usize];
    let mut u_plane = vec![0u8; uv_w * uv_h];
    let mut v_plane = vec![0u8; uv_w * uv_h];

    unsafe {
        copy_plane(y_ptr, picture.stride[0], &mut y_plane, width as usize, y_h);
        if picture.p.layout == DAV1D_PIXEL_LAYOUT_I420 {
            copy_plane(u_ptr, picture.stride[1], &mut u_plane, uv_w, uv_h);
            copy_plane(v_ptr, picture.stride[1], &mut v_plane, uv_w, uv_h);
        }
    }

    Some(DecodedFrame {
        pts_secs,
        width,
        height,
        y_plane,
        u_plane,
        v_plane,
        y_stride: width as usize,
        uv_stride: uv_w,
    })
}

fn decode_thread_count() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 8) as i32)
        .unwrap_or(2)
}

pub enum VideoDecoder {
    Av1(Av1Decoder),
    H264(H264Decoder),
    H265(Box<H265Decoder>),
}

impl VideoDecoder {
    pub fn for_codec(codec: VideoCodec, extradata: &[u8]) -> Result<Self> {
        match codec {
            VideoCodec::Av1 => Ok(Self::Av1(Av1Decoder::new()?)),
            VideoCodec::H264 => Ok(Self::H264(H264Decoder::new(extradata)?)),
            VideoCodec::H265 => Ok(Self::H265(Box::new(H265Decoder::new(extradata)?))),
            VideoCodec::Unknown => Err(PlayerError::VideoDecode(
                "unsupported video codec".into(),
            )),
        }
    }

    pub fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<DecodedFrame>> {
        match self {
            Self::Av1(d) => d.decode(packet),
            Self::H264(d) => d.decode(packet),
            Self::H265(d) => d.decode(packet),
        }
    }

    pub fn flush(&mut self) -> Result<Vec<DecodedFrame>> {
        match self {
            Self::Av1(d) => d.flush(),
            Self::H264(d) => d.flush(),
            Self::H265(d) => d.flush(),
        }
    }
}

unsafe fn copy_plane(
    src: *const u8,
    stride: isize,
    dst: &mut [u8],
    width: usize,
    height: usize,
) {
    let stride_abs = stride.unsigned_abs();
    if stride < 0 {
        let start = src.add(stride_abs * (height - 1));
        for row in 0..height {
            std::ptr::copy_nonoverlapping(
                start.sub(row * stride_abs),
                dst.as_mut_ptr().add(row * width),
                width,
            );
        }
    } else {
        for row in 0..height {
            std::ptr::copy_nonoverlapping(
                src.add(row * stride_abs),
                dst.as_mut_ptr().add(row * width),
                width,
            );
        }
    }
}