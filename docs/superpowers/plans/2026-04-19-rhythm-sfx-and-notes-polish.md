# Rhythm SFX Rework + Note Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make rhythm SFX quieter and non-layered (hybrid Clone Hero / Stepmania), smooth out marker note animation with sub-frame tweening, and add a stationary cyan "hit moment" ring inside each cell that hosts an approaching note.

**Architecture:** `SfxBank` gains master volume, voice-cap (VecDeque of live sinks with drop-oldest), per-grade cooldown dedup, and a collapsed sample table (Perfect/Great/Good → shared `tick.ogg`; Poor/Miss stay distinct). Marker rendering in `rhythm/render.rs` switches to a two-image alpha-blended tween between adjacent sprite frames, using new `*_frame_tweened` pickers on `marker::Markers`. A new `draw_hit_rings` pass paints a cyan receptor ring under each approaching-note cell, inserted between `draw_notes` and `draw_notes_markers` in the per-frame pipeline.

**Tech Stack:** Rust, rodio (audio), egui (painter), wgpu tile shaders, clap CLI, serde JSON for marker manifests.

**Spec:** `docs/superpowers/specs/2026-04-19-rhythm-sfx-and-notes-polish-design.md`

---

## File Structure

**Modified:**
- `crates/juballer-deck/src/rhythm/sfx.rs` — new fields (`master_volume`, `active`, `last_played`), `play` rewrite, collapsed `sample_filename`, volume helpers.
- `crates/juballer-deck/src/rhythm/marker.rs` — new `approach_frame_tweened` / `grade_frame_tweened` methods.
- `crates/juballer-deck/src/rhythm/render.rs` — switch `draw_notes_markers` to tweened pickers; add `draw_hit_rings` fn.
- `crates/juballer-deck/src/rhythm/mod.rs` — wire SFX volume into `play_chart_inner`, insert `draw_hit_rings` call in the per-frame pipeline (between `draw_notes` and `draw_notes_markers`).
- `crates/juballer-deck/src/cli.rs` — new `--sfx-volume` flag on `Play`, thread into calls.
- `crates/juballer-deck/src/rhythm/picker.rs` — forward new volume arg through the picker → re-exec path (mirrors `mute_sfx`).
- `crates/juballer-deck/src/rhythm/tutorial.rs` / `calibrate.rs` — already pass `mute_sfx=true`; no behavior change but signature updates may cascade.

**Created:**
- `assets/sample/sfx/tick.ogg` — shared quiet click for Perfect/Great/Good.

Kept on disk but no longer referenced by bank: `perfect.ogg`, `great.ogg`, `good.ogg`.

---

## Task 1: Collapsed sample filename table

Goal: `sample_filename(Perfect|Great|Good) == "tick.ogg"` while Poor/Miss remain distinct. Test first so the rest of the bank logic can depend on this mapping.

**Files:**
- Modify: `crates/juballer-deck/src/rhythm/sfx.rs` (constant `GRADES`, fn `sample_filename`, existing test `sample_filenames_are_distinct`).

- [ ] **Step 1: Update the filename-distinctness test to match new contract**

Replace the existing `sample_filenames_are_distinct` test — we now *expect* three grades to share a file.

```rust
// in sfx.rs tests module
#[test]
fn sample_filenames_collapse_hit_grades_but_keep_miss_poor_distinct() {
    // Perfect / Great / Good all map to the shared tick.ogg.
    let hit = sample_filename(Grade::Perfect);
    assert_eq!(hit, "tick.ogg");
    assert_eq!(sample_filename(Grade::Great), hit);
    assert_eq!(sample_filename(Grade::Good), hit);
    // Poor and Miss keep their own distinct samples.
    assert_eq!(sample_filename(Grade::Poor), "poor.ogg");
    assert_eq!(sample_filename(Grade::Miss), "miss.ogg");
    assert_ne!(sample_filename(Grade::Poor), sample_filename(Grade::Miss));
    assert_ne!(sample_filename(Grade::Poor), hit);
    assert_ne!(sample_filename(Grade::Miss), hit);
}
```

Delete the old `sample_filenames_are_distinct` test — the new one replaces its contract.

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p juballer-deck --lib rhythm::sfx -- sample_filenames_collapse
```

Expected: FAIL — `sample_filename(Grade::Perfect)` currently returns `"perfect.ogg"`, not `"tick.ogg"`.

- [ ] **Step 3: Rewrite `sample_filename` to collapse hit grades**

```rust
pub const fn sample_filename(grade: Grade) -> &'static str {
    match grade {
        // Perfect / Great / Good share a single quiet click (Clone-Hero-
        // style: hit feedback is minimal; the song carries the rhythm).
        Grade::Perfect | Grade::Great | Grade::Good => "tick.ogg",
        Grade::Poor => "poor.ogg",
        Grade::Miss => "miss.ogg",
    }
}
```

- [ ] **Step 4: Run the full sfx test module**

```bash
cargo test -p juballer-deck --lib rhythm::sfx
```

Expected: PASS on the new test. The existing `load_from_dev_dir_reads_samples_when_present` may now fail if `tick.ogg` doesn't yet exist on disk — acceptable for this task, will be resolved in Task 2. If it fails with a "no loaded sample" message, note it and continue; the next task fixes it.

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/src/rhythm/sfx.rs
git commit -m "feat(rhythm/sfx): collapse Perfect/Great/Good to shared tick.ogg"
```

