// bg_dancing_cubes.wgsl — HUD background.
//
// Port of "dancing cubes" — https://www.shadertoy.com/view/MsdBR8 —
// itself a modification of Shane's "Raymarched Reflections"
// (https://www.shadertoy.com/view/4dt3zn) with per-cube FFT sizing.
// License: CC BY-NC-SA 3.0 (Shadertoy default). See LICENSES.md.
//
// juballer port collapses the original two-pass (Image + Buffer A
// low-pass filter) setup into a single pass and replaces the iChannel0
// FFT read with `game_audio(x)` driven by game state. Per-cube size
// follows the FFT-equivalent bin picked by that cube's unique ID, so
// the grid "breathes" with combo + hit flashes.
//
// Raymarch iterations dropped from 96 → 56 (first pass) and 48 → 28
// (reflection pass) since the HUD strip is much smaller than a full
// Shadertoy canvas. Still visually the same at this scale.

const FAR: f32 = 30.0;

// game_audio(x) is provided by the standard uniform prelude and reads
// live FFT bins of the currently-playing music.

fn map(p_in: vec3<f32>) -> f32 {
    let fl = floor(p_in);
    let n = sin(dot(fl, vec3<f32>(7.0, 157.0, 113.0)));
    let rnd = fract(vec3<f32>(2097152.0, 262144.0, 32768.0) * n) * 0.16 - 0.08;
    var p = fract(p_in + rnd) - 0.5;

    let eq = game_audio(fract(n));
    let boxsize = 0.1 + eq * 0.6;

    p = abs(p);
    return max(p.x, max(p.y, p.z)) - boxsize + dot(p, p) * 0.5;
}

fn trace(ro: vec3<f32>, rd: vec3<f32>) -> f32 {
    var t: f32 = 0.0;
    for (var i: i32 = 0; i < 56; i = i + 1) {
        let d = map(ro + rd * t);
        if (abs(d) < 0.002 || t > FAR) { break; }
        t = t + d * 0.75;
    }
    return t;
}

fn trace_ref(ro: vec3<f32>, rd: vec3<f32>) -> f32 {
    var t: f32 = 0.0;
    for (var i: i32 = 0; i < 28; i = i + 1) {
        let d = map(ro + rd * t);
        if (abs(d) < 0.0025 || t > FAR) { break; }
        t = t + d;
    }
    return t;
}

fn soft_shadow(ro: vec3<f32>, lp: vec3<f32>, k: f32) -> f32 {
    let max_it: i32 = 16;
    let rd_raw = lp - ro;
    let end = max(length(rd_raw), 0.001);
    let rd = rd_raw / end;
    let step_dist = end / f32(max_it);
    var shade: f32 = 1.0;
    var dist: f32 = 0.005;
    for (var i: i32 = 0; i < max_it; i = i + 1) {
        let h = map(ro + rd * dist);
        shade = min(shade, smoothstep(0.0, 1.0, k * h / max(dist, 1e-6)));
        dist = dist + clamp(h, 0.02, 0.2);
        if (h < 0.0 || dist > end) { break; }
    }
    return min(max(shade, 0.0) + 0.25, 1.0);
}

fn get_normal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(0.0035, -0.0035);
    return normalize(
        e.xyy * map(p + e.xyy) +
        e.yyx * map(p + e.yyx) +
        e.yxy * map(p + e.yxy) +
        e.xxx * map(p + e.xxx)
    );
}

fn object_color(p: vec3<f32>) -> vec3<f32> {
    var col = vec3<f32>(1.0);
    if (fract(dot(floor(p), vec3<f32>(0.5))) > 0.001) {
        col = vec3<f32>(0.6, 0.3, 1.0);
    }
    return col;
}

fn do_color(sp: vec3<f32>, rd: vec3<f32>, sn: vec3<f32>, lp: vec3<f32>) -> vec3<f32> {
    let ld_raw = lp - sp;
    let l_dist = max(length(ld_raw), 0.001);
    let ld = ld_raw / l_dist;
    let atten = 1.0 / (1.0 + l_dist * 0.2 + l_dist * l_dist * 0.1);
    let diff = max(dot(sn, ld), 0.0);
    let spec = pow(max(dot(reflect(-ld, sn), -rd), 0.0), 8.0);
    let obj = object_color(sp);
    return (obj * (diff + 0.15) + vec3<f32>(1.0, 0.6, 0.2) * spec * 2.0) * atten;
}

@fragment
fn fs_main(@location(0) uv_in: vec2<f32>) -> @location(0) vec4<f32> {
    let uv = (uv_in - vec2<f32>(0.5)) * vec2<f32>(u.resolution.x / max(u.resolution.y, 1.0), 1.0);
    var rd = normalize(vec3<f32>(uv, 1.0));

    let cs = cos(u.time * 0.25);
    let si = sin(u.time * 0.25);
    let mxy = mat2x2<f32>(cs, si, -si, cs);
    let rd_xy = mxy * rd.xy;
    rd = vec3<f32>(rd_xy, rd.z);
    let rd_xz = mxy * vec2<f32>(rd.x, rd.z);
    rd = vec3<f32>(rd_xz.x, rd.y, rd_xz.y);

    var ro = vec3<f32>(0.0, 0.0, u.time * 1.5);
    let lp = ro + vec3<f32>(0.0, 1.0, -0.5);

    let t1 = trace(ro, rd);
    let fog = smoothstep(0.0, 0.95, t1 / FAR);
    ro = ro + rd * t1;
    var sn = get_normal(ro);
    var scene_col = do_color(ro, rd, sn, lp);
    let sh = soft_shadow(ro, lp, 16.0);

    rd = reflect(rd, sn);
    let t2 = trace_ref(ro + rd * 0.01, rd);
    ro = ro + rd * t2;
    sn = get_normal(ro);
    scene_col = scene_col + do_color(ro, rd, sn, lp) * 0.35;
    scene_col = scene_col * sh;
    scene_col = mix(scene_col, vec3<f32>(0.0), fog);
    let out_rgb = sqrt(clamp(scene_col, vec3<f32>(0.0), vec3<f32>(1.0)));
    // Life dim the whole thing; HUD text still readable when life = 0.
    return vec4<f32>(out_rgb * mix(0.4, 1.0, u.state.x), 1.0);
}
