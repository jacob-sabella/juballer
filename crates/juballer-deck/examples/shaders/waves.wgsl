@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let c = uv - vec2<f32>(0.5, 0.5);
    let d = length(c) * 2.0;
    let t = u.time;
    let ring = 0.5 + 0.5 * sin(d * 12.0 - t * 3.0);
    let r = ring;
    let g = 0.5 + 0.5 * sin(d * 12.0 - t * 3.0 + 2.094);
    let b = 0.5 + 0.5 * sin(d * 12.0 - t * 3.0 + 4.188);
    return vec4<f32>(r, g, b, 1.0);
}
