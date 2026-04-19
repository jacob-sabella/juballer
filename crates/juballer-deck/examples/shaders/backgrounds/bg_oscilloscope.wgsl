// bg_oscilloscope.wgsl — HUD background.
//
// Port of "Oscilloscope music" by incription (2021)
//   https://www.shadertoy.com/view/slc3DX
// License: CC BY-NC-SA 3.0 (Shadertoy default). See LICENSES.md.
//
// Original draws a Lissajous curve from a self-generated audio
// waveform. juballer port replaces the freq() function's hard-coded
// tones with a curve driven by the game's beat phase + combo intensity
// so the oscilloscope "plays" the song the player is actually hitting.

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318530;
const SAMPLES: f32 = 220.0;

// Beat-phase oscillator. Lower sample count than the original (600 → 220)
// because the HUD top bar is a narrow strip; the curve density stays
// roughly the same per visible pixel.
fn rot(a: f32) -> mat2x2<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat2x2<f32>(c, -s, s, c);
}

fn combo_amp() -> f32 {
    return clamp(log(1.0 + u.state.y) * 0.25, 0.0, 1.0);
}

// Curve position at parametric time `t` (seconds). Originally:
//   vec2(wave(A3*2, .5, t) + wave(A4*4, .5, t),
//        wave(A3*(2+PI/5000), .5, t) + wave(A3, .5, t))
// with rotation and 0.5 scale. Here we drive the two component
// frequencies from the game's BPM (u.cursor.y) and a combo-dependent
// offset so the curve warps as the combo grows.
fn curve(t: f32) -> vec2<f32> {
    let bpm_hz = max(u.cursor.y, 30.0) / 60.0;
    // Bass (low bin) pulses the overall curve size; highs (top bin)
    // modulate the inner Lissajous frequency so snares/hats wobble the
    // shape.
    let bass = game_audio(0.05);
    let high = game_audio(0.85);
    let amp = 0.25 + 0.55 * bass + 0.15 * combo_amp();
    let fx = sin(TAU * bpm_hz * 2.0 * t) * (1.0 + 0.35 * high);
    let fy = sin(TAU * bpm_hz * (3.0 + 0.5 * combo_amp() + 0.8 * high) * t
                 + PI * 0.5 * u.toggle_on);
    let r = rot(t * 0.6);
    return r * vec2<f32>(fx, fy) * amp;
}

fn sd_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

fn sd_box(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

// Integrate segment distances across SAMPLES to draw a glowing curve.
//
// Sweep spans ~1.5 s of the curve so the Lissajous traces out its full
// shape regardless of frame rate. Tying dt to `delta_time / SAMPLES`
// (earlier version) collapsed the trace to a single dot at 60fps because
// 16 ms / 220 = 73µs of sweep per frame is less than a radian of phase
// at any musical frequency.
fn sd_sound(uv: vec2<f32>) -> f32 {
    var hits: f32 = 0.0;
    let t0 = u.time;
    var prev = curve(t0);
    let bpm_hz = max(u.cursor.y, 30.0) / 60.0;
    // Cover at least two beats so the curve's full periodicity is visible.
    let sweep = clamp(2.0 / bpm_hz, 0.8, 3.0);
    let dt = sweep / SAMPLES;
    var t = t0;
    for (var i: f32 = 1.0; i < SAMPLES; i = i + 1.0) {
        t = t + dt;
        let f = curve(t);
        hits = hits + min(1.0, 1.0 / (sd_segment(uv, prev, f) * 2500.0));
        prev = f;
    }
    return 200.0 * hits / SAMPLES;
}

@fragment
fn fs_main(@location(0) uv_in: vec2<f32>) -> @location(0) vec4<f32> {
    let aspect = u.resolution.x / max(u.resolution.y, 1.0);
    var uv = uv_in - vec2<f32>(0.5);
    uv.x = uv.x * aspect;

    // Faint grid cells backdrop.
    let cell = (vec2<f32>(fract((uv.x + 0.5) * 8.0),
                          fract((uv.y + 0.5) * 8.0)) - 0.5);

    var col = vec3<f32>(0.0);
    col = mix(col, vec3<f32>(0.03), f32(sd_box(cell, vec2<f32>(0.49)) <= 0.0));
    // Curve — green at rest, snaps to last-hit grade color on a fresh hit.
    let tint = mix(vec3<f32>(0.40, 0.98, 0.40), u.accent.rgb, clamp(u.flash, 0.0, 1.0));
    col = mix(col, tint, sd_sound(uv * 3.0));

    // Vignette.
    var vuv = uv_in;
    vuv = vuv * (1.0 - vec2<f32>(vuv.y, vuv.x));
    col = col * pow(max(vuv.x * vuv.y * 30.0, 0.0001), 0.5);
    // Cool blue wash like the original.
    col = col * vec3<f32>(0.0, 0.667, 1.0);
    // Life dim — the HUD stays readable even at 0 life.
    col = col * mix(0.35, 1.0, u.state.x);
    return vec4<f32>(col, 1.0);
}
