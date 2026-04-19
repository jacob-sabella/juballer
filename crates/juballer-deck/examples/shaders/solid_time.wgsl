@fragment
fn fs_main(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let t = u.time;
    let r = 0.5 + 0.5 * sin(t);
    let g = 0.5 + 0.5 * sin(t + 2.094);
    let b = 0.5 + 0.5 * sin(t + 4.188);
    return vec4<f32>(r, g, b, 1.0);
}
