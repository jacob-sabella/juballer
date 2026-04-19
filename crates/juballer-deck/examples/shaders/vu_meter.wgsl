// vu_meter.wgsl — bouncing vertical bars for media/audio tiles.
@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) { return vec4<f32>(0.0); }
    let bar_count = 7.0;
    let bar_x = floor(uv.x * bar_count);
    let phase = bar_x * 0.9 + u.time * 2.3;
    let height = 0.35 + 0.55 * (0.5 + 0.5 * sin(phase));
    let mask = step(1.0 - height, uv.y);
    let within_bar = step(0.15, fract(uv.x * bar_count));
    let col = mix(u.accent.rgb * 0.7, u.accent.rgb * 1.3, height);
    let a = mask * within_bar * 0.55;
    return vec4<f32>(col * a, a);
}
