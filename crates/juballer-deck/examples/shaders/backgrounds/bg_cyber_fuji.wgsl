// bg_cyber_fuji.wgsl — HUD background.
//
// Port of "Cyber Fuji 2020 audio reactive"
//   https://www.shadertoy.com/view/fd2GRw
// License: CC BY-NC-SA 3.0 (Shadertoy default). See LICENSES.md.
//
// Original is a synthwave Fuji scene with sun / grid / cloud / mountain
// that all flex on iChannel0 FFT reads. juballer port keeps the full
// scene and pipes our `game_audio` shim in everywhere the original
// sampled iChannel0. "Battery" (original mouse.y substitute) maps to
// the player's life bar so the scene dims + flattens as life drops.

// game_audio(x) is provided by the standard uniform prelude and reads
// live FFT bins of the currently-playing music.

fn gmod(x: f32, y: f32) -> f32 {
    return x - y * floor(x / max(y, 1e-6));
}

fn sun(uv: vec2<f32>, battery: f32) -> f32 {
    var val = smoothstep(0.3, 0.29, length(uv));
    val = val - 0.4 + pow(game_audio(0.10), 1.0) * 1.0;
    let bloom = smoothstep(0.7, 0.0, length(uv));
    var cut = 3.0 * sin((uv.y + u.time * 0.2 * (battery + 0.02)) * 100.0)
              + clamp(uv.y * 14.0 + 1.0, -6.0, 6.0);
    cut = clamp(cut, 0.0, 1.0);
    return clamp(val * cut, 0.0, 1.0) + bloom * 0.6;
}

fn grid(uv_in: vec2<f32>, battery: f32) -> f32 {
    let size = vec2<f32>(uv_in.y, uv_in.y * uv_in.y * 0.2) * 0.01;
    var uv = uv_in + vec2<f32>(0.0, u.time * 4.0 * (battery + 0.05));
    uv = abs(vec2<f32>(fract(uv.x), fract(uv.y)) - 0.7 + pow(game_audio(0.10), 0.7) * 1.0);
    var lines = smoothstep(size, vec2<f32>(0.0), uv);
    lines = lines + smoothstep(size * 5.0, vec2<f32>(0.0), uv) * 0.4 * battery;
    return clamp(lines.x + lines.y, 0.0, 3.0);
}

fn dot2v(v: vec2<f32>) -> f32 { return dot(v, v); }

fn sd_trapezoid(p_in: vec2<f32>, r1: f32, r2: f32, he: f32) -> f32 {
    let k1 = vec2<f32>(r2, he);
    let k2 = vec2<f32>(r2 - r1, 2.0 * he);
    var p = p_in;
    p.x = abs(p.x);
    let ca_x_ref = select(r2, r1, p.y < 0.0);
    let ca = vec2<f32>(p.x - min(p.x, ca_x_ref), abs(p.y) - he);
    let cb = p - k1 + k2 * clamp(dot(k1 - p, k2) / dot2v(k2), 0.0, 1.0);
    let s = select(1.0, -1.0, cb.x < 0.0 && ca.y < 0.0);
    return s * sqrt(min(dot2v(ca), dot2v(cb)));
}