---

## Task 2: Add `tick.ogg` asset

Goal: ship a `tick.ogg` so the bank has something to play for Perfect/Great/Good. The spec lets us start with a placeholder; we use the existing `good.ogg` (softest current sample) as the initial tick, so the bank is functionally complete immediately. A real tick can replace it later without code changes.

**Files:**
- Create: `assets/sample/sfx/tick.ogg`

- [ ] **Step 1: Copy `good.ogg` → `tick.ogg` as the placeholder tick sample**

```bash
cp assets/sample/sfx/good.ogg assets/sample/sfx/tick.ogg
file assets/sample/sfx/tick.ogg
```

Expected `file` output: `Ogg data, Vorbis audio, mono, 44100 Hz, …`.

- [ ] **Step 2: Run the sfx test module — dev-dir loader should now find `tick.ogg`**

```bash
cargo test -p juballer-deck --lib rhythm::sfx
```

Expected: PASS, including `load_from_dev_dir_reads_samples_when_present` (it only needs *any* sample loaded; tick.ogg counts).

- [ ] **Step 3: Commit**

```bash
git add assets/sample/sfx/tick.ogg
git commit -m "feat(rhythm/sfx): add placeholder tick.ogg (copy of good.ogg)"
```

---

## Task 3: `SfxBank` master volume

Goal: bank can be constructed with a master volume and applies it per play via `sink.set_volume`. Per-grade factor table is applied on top.

**Files:**
- Modify: `crates/juballer-deck/src/rhythm/sfx.rs`

- [ ] **Step 1: Add the volume test**

```rust
// in sfx.rs tests module
#[test]
fn default_master_volume_is_moderate_and_clamped() {
    let bank = SfxBank::new_empty();
    // Default comes out at ~0.35 per spec. The exact value matters — it is
    // the knob the player hears as "default loudness".
    assert!((bank.master_volume() - 0.35).abs() < 1e-6);

    let mut bank = SfxBank::new_empty();
    bank.set_master_volume(2.0);
    assert_eq!(bank.master_volume(), 1.0, "volume must clamp to [0, 1]");
    bank.set_master_volume(-0.5);
    assert_eq!(bank.master_volume(), 0.0, "volume must clamp to [0, 1]");
}

#[test]
fn per_grade_volume_factor_table() {
    // Tick grades share the soft factor; Poor is nominal; Miss pops louder.
    assert!((grade_volume_factor(Grade::Perfect) - 0.6).abs() < 1e-6);
    assert!((grade_volume_factor(Grade::Great) - 0.6).abs() < 1e-6);
    assert!((grade_volume_factor(Grade::Good) - 0.6).abs() < 1e-6);
    assert!((grade_volume_factor(Grade::Poor) - 1.0).abs() < 1e-6);
    assert!((grade_volume_factor(Grade::Miss) - 1.1).abs() < 1e-6);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p juballer-deck --lib rhythm::sfx -- default_master_volume_is_moderate_and_clamped per_grade_volume_factor_table
```

Expected: FAIL — `master_volume`, `set_master_volume`, and `grade_volume_factor` don't exist yet.

- [ ] **Step 3: Add the volume fields + API**

Add a `master_volume: f32` field to `SfxBank` (initialize to `0.35` in every constructor — `new_empty`, the `Ok` branch of `load_from_dir`, and the `Err` branch). Add `pub fn master_volume(&self) -> f32` and `pub fn set_master_volume(&mut self, v: f32)` that clamps to `[0.0, 1.0]`. Add the new fn below:

```rust
/// Per-grade loudness factor applied on top of the bank's master volume.
/// Matches the hybrid Clone Hero / Stepmania design: shared soft tick for
/// P/G/G, nominal Poor, slightly boosted Miss so it pops above the song.
pub(crate) fn grade_volume_factor(grade: Grade) -> f32 {
    match grade {
        Grade::Perfect | Grade::Great | Grade::Good => 0.6,
        Grade::Poor => 1.0,
        Grade::Miss => 1.1,
    }
}
```

In `SfxBank::play`, right after `Sink::try_new(handle)` succeeds and before `sink.append(...)`, set volume:

