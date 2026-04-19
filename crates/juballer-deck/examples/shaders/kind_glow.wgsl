// kind_glow.wgsl — adaptive per-tile shader:
//   kind==0 action  -> diagonal shimmer band in accent
//   kind==1 nav     -> slow radial breathing pulse
//   kind==2 toggle  -> bottom fill in state color, height = toggle_on
//   press flash overlays a bright ripple on top of any kind
// Output is pre-multiplied with its own alpha; egui icon+label paints on top.

fn nav_pulse(uv: vec2<f32>) -> vec4<f32> {
    let c = uv - vec2<f32>(0.5, 0.5);
    let d = length(c);
    let breath = 0.5 + 0.5 * sin(u.time * 1.6);
    let ring = 0.5 + 0.5 * sin(d * 14.0 - u.time * 2.0);
    let falloff = smoothstep(0.85, 0.1, d);
    let i = falloff * (0.35 + 0.55 * breath) + 0.25 * ring * falloff;
    return vec4<f32>(u.accent.rgb * i, i);
}

fn toggle_fill(uv: vec2<f32>) -> vec4<f32> {
    let y = 1.0 - uv.y;
    let off_h = 5.0 / u.resolution.y;
    let on_h = 0.55;
    let fill_to = mix(off_h, on_h, clamp(u.toggle_on, 0.0, 1.0));
    let edge = smoothstep(fill_to, fill_to - 0.05, y);
    let shimmer = 0.5 + 0.5 * sin(uv.x * 11.0 + u.time * 1.8);
    let shimmer_mix = mix(0.0, 0.35, clamp(u.toggle_on, 0.0, 1.0));
    let rgb = u.state.rgb * (1.0 + shimmer_mix * shimmer);
    return vec4<f32>(rgb * edge, edge);
}

fn action_shimmer(uv: vec2<f32>) -> vec4<f32> {
    let band = 0.5 + 0.5 * sin((uv.x + uv.y) * 5.0 - u.time * 1.3);
    let c = uv - vec2<f32>(0.5, 0.5);
    let vignette = smoothstep(0.85, 0.1, length(c));
    let i = vignette * (0.22 + 0.35 * band);
    return vec4<f32>(u.accent.rgb * i, i);
}

fn press_overlay(uv: vec2<f32>) -> vec4<f32> {
    if (u.flash <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let c = uv - vec2<f32>(0.5, 0.5);
    let d = length(c);
    let progress = 1.0 - u.flash;
    let ring_r = progress * 0.6;
    let ring_w = 0.05 + 0.08 * progress;
    let r = 1.0 - smoothstep(ring_w * 0.5, ring_w, abs(d - ring_r));
    let fade = u.flash * u.flash;
    let a = r * fade;
    return vec4<f32>(vec3<f32>(1.0, 1.0, 1.0) * a, a);
}

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    var base: vec4<f32>;
    if (u.kind < 0.5) {
        base = action_shimmer(uv);
    } else if (u.kind < 1.5) {
        base = nav_pulse(uv);
    } else {
        base = toggle_fill(uv);
    }

    let ripple = press_overlay(uv);
    let out_rgb = ripple.rgb + base.rgb * (1.0 - ripple.a);
    let out_a = clamp(ripple.a + base.a * (1.0 - ripple.a), 0.0, 1.0);
    return vec4<f32>(out_rgb, out_a);
}
