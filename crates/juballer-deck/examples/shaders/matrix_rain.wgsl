fn hash(p: vec2<f32>) -> f32 {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q.x + q.y) * 43758.5453);
}

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let cols = 16.0;
    let rows = 20.0;
    let cell = vec2<f32>(floor(uv.x * cols), floor(uv.y * rows));
    let t = u.time;
    let speed = 1.0 + hash(vec2<f32>(cell.x, 0.0)) * 2.0;
    let fall = fract(cell.y / rows - t * speed * 0.15);
    let glyph = hash(cell + vec2<f32>(floor(t * speed * 4.0), 0.0));
    let flicker = step(0.15, glyph);
    let head = 1.0 - fall;
    let trail = pow(head, 4.0);
    let col = vec3<f32>(0.1, 0.9, 0.3) * trail * flicker;
    return vec4<f32>(col, 1.0);
}
