struct Push {
    color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Push;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    // Fullscreen triangle in clip space; the scissor + viewport restrict it to the region.
    var p = array<vec2<f32>, 3>(
        vec2(-1.0, -3.0), vec2(-1.0, 1.0), vec2(3.0, 1.0)
    );
    return vec4(p[vid], 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> { return u.color; }
