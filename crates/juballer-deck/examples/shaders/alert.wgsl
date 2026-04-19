// alert.wgsl — urgent red pulsing for alert/notification tiles.
@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) { return vec4<f32>(0.0); }
    let pulse = 0.5 + 0.5 * sin(u.time * 5.0);
    let c = uv - vec2<f32>(0.5, 0.5);
    let d = length(c);
    let vignette = smoothstep(0.85, 0.1, d);
    let scan = 0.5 + 0.5 * sin(uv.y * 30.0 + u.time * 6.0);
    let red = vec3<f32>(0.95, 0.35, 0.42);
    let i = vignette * (0.35 + 0.4 * pulse) + 0.1 * scan * vignette;
    return vec4<f32>(red * i, i);
}
