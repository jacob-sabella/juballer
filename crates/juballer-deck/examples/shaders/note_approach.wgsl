// note_approach.wgsl — rhythm-mode per-tile note indicator.
//
// Per-pixel procedural animation (no sprite-sheet frame stepping) so
// everything stays smooth regardless of frame rate. `u.kind` selects the
// phase; per-grade effects are genuinely different geometry, not the same
// burst re-tinted.
//
// kind meanings:
//   0 = approach (not yet judged)
//   1 = Perfect    — rotating 8-ray starburst, bright green
//   2 = Great      — 6-ray starburst, yellow, pulsing
//   3 = Good       — concentric expanding rings, orange
//   4 = Poor       — jittery shake + tight pulse, warm red
//   5 = Miss       — dark red fade + crack lines
//
// Uniforms in use:
//   u.kind       — phase selector (see above)
//   u.flash      — approach 0..1 (0 entered window, 1 at hit)
//   u.toggle_on  — 1.0 if a long note is currently being held
//   u.time       — seconds since app start (continuous rotation source)
//   u.state.x    — judgment freeze 0..1 (1 right at hit, fades over trail)
//   u.state.y    — hold_progress 0..1 (1 at press, 0 at release_time)
//   u.state.z    — is_long flag
//   u.state.w    — stack count (dots badge)
//   u.cursor     — long-note arrow direction (head → tail)
//   u.accent     — tile color

fn ring_mask(p: vec2<f32>, half: f32, width: f32) -> f32 {
    let d = max(abs(p.x), abs(p.y));
    let inner = half - width;
    return smoothstep(inner - 0.005, inner + 0.005, d)
         - smoothstep(half - 0.005,  half + 0.005, d);
}

fn circle_ring(r: f32, radius: f32, width: f32) -> f32 {
    return 1.0 - smoothstep(width * 0.4, width, abs(r - radius));
}

/// Rotated radial "ray" burst. `rays` = number of arms; `phase` drives the
/// rotation; `tightness` controls how sharp each arm is (higher = sharper).
fn starburst(p: vec2<f32>, rays: f32, phase: f32, tightness: f32) -> f32 {
    let ang = atan2(p.y, p.x) + phase;
    let m = 0.5 + 0.5 * cos(ang * rays);
    return pow(m, tightness);
}

