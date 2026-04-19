# Per-tile custom WGSL shaders

juballer-deck can render a custom fragment shader into any tile. The shader
draws underneath the standard egui tile overlay (icon, label, flash, border),
so you get a shader background with the same tile chrome on top.

## Config

Point a button at a WGSL file via the `shader` field in a page's `[[button]]`
table:

```toml
[[button]]
row = 0
col = 0
action = "shell.run"
args = { cmd = "true" }
icon = "P"
label = "plasma"
shader = { wgsl = "/home/user/shaders/plasma.wgsl" }
```

For a v4l2 webcam source, use the `video` form:

```toml
[[button]]
row = 0
col = 1
action = "shell.run"
args = { cmd = "true" }
icon = "CAM"
label = "cam"
shader = { video = "v4l2:///dev/video0" }
```

Actions can also swap a tile's shader at runtime:

- `tile.set_shader { wgsl = "/path/to.wgsl" }` — set a custom WGSL shader
- `tile.clear_shader` — remove the shader, fall back to plain egui

## Uniforms

Every tile shader receives a uniform block at group 0, binding 0 with this
fixed 80-byte layout:

```wgsl
struct Uniforms {
    resolution: vec2<f32>,  // tile size in pixels
    time: f32,              // seconds since deck boot
    delta_time: f32,        // seconds since the previous frame

    cursor: vec2<f32>,      // always (0,0) on the deck — no pointer (yet)
    kind: f32,              // 0=Action, 1=Nav, 2=Toggle
    bound: f32,             // 1.0 if this cell has a bound action, else 0.0

    toggle_on: f32,         // 1.0 when a Toggle tile is in its "on" state
    flash: f32,             // 1.0 at moment of press, decays to 0 over ~280ms
    _pad0: vec2<f32>,       // alignment

    accent: vec4<f32>,      // per-kind primary accent color (rgba 0..1)
    state: vec4<f32>,       // tile.state_color if set, else accent
};
@group(0) @binding(0) var<uniform> u: Uniforms;
```

The first 16 bytes (`resolution`, `time`, `delta_time`) match the original
layout, so legacy shaders that only read those fields keep working.

### What each state uniform means

- `kind` matches the Rust `ActionKind` discriminant of the bound action:
  - `0.0` — Action (shell, media, http, …)
  - `1.0` — Nav (page/profile switch)
  - `2.0` — Toggle (on/off, cycle N)
- `bound` lets a shader distinguish bound cells from the empty ones that
  don't dispatch anything. Use this to short-circuit to transparent for
  empty cells.
- `toggle_on` is only meaningful when `kind == 2.0`. It's `1.0` when the
  tile currently carries a `state_color` (how the `toggle.onoff` and
  `toggle.cycle_n` builtins signal they're on), else `0.0`.
- `flash` decays from `1.0` to `0.0` over the flash window on press. You
  can square it for a snappier tail, or use it linearly as an alpha.
- `accent` is the per-kind accent: `theme.accent` (lavender) for Nav,
  `theme.ok` (green) for Toggle, `theme.accent_alt` (blue) for Action.
- `state` mirrors `tile.state_color` if the action has set one; otherwise
  it falls back to `accent`. Useful when a toggle has chosen a specific
  active color (e.g. DND → red).

### Writing a kind-adaptive shader

The simplest adaptive pattern is a switch on `u.kind`:

```wgsl
@fragment
fn fs_main(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    if (u.bound < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0); // leave empty cells alone
    }
    let uv = frag_pos.xy / u.resolution;
    if (u.kind < 0.5) {
        // Action — shimmer in the accent color.
        let band = 0.5 + 0.5 * sin((uv.x + uv.y) * 4.5 - u.time);
        let a = 0.15 * band;
        return vec4<f32>(u.accent.rgb * a, a);
    } else if (u.kind < 1.5) {
        // Nav — radial pulse.
        let d = length(uv - vec2<f32>(0.5, 0.5));
        let pulse = 0.5 + 0.5 * sin(u.time * 2.0);
        let a = smoothstep(0.6, 0.0, d) * pulse * 0.4;
        return vec4<f32>(u.accent.rgb * a, a);
    } else {
        // Toggle — fill bar at the bottom, height follows toggle_on.
        let y = 1.0 - uv.y;
        let fill_to = mix(0.01, 0.45, u.toggle_on);
        let edge = smoothstep(fill_to, fill_to - 0.03, y);
        let a = edge * 0.7;
        return vec4<f32>(u.state.rgb * a, a);
    }
}
```

### Reading `flash` for press feedback

```wgsl
if (u.flash > 0.0) {
    let d = length((frag_pos.xy / u.resolution) - vec2<f32>(0.5, 0.5));
    let ring_r = (1.0 - u.flash) * 0.55;
    let ring = 1.0 - smoothstep(0.03, 0.06, abs(d - ring_r));
    let a = ring * u.flash * u.flash;
    return vec4<f32>(u.accent.rgb * a, a);
}
```

## Minimum shader

Only a fragment entry point is required. The deck auto-injects a standard
full-quad vertex shader and the `Uniforms` struct if they are absent:

```wgsl
@fragment
fn fs_main(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag_pos.xy / u.resolution;
    return vec4<f32>(uv.x, uv.y, 0.5 + 0.5 * sin(u.time), 1.0);
}
```

## Vertex override

Declare your own `vs_main` if you need attribute-free procedural geometry or
different clip-space topology. The deck skips the injection when it detects
an existing `vs_main`.

## Hot reload

The deck watches the WGSL files referenced by the active page. Saving the
file triggers a recompile on the next frame. If the new source fails to
compile, the old pipeline is discarded and the tile renders a plain
placeholder until you fix the file; the error is logged.

## Examples

The deck repo ships ten reference shaders in
`crates/juballer-deck/examples/shaders/`:

Legacy (resolution + time only):

- `solid_time.wgsl` — flat color cycling through the HSV wheel
- `waves.wgsl` — concentric color rings
- `plasma.wgsl` — classic sine-sum plasma
- `matrix_rain.wgsl` — falling green glyph-flicker trails

State-aware presets (kind / bound / toggle_on / flash / accent / state):

- `ambient_warmth.wgsl` — safe default. Subtle vignette + slow color drift
  in the accent color; good to pair with any tile.
- `nav_pulse.wgsl` — slow radial pulse in the accent color, only active
  when the tile is bound.
- `toggle_bar.wgsl` — animated bottom "fill" bar that grows with
  `toggle_on` and shimmers in the state color.
- `press_ripple.wgsl` — concentric rings from center triggered by
  `flash`; transparent otherwise.
- `kind_glow.wgsl` — one-size-fits-all: adapts to `u.kind` (action
  shimmer, nav pulse, toggle fill) and overlays a press ripple from
  `flash`. Use this as a default for all bound tiles.
- `empty_dotgrid.wgsl` — designed for unbound cells: dim dot grid + tiny
  center crosshair. Bound cells are short-circuited to transparent.

## Performance budget

Tile shaders run up to 16× per frame at 60fps. Keep fragment math cheap —
no loops with unbounded iteration, no textures, no push constants beyond
the 80-byte uniform block. The default-format blend is alpha-blended, so
returning `vec4(0.0)` always means "no effect"; use this to let empty or
off-state cells fall through to the default egui chrome.
