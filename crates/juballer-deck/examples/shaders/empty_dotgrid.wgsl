// empty_dotgrid.wgsl
// Designed for unbound cells (u.bound < 0.5): dim dot grid plus a tiny
// crosshair at the center, giving "nothing here" a clear-but-quiet affordance.
// Bound cells (u.bound >= 0.5) receive a transparent color so the shader has
// no effect — bind it explicitly to empty cells only via config.

@fragment
fn fs_main(@builtin(position) frag_pos: vec4<f32>, @location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound >= 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // Dot spacing in pixels so the pattern looks uniform regardless of tile
    // size. Each dot is ~1.2px, spacing 14px. We use framebuffer-absolute
    // coords here (pattern is periodic, so tile offset is inconsequential).
    let spacing = 14.0;
    let r = 1.1;
    let px = frag_pos.xy;
    let cell = fract(px / spacing) * spacing - vec2<f32>(spacing * 0.5, spacing * 0.5);
    let dot = smoothstep(r + 0.6, r - 0.3, length(cell));

    // Crosshair at tile center.
    let c = uv - vec2<f32>(0.5, 0.5);
    let dx = abs(c.x);
    let dy = abs(c.y);
    let arm_len = 0.06;
    let arm_w = 0.004;
    let hbar = step(dy, arm_w) * step(dx, arm_len);
    let vbar = step(dx, arm_w) * step(dy, arm_len);
    let cross = clamp(hbar + vbar, 0.0, 1.0);

    // Fade the whole thing near the tile edges so it doesn't fight the border.
    let vignette = smoothstep(0.55, 0.2, length(c));

    let dots_a = dot * 0.22 * vignette;
    let cross_a = cross * 0.35;
    let a = max(dots_a, cross_a);

    // Neutral gray so we don't compete with the theme accent.
    let rgb = vec3<f32>(0.55, 0.58, 0.65);
    return vec4<f32>(rgb * a, a);
}
