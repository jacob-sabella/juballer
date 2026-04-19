// heartbeat.wgsl — slow pulsing ring in green for health-probe tiles.
@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) { return vec4<f32>(0.0); }
    let c = uv - vec2<f32>(0.5, 0.5);
    let d = length(c);
    // Two quick beats per cycle (lub-dub).
    let t = u.time * 1.2;
    let beat1 = smoothstep(0.0, 0.15, fract(t)) * (1.0 - smoothstep(0.15, 0.35, fract(t)));
    let beat2 = smoothstep(0.35, 0.45, fract(t)) * (1.0 - smoothstep(0.45, 0.60, fract(t)));
    let beat = max(beat1, beat2 * 0.8);
    let ring_r = 0.15 + 0.35 * beat;
    let ring_w = 0.1 + 0.25 * beat;
    let ring = 1.0 - smoothstep(ring_w * 0.4, ring_w, abs(d - ring_r));
    let green = vec3<f32>(0.65, 0.89, 0.63);
    let intensity = ring * (0.3 + 0.7 * beat);
    return vec4<f32>(green * intensity, intensity);
}
