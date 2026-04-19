# Rhythm Mode — SFX Rework + Note Rendering Polish

**Date:** 2026-04-19
**Scope:** `crates/juballer-deck/src/rhythm/` — `sfx.rs`, `render.rs`, `marker.rs`, `cli.rs` (flag), asset directory under `assets/sample/sfx/`.

## Motivation

Playtesting feedback:

1. **Hit/close/miss SFX are too loud and stack.** Chord taps (e.g. four notes
   judged in the same frame) fire four full-volume sinks simultaneously — slot-
   machine effect. Each grade also has a distinct sample, so a streak sounds
   noisy and uneven.
2. **Note rendering looks steppy.** Marker sprite sheets advance with
   `floor(offset_s * fps)`; at 60 Hz display and 30–34 fps art, each frame
   shows for two display frames. The approach pulse and judgment burst both
   visibly pop between frames.
3. **No visual "target" inside the cell.** Players have no anchor for *when*
   the note is supposed to land — the approach sprite grows but nothing marks
   the hit moment itself.

## Goals

- Quieter, less layered SFX modeled on Clone Hero / Stepmania.
- Smoother note animation without regenerating art.
- Add a stationary "hit moment" ring inside each cell hosting an approaching
  note, to anchor timing visually.

## Non-Goals

- Changing the memon chart format, judge windows, or scoring.
- Regenerating marker sprite sheets (all smoothness gains come from sub-frame
  tweening in code).
- Remaking the SFX samples themselves — `tick.ogg` can be sourced or generated
  via `scripts/generate_markers.sh`-style ImageMagick equivalent for audio, or
  hand-placed. Sample sourcing is out of scope for this spec; the spec only
  defines how the bank consumes them.
- Tutorial / calibration flows: they keep calling `SfxBank::play` unchanged.

---

## Section 1 — SFX Rework (Hybrid CH/SM)

### Sample layout

`assets/sample/sfx/`:

| File            | Role                               | Status         |
|-----------------|------------------------------------|----------------|
| `tick.ogg`      | Shared quiet click for P/G/G       | **NEW**        |
| `poor.ogg`      | Distinct "close" sound              | keep           |
| `miss.ogg`      | Distinct loud miss                  | keep           |
| `perfect.ogg`   | Retired (unused)                    | leave on disk  |
| `great.ogg`     | Retired (unused)                    | leave on disk  |
| `good.ogg`      | Retired (unused)                    | leave on disk  |

`sample_filename(grade)` in `sfx.rs` collapses:

```text
Perfect | Great | Good  → "tick.ogg"
Poor                    → "poor.ogg"
Miss                    → "miss.ogg"
```

Loader continues to tolerate missing files → silent no-op.

### Bank behavior

`SfxBank` gains:

- `master_volume: f32` — default `0.35`. Clamped `[0.0, 1.0]`.
- `active: VecDeque<Sink>` — cap `3`. On new play when full, `pop_front()`
  (drops the sink; rodio stops it). Replaces current `sink.detach()` pattern.
- `last_played: HashMap<Grade, Instant>` — 15ms per-grade cooldown. If
  `Instant::now() - last < 15ms`, skip. Kills 4-note chord pileup where all
  four ticks fire in the same frame.

### Per-grade volume

Applied via `sink.set_volume(master_volume * factor)`:

| Grade    | Sample       | Factor |
|----------|--------------|--------|
| Perfect  | `tick.ogg`   | 0.6    |
| Great    | `tick.ogg`   | 0.6    |
| Good     | `tick.ogg`   | 0.6    |
| Poor     | `poor.ogg`   | 1.0    |
| Miss     | `miss.ogg`   | 1.1    |

`Sink` allows volume > 1.0; rodio scales digitally and will clip if the source
is already near full-scale. `miss.ogg` is mono and typically peaks well under
0dBFS, so 1.1 is safe; if clipping appears in testing we drop to 1.0.

### CLI flag

New `juballer-deck` CLI flag `--sfx-volume <0..1>` that overrides the
`master_volume` default. Existing `--mute-sfx` remains.

### Tests

- Bank with `master_volume = 0.0` plays nothing (existing muted test covers
  this in spirit — extend to assert volume path).
- 5 calls to `play(Perfect)` within 5ms result in ≤1 active sink (cooldown).
- 5 calls to `play(Miss)` within 1ms across rapid loop produce ≤3 active sinks
  (voice cap).
- `sample_filename(Perfect) == sample_filename(Great) == sample_filename(Good)
  == "tick.ogg"`.
- `sample_filename(Poor) != sample_filename(Miss)`.

---

## Section 2 — Note Rendering Polish

### Sub-frame tween

In `rhythm/render.rs :: draw_notes_markers`, the current flow:

```text
for slot in slots:
  pick (tex, uv) via m.approach_frame / m.grade_frame
  painter.image(tex, tile, uv, WHITE)
```

becomes:

