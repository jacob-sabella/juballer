struct Uniforms {
    // 2x3 affine in column-major: m00 m10 _, m01 m11 _, m02 m12 _ (vec3 padding)
    col0: vec3<f32>,
    col1: vec3<f32>,
    col2: vec3<f32>,
    viewport_size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var src_smp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) src_uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // Fullscreen triangle in clip space.
    var p = array<vec2<f32>, 3>(
        vec2(-1.0, -3.0), vec2(-1.0, 1.0), vec2(3.0, 1.0)
    );
    let clip = p[vid];
    // Convert clip to screen-space pixel coords.
    let half = u.viewport_size * 0.5;
    let screen_xy = vec2<f32>((clip.x + 1.0) * half.x, (1.0 - clip.y) * half.y);
    // Apply inverse rotation: dst = M * src => src = M^-1 * dst. CPU uploads M^-1 already.
    let sx = u.col0.x * screen_xy.x + u.col1.x * screen_xy.y + u.col2.x;
    let sy = u.col0.y * screen_xy.x + u.col1.y * screen_xy.y + u.col2.y;
    var out: VsOut;
    out.pos = vec4(clip, 0.0, 1.0);
    out.src_uv = vec2(sx / u.viewport_size.x, sy / u.viewport_size.y);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.src_uv.x < 0.0 || in.src_uv.x > 1.0 || in.src_uv.y < 0.0 || in.src_uv.y > 1.0) {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    return textureSampleLevel(src_tex, src_smp, in.src_uv, 0.0);
}