```rust
let vol = self.master_volume * grade_volume_factor(grade);
sink.set_volume(vol);
```

Leave the rest of `play` untouched for now (voice-cap + cooldown come in Tasks 4 and 5).

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p juballer-deck --lib rhythm::sfx
```

Expected: PASS on all sfx tests.

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/src/rhythm/sfx.rs
git commit -m "feat(rhythm/sfx): master volume + per-grade factor table"
```

---

## Task 4: Voice cap — at most 3 simultaneous sinks

Goal: rapid-fire plays don't accumulate unbounded sinks. Oldest in-flight sink is dropped (which stops it in rodio) to make room for a new one.

**Files:**
- Modify: `crates/juballer-deck/src/rhythm/sfx.rs`

- [ ] **Step 1: Add the voice-cap test**

```rust
#[test]
fn voice_cap_keeps_at_most_three_active_sinks() {
    // Exercise with an empty bank: even though play() is a no-op on missing
    // samples, we can drive the queue bookkeeping by calling the internal
    // push_sink helper directly. That keeps the test deterministic across
    // environments without audio devices.
    let mut bank = SfxBank::new_empty();
    for _ in 0..10 {
        bank.push_dummy_sink_for_test();
    }
    assert!(bank.active_sink_count() <= 3, "voice cap must clamp to 3");
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p juballer-deck --lib rhythm::sfx -- voice_cap_keeps_at_most_three
```

Expected: FAIL — `push_dummy_sink_for_test` and `active_sink_count` don't exist.

- [ ] **Step 3: Add voice-queue bookkeeping**

Add to `SfxBank`:

```rust
use std::collections::VecDeque;

// … inside struct SfxBank:
active: VecDeque<Sink>,
```

Initialize `active: VecDeque::with_capacity(4)` in all three constructors.

Add the constant and helpers:

```rust
const MAX_ACTIVE_SINKS: usize = 3;

impl SfxBank {
    #[cfg(test)]
    pub fn active_sink_count(&self) -> usize {
        self.active.len()
    }

    /// Push a new `Sink` into the active queue, dropping the oldest if the
    /// queue is already at capacity. Dropping a rodio `Sink` stops its
    /// currently-playing source, which is exactly the voice-steal we want.
    fn enqueue_sink(&mut self, sink: Sink) {
        while self.active.len() >= MAX_ACTIVE_SINKS {
            self.active.pop_front();
        }
        self.active.push_back(sink);
    }

    #[cfg(test)]
    fn push_dummy_sink_for_test(&mut self) {
        // Sink::new_idle doesn't need an output handle — perfect for
        // headless tests exercising queue bookkeeping only.
        let (sink, _queue) = Sink::new_idle();
        self.enqueue_sink(sink);
    }
}
```

Update `play` to use `self.enqueue_sink(sink)` instead of `sink.detach()`. Also drop finished sinks before enqueuing so the cap reflects *live* voices:

```rust
// Near the top of play(), after muted/handle guards:
self.active.retain(|s| !s.empty());
```

Note `self` must be `&mut self` now — update the `play` signature accordingly. Every call site already calls through `&mut SfxBank` (`play_chart_inner` constructs the bank locally), so this should compile cleanly. If not, the compiler will point at the callers in `picker.rs` / `mod.rs` — change the local binding to `let mut sfx = …;`.

Since `play` mutated `self`, `grade_color` callers won't be affected, but `#[cfg(test)]` tests calling `bank.play(...)` on an immutable bank need `let mut bank = …`.

- [ ] **Step 4: Update the existing `new_empty_constructs_and_play_is_noop` test to use `mut`**

```rust
#[test]
fn new_empty_constructs_and_play_is_noop() {
    let mut bank = SfxBank::new_empty();
    assert!(!bank.is_ready());
    for g in [Grade::Perfect, Grade::Great, Grade::Good, Grade::Poor, Grade::Miss] {
        bank.play(g);
    }
}
```

Update `muted_bank_plays_nothing` and `load_from_missing_dir_is_silent_no_panic` the same way (add `mut` and call `bank.play(...)` as `&mut`).

- [ ] **Step 5: Run all sfx tests to verify**

```bash
cargo test -p juballer-deck --lib rhythm::sfx
```

Expected: PASS on everything including the new `voice_cap_keeps_at_most_three_active_sinks`.

- [ ] **Step 6: Commit**

```bash
git add crates/juballer-deck/src/rhythm/sfx.rs
git commit -m "feat(rhythm/sfx): voice cap — max 3 concurrent sinks, drop oldest"
```

---

## Task 5: Per-grade cooldown dedup

Goal: a 4-note chord judged in the same frame fires *one* sink per distinct grade, not four stacked ticks. 15ms window per grade.

