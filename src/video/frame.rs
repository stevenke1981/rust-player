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
    let uv_w = w / 2;
    let uv_h = h / 2;

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