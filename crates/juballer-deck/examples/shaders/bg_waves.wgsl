// bg_waves.wgsl — example HUD top-bar background shader.
//
// Demonstrates the background uniform convention (see
// `crates/juballer-deck/src/rhythm/background.rs` for the full table):
//   u.cursor.x = music_ms / 1000.0
//   u.cursor.y = current BPM
//   u.toggle_on = beat_phase in [0, 1)
//   u.kind = last-grade idx (0 none, 1 perfect, 2 great, 3 good,
//                             4 poor, 5 miss)
//   u.bound = 1 if any cell is held
//   u.flash = last-hit-freshness in [0, 1]
//   u.accent = last-hit grade color (rgba)
//   u.state.x = life 0..1
//   u.state.y = combo
//   u.state.z = score / 10_000
//   u.state.w = held-cell mask as f32
//
// Visual: horizontal energy band whose amplitude swells with the combo,
// hue snaps to the last-hit accent, and scrolls with music time.

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let music_s = u.cursor.x;
    let beat    = u.toggle_on;
    let life    = u.state.x;
    let combo   = u.state.y;
    let flash   = u.flash;
    let tint    = u.accent.rgb;

    // Base gradient: darker at the top, slightly warmer at the bottom,
    // shifted by a slow music-time drift.
    let scroll = music_s * 0.15;
    let g = 0.10 + 0.15 * sin(uv.y * 3.0 + scroll);
    var rgb = vec3<f32>(g * 0.6, g * 0.7, g);

    // Travelling wave band — phase advances with music time.
    let band_center = 0.5 + 0.20 * sin(music_s * 0.6);
    let band = exp(-pow((uv.y - band_center) / 0.18, 2.0));

    // Combo-driven amplitude (log curve so dense songs don't saturate).
    let combo_amp = clamp(log(1.0 + combo) * 0.15, 0.0, 0.75);

    // Horizontal ripple; frequency climbs subtly with combo.
    let k = 6.0 + combo_amp * 8.0;
    let ripple = 0.5 + 0.5 * sin(uv.x * k - music_s * 2.5 + beat * 6.283);

    // Mix the band into the base, tint with the last-hit accent colour.
    let band_rgb = mix(vec3<f32>(0.15, 0.35, 0.70), tint, clamp(flash, 0.0, 1.0));
    rgb = rgb + band_rgb * band * (0.25 + 0.75 * ripple) * (0.35 + combo_amp);

    // Brief flash-burst when a Perfect/Great fires.
    rgb = rgb + tint * flash * 0.25;

    // Life-bar-driven fade: dim everything as life drops. Stays dim
    // rather than going black so the HUD text remains readable.
    let life_dim = mix(0.35, 1.0, life);
    rgb = rgb * life_dim;

    return vec4<f32>(rgb, 1.0);
}
