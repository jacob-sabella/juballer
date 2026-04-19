@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let p = (uv - vec2<f32>(0.5, 0.5)) * 4.0;
    let t = u.time;
    var v: f32 = 0.0;
    v = v + sin(p.x + t);
    v = v + sin((p.y + t) * 0.5);
    v = v + sin((p.x + p.y + t) * 0.5);
    let cx = p.x + 0.5 * sin(t / 5.0);
    let cy = p.y + 0.5 * cos(t / 3.0);
    v = v + sin(sqrt(cx * cx + cy * cy + 1.0) + t);
    v = v * 0.25;
    let r = 0.5 + 0.5 * sin(v * 3.14159);
    let g = 0.5 + 0.5 * sin(v * 3.14159 + 2.094);
    let b = 0.5 + 0.5 * sin(v * 3.14159 + 4.188);
    return vec4<f32>(r, g, b, 1.0);
}
