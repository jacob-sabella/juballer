// lock_frost.wgsl — cool blue scanline + ice shimmer for lock/security tiles.
@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) { return vec4<f32>(0.0); }
    // Vertical scanline that slowly sweeps.
    let scan_y = fract(u.time * 0.2);
    let scan = exp(-abs(uv.y - scan_y) * 8.0);
    // Ice shimmer — dithered diagonal.
    let shimmer = 0.5 + 0.5 * sin(uv.x * 30.0 + uv.y * 30.0 + u.time * 2.0);
    let frost = vec3<f32>(0.54, 0.78, 0.92);
    let i = 0.2 + 0.5 * scan + 0.08 * shimmer;
    return vec4<f32>(frost * i, i);
}
