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

pub fn copy_yuv420_plane(
    src: &[u8],
    src_stride: usize,
    dst: &mut [u8],
    width: usize,
    height: usize,
) {
    for row in 0..height {
        let src_start = row * src_stride;
        let dst_start = row * width;
        let end = src_start + width;
        if end <= src.len() && dst_start + width <= dst.len() {
            dst[dst_start..dst_start + width].copy_from_slice(&src[src_start..end]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn pack_i420(
    pts_secs: f64,
    width: u32,
    height: u32,
    y: &[u8],
    y_stride: usize,
    u: &[u8],
    u_stride: usize,
    v: &[u8],
    v_stride: usize,
) -> DecodedFrame {
    let w = width as usize;
    let h = height as usize;
    // UV plane dimensions must match render-side validation (RenderPipeline::upload_frame
    // and create_plane_textures use div_ceil(2)). Using w/2 here caused odd-dimension
    // frames to fail plane-size validation and be rejected (black screen).
    let uv_w = w.div_ceil(2);
    let uv_h = h.div_ceil(2);

    let mut y_plane = vec![0u8; w * h];
    let mut u_plane = vec![0u8; uv_w * uv_h];
    let mut v_plane = vec![0u8; uv_w * uv_h];

    copy_yuv420_plane(y, y_stride, &mut y_plane, w, h);
    copy_yuv420_plane(u, u_stride, &mut u_plane, uv_w, uv_h);
    copy_yuv420_plane(v, v_stride, &mut v_plane, uv_w, uv_h);

    DecodedFrame {
        pts_secs,
        width,
        height,
        y_plane,
        u_plane,
        v_plane,
        y_stride: w,
        uv_stride: uv_w,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_i420_planes(w: u32, h: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize, usize) {
        let y_len = (w * h) as usize;
        let uv_w = w.div_ceil(2) as usize;
        let uv_h = h.div_ceil(2) as usize;
        (
            vec![128u8; y_len],
            vec![0u8; uv_w * uv_h],
            vec![255u8; uv_w * uv_h],
            w as usize,
            uv_w,
        )
    }

    #[test]
    fn pack_i420_even_dimensions() {
        let w = 640u32;
        let h = 480u32;
        let (y, u, v, y_stride, uv_stride) = make_i420_planes(w, h);
        let frame = pack_i420(0.0, w, h, &y, y_stride, &u, uv_stride, &v, uv_stride);
        assert_eq!(frame.y_plane.len(), (w * h) as usize);
        assert_eq!(frame.u_plane.len(), (w / 2 * h / 2) as usize);
        assert_eq!(frame.v_plane.len(), (w / 2 * h / 2) as usize);
        assert_eq!(frame.uv_stride, (w / 2) as usize);
    }

    #[test]
    fn pack_i420_odd_width() {
        // Odd width must not truncate UV plane (black screen regression test for R2).
        let w = 641u32;
        let h = 480u32;
        let (y, u, v, y_stride, uv_stride) = make_i420_planes(w, h);
        let frame = pack_i420(0.0, w, h, &y, y_stride, &u, uv_stride, &v, uv_stride);
        let uv_expected = (w.div_ceil(2) * h.div_ceil(2)) as usize;
        assert_eq!(frame.y_plane.len(), (w * h) as usize);
        assert_eq!(frame.u_plane.len(), uv_expected);
        assert_eq!(frame.v_plane.len(), uv_expected);
        assert_eq!(frame.uv_stride, w.div_ceil(2) as usize);
    }

    #[test]
    fn pack_i420_odd_height() {
        let w = 640u32;
        let h = 481u32;
        let (y, u, v, y_stride, uv_stride) = make_i420_planes(w, h);
        let frame = pack_i420(0.0, w, h, &y, y_stride, &u, uv_stride, &v, uv_stride);
        let uv_expected = (w.div_ceil(2) * h.div_ceil(2)) as usize;
        assert_eq!(frame.y_plane.len(), (w * h) as usize);
        assert_eq!(frame.u_plane.len(), uv_expected);
        assert_eq!(frame.v_plane.len(), uv_expected);
    }

    #[test]
    fn pack_i420_odd_both() {
        let w = 641u32;
        let h = 481u32;
        let (y, u, v, y_stride, uv_stride) = make_i420_planes(w, h);
        let frame = pack_i420(0.0, w, h, &y, y_stride, &u, uv_stride, &v, uv_stride);
        let uv_expected = (w.div_ceil(2) * h.div_ceil(2)) as usize;
        assert_eq!(frame.y_plane.len(), (w * h) as usize);
        assert_eq!(frame.u_plane.len(), uv_expected);
        assert_eq!(frame.v_plane.len(), uv_expected);
    }

    #[test]
    fn pack_i420_ptz_stored_correctly() {
        let w = 320u32;
        let h = 240u32;
        let (y, u, v, y_stride, uv_stride) = make_i420_planes(w, h);
        let frame = pack_i420(12.345, w, h, &y, y_stride, &u, uv_stride, &v, uv_stride);
        assert!((frame.pts_secs - 12.345).abs() < f64::EPSILON);
    }
}