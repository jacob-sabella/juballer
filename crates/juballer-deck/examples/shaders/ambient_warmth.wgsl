// ambient_warmth.wgsl
// Safe default. Very subtle radial vignette + a slow color drift in the tile's
// accent color, mixed against the theme base. No hard edges, no loud motion —
// it just makes every tile feel alive without distracting from the icon.

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let c = uv - vec2<f32>(0.5, 0.5);
    let d = length(c);

    // Vignette: bright-ish in the middle, fading to transparent at the edges.
    let vignette = smoothstep(0.75, 0.05, d);

    // Slow color drift — different phase per component so the accent wanders.
    let drift = 0.5 + 0.5 * sin(u.time * 0.35);
    let phase2 = 0.5 + 0.5 * sin(u.time * 0.21 + 1.9);

    // Warm-cool wobble in the tile's accent color.
    let tint = mix(u.accent.rgb * 0.85, u.accent.rgb * 1.15, drift);

    // Slightly brighter in a slowly-rotating lobe to give some life without
    // registering as motion.
    let angle = atan2(c.y, c.x);
    let lobe = 0.5 + 0.5 * sin(angle * 1.0 + u.time * 0.25);
    let boost = 1.0 + 0.12 * lobe;

    let rgb = tint * boost;
    // Cap alpha low so the shader reads as a background wash; egui paints
    // on top anyway.
    let a = vignette * (0.22 + 0.08 * phase2);
    return vec4<f32>(rgb * a, a);
}
