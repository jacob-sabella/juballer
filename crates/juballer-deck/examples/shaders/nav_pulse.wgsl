// nav_pulse.wgsl
// Subtle radial pulse in the tile's accent color. Slow 3s cycle. Only draws
// anything when the tile is bound (u.bound > 0.5); unbound tiles receive a
// fully-transparent color so they fall back to the default egui chrome.
//
// Intended for `Nav` tiles but reads the accent uniform so the caller can reuse
// it for other kinds. Cost: 1 length() + 2 sin() per pixel.

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let c = uv - vec2<f32>(0.5, 0.5);
    let d = length(c);

    // Two-tap pulse: one slow breathing envelope + a faster ring for motion.
    let period = 3.0;
    let breath = 0.5 + 0.5 * sin(u.time * 6.283 / period);
    let ring = 0.5 + 0.5 * sin(d * 10.0 - u.time * 1.5);

    // Falloff from center to edge so the vignette sits behind the icon.
    let falloff = smoothstep(0.6, 0.0, d);
    let intensity = falloff * (0.35 + 0.25 * breath) + 0.08 * ring * falloff;

    let rgb = u.accent.rgb * intensity;
    return vec4<f32>(rgb, intensity * 0.55);
}
