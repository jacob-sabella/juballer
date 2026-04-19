// bg_galaxy.wgsl — HUD background.
//
// Port of "Audio Reactive Galaxy" (CBS, 2022)
//   https://www.shadertoy.com/view/NdG3zw
// Credits parallax-scrolling fractal galaxy inspired by JoshP's
// "Simplicity" (https://www.shadertoy.com/view/lslGWr).
// License: CC BY-NC-SA 3.0 (Shadertoy default). See LICENSES.md.
//
// Original samples iChannel0 at four FFT positions (.01, .07, .15, .30)
// to drive the field strength + colour mix for two parallax layers of
// a fractal-fold galaxy. juballer port routes those four reads through
// `game_audio(x)` so the galaxy brightens on hits and warps with combo.

// game_audio(x) is provided by the standard uniform prelude and reads
// live FFT bins of the currently-playing music.

fn field(p_in: vec3<f32>, s: f32) -> f32 {
    let strength = 7.0 + 0.03 * log(1.0e-6 + fract(sin(u.time) * 4373.11));
    var accum = s * 0.25;
    var prev: f32 = 0.0;
    var tw: f32 = 0.0;
    var p = p_in;
    for (var i: i32 = 0; i < 26; i = i + 1) {
        let mag = dot(p, p);
        p = abs(p) / max(mag, 1e-6) + vec3<f32>(-0.5, -0.4, -1.5);
        let w = exp(-f32(i) / 7.0);
        accum = accum + w * exp(-strength * pow(abs(mag - prev), 2.2));
        tw = tw + w;
        prev = mag;
    }
    return max(0.0, 5.0 * accum / tw - 0.7);
}

fn field2(p_in: vec3<f32>, s: f32) -> f32 {
    let strength = 7.0 + 0.03 * log(1.0e-6 + fract(sin(u.time) * 4373.11));
    var accum = s * 0.25;
    var prev: f32 = 0.0;
    var tw: f32 = 0.0;
    var p = p_in;
    for (var i: i32 = 0; i < 18; i = i + 1) {
        let mag = dot(p, p);
        p = abs(p) / max(mag, 1e-6) + vec3<f32>(-0.5, -0.4, -1.5);
        let w = exp(-f32(i) / 7.0);
        accum = accum + w * exp(-strength * pow(abs(mag - prev), 2.2));
        tw = tw + w;
        prev = mag;
    }
    return max(0.0, 5.0 * accum / tw - 0.7);
}

fn nrand3(co: vec2<f32>) -> vec3<f32> {
    let a = fract(cos(co.x * 8.3e-3 + co.y) * vec3<f32>(1.3e5, 4.7e5, 2.9e5));
    let b = fract(sin(co.x * 0.3e-3 + co.y) * vec3<f32>(8.1e5, 1.0e5, 0.1e5));
    return mix(a, b, vec3<f32>(0.5));
}

@fragment
fn fs_main(@location(0) uv_in: vec2<f32>) -> @location(0) vec4<f32> {
    let uv = 2.0 * uv_in - vec2<f32>(1.0);
    let uvs = uv * u.resolution / max(u.resolution.x, u.resolution.y);
    var p = vec3<f32>(uvs * 0.25, 0.0) + vec3<f32>(1.0, -1.3, 0.0);
    p = p + 0.2 * vec3<f32>(sin(u.time / 16.0), sin(u.time / 12.0), sin(u.time / 128.0));

    // Original read iChannel0 at 4 FFT bins; we synthesise via the game.
    let f0 = game_audio(0.01);
    let f1 = game_audio(0.07);
    let f2 = game_audio(0.15);
    let f3 = game_audio(0.30);

    let t = field(p, f2);
    let v = (1.0 - exp((abs(uv.x) - 1.0) * 6.0))
          * (1.0 - exp((abs(uv.y) - 1.0) * 6.0));

    var p2 = vec3<f32>(
        uvs / (4.0 + sin(u.time * 0.11) * 0.2 + 0.2 + sin(u.time * 0.15) * 0.3 + 0.4),
        1.5,
    ) + vec3<f32>(2.0, -1.3, -1.0);
    p2 = p2 + 0.25 * vec3<f32>(sin(u.time / 16.0), sin(u.time / 12.0), sin(u.time / 128.0));
    let t2 = field2(p2, f3);
    let c2 = mix(vec4<f32>(0.4), vec4<f32>(1.0), v)
           * vec4<f32>(1.3 * t2 * t2 * t2, 1.8 * t2 * t2, t2 * f0, t2);

    // Stars.
    let seed = floor(p.xy * 2.0 * u.resolution.x);
    let rnd = nrand3(seed);
    var starcolor = vec4<f32>(pow(rnd.y, 40.0));
    let seed2 = floor(p2.xy * 2.0 * u.resolution.x);
    let rnd2 = nrand3(seed2);
    starcolor = starcolor + vec4<f32>(pow(rnd2.y, 40.0));

    let base = mix(vec4<f32>(f3 - 0.3), vec4<f32>(1.0), v)
             * vec4<f32>(1.5 * f2 * t * t * t, 1.2 * f1 * t * t, f3 * t, 1.0);
    var col = base + c2 + starcolor;
    col = col * vec4<f32>(vec3<f32>(mix(0.45, 1.0, u.state.x)), 1.0);
    return col;
}