**Files:**
- Modify: `crates/juballer-deck/src/rhythm/sfx.rs`

- [ ] **Step 1: Add the cooldown test**

```rust
#[test]
fn cooldown_suppresses_rapid_same_grade_plays() {
    let mut bank = SfxBank::new_empty();
    // No audio device + no samples → real play() no-ops. But the cooldown
    // check runs *before* the handle/sample guards, so we can still
    // exercise it by observing the timestamps map directly.
    assert!(bank.cooldown_allows(Grade::Perfect));
    bank.mark_cooldown_now(Grade::Perfect);
    // Immediately afterward, within the 15ms window → suppressed.
    assert!(!bank.cooldown_allows(Grade::Perfect));
    // Different grade is unaffected.
    assert!(bank.cooldown_allows(Grade::Miss));
}

#[test]
fn cooldown_clears_after_window() {
    use std::thread::sleep;
    use std::time::Duration;
    let mut bank = SfxBank::new_empty();
    bank.mark_cooldown_now(Grade::Great);
    assert!(!bank.cooldown_allows(Grade::Great));
    // 20ms > 15ms window → allowed again.
    sleep(Duration::from_millis(20));
    assert!(bank.cooldown_allows(Grade::Great));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p juballer-deck --lib rhythm::sfx -- cooldown_suppresses_rapid cooldown_clears_after_window
```

Expected: FAIL — `cooldown_allows` / `mark_cooldown_now` don't exist.

- [ ] **Step 3: Add cooldown bookkeeping**

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

const COOLDOWN: Duration = Duration::from_millis(15);

// In SfxBank:
last_played: HashMap<Grade, Instant>,

// Initialize in every constructor: last_played: HashMap::new(),

impl SfxBank {
    fn cooldown_allows(&self, grade: Grade) -> bool {
        match self.last_played.get(&grade) {
            None => true,
            Some(t) => t.elapsed() >= COOLDOWN,
        }
    }

    fn mark_cooldown_now(&mut self, grade: Grade) {
        self.last_played.insert(grade, Instant::now());
    }
}
```

Wire into `play`: at the very top (after `if self.muted { return; }`) add:

```rust
if !self.cooldown_allows(grade) {
    return;
}
```

At the end of `play` — right after `self.enqueue_sink(sink)` — call `self.mark_cooldown_now(grade)`.

`Grade` needs `Hash + Eq` for `HashMap`. Check `rhythm/judge.rs`: if it doesn't derive those yet, add them. The enum is small and `Copy`, so adding `#[derive(Hash, Eq)]` next to existing derives is safe.

- [ ] **Step 4: Verify `Grade` has required traits**

```bash
grep -n "derive.*Grade\|enum Grade" crates/juballer-deck/src/rhythm/judge.rs
```

If the existing derive line lacks `Hash` or `Eq`, edit `judge.rs` to add them. Keep existing derives intact (likely `Debug, Clone, Copy, PartialEq`).

- [ ] **Step 5: Run all sfx tests**

```bash
cargo test -p juballer-deck --lib rhythm::sfx
```

Expected: PASS on every test including both new cooldown tests.

- [ ] **Step 6: Commit**

```bash
git add crates/juballer-deck/src/rhythm/sfx.rs crates/juballer-deck/src/rhythm/judge.rs
git commit -m "feat(rhythm/sfx): 15ms per-grade cooldown kills chord pileup"
```

---

## Task 6: `--sfx-volume` CLI flag

Goal: expose master volume through `juballer-deck play --sfx-volume 0.5`. Thread it through the play / picker path so it reaches `SfxBank::set_master_volume` before the first note.

**Files:**
- Modify: `crates/juballer-deck/src/cli.rs`
- Modify: `crates/juballer-deck/src/rhythm/mod.rs`
- Modify: `crates/juballer-deck/src/rhythm/picker.rs`
- Modify: `crates/juballer-deck/src/rhythm/tutorial.rs`
- Modify: `crates/juballer-deck/src/rhythm/calibrate.rs`

- [ ] **Step 1: Add a CLI-parser test for the new flag**

```rust
// crates/juballer-deck/src/cli.rs tests module (near parses_play_mute_sfx_flag)
#[test]
fn parses_play_sfx_volume_flag() {
    let cli = Cli::try_parse_from(["juballer-deck", "play", "/tmp/x.memon"]).unwrap();
    match cli.cmd {
        Some(SubCmd::Play { sfx_volume, .. }) => {
            // Default: None → bank's own default (0.35) applies.
            assert!(sfx_volume.is_none(), "sfx_volume defaults to None");
        }
        _ => panic!("expected Play subcommand"),
    }
    let cli = Cli::try_parse_from([
        "juballer-deck", "play", "/tmp/x.memon", "--sfx-volume", "0.5",
    ])
    .unwrap();
    match cli.cmd {
        Some(SubCmd::Play { sfx_volume, .. }) => {
            assert_eq!(sfx_volume, Some(0.5));
        }
        _ => panic!("expected Play subcommand"),
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p juballer-deck --lib cli:: -- parses_play_sfx_volume_flag
```

Expected: FAIL — the `Play` variant has no `sfx_volume` field.

- [ ] **Step 3: Add the CLI field**

Edit `cli.rs` around line 60–74 to extend the `Play` variant:

```rust
Play {
    #[arg(value_name = "CHART")]
    chart: Option<PathBuf>,
    #[arg(long, default_value = "BSC")]
    difficulty: String,
    #[arg(long, default_value_t = 0)]
    audio_offset_ms: i32,
    #[arg(long, default_value_t = false)]
    mute_sfx: bool,
    /// Master volume for hit SFX in 0.0..=1.0. Overrides the bank
    /// default (~0.35). Independent of `--mute-sfx`.
    #[arg(long)]
    sfx_volume: Option<f32>,
},
```

Update the `Some(SubCmd::Play { … })` destructure (currently at line 136-141) to bind `sfx_volume`, and pass it into the two call sites (picker dispatch and `play_with_opts`). Signature will change in the next step.

- [ ] **Step 4: Thread `sfx_volume: Option<f32>` through the rhythm entry points**

In `rhythm/mod.rs`, update the signatures:

- `play_with_opts(chart, difficulty, audio_offset_ms, mute_sfx, opts)` → add `sfx_volume: Option<f32>` between `mute_sfx` and `opts`.
- `play` → same.
- `play_chart_opts(chart, user_offset_ms, mute_sfx, countdown_ms)` → add `sfx_volume` after `mute_sfx`.
- `play_chart(chart, user_offset_ms, mute_sfx)` → add `sfx_volume` after `mute_sfx`.
- `play_chart_with_hook(…)` → same.
- `play_chart_inner(…)` (line ~296) → same.

Inside `play_chart_inner`, where `let mut sfx = SfxBank::load_default();` lives (line ~340):

```rust
let mut sfx = SfxBank::load_default();
if let Some(v) = sfx_volume {
    sfx.set_master_volume(v);
}
sfx.set_muted(mute_sfx);
```

Update callers:
- `tutorial.rs :: run_tutorial` — pass `None` for `sfx_volume` (tutorial also passes `mute_sfx=true`, so volume is irrelevant; `None` keeps the bank at its default).
- `calibrate.rs` — pass `None`.
- `picker.rs` — the picker takes `mute_sfx: bool`; extend its signature with `sfx_volume: Option<f32>` and forward it to the re-exec / child `play_with_opts` path.

In `cli.rs`, change the two Play-dispatch calls to pass the new arg:

```rust
// Directory → picker
return crate::rhythm::pick(
    &chart,
    &difficulty,
    audio_offset_ms,
    mute_sfx,
    sfx_volume,
    opts.backgrounds.clone(),
    opts.background_index,
);

// Single chart file
return crate::rhythm::play_with_opts(
    &chart,
    &difficulty,
    audio_offset_ms,
    mute_sfx,
    sfx_volume,
    opts,
);
```

- [ ] **Step 5: Fix picker's re-exec CLI args**

In `picker.rs`, the picker spawns `juballer-deck play <chart>` as a child process (search for the `Command::new` around lines 1175 / 1309 referenced above). Append `--sfx-volume <v>` when `sfx_volume` is `Some`:

```rust
let mut cmd = /* existing Command::new(...) + args */;
if exec_mute_sfx {
    cmd.arg("--mute-sfx");
}
if let Some(v) = exec_sfx_volume {
    cmd.arg("--sfx-volume").arg(format!("{v}"));
}
```

Add `let exec_sfx_volume = sfx_volume;` near the existing `let exec_mute_sfx = mute_sfx;` binding.

- [ ] **Step 6: Run both the cli test and the full crate tests**

```bash
cargo test -p juballer-deck --lib cli::
cargo test -p juballer-deck --lib rhythm::sfx
cargo build -p juballer-deck
```

Expected: `parses_play_sfx_volume_flag` passes, all other tests still pass, crate builds clean.

- [ ] **Step 7: Commit**

```bash
git add crates/juballer-deck/src/cli.rs \
        crates/juballer-deck/src/rhythm/mod.rs \
        crates/juballer-deck/src/rhythm/picker.rs \
        crates/juballer-deck/src/rhythm/tutorial.rs \
        crates/juballer-deck/src/rhythm/calibrate.rs
git commit -m "feat(rhythm/cli): --sfx-volume flag threaded through play path"
```

---

## Task 7: Tweened marker frame pickers

Goal: `Markers` exposes tween-friendly frame pickers that return *both* current and next frame UVs plus a fractional `t ∈ [0,1]`. Marker-agnostic; keeps existing `approach_frame` / `grade_frame` for test + backward use.

**Files:**
- Modify: `crates/juballer-deck/src/rhythm/marker.rs`

- [ ] **Step 1: Add tests for the new tween pickers**

Add a small test module at the bottom of `marker.rs` (after any existing tests — if the file has none, gate the module on `#[cfg(test)]`). The math-only logic doesn't need a real texture, so we factor a pure helper:

```rust
/// Pure helper: given `offset_ms`, `fps`, and `count`, return
/// (frame_a, frame_b, t) matching the tween contract:
///  - frame_a in [0, count)
///  - frame_b = min(frame_a + 1, count - 1)
///  - t in [0, 1]; at the last frame t clamps to 0 so the anim holds
fn tween_frame_at(offset_s: f64, fps: f64, count: usize) -> Option<(usize, usize, f32)> {
    if count == 0 { return None; }
    let f = offset_s * fps;
    if f < 0.0 { return None; }
    let frame_a = f.floor() as usize;
    if frame_a >= count { return None; }
    let frame_b = (frame_a + 1).min(count - 1);
    let raw_t = (f - frame_a as f64) as f32;
    let t = if frame_b == frame_a { 0.0 } else { raw_t.clamp(0.0, 1.0) };
    Some((frame_a, frame_b, t))
}

#[cfg(test)]
mod tween_tests {
    use super::tween_frame_at;

    #[test]
    fn frame_boundary_yields_zero_t() {
        // Exactly on a boundary inside the sheet: offset_s * fps == 2 → t ≈ 0.
        let (a, b, t) = tween_frame_at(2.0 / 30.0, 30.0, 16).unwrap();
        assert_eq!(a, 2);
        assert_eq!(b, 3);
        assert!(t.abs() < 1e-5);
    }

    #[test]
    fn midway_yields_half_t() {
        let (a, b, t) = tween_frame_at(2.5 / 30.0, 30.0, 16).unwrap();
        assert_eq!(a, 2);
        assert_eq!(b, 3);
        assert!((t - 0.5).abs() < 1e-5);
    }

    #[test]
    fn last_frame_clamps_b_and_zero_t() {
        // offset past the last real frame but still inside count.
        let (a, b, t) = tween_frame_at((15.0 + 0.7) / 30.0, 30.0, 16).unwrap();
        assert_eq!(a, 15);
        assert_eq!(b, 15, "frame_b must clamp to count - 1 at the tail");
        assert!(t.abs() < 1e-5, "t must be 0 at the tail to hold the last frame");
    }

    #[test]
    fn past_end_returns_none() {
        assert!(tween_frame_at(1.0, 30.0, 16).is_none()); // 30 frames > count 16
    }

    #[test]
    fn negative_offset_returns_none() {
        assert!(tween_frame_at(-0.1, 30.0, 16).is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p juballer-deck --lib rhythm::marker
```

Expected: FAIL — `tween_frame_at` doesn't exist.

- [ ] **Step 3: Add the helper + tween frame pickers on `Markers`**

Above `impl Markers`, add the free `tween_frame_at` helper exactly as shown in Step 1. Then add the two methods:

```rust
impl Markers {
    /// Tweened approach picker. `offset_ms` is `music_time - hit_time` and
    /// should be negative during lead-in. Returns `(texture, uv_a, uv_b, t)`
    /// for a crossfade between two adjacent sprite frames. `None` once the
    /// approach is past the hit moment (offset_ms >= 0).
    pub fn approach_frame_tweened(
        &self,
        offset_ms: f64,
    ) -> Option<(&TextureHandle, Rect, Rect, f32)> {
        // Approach uses a "count minus index from the end" convention (see
        // approach_frame below): raw goes negative → frame index = raw + count.
        let fps = self.approach.fps;
        let count = self.approach.count;
        let raw = offset_ms / 1000.0 * fps;
        if raw >= 0.0 {
            return None;
        }
        let f = raw + count as f64;
        if f < 0.0 || f >= count as f64 {
            return None;
        }
        let frame_a = f.floor() as usize;
        let frame_b = (frame_a + 1).min(count - 1);
        let t = if frame_b == frame_a { 0.0 } else { (f - frame_a as f64) as f32 };
        Some((
            &self.approach.texture,
            self.approach.frame_uv(frame_a),
            self.approach.frame_uv(frame_b),
            t.clamp(0.0, 1.0),
        ))
    }

    /// Tweened grade-burst picker. `offset_ms` is `music_time - judged_at_ms`
    /// (>= 0). Returns None once the animation ends.
    pub fn grade_frame_tweened(
        &self,
        phase: GradePhase,
        offset_ms: f64,
    ) -> Option<(&TextureHandle, Rect, Rect, f32)> {
        let anim = match phase {
            GradePhase::Perfect => &self.perfect,
            GradePhase::Great => &self.great,
            GradePhase::Good => &self.good,
            GradePhase::Poor => &self.poor,
            GradePhase::Miss => &self.miss,
        };
        let offset_s = offset_ms / 1000.0;
        let (a, b, t) = tween_frame_at(offset_s, anim.fps, anim.count)?;
        Some((&anim.texture, anim.frame_uv(a), anim.frame_uv(b), t))
    }
}
```

Leave `approach_frame` and `grade_frame` intact for tests and callers that don't need tweening.

- [ ] **Step 4: Run the marker tests**

```bash
cargo test -p juballer-deck --lib rhythm::marker
```

Expected: PASS on `tween_frame_at` tests.

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/src/rhythm/marker.rs
git commit -m "feat(rhythm/marker): tweened frame pickers for sub-frame blend"
```

---

## Task 8: Switch `draw_notes_markers` to tweened rendering

Goal: marker pass paints two alpha-weighted sprite images per cell instead of one, smoothing the animation without touching the sprite sheets.

**Files:**
- Modify: `crates/juballer-deck/src/rhythm/render.rs`

- [ ] **Step 1: Replace the `pick` block in `draw_notes_markers`**

Open `render.rs` and locate the `let pick = match slot.hit { … };` block (around line 118). Replace it with the tweened pickers:

```rust
let pick = match slot.hit {
    Some(h) => {
        let phase = super::marker::grade_to_phase(h.grade);
        let since = music - h.judged_at_ms;
        m.grade_frame_tweened(phase, since)
    }
    None => {
        let offset = music - slot.note.hit_time_ms;
        m.approach_frame_tweened(offset)
    }
};
if let Some((tex, uv_a, uv_b, t)) = pick {
    // Crossfade between adjacent sprite frames. Alphas are multiplicative
    // on the tint's alpha channel; linear texture sampling on the marker
    // atlases means the blend is bit-safe.
    let alpha_a = ((1.0 - t) * 255.0).round() as u8;
    let alpha_b = (t * 255.0).round() as u8;
    if alpha_a > 0 {
        painter.image(
            tex.id(),
            tile,
            uv_a,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha_a),
        );
    }
    if alpha_b > 0 {
        painter.image(
            tex.id(),
            tile,
            uv_b,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha_b),
        );
    }
}
```

- [ ] **Step 2: Build the crate**

```bash
cargo build -p juballer-deck
```

Expected: clean build. Compiler errors here usually mean the tween picker return tuple doesn't match the destructure — re-check Task 7 Step 3.

- [ ] **Step 3: Run crate tests to make sure nothing regressed**

```bash
cargo test -p juballer-deck --lib
```

Expected: PASS. (There are no direct unit tests for `draw_notes_markers` — it requires an egui+wgpu context — but the tween pickers are covered by Task 7.)

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/rhythm/render.rs
git commit -m "feat(rhythm/render): sub-frame tween between marker sprite frames"
```

---

## Task 9: Hit-moment ring pass

Goal: a stationary cyan ring per cell hosting an approaching note, drawn under the sprite so the sprite grows past it at the hit moment.

**Files:**
- Modify: `crates/juballer-deck/src/rhythm/render.rs`
- Modify: `crates/juballer-deck/src/rhythm/mod.rs`

- [ ] **Step 1: Add `draw_hit_rings` to `render.rs`**

Place the new function directly above `draw_notes_markers` (so the two live next to each other):

