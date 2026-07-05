struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var u_tex: texture_2d<f32>;
@group(0) @binding(2) var v_tex: texture_2d<f32>;
@group(0) @binding(3) var tex_sampler: sampler;

// BT.709 YUV -> RGB
fn yuv_to_rgb(y: f32, u: f32, v: f32) -> vec3<f32> {
    let y_norm = y;
    let u_norm = u - 0.5;
    let v_norm = v - 0.5;
    let r = y_norm + 1.5748 * v_norm;
    let g = y_norm - 0.1873 * u_norm - 0.4681 * v_norm;
    let b = y_norm + 1.8556 * u_norm;
    return vec3<f32>(clamp(r, 0.0, 1.0), clamp(g, 0.0, 1.0), clamp(b, 0.0, 1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let y = textureSample(y_tex, tex_sampler, in.uv).r;
    let u = textureSample(u_tex, tex_sampler, in.uv).r;
    let v = textureSample(v_tex, tex_sampler, in.uv).r;
    let rgb = yuv_to_rgb(y, u, v);
    return vec4<f32>(rgb, 1.0);
}