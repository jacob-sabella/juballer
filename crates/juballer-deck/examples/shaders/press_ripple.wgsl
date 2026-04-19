// press_ripple.wgsl
// Concentric rings from the tile center triggered by the `flash` uniform.
// Flash runs 1.0 → 0.0 over the press window, so we expand the rings as flash
// decays. Alpha follows flash so the overlay disappears entirely between
// presses, leaving the egui chrome untouched. Color follows u.accent.

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.flash <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let c = uv - vec2<f32>(0.5, 0.5);
    // Aspect-correct so rings stay circular on non-square tiles.
    let aspect = u.resolution.x / u.resolution.y;
    let p = vec2<f32>(c.x * aspect, c.y);
    let d = length(p);

    let progress = 1.0 - u.flash; // 0 at press, 1 at end of window.
    let ring_r = progress * 0.55;
    let ring_w = 0.035 + 0.05 * progress;

    // Two rings for depth.
    let r1 = 1.0 - smoothstep(ring_w * 0.5, ring_w, abs(d - ring_r));
    let r2 = 1.0 - smoothstep(ring_w * 0.5, ring_w, abs(d - ring_r * 0.55));

    let rings = max(r1, r2 * 0.6);
    let fade = u.flash * u.flash; // square the fade for a snappier tail.

    let a = rings * fade * 0.85;
    return vec4<f32>(u.accent.rgb * a, a);
}