```rust
/// Paint a stationary cyan "hit moment" ring inside every cell that currently
/// hosts an approaching (not-yet-judged) note. Runs *before*
/// `draw_notes_markers` so the approach sprite composes on top of the ring,
/// visually growing to meet it at the hit moment.
///
/// The ring uses a dedicated egui Area + id so its draw buffer never
/// collides with the markers' Area (see the note in `draw_notes_markers`).
pub fn draw_hit_rings(
    frame: &mut Frame,
    overlay: &mut EguiOverlay,
    state: &GameState,
) {
    let slots = render_slots(state);
    let cell_rects = *frame.cell_rects();
    let viewport_w = frame.viewport_w() as f32;
    let viewport_h = frame.viewport_h() as f32;
    let music = state.music_time_ms;
    let lead_ms = state.lead_in_ms;

    overlay.draw(frame, |rc| {
        egui::Area::new(egui::Id::new("rhythm_hit_rings_root"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(viewport_w);
                ui.set_height(viewport_h);
                let painter = ui.painter();
                for (idx, slot) in slots.iter().enumerate() {
                    let Some(slot) = slot else { continue };
                    // Only draw the target while the note is still pending.
                    if slot.hit.is_some() {
                        continue;
                    }
                    // Skip long notes — their hold visualization has its own
                    // shader path (`draw_notes`) and shouldn't get a target
                    // ring on top.
                    if slot.note.is_long() {
                        continue;
                    }
                    let rect_core = cell_rects[idx];
                    let cx = rect_core.x as f32 + rect_core.w as f32 * 0.5;
                    let cy = rect_core.y as f32 + rect_core.h as f32 * 0.5;
                    let radius = rect_core.w.min(rect_core.h) as f32 * 0.5 * 0.35;
                    let approach =
                        approach_factor(music, slot.note.hit_time_ms, lead_ms);
                    let alpha = (approach * 0.9 * 255.0).clamp(0.0, 255.0) as u8;
                    if alpha == 0 {
                        continue;
                    }
                    let center = egui::pos2(cx, cy);
                    // Inner low-alpha disc gives the ring a backing plate so
                    // it reads as a target over bright shader backgrounds.
                    painter.circle_filled(
                        center,
                        radius,
                        egui::Color32::from_rgba_unmultiplied(20, 40, 60, 20),
                    );
                    painter.circle_stroke(
                        center,
                        radius,
                        egui::Stroke::new(
                            2.0,
                            egui::Color32::from_rgba_unmultiplied(94, 232, 255, alpha),
                        ),
                    );
                }
            });
    });
}
```

- [ ] **Step 2: Wire the new pass into the per-frame pipeline in `mod.rs`**

Edit `crates/juballer-deck/src/rhythm/mod.rs` around line 667. Insert the `draw_hit_rings` call between `draw_notes` and `draw_notes_markers`. The two passes must share their own dedicated overlays to avoid draw-buffer collision, so pull a new `hit_ring_overlay` from the same source that constructs `marker_overlay` (search backward for `let mut marker_overlay = `; declare `hit_ring_overlay` the same way, right after it).

```rust
// Existing:
//   let mut marker_overlay = EguiOverlay::new(&app, ...);
// Add directly below:
let mut hit_ring_overlay = EguiOverlay::new(&app, /* same ctor args as marker_overlay */);
```

Then in the per-frame pipeline:

```rust
render::draw_notes(
    frame,
    &state,
    &mut shader_cache,
    &shader_path,
    boot_secs,
    dt,
);
// 2a. Hit-moment rings — stationary cyan targets drawn under the sprites.
render::draw_hit_rings(frame, &mut hit_ring_overlay, &state);
// 2b. Tap-note markers — PNG sprite path via its own dedicated
// EguiOverlay (shares none of the HUD overlay's renderer state).
render::draw_notes_markers(frame, &mut marker_overlay, &mut markers, &marker_dir, &state);
```

- [ ] **Step 3: Build the crate**

```bash
cargo build -p juballer-deck
```

Expected: clean build. If `EguiOverlay::new` takes different constructor args than what's used for `marker_overlay`, copy the exact invocation from that line.

- [ ] **Step 4: Run the full crate test suite**

```bash
cargo test -p juballer-deck --lib
```

Expected: PASS. The new render fn has no direct unit test (needs wgpu+egui context), but must not regress anything.

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/src/rhythm/render.rs crates/juballer-deck/src/rhythm/mod.rs
git commit -m "feat(rhythm/render): cyan hit-moment ring under approaching notes"
```

---

## Task 10: Workspace-wide sanity check

Goal: make sure the deck binary compiles, the shader-smoke integration test still passes, and no warnings introduced.

**Files:** (no changes — verification only)

- [ ] **Step 1: Full workspace build**

```bash
cargo build --workspace
```

Expected: clean build, no warnings added by this branch.

- [ ] **Step 2: Full workspace tests**

```bash
cargo test --workspace
```

Expected: PASS across `juballer-core`, `juballer-deck`, and siblings. If `shader_smoke` or any other integration test fails and the failure is unrelated to this branch, note it but do not block the plan on pre-existing breakage.

- [ ] **Step 3: Manual smoke (optional but strongly recommended)**

```bash
cargo run -p juballer-deck -- play --sfx-volume 0.4 path/to/chart-dir/
```

Listen for:
- Perfect/Great/Good now share a single soft tick (not three distinct samples).
- A 4-note chord produces at most one tick per distinct grade (cooldown working).
- Miss is still distinctly loud.

Watch for:
- Cyan hit ring appearing in each cell with an incoming note, fading in with approach, vanishing on judgment.
- Marker sprite animation visibly smoother — no frame stepping on the approach pulse or burst.

If anything reads wrong, revert the offending task's commit and revisit the spec.

- [ ] **Step 4: Final commit (only if any follow-up tweaks needed — otherwise skip)**
