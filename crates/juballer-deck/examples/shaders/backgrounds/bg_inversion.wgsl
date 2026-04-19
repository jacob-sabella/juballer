// bg_inversion.wgsl — HUD background.
//
// Port of "The Inversion Machine" by Kali
//   https://www.shadertoy.com/view/4dsGD7
// License: CC BY-NC-SA 3.0 (Shadertoy default). See LICENSES.md.
//
// Inversive-geometry raymarch: space is inverted through the origin
// (p / dot(p,p)), scaled, and fed through a sin() fold, then three
// infinite cylinders + a cubic blob supply the SDF. Original shook the
// camera with a fixed `sin(t*40)*.007` offset on p.x. juballer port
// drives that shake amplitude from `game_audio()` so the machine jolts
// on bass/flash + combo.
//
// Raymarch iteration count kept at 60 (same as source) since the fold
// is cheap and the HUD strip is small.

const WIDTH: f32 = 0.22;
const SCALE: f32 = 4.0;
const DETAIL: f32 = 0.002;

// game_audio(x) is provided by the standard uniform prelude and reads
// live FFT bins of the currently-playing music.

fn rand2(co: vec2<f32>) -> f32 {
    return fract(sin(dot(co, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn rot_mat(a: f32) -> mat2x2<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat2x2<f32>(c, s, -s, c);
}

// Shake amplitude: original was a fixed 0.007. Here it's a floor of
// 0.003 plus bass (combo+beat) and flash kicks, so the machine is
// mostly still but slams when the player is on a streak and hits.
fn shake_amp() -> f32 {
    let bass = game_audio(0.10);
    let flash = clamp(u.flash, 0.0, 1.0);
    return 0.003 + 0.018 * bass + 0.012 * flash;
}

fn de(p_in: vec3<f32>) -> f32 {
    let t = u.time;
    var p = p_in;
    let dotp = max(dot(p, p), 1e-6);
    p.x = p.x + sin(t * 40.0) * shake_amp();
    p = p / dotp * SCALE;
    p = sin(p + vec3<f32>(sin(1.0 + t) * 2.0, -t, -t * 2.0));
    var d = length(p.yz) - WIDTH;
    d = min(d, length(p.xz) - WIDTH);
    d = min(d, length(p.xy) - WIDTH);
    d = min(d, length(p * p * p) - WIDTH * 0.3);
    return d * dotp / SCALE;
}

fn normal_at(p: vec3<f32>) -> vec3<f32> {
    let e = vec3<f32>(0.0, DETAIL, 0.0);
    return normalize(vec3<f32>(
        de(p + e.yxx) - de(p - e.yxx),
        de(p + e.xyx) - de(p - e.xyx),
        de(p + e.xxy) - de(p - e.xxy),
    ));
}

fn lightdir() -> vec3<f32> {
    return -vec3<f32>(0.2, 0.5, 1.0);
}

fn light_at(p: vec3<f32>, dir: vec3<f32>) -> f32 {
    let ldir = normalize(lightdir());
    let n = normal_at(p);
    let diff = max(0.0, dot(ldir, -n)) + 0.1 * max(0.0, dot(normalize(dir), -n));
    let r = reflect(ldir, n);
    let spec = max(0.0, dot(dir, -r));
    return diff + pow(spec, 20.0) * 0.7;
}

fn raymarch(ro: vec3<f32>, dir: vec3<f32>, uv: vec2<f32>, rot: mat2x2<f32>) -> f32 {
    var totdist: f32 = 0.0;
    var st: f32 = 0.0;
    var d: f32 = 0.0;
    var p: vec3<f32> = ro;
    let ra = rand2(uv * u.time) - 0.5;
    let ras = max(0.0, sign(-0.5 + rand2(vec2<f32>(1.3456, 0.3573) * floor(30.0 + u.time * 20.0))));
    let rab = rand2(vec2<f32>(1.2439, 2.3453) * floor(10.0 + u.time * 40.0)) * ras;
    let rac = rand2(vec2<f32>(1.1347, 1.0331) * floor(40.0 + u.time));
    let ral = rand2(vec2<f32>(1.0, 1.0) + floor(uv.yy * 300.0) * u.time) - 0.5;

    var hit: bool = false;
    for (var i: i32 = 0; i < 60; i = i + 1) {
        p = ro + totdist * dir;
        d = de(p);
        if (d < DETAIL) {
            hit = true;
            break;
        }
        if (totdist > 2.0) { break; }
        totdist = totdist + d;
        st = st + max(0.0, 0.04 - d);
    }

    let li = uv * rot;
    let backg = 0.45 * pow(1.5 - min(1.0, length(li + vec2<f32>(0.0, -0.6))), 1.5);

    var col: f32;
    if (hit) {
        col = light_at(p - DETAIL * dir, dir);
    } else {
        col = backg;
    }
    col = col + smoothstep(0.0, 1.0, st) * 0.8 * (0.1 + rab);
    col = col + pow(max(0.0, 1.0 - length(p)), 8.0) * (0.5 + 10.0 * rab);
    col = col + pow(max(0.0, 1.0 - length(p)), 30.0) * 50.0;
    col = mix(col, backg, 1.0 - exp(-0.25 * pow(totdist, 3.0)));
    if (rac > 0.7) {
        col = col * 0.7 + (0.3 + ra + ral * 0.5) * ((uv.y + u.time * 2.0) - floor((uv.y + u.time * 2.0) / 0.25) * 0.25);
    }
    // Original had an "intro fade from grey" over the first 3 seconds;
    // skipped here since the HUD runs continuously — no intro frame.
    return col + ra * 0.03 + (ral * 0.1 + ra * 0.1) * rab;
}

@fragment
fn fs_main(@location(0) uv_in: vec2<f32>) -> @location(0) vec4<f32> {
    let t = u.time;
    let aspect_y = u.resolution.y / max(u.resolution.x, 1.0);
    var uv = 2.0 * uv_in - vec2<f32>(1.0);
    uv.y = uv.y * aspect_y;

    let ro = vec3<f32>(0.0, 0.1, -1.2);
    var dir = normalize(vec3<f32>(uv, 1.0));
    let rm = rot_mat(t);
    let dxy = dir.xy * rm;
    dir = vec3<f32>(dxy, dir.z);

    let col = raymarch(ro, dir, uv, rm);
    let life_dim = mix(0.45, 1.0, u.state.x);
    return vec4<f32>(vec3<f32>(col) * life_dim, 1.0);
}
