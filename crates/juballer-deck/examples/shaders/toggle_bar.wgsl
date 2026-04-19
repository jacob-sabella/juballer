// toggle_bar.wgsl
// Animated horizontal "fill" bar along the bottom of the tile. When the toggle
// is off (u.toggle_on < 0.5) it renders a thin line across the bottom edge;
// when on, the fill rises to ~45% of the tile with a subtle shimmer in the
// state color. Transparent above the fill so the egui icon/label sit on top.

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    // Flip so 0 = bottom, 1 = top — more intuitive for a "fill" metaphor.
    let y = 1.0 - uv.y;

    // Off: a 2px-ish hairline at the very bottom.
    // On : a fill that occupies the bottom 45% of the tile.
    let off_height = 3.0 / u.resolution.y;
    let on_height = 0.45;
    let fill_to = mix(off_height, on_height, clamp(u.toggle_on, 0.0, 1.0));

    // Shimmer: horizontal sine advected by time, only meaningful when on.
    let shimmer = 0.5 + 0.5 * sin(uv.x * 12.0 + u.time * 2.0);
    let shimmer_mix = mix(0.0, 0.35, clamp(u.toggle_on, 0.0, 1.0));

    // Gradient inside the fill: darker at top edge for depth.
    let edge = smoothstep(fill_to, fill_to - 0.03, y);
    let depth = smoothstep(0.0, fill_to, y);

    let base = u.state.rgb * (0.75 + 0.25 * depth);
    let with_shimmer = base + u.state.rgb * shimmer * shimmer_mix;

    let a = edge * mix(0.65, 0.9, clamp(u.toggle_on, 0.0, 1.0));
    return vec4<f32>(with_shimmer * a, a);
}
