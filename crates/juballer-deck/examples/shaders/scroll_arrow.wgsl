// scroll_arrow.wgsl — directional sweep for scroll up/down buttons. Accent color drives hue.
// Direction encoded via kind value convention: we just sweep vertically always.
@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) { return vec4<f32>(0.0); }
    // Multiple chevron bands sweeping upward.
    let speed = 1.5;
    let band = fract(uv.y * 3.0 + u.time * speed);
    let chevron = smoothstep(0.6, 0.95, band) * (1.0 - smoothstep(0.0, 0.3, abs(uv.x - 0.5)));
    let intensity = chevron * 0.7;
    return vec4<f32>(u.accent.rgb * intensity, intensity);
}
