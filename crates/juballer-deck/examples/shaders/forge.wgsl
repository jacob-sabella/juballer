// forge.wgsl — orange/yellow flickering heat for build/compile tiles.
@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) { return vec4<f32>(0.0); }
    // Rising heat — warmer at bottom, cooler at top, with flickering.
    let heat = (1.0 - uv.y);
    let flicker_x = sin(uv.x * 12.0 + u.time * 5.0) * 0.5 + 0.5;
    let flicker_y = sin(uv.y * 5.0 + u.time * 3.0) * 0.5 + 0.5;
    let flick = mix(0.7, 1.3, flicker_x * flicker_y);
    let orange = vec3<f32>(0.98, 0.63, 0.47);
    let yellow = vec3<f32>(0.96, 0.89, 0.56);
    let col = mix(orange, yellow, heat);
    let i = heat * flick * 0.55;
    return vec4<f32>(col * i, i);
}
