// bg_fractal.wgsl — HUD background.
//
// Port of "Fractal Audio 01" (https://www.shadertoy.com/view/llB3W1)
// License: CC BY-NC-SA 3.0 (Shadertoy default). See LICENSES.md.
//
// Original samples an audio FFT texture (iChannel0) for a "pulse"
// parameter that drives a Mandelbrot-ish iteration seed + a tint
// texture. juballer port replaces the FFT read with `game_audio(x)`,
// a synthesized spectrum driven by our beat/combo/flash channels.
// Iteration count dropped from 150 → 60 because the HUD strip is a
// small fraction of a full Shadertoy canvas.

const ITERS: i32 = 60;

// game_audio(x) is provided by the standard uniform prelude and reads
// live FFT bins of the currently-playing music.

fn fractal(p_in: vec2<f32>, point: vec2<f32>) -> i32 {
    let so = (-1.0 + 2.0 * point) * 0.4;
    let seed = vec2<f32>(0.098386255 + so.x, 0.6387662 + so.y);
    var p = p_in;
    for (var i: i32 = 0; i < ITERS; i = i + 1) {
        if (length(p) > 2.0) {
            return i;
        }
        let r = p;
        p = vec2<f32>(p.x * p.x - p.y * p.y, 2.0 * p.x * p.y);
        p = vec2<f32>(p.x * r.x - p.y * r.y + seed.x,
                      r.x * p.y + p.x * r.y + seed.y);
    }
    return 0;
}

fn color_of(i: i32) -> vec3<f32> {
    var f = f32(i) / f32(ITERS) * 2.0;
    f = f * f * 2.0;
    return vec3<f32>(sin(f * 2.0), sin(f * 3.0), abs(sin(f * 7.0)));
}

fn sample_music_a() -> f32 {
    return 0.5 * (game_audio(0.15) + game_audio(0.30));
}

@fragment
fn fs_main(@location(0) uv_in: vec2<f32>) -> @location(0) vec4<f32> {
    let aspect = u.resolution.x / max(u.resolution.y, 1.0);

    var position = 3.0 * (uv_in - vec2<f32>(0.5));
    position.x = position.x * aspect;
    var pos2 = 2.0 * (vec2<f32>(1.0 - uv_in.x, 1.0 - uv_in.y) - vec2<f32>(0.5));
    pos2.x = pos2.x * aspect;

    let t3_raw = game_audio(length(position) * 0.5);
    let t3 = abs(vec3<f32>(0.5, 0.1, 0.5) - vec3<f32>(t3_raw)) * 2.0;
    let pulse = 0.5 + sample_music_a() * 1.8;

    let inv_fract = color_of(fractal(pos2, vec2<f32>(0.55 + sin(u.time / 3.0 + 0.5) / 2.0, pulse * 0.9)));
    let fract4 = color_of(fractal(position / 1.6, vec2<f32>(0.6 + cos(u.time / 2.0 + 0.5) / 2.0, pulse * 0.8)));
    let c = color_of(fractal(position, vec2<f32>(0.5 + sin(u.time / 3.0) / 2.0, pulse)));

    let fract01 = c;
    var col = fract01 / max(t3, vec3<f32>(0.05))
            + fract01 * t3
            + inv_fract * 0.6
            + fract4 * 0.3;
    col = col * mix(0.45, 1.0, u.state.x);
    return vec4<f32>(col, 1.0);
}