fn sd_line(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

fn sd_box(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

fn op_smooth_union(d1: f32, d2: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (d2 - d1) / k, 0.0, 1.0);
    return mix(d2, d1, h) - k * h * (1.0 - h);
}

fn sd_cloud(p: vec2<f32>, a1: vec2<f32>, b1: vec2<f32>, a2: vec2<f32>, b2: vec2<f32>, w: f32) -> f32 {
    let line1 = sd_line(p, a1, b1);
    let line2 = sd_line(p, a2, b2);
    let ww = vec2<f32>(w * 1.5, 0.0);
    let left = max(a1 + ww, a2 + ww);
    let right = min(b1 - ww, b2 - ww);
    let boxc = (left + right) * 0.5;
    let boxh = abs(a2.y - a1.y) * 0.5;
    let boxv = sd_box(p - boxc, vec2<f32>(0.04, boxh)) + w;
    let u1 = op_smooth_union(line1, boxv, 0.05);
    let u2 = op_smooth_union(line2, boxv, 0.05);
    return min(u1, u2);
}

@fragment
fn fs_main(@location(0) uv_in: vec2<f32>) -> @location(0) vec4<f32> {
    // Shadertoy origin is bottom-left; wgpu's uv_in is top-left. Flip y
    // so the sun/mountain stay up and the grid stays down.
    var uv = vec2<f32>(2.0 * uv_in.x - 1.0, 1.0 - 2.0 * uv_in.y);
    uv.x = uv.x * (u.resolution.x / max(u.resolution.y, 1.0));
    // Life drives the "battery" the original used for brightness/grid
    // speed — full life is a fully-lit scene, 0 life dims everything.
    let battery = clamp(u.state.x, 0.15, 1.0);

    let fog = smoothstep(0.1, -0.02, abs(uv.y + 0.2));
    var col = vec3<f32>(0.0, 0.1, 0.2);
    if (uv.y < -0.2) {
        var g_uv = uv;
        g_uv.y = 3.0 / (abs(uv.y + 0.2) + 0.05);
        g_uv.x = g_uv.x * g_uv.y * 1.0;
        let gv = grid(g_uv, battery);
        col = mix(col, vec3<f32>(1.0, 0.5, 1.0), gv);
    } else {
        let fujiD = min(uv.y * 4.5 - 0.5, 1.0);
        var uv2 = uv;
        uv2.y = uv2.y - (battery * 1.1 - 0.51);

        let sunUV = uv2 + vec2<f32>(0.75, 0.2);
        col = vec3<f32>(1.0, 0.2, 1.0);
        let sunVal = sun(sunUV, battery);
        col = mix(col, vec3<f32>(1.0, 0.4, 0.1), sunUV.y * 2.0 + 0.2);
        col = mix(vec3<f32>(0.0), col, sunVal);

        let fujiVal = sd_trapezoid(uv2 + vec2<f32>(-0.75 + sunUV.y * 0.0, 0.5),
                                    1.75 + pow(game_audio(0.10), 2.1),
                                    0.2, 0.5);
        let waveVal = uv2.y + sin(uv2.x * 20.0 + u.time * 2.0) * 0.05 + 0.2;
        let wave_w = smoothstep(0.0, 0.01, waveVal);
        col = mix(col, mix(vec3<f32>(0.0, 0.0, 0.25), vec3<f32>(1.0, 0.0, 0.5), fujiD),
                  step(fujiVal, 0.0));
        col = mix(col, vec3<f32>(1.0, 0.5, 1.0), wave_w * step(fujiVal, 0.0));
        col = mix(col, vec3<f32>(1.0, 0.5, 1.0), 1.0 - smoothstep(0.0, 0.01, abs(fujiVal)));
        col = col + mix(col, mix(vec3<f32>(1.0, 0.12, 0.8), vec3<f32>(0.0, 0.0, 0.2),
                                   clamp(uv2.y * 3.5 + 3.0, 0.0, 1.0)),
                         step(0.0, fujiVal));

        var cloudUV = uv2;
        cloudUV.x = gmod(cloudUV.x + u.time * 0.1, 4.0) - 2.0;
        let cloudTime = u.time * 0.5;
        var cloudY = -0.9 + pow(game_audio(0.35), 0.7);
        let cv1 = sd_cloud(cloudUV,
            vec2<f32>(0.1 + sin(cloudTime + 140.5) * 0.1, cloudY),
            vec2<f32>(1.05 + cos(cloudTime * 0.9 - 36.56) * 0.1, cloudY),
            vec2<f32>(0.2 + cos(cloudTime * 0.867 + 387.165) * 0.1, 0.25 + cloudY),
            vec2<f32>(0.5 + cos(cloudTime * 0.9675 - 15.162) * 0.09, 0.25 + cloudY),
            0.075);
        cloudY = -1.0 + pow(game_audio(0.25), 0.7);
        let cv2 = sd_cloud(cloudUV,
            vec2<f32>(-0.9 + cos(cloudTime * 1.02 + 541.75) * 0.1, cloudY),
            vec2<f32>(-0.5 + sin(cloudTime * 0.9 - 316.56) * 0.1, cloudY),
            vec2<f32>(-1.5 + cos(cloudTime * 0.867 + 37.165) * 0.1, 0.25 + cloudY),
            vec2<f32>(-0.6 + sin(cloudTime * 0.9675 + 665.162) * 0.09, 0.25 + cloudY),
            0.075);
        let cv = min(cv1, cv2);
        col = mix(col, vec3<f32>(0.0, 0.0, 0.2), 1.0 - smoothstep(0.075 - 0.0001, 0.075, cv));
        col = col + vec3<f32>(1.0) * (1.0 - smoothstep(0.0, 0.01, abs(cv - 0.075)));
    }
    col = col + vec3<f32>(fog * fog * fog);
    col = mix(vec3<f32>(col.r) * 0.5, col, battery * 0.7);
    return vec4<f32>(col, 1.0);
}