/// Cheap pseudo-random for shake offsets.
fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let approach = clamp(u.flash, 0.0, 1.0);
    let freeze = clamp(u.state.x, 0.0, 1.0);
    let hold_progress = clamp(u.state.y, 0.0, 1.0);
    let is_long = u.state.z;
    let holding = u.toggle_on;
    let p = uv - vec2<f32>(0.5, 0.5);
    let d_max = max(abs(p.x), abs(p.y));
    let r = length(p);
    let tint = u.accent.rgb;
    let edge = 0.01;

    var out_rgb = vec3<f32>(0.0);
    var out_a = 0.0;

    if (holding > 0.5) {
        // ── Long-note hold: bright cyan square w/ thick outline + drain bar.
        // Hardcoded cyan (NOT `tint`) so the held cell is unmistakable
        // versus the white-tinted approach visuals on neighbouring cells —
        // previous version used the accent tint, which is white before
        // judgment, making a held cell look identical to a same-instant
        // approach peak on another cell. The thick outline + saturated
        // hue eliminates that ambiguity.
        let hold_color = vec3<f32>(0.30, 0.95, 1.0);
        let outline_color = vec3<f32>(1.0, 1.0, 1.0);
        let half_outer = 0.42;
        let half_inner = 0.34;
        let outer_mask = 1.0 - smoothstep(half_outer - edge, half_outer + edge, d_max);
        let inner_mask = 1.0 - smoothstep(half_inner - edge, half_inner + edge, d_max);
        let outline_mask = clamp(outer_mask - inner_mask, 0.0, 1.0);
        let pulse = 0.85 + 0.15 * sin(u.time * 8.0);
        out_rgb = hold_color * pulse;
        out_a = inner_mask;
        // White outline on top so the cell pops over any background.
        out_rgb = mix(out_rgb, outline_color, outline_mask);
        out_a = max(out_a, outline_mask);
        // Drain bar — bright white on cyan so the hold-time-remaining
        // read is unambiguous.
        let bar_y_top = 0.04;
        let bar_y_bot = 0.10;
        let in_band = step(uv.y, bar_y_bot) * (1.0 - step(uv.y, bar_y_top));
        let bar_x_max = 0.10 + hold_progress * 0.80;
        let in_x = step(uv.x, bar_x_max) * (1.0 - step(uv.x, 0.10));
        let bar_a = in_band * in_x * 0.95;
        out_rgb = mix(out_rgb, vec3<f32>(1.0, 1.0, 1.0), bar_a);
        out_a = max(out_a, bar_a);
    } else if (u.kind < 0.5) {
        // ── Approach (not yet judged) ────────────────────────────────────
        // Classic target-lock: a persistent outer circle sits at a fixed
        // ~40% radius for the whole lead-in, a small solid dot sits at
        // tile center as the hit target, and a shrinking closing ring
        // sweeps from near the outer circle down onto the target dot.
        // When the closing ring overlaps the center dot → now is the
        // moment to tap. Everything is computed from a smoothstep-eased
        // `approach` so there's no jitter, no frame-rate coupling, no
        // abrupt transitions.
        let eased = smoothstep(0.0, 1.0, approach);

        // Persistent outer circle — constant size, gentle brightness ramp.
        let outer_r = 0.42;
        let outer_w = 0.018;
        let outer_m = circle_ring(r, outer_r, outer_w);
        let outer_rgb = tint * (0.55 + 0.25 * eased);

        // Center target dot — small, always visible. Gets subtly brighter
        // as the note nears hit so the eye picks out what to aim at.
        let target_r = 0.045;
        let target_m = 1.0 - smoothstep(target_r - 0.006, target_r + 0.006, r);
        let target_rgb = mix(
            tint * 0.55,
            vec3<f32>(1.0, 1.0, 1.0),
            eased,
        );

        // Closing ring — shrinks from ~0.40 (hugging the outer circle) to
        // the target dot's radius at hit time. Its thickness tapers so
        // it reads as "tightening". When eased → 1, it lands *on* the
        // target dot.
        let close_r = mix(0.38, target_r + 0.010, eased);
        let close_w = mix(0.060, 0.020, eased);
        let close_m = circle_ring(r, close_r, close_w);
        let close_rgb = tint * (0.85 + 0.15 * eased);

        // Visibility ramp — starts at 0.30 and climbs to 1.0 as the note
        // nears hit. Smoothstep'd so the entry is soft, not a linear pop.
        let vis = mix(0.30, 1.0, smoothstep(0.0, 1.0, eased));
        // No pulse term — the closing-ring motion itself is the rhythm
        // cue. The previous pulse `sin(u.time * (4 + 8*approach))` made
        // the frequency creep with approach which read as jitter at the
        // final moment instead of a smooth convergence.
        let ring_m = outer_m;
        let disc_m = close_m;
        let ring_rgb = outer_rgb;
        let disc_rgb = close_rgb;

        // Long-note arrow / tail hint — drawn over/under the ring so it
        // reads as a direction indicator, not a separate shape.
        var tail_rgb = vec3<f32>(0.0);
        var tail_m = 0.0;
        if (is_long > 0.5) {
            let dir_len = length(u.cursor);
            let ts = 0.25 + 0.55 * approach;
            if (dir_len > 0.001) {
                let dir = u.cursor / dir_len;
                let perp = vec2<f32>(-dir.y, dir.x);
                let t_ax = dot(p, dir);
                let n_ax = dot(p, perp);
                let shaft_w = 0.035;
                let sh_mask = step(0.20, t_ax) * (1.0 - step(0.42, t_ax))
                    * (1.0 - smoothstep(shaft_w - 0.01, shaft_w + 0.005, abs(n_ax)));
                let head_t0 = 0.38;
                let head_t1 = 0.48;
                let hp = clamp((t_ax - head_t0) / max(head_t1 - head_t0, 1e-3), 0.0, 1.0);
                let head_w = 0.09 * (1.0 - hp);
                let hd_mask = step(head_t0, t_ax) * (1.0 - step(head_t1, t_ax))
                    * (1.0 - smoothstep(head_w - 0.008, head_w + 0.004, abs(n_ax)));
                tail_m = max(sh_mask, hd_mask) * ts;
                tail_rgb = tint * 0.95;
            } else {
                let in_x = 1.0 - smoothstep(0.04, 0.06, abs(p.x));
                let in_y = step(0.0, p.y) * (1.0 - step(0.45, p.y));
                tail_m = in_x * in_y * ts;
                tail_rgb = tint * 0.85;
            }
        }

        // Stacking order (back → front):
        //   tail hint (if long note) → outer ring → closing ring → target dot
        // so the target dot always reads on top and the closing ring
        // visually "lands" on it at hit time.
        let base_a = max(max(max(ring_m, disc_m), tail_m), target_m);
        let base_rgb = ring_rgb * ring_m
            + disc_rgb * disc_m * (1.0 - ring_m)
            + tail_rgb * tail_m * (1.0 - ring_m) * (1.0 - disc_m)
            + target_rgb * target_m;
        out_rgb = base_rgb;
        out_a = base_a * vis;
    } else {
        // ── Judgment phase — per-grade geometry. `t` = animation progress
        // 0..1 where 0 is at-judgment and 1 is trail-end. Easier to reason
        // about than `freeze` (which is 1→0).
        let t = 1.0 - freeze;
        if (u.kind < 1.5) {
            // Perfect — 8-ray rotating starburst + bright core, green accent.
            let radius = mix(0.10, 0.55, t);
            let rays = starburst(p, 8.0, u.time * 3.0, 6.0);
            let ray_r = smoothstep(radius + 0.05, radius - 0.02, r);
            let ray_mask = rays * ray_r;
            // Glowing core that shrinks as burst expands.
            let core = exp(-pow(r / mix(0.08, 0.24, t), 2.0)) * (1.0 - t);
            let a = clamp(ray_mask + core, 0.0, 1.0) * (1.0 - t * 0.3);
            out_rgb = tint * (1.1 + 0.6 * ray_mask + 0.9 * core);
            out_a = a;
        } else if (u.kind < 2.5) {
            // Great — 6-ray pulsing starburst, slightly softer.
            let radius = mix(0.10, 0.48, t);
            let pulse = 0.5 + 0.5 * sin(u.time * 9.0);
            let rays = starburst(p, 6.0, u.time * 2.2, 4.5);
            let ray_r = smoothstep(radius + 0.06, radius - 0.02, r);
            let core = exp(-pow(r / mix(0.09, 0.20, t), 2.0)) * (1.0 - t) * (0.65 + 0.35 * pulse);
            let a = clamp(rays * ray_r + core, 0.0, 1.0) * (1.0 - t * 0.3);
            out_rgb = tint * (0.95 + 0.5 * rays * ray_r + 0.8 * core);
            out_a = a;
        } else if (u.kind < 3.5) {
            // Good — concentric expanding rings, no rays.
            let r1 = mix(0.08, 0.45, t);
            let r2 = mix(0.00, 0.30, t);
            let w = 0.04 + 0.02 * t;
            let m1 = circle_ring(r, r1, w);
            let m2 = circle_ring(r, r2, w * 0.8);
            let a = clamp(m1 * 0.9 + m2 * 0.6, 0.0, 1.0) * (1.0 - t * 0.4);
            out_rgb = tint * (0.75 + 0.6 * (m1 + m2));
            out_a = a;
        } else if (u.kind < 4.5) {
            // Poor — jittery single tight pulse, shaky.
            let shake = vec2<f32>(
                hash2(vec2<f32>(floor(u.time * 40.0), 1.0)) - 0.5,
                hash2(vec2<f32>(floor(u.time * 40.0), 2.0)) - 0.5,
            ) * 0.04;
            let rs = length(p - shake);
            let rad = mix(0.12, 0.30, t);
            let m = exp(-pow(rs / max(rad, 1e-3), 2.0)) * (1.0 - t);
            out_rgb = tint * (0.8 + 0.4 * m);
            out_a = clamp(m, 0.0, 1.0);
        } else {
            // Miss — dark red fade + two diagonal crack lines.
            let fade = (1.0 - t);
            let crack_w = 0.02 + 0.015 * t;
            let diag1 = abs(p.x + p.y) - 0.02;
            let diag2 = abs(p.x - p.y) - 0.02;
            let c1 = 1.0 - smoothstep(crack_w * 0.5, crack_w, diag1);
            let c2 = 1.0 - smoothstep(crack_w * 0.5, crack_w, diag2);
            let core = exp(-pow(r / 0.32, 2.0)) * fade * 0.6;
            let crack = max(c1, c2) * fade * 0.8;
            let a = clamp(core + crack, 0.0, 1.0);
            out_rgb = tint * (0.5 + 0.4 * crack) + vec3<f32>(0.10, 0.0, 0.0) * core;
            out_a = a;
        }
    }

    // Stack badge — row of dots top-right.
    let stack = clamp(u.state.w, 0.0, 6.0);
    if (stack > 0.5) {
        let dot_r = 0.018;
        let spacing = 0.05;
        let x0 = 0.92;
        let y0 = 0.08;
        for (var i: i32 = 0; i < 6; i = i + 1) {
            if (f32(i) >= stack) { break; }
            let cx = x0 - f32(i) * spacing;
            let dx = uv.x - cx;
            let dy = uv.y - y0;
            let rr = sqrt(dx * dx + dy * dy);
            let dm = 1.0 - smoothstep(dot_r * 0.7, dot_r, rr);
            if (dm > 0.01) {
                out_rgb = mix(out_rgb, vec3<f32>(1.0, 1.0, 1.0), dm);
                out_a = max(out_a, dm * 0.95);
            }
        }
    }

    return vec4<f32>(out_rgb, out_a);
}