```text
(tex_a, uv_a, tex_b, uv_b, t) = pick_tweened(…)
painter.image(tex_a, tile, uv_a, WHITE @ (1-t))
painter.image(tex_b, tile, uv_b, WHITE @ t)
```

New API on `rhythm/marker.rs`:

```rust
// (tex, uv_a, uv_b, t) where t in [0,1] blends A -> B.
// tex is shared across both UVs since approach/grade each live in one sheet.
pub fn approach_frame_tweened(&self, offset_ms: f64)
    -> Option<(&TextureHandle, Rect, Rect, f32)>;
pub fn grade_frame_tweened(&self, phase: GradePhase, offset_ms: f64)
    -> Option<(&TextureHandle, Rect, Rect, f32)>;
```

Implementation: same math as current `*_frame`, but instead of
`floor`, keep both `frame` and `frame + 1` and the fractional remainder `t`.
If `frame + 1 >= count`, clamp `frame + 1` to `count - 1` and `t` to `0` so
the last frame holds.

Existing `approach_frame` / `grade_frame` stay for callers that don't tween
(tests, future uses). `draw_notes_markers` switches to the tweened variants.

**Cost**: one extra `painter.image` per active cell. 16 cells max, already
paid during judgment bursts. Negligible in the egui painter.

### Hit-moment ring

New function in `render.rs`:

```rust
pub fn draw_hit_rings(
    frame: &mut Frame,
    overlay: &mut EguiOverlay,
    state: &GameState,
);
```

Called **before** `draw_notes_markers`, so sprites paint on top of the ring.

Behavior per cell:

- Only paints if the slot holds a note that is **not yet judged**
  (`slot.hit.is_none()`). Once judged, the ring disappears and the grade burst
  takes over.
- Circle centered in the cell, radius = `0.35 * min(cell_w, cell_h) / 2`.
- Stroke: 2px, color `Color32::from_rgb(94, 232, 255)` (cyan, `#5EE8FF`).
- Alpha tied to approach: `alpha = approach_factor * 0.9` — ring fades in
  with the incoming note so it doesn't draw on every empty cell.
- Filled inner disc with very low alpha (~20/255) so the ring reads as a
  solid target even over bright shader backgrounds.

No pulse animation in this iteration — static ring is sufficient to anchor
timing. Pulse can land later if needed.

### Call order in `rhythm/render` loop

Existing per-frame pipeline (roughly):

```text
paint_backgrounds      (clears cells)
draw_notes             (long-note shader path)
draw_notes_markers     (tap sprite markers)
draw_hud               (top HUD)
```

New order:

```text
paint_backgrounds
draw_notes
draw_hit_rings         ← NEW, below sprites
draw_notes_markers     (now tweened)
draw_hud
```

`draw_hit_rings` shares the markers' egui `Area` id prefix pattern but uses
its own unique id (`rhythm_hit_rings_root`) so egui doesn't collapse its
draw buffer into the markers'.

### Tests

- `approach_frame_tweened` at offset exactly on a frame boundary yields
  `t ≈ 0.0` and `frame_b = frame_a + 1`.
- Near last frame, clamps so `frame_b` doesn't exceed `count - 1`.
- `grade_frame_tweened` past animation end returns `None` (same contract as
  current `grade_frame`).

---

## Files Touched

- `crates/juballer-deck/src/rhythm/sfx.rs` — bank struct, play path, CLI wiring
  surface.
- `crates/juballer-deck/src/rhythm/marker.rs` — new tweened frame pickers.
- `crates/juballer-deck/src/rhythm/render.rs` — `draw_hit_rings` + tween call,
  call-order update in the per-frame paint pipeline.
- `crates/juballer-deck/src/cli.rs` — `--sfx-volume` flag.
- `crates/juballer-deck/src/rhythm/mod.rs` / `play` entry — thread the new
  volume argument from CLI into `SfxBank::load_default` (new
  `load_default_with_volume(v)` or setter post-load).
- `assets/sample/sfx/tick.ogg` — new asset. Sourcing is out-of-band (existing
  sample can be substituted short-term; final tick asset TBD by whoever
  generates it).

## Risks / Open Questions

- **`tick.ogg` asset**: the spec doesn't dictate *how* the sample is produced.
  If the file is missing at implementation time, the bank's existing
  missing-sample tolerance kicks in (silent P/G/G) and the feature still
  functions for Poor/Miss. The PR landing this change should either ship
  `tick.ogg` or document how to drop one in.
- **Ring visibility on bright shader backgrounds**: cyan 2px stroke may wash
  out. Mitigation: the inner low-alpha disc gives the ring a backing plate;
  if that's still insufficient we add a 1px black outer stroke.
- **Sub-frame tween with non-LINEAR texture filtering**: markers are already
  loaded with `TextureOptions::LINEAR` (`marker.rs`), so frame-to-frame alpha
  crossfade is bit-safe. No texture options change needed.
