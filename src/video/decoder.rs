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
use crate::video::demux::VideoPacket;
use crate::video::obu::mp4_sample_to_obu_stream;

#[derive(Clone)]
pub struct DecodedFrame {
    pub pts_secs: f64,
    pub width: u32,
    pub height: u32,
    pub y_plane: Vec<u8>,
    pub u_plane: Vec<u8>,
    pub v_plane: Vec<u8>,
    pub y_stride: usize,
    pub uv_stride: usize,
}

pub struct Av1Decoder {
    context: Option<Dav1dContext>,
}

impl Av1Decoder {
    pub fn new() -> Result<Self> {
        let mut settings: Dav1dSettings = unsafe { std::mem::zeroed() };
        unsafe {
            dav1d_default_settings(NonNull::new(&mut settings).unwrap());
        }
        settings.n_threads = 2;
        settings.max_frame_delay = 1;

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
    let _y_stride = picture.stride[0].unsigned_abs() as usize;
    let _uv_stride = picture.stride[1].unsigned_abs() as usize;

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
        if picture.p.layout as u32 == DAV1D_PIXEL_LAYOUT_I420 as u32 {
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