//! Rhythm-game mode.
//!
//! Loads a [memon v1.0.0](https://memon-spec.readthedocs.io) chart, plays the audio
//! in-process via rodio, shows approaching notes in the 4×4 cell shaders, and
//! judges player input. Runs alongside the regular deck app but bypasses the
//! DeckApp widget/action pipeline — see [`play`].

pub mod audio;
pub mod background;
pub mod calibrate;
pub mod chart;
pub mod chart_overrides;
pub mod exit;
pub mod favorites;
pub mod judge;
pub mod marker;
pub mod mods_ui;
pub mod notes;
pub mod pagination;
pub mod picker;
pub mod picker_view;
pub mod render;
pub mod scores;
pub mod settings_ui;
pub mod sfx;
pub mod spectrum;
pub mod state;
pub mod textfx;
pub mod tutorial;

pub use picker::pick;
pub use settings_ui::run as settings;

pub use audio::Audio;
pub use calibrate::run as calibrate_audio;
pub use chart::{load as load_chart, Chart, Note};
pub use judge::{judge as judge_delta, Grade};
pub use notes::ScheduledNote;
pub use scores::{ScoreBook, ScoreRecord};
pub use sfx::SfxBank;
pub use state::GameState;
pub use tutorial::run_tutorial;

use crate::shader::ShaderPipelineCache;
use crate::Result;
use juballer_core::input::Event;
use juballer_core::{App, PresentMode};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Default pre-song countdown duration in milliseconds. Gives the player a
/// moment to settle fingers on the cells before notes start flying. Set to 0
/// to skip the countdown (see [`calibrate::run`], which wants the metronome
/// to start immediately).
pub const DEFAULT_COUNTDOWN_MS: u32 = 3_000;

/// Entry point for rhythm mode. Loads `chart_path`, starts audio, opens a
/// fullscreen window, runs the play loop until the song ends or the user exits.
///
/// `user_offset_ms` shifts the perceived music time — set positive if the audio
/// consistently lags your inputs. The mean signed input delta is logged on exit
/// so you can iterate.
///
/// `mute_sfx` silences per-grade hit sound effects (see [`sfx`]). The song
/// itself is unaffected.
pub fn play(
    chart_path: &Path,
    difficulty: &str,
    user_offset_ms: i32,
    mute_sfx: bool,
    sfx_volume: Option<f32>,
) -> Result<()> {
    play_with_opts(
        chart_path,
        difficulty,
        user_offset_ms,
        mute_sfx,
        sfx_volume,
        PlayOpts::default(),
    )
}

/// Session-scoped options that don't change mid-session.
///
/// Filled in by the CLI dispatcher from `deck.toml` (mods, lead-in window,
/// asset override dir) and applied once at `play_with_opts` entry.
#[derive(Debug, Clone, Default)]
pub struct PlayOpts {
    pub no_fail: bool,
    /// Zero = fall back to the built-in default (see [`state::RENDER_LEAD_MS`]).
    pub lead_in_ms: u32,
    pub asset_dir: Option<PathBuf>,
    /// List of background entries (wgsl shaders or images) — one is
    /// picked deterministically per chart. See [`background`] module
    /// docs for the shader uniform convention.
    pub backgrounds: Vec<PathBuf>,
    /// Fixed index into [`Self::backgrounds`] to pin every chart to the
    /// same slot. `None` = mix (per-chart hash). Out-of-range falls
    /// back to mix with a warn log.
    pub background_index: Option<usize>,
}

pub fn play_with_opts(
    chart_path: &Path,
    difficulty: &str,
    user_offset_ms: i32,
    mute_sfx: bool,
    sfx_volume: Option<f32>,
    opts: PlayOpts,
) -> Result<()> {
    let chart = load_chart(chart_path, difficulty)?;
    tracing::info!(
        target: "juballer::rhythm",
        "loaded chart '{}' by {} ({} notes, {:.0} BPM)",
        chart.title,
        chart.artist,
        chart.notes.len(),
        chart.bpm
    );
    // Per-chart override (set from results screen "APPLY OFFSET TO THIS
    // SONG") takes precedence over the global `[rhythm] audio_offset_ms`.
    // Missing book / missing entry → fall through to the CLI value.
    let resolved_offset = match chart_overrides::ChartOverrideBook::load_default() {
        Ok(book) => match book.get(chart_path) {
            Some(o) => {
                tracing::info!(
                    target: "juballer::rhythm",
                    "per-chart audio_offset_ms override: {} (was {user_offset_ms})",
                    o.audio_offset_ms
                );
                o.audio_offset_ms
            }
            None => user_offset_ms,
        },
        Err(e) => {
            tracing::warn!(
                target: "juballer::rhythm",
                "chart_overrides load failed: {e}"
            );
            user_offset_ms
        }
    };
    let persist = Some(ScorePersist::new(chart_path, difficulty));
    play_chart_inner(
        chart,
        resolved_offset,
        mute_sfx,
        sfx_volume,
        DEFAULT_COUNTDOWN_MS,
        persist,
        NoHook,
        opts,
    )
}

/// Bundle describing where to read/write high-score entries for the session.
struct ScorePersist {
    chart_path: PathBuf,
    difficulty: String,
    book_path: PathBuf,
}

impl ScorePersist {
    fn new(chart_path: &Path, difficulty: &str) -> Self {
        Self {
            chart_path: chart_path.to_path_buf(),
            difficulty: difficulty.to_string(),
            book_path: ScoreBook::default_path(),
        }
    }

    /// Best score known for this (chart, difficulty) at session start. Load
    /// errors are logged and treated as "no best yet".
    fn load_best(&self) -> Option<u64> {
        match ScoreBook::load(&self.book_path) {
            Ok(book) => book
                .best(&self.chart_path, &self.difficulty)
                .map(|r| r.score),
            Err(e) => {
                tracing::warn!(
                    target: "juballer::rhythm",
                    "score book load failed ({}): {e}",
                    self.book_path.display()
                );
                None
            }
        }
    }

    /// Append a record for the finished session. Load + save errors are
    /// logged; a failure here shouldn't block process exit.
    fn persist(&self, record: ScoreRecord) {
        let mut book = match ScoreBook::load(&self.book_path) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    target: "juballer::rhythm",
                    "score book load failed before save ({}): {e} — starting fresh",
                    self.book_path.display()
                );
                ScoreBook::new()
            }
        };
        book.record(&self.chart_path, &self.difficulty, record);
        if let Err(e) = book.save(&self.book_path) {
            tracing::warn!(
                target: "juballer::rhythm",
                "score book save failed ({}): {e}",
                self.book_path.display()
            );
        } else {
            tracing::info!(
                target: "juballer::rhythm",
                "score recorded → {}",
                self.book_path.display()
            );
        }
    }
}

/// Shared rhythm loop: takes an already-materialised [`Chart`] and runs it
/// through the normal playfield (audio + shader note approach + HUD). Used
/// by [`play`] (chart from disk) and [`calibrate::run`] (chart generated in
/// memory). The exit banner is printed via `log_final` either way.
///
/// `mute_sfx=true` disables the per-grade hit sounds; the song sink in
/// [`audio::Audio`] is independent and always plays. Calibrate mode passes
/// `mute_sfx=true` + `persist=None`; gameplay `play` passes both.
pub fn play_chart(
    chart: Chart,
    user_offset_ms: i32,
    mute_sfx: bool,
    sfx_volume: Option<f32>,
) -> Result<()> {
    play_chart_inner(
        chart,
        user_offset_ms,
        mute_sfx,
        sfx_volume,
        DEFAULT_COUNTDOWN_MS,
        None,
        NoHook,
        PlayOpts::default(),
    )
}

/// Variant of [`play_chart`] that lets the caller override the pre-song
/// countdown duration. `countdown_ms = 0` kicks audio off immediately (used
/// by calibration). Otherwise the window opens, the HUD shows a big pulsing
/// "3 → 2 → 1 → GO!" in the grid area, and audio starts when the counter
/// finishes.
pub fn play_chart_opts(
    chart: Chart,
    user_offset_ms: i32,
    mute_sfx: bool,
    sfx_volume: Option<f32>,
    countdown_ms: u32,
) -> Result<()> {
    play_chart_inner(
        chart,
        user_offset_ms,
        mute_sfx,
        sfx_volume,
        countdown_ms,
        None,
        NoHook,
        PlayOpts::default(),
    )
}

/// Narration hook invoked every frame. Given the live [`GameState`] and the
/// current `music_ms`, it returns `Some(label)` to paint a narration strip
/// under the HUD title, or `None` to hide it. The blanket impl for
/// `FnMut(&GameState, f64) -> Option<String>` lets callers pass a plain
/// closure; the [`NoHook`] ZST is used internally when no narration is wanted.
pub trait NarrationHook {
    fn narrate(&mut self, state: &GameState, music_ms: f64) -> Option<String>;
}

impl<F> NarrationHook for F
where
    F: FnMut(&GameState, f64) -> Option<String>,
{
    fn narrate(&mut self, state: &GameState, music_ms: f64) -> Option<String> {
        (self)(state, music_ms)
    }
}

/// Zero-sized hook that always returns `None`.
///
/// Used by plain [`play_chart`] so the hook branch compiles out and adds no
/// overhead.
pub struct NoHook;

impl NarrationHook for NoHook {
    fn narrate(&mut self, _state: &GameState, _music_ms: f64) -> Option<String> {
        None
    }
}

/// Like [`play_chart`] but with a narration hook painted below the HUD title.
/// Used by tutorial mode; tests can stub this via the blanket `FnMut` impl.
pub fn play_chart_with_hook<H: NarrationHook + 'static>(
    chart: Chart,
    user_offset_ms: i32,
    mute_sfx: bool,
    sfx_volume: Option<f32>,
    hook: H,
) -> Result<()> {
    play_chart_inner(
        chart,
        user_offset_ms,
        mute_sfx,
        sfx_volume,
        DEFAULT_COUNTDOWN_MS,
        None,
        hook,
        PlayOpts::default(),
    )
}

fn play_chart_inner<H: NarrationHook + 'static>(
    chart: Chart,
    user_offset_ms: i32,
    mute_sfx: bool,
    sfx_volume: Option<f32>,
    countdown_ms: u32,
    persist: Option<ScorePersist>,
    mut narration: H,
    opts: PlayOpts,
) -> Result<()> {
    // Resolve shader path relative to CARGO_MANIFEST_DIR at build time; at run time
    // we probe for it under the crate root, falling back to the exe dir.
    let shader_path = resolve_shader_path();
    if !shader_path.exists() {
        return Err(crate::Error::Config(format!(
            "rhythm: note_approach.wgsl not found at {}",
            shader_path.display()
        )));
    }

    // SharedSpectrum is cloned into the audio source so the mixer thread
    // mirrors each sample. Main thread reads `snapshot()` once per frame
    // and feeds the result into BackgroundInputs for shader sampling.
    let spectrum = spectrum::SharedSpectrum::new();
    let audio = match Audio::load_delayed(
        &chart.audio_path,
        user_offset_ms,
        countdown_ms,
        Some(spectrum.clone()),
    ) {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(target: "juballer::rhythm", "audio init failed: {e}");
            return Err(e);
        }
    };
    if countdown_ms > 0 {
        tracing::info!(
            target: "juballer::rhythm",
            "pre-song countdown: {}ms before audio starts",
            countdown_ms
        );
    }

    // Per-grade hit-sound bank. Uses its own rodio OutputStream (see
    // `sfx.rs`) so a burst of hits can't preempt or stall the song sink
    // above. Silent if samples aren't on disk or no audio device is
    // available; muted entirely when `--mute-sfx` is passed.
    let mut sfx = SfxBank::load_default();
    if let Some(v) = sfx_volume {
        sfx.set_master_volume(v);
    }
    sfx.set_muted(mute_sfx);
    if mute_sfx {
        tracing::info!(target: "juballer::rhythm", "hit sounds muted via --mute-sfx");
    }

    let mut app = App::builder()
        .title("juballer — rhythm")
        .present_mode(PresentMode::Fifo)
        .bg_color(juballer_core::Color::BLACK)
        .controller_vid_pid(0x1973, 0x0011)
        .build()?;
    app.set_debug(false);

    let mut state = GameState::new(chart);
    state.no_fail = opts.no_fail;
    if opts.lead_in_ms > 0 {
        state.lead_in_ms = opts.lead_in_ms as f64;
    }
    if state.no_fail {
        tracing::info!(target: "juballer::rhythm", "mod enabled: no-fail");
    }
    // Preload personal best so the HUD can show "BEST: N" from frame 0.
    if let Some(p) = persist.as_ref() {
        state.best_score = p.load_best();
        if let Some(b) = state.best_score {
            tracing::info!(target: "juballer::rhythm", "personal best: {b}");
        }
    }
    // Guard against double-writing the session record on the finish → exit sequence.
    let mut persisted = false;
    let mut overlay = juballer_egui::EguiOverlay::new();
    let mut marker_overlay = juballer_egui::EguiOverlay::new();
    let mut hit_ring_overlay = juballer_egui::EguiOverlay::new();
    // Dedicated overlay for the multi-cell tail-arrow chain that
    // telegraphs upcoming long notes. Separate from marker_overlay so
    // its egui_wgpu::Renderer state doesn't collide.
    let mut long_tail_overlay = juballer_egui::EguiOverlay::new();
    // Dedicated overlay for image-mode HUD backgrounds (shader mode
    // writes directly via frame.with_region_raw). Each EguiOverlay
    // owns its egui_wgpu::Renderer; sharing one across passes clobbers
    // draw buffers.
    let mut bg_image_overlay = juballer_egui::EguiOverlay::new();
    let mut shader_cache = ShaderPipelineCache::new();
    // Per-grade marker sprite set, lazy-loaded on first egui frame.
    // Baked frame-by-frame animations read more polished than the
    // procedural shader burst, at the cost of loading PNGs.
    let mut markers: Option<marker::Markers> = None;
    // Marker pack resolution order:
    //   1. <opts.asset_dir>/markers/tap/juballer_default/  (user override)
    //   2. bundled default under CARGO_MANIFEST_DIR/../../assets/
    let marker_dir = opts
        .asset_dir
        .as_deref()
        .map(|d| d.join("markers/tap/juballer_default"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(marker::default_marker_dir);
    // HUD jacket-art cache: textures are allocated on first frame that
    // actually renders the jacket, then reused. Negative results cached
    // too so a missing file doesn't retry every frame.
    let mut hud_jackets = render::HudJacketCache::new();
    // HUD top-bar background: resolved once per session from
    // `opts.backgrounds` via chart-path hash. None = no background.
    let background = background::pick_for_chart(
        &state.chart.audio_path,
        &opts.backgrounds,
        opts.background_index,
    );
    let mut bg_img_cache = background::BackgroundImageCache::new();
    if let Some(bg) = &background {
        tracing::info!(target: "juballer::rhythm::background", "background: {:?}", bg);
    }
    // Running held-cell bitmask + last-hit snapshot, both fed into the
    // background shader so custom WGSL can react to gameplay state.
    let mut held_mask: u16 = 0;
    let mut last_hit_music_ms: Option<f64> = None;
    let mut last_hit_grade: Option<Grade> = None;
    let boot = Instant::now();
    let mut last_frame = Instant::now();
    // Wall-clock instant the player's life first hit 0, so we can hold the
    // FAILED banner on-screen for a short grace before exiting.
    let mut failed_at: Option<Instant> = None;
    // Set when the player taps APPLY GLOBAL or APPLY SONG on the
    // results screen, so the HUD can flash a "saved" toast.
    let mut offset_applied_at: Option<Instant> = None;
    // Emergency-exit: all four corners held simultaneously for 3s.
    let mut corner_downs: [Option<Instant>; 4] = [None; 4];
    let corners: [(u8, u8); 4] = [(0, 0), (0, 3), (3, 0), (3, 3)];

    app.run(move |frame, events| {
        // Translate each input event, updating game state at the event's `ts`.
        let mut want_exit = false;
        for ev in events {
            match ev {
                Event::KeyDown { row, col, ts, .. } => {
                    // Post-finish: results screen is interactive.
                    //
                    //   (3,0) → APPLY OFFSET GLOBALLY  (writes
                    //          `[rhythm] audio_offset_ms` in deck.toml)
                    //   (3,1) → APPLY OFFSET TO THIS SONG  (writes
                    //          per-chart override book)
                    //   any other cell → CONTINUE (exit, return-to-picker
                    //          if RETURN_TO env says so)
                    //
                    // The screen otherwise stays up indefinitely — there
                    // is no auto-exit timer, players need time to read.
                    if state.finished {
                        let suggest = state.recommended_audio_offset_ms();
                        if (*row, *col) == (3, 0) {
                            if let Some(off) = suggest {
                                let path =
                                    crate::config::paths::default_config_dir().join("deck.toml");
                                if let Err(e) = write_global_audio_offset(&path, off) {
                                    tracing::warn!(target: "juballer::rhythm",
                                        "apply offset (global) failed: {e}");
                                } else {
                                    tracing::info!(target: "juballer::rhythm",
                                        "applied audio_offset_ms={off} globally to {}",
                                        path.display());
                                }
                            }
                            offset_applied_at = Some(Instant::now());
                        } else if (*row, *col) == (3, 1) {
                            if let (Some(off), Some(p)) = (suggest, persist.as_ref()) {
                                match chart_overrides::ChartOverrideBook::load_default() {
                                    Ok(mut book) => {
                                        book.set_offset(&p.chart_path, off);
                                        if let Err(e) = book.save_default() {
                                            tracing::warn!(target: "juballer::rhythm",
                                                "apply offset (song) save failed: {e}");
                                        } else {
                                            tracing::info!(target: "juballer::rhythm",
                                                "applied audio_offset_ms={off} to chart {}",
                                                p.chart_path.display());
                                        }
                                    }
                                    Err(e) => tracing::warn!(target: "juballer::rhythm",
                                        "chart_overrides load failed: {e}"),
                                }
                            }
                            offset_applied_at = Some(Instant::now());
                        } else {
                            want_exit = true;
                        }
                        continue;
                    }
                    let press_ms = music_time_from_ts(&audio, *ts);
                    // Track the held-cell bitmask regardless of music_ms so
                    // warm-up taps during the countdown still show up in the
                    // `bound` / `held_mask` channels of the background shader.
                    let bit = 1u16 << ((*row as u16) * 4 + (*col as u16));
                    held_mask |= bit;
                    if press_ms >= 0.0 {
                        if let Some(grade) = state.on_press(*row, *col, press_ms) {
                            tracing::debug!(
                                target: "juballer::rhythm",
                                "hit ({},{}) -> {:?} @ {press_ms:.1}ms",
                                row, col, grade
                            );
                            sfx.play(grade);
                            last_hit_music_ms = Some(press_ms);
                            last_hit_grade = Some(grade);
                        }
                    }
                    // Track corner-hold emergency exit.
                    for (i, (r, c)) in corners.iter().enumerate() {
                        if r == row && c == col {
                            corner_downs[i] = Some(*ts);
                        }
                    }
                }
                Event::KeyUp { row, col, ts, .. } => {
                    let release_ms = music_time_from_ts(&audio, *ts);
                    let bit = 1u16 << ((*row as u16) * 4 + (*col as u16));
                    held_mask &= !bit;
                    if release_ms >= 0.0 {
                        if let Some(g) = state.on_release(*row, *col, release_ms) {
                            tracing::debug!(
                                target: "juballer::rhythm",
                                "release ({},{}) -> {:?} @ {release_ms:.1}ms",
                                row, col, g
                            );
                            sfx.play(g);
                            last_hit_music_ms = Some(release_ms);
                            last_hit_grade = Some(g);
                        }
                    }
                    for (i, (r, c)) in corners.iter().enumerate() {
                        if r == row && c == col {
                            corner_downs[i] = None;
                        }
                    }
                }
                Event::Quit => {
                    want_exit = true;
                }
                Event::Unmapped { key, .. } => {
                    // Escape leaves rhythm mode cleanly.
                    if key.0 == "NAMED_Escape" {
                        want_exit = true;
                    }
                }
                _ => {}
            }
        }

        // All four corners held ≥ 3s → exit.
        if corner_downs.iter().all(|o| o.is_some()) {
            let earliest = corner_downs
                .iter()
                .filter_map(|o| *o)
                .min()
                .unwrap_or_else(Instant::now);
            if earliest.elapsed() >= std::time::Duration::from_secs(3) {
                want_exit = true;
            }
        }

        // Advance clock.
        let music_ms = audio.position_ms();
        // Pre-song countdown phase: music_ms is negative. Don't advance the
        // game-state clock — that would start judging notes (and auto-MISSing
        // them) before audio has even begun. Rendering continues so we can
        // paint the countdown overlay; note draws self-gate via the render
        // window.
        let in_countdown = music_ms < 0.0;
        if !in_countdown {
            state.tick(music_ms);
        }

        // Hard-fail: life bar drained. Stop the music the moment we detect
        // it, remember the wall-clock instant, then let the render pass paint
        // the FAILED banner for a short grace before exiting below.
        if state.failed && failed_at.is_none() {
            failed_at = Some(Instant::now());
            audio.stop();
            log_final(&state);
        }

        // Mark finished when the sink drained AND we've passed the last note's MISS
        // cutoff. Keep the HUD up for a few seconds so the player can read it.
        let last_hit = state
            .chart
            .notes
            .last()
            .map(|n| n.hit_time_ms)
            .unwrap_or(0.0);
        if !state.finished
            && !in_countdown
            && (audio.is_finished() || music_ms > last_hit + 2_000.0)
            && state.judged_notes() == state.total_notes()
        {
            state.finished = true;
            log_final(&state);
            // Persist as soon as we're finished so the banner shows the new
            // personal best if this run beat it.
            if !persisted {
                if let Some(p) = persist.as_ref() {
                    let record = ScoreRecord::from_state(&state);
                    p.persist(record);
                    // Refresh best to reflect this run (may bump it).
                    state.best_score = match state.best_score {
                        Some(prev) => Some(prev.max(state.score)),
                        None => Some(state.score),
                    };
                }
                persisted = true;
            }
        }

        // 1. Background fill.
        render::paint_backgrounds(frame);
        // 2. Per-tile note shaders.
        let boot_secs = boot.elapsed().as_secs_f32();
        let dt = {
            let now = Instant::now();
            let d = now.duration_since(last_frame).as_secs_f32();
            last_frame = now;
            d
        };

        // Build the per-frame channel bundle the background consumes.
        let bg_inputs = background::BackgroundInputs {
            music_ms: state.music_time_ms,
            bpm: state.chart.schedule.bpm_at(state.music_time_ms),
            beat_phase: {
                let b = state.chart.schedule.bpm_at(state.music_time_ms).max(1.0);
                let ms_per_beat = 60_000.0 / b;
                ((state.music_time_ms.rem_euclid(ms_per_beat)) / ms_per_beat) as f32
            },
            combo: state.combo,
            score: state.score,
            life: state.life,
            held_mask,
            last_grade: last_hit_grade
                .map(|g| match g {
                    Grade::Perfect => 1.0,
                    Grade::Great => 2.0,
                    Grade::Good => 3.0,
                    Grade::Poor => 4.0,
                    Grade::Miss => 5.0,
                })
                .unwrap_or(0.0),
            last_hit_elapsed_ms: last_hit_music_ms
                .map(|h| state.music_time_ms - h)
                .unwrap_or(1e9),
            last_hit_accent: last_hit_grade
                .map(|g| render::grade_color(Some(g)))
                .unwrap_or([1.0, 1.0, 1.0, 1.0]),
            spectrum: spectrum.snapshot(),
        };

        // HUD top-bar background (shader mode) — rendered raw into the
        // top region so the HUD's text overlay composes on top of it.
        // Image-mode backgrounds paint from inside the HUD's own
        // overlay below (see draw_hud_with_narration call).
        if let Some(bg) = &background {
            if matches!(bg, background::Background::Shader(_)) {
                let top_rect = frame.top_region_rect();
                background::draw_shader(
                    frame,
                    bg,
                    top_rect,
                    bg_inputs,
                    &mut shader_cache,
                    boot_secs,
                    dt,
                );
            }
        }

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
        // 2a'. Long-note tail arrows — chain of chevrons across every cell
        // between the head and the memon `p` tail position, pointing at the
        // head. Painted after the rings so it composes over them but
        // before the marker sprites so the head sprite still wins on the
        // head cell.
        render::draw_long_tail_arrows(frame, &mut long_tail_overlay, &state);
        // 2b. Tap-note markers — PNG sprite path via its own dedicated
        // EguiOverlay (shares none of the HUD overlay's renderer state).
        render::draw_notes_markers(
            frame,
            &mut marker_overlay,
            &mut markers,
            &marker_dir,
            &state,
        );

        // 2c. Image-mode HUD background: painted into its own overlay
        // right before the HUD so the HUD text sits on top.
        if let Some(bg) = &background {
            if matches!(bg, background::Background::Image(_)) {
                let top_rect = frame.top_region_rect();
                bg_image_overlay.draw(frame, |rc| {
                    background::draw_image(rc, bg, top_rect, &mut bg_img_cache);
                });
            }
        }
        // 3. HUD. Narration hook runs every frame — returning `Some(text)`
        // causes the renderer to paint a translucent strip under the title.
        let narration_text = narration.narrate(&state, music_ms);
        render::draw_hud_with_narration(
            frame,
            &mut overlay,
            &state,
            state.finished,
            &mut hud_jackets,
            narration_text.as_deref(),
            offset_applied_at,
        );
        // 4. Pre-song countdown overlay. Only active during the delay window;
        // we also hold a brief "GO!" frame (~400ms) once audio has kicked off
        // so the word has time to register visually.
        if music_ms < 400.0 && countdown_ms > 0 {
            render::draw_countdown(frame, &mut overlay, -music_ms);
        }

        // No auto-exit on finish — the results screen stays up until the
        // player taps any cell (which sets want_exit above). Failed runs
        // still auto-clear after a 2.5 s grace because there's no useful
        // input to make on the FAILED banner.
        let post_fail_grace = failed_at
            .map(|t| t.elapsed() >= std::time::Duration::from_millis(2_500))
            .unwrap_or(false);
        if want_exit || post_fail_grace {
            audio.stop();
            log_final(&state);
            // Persist on abrupt exit too (only if any note was actually
            // judged — don't pollute the book with zero-note quits).
            if !persisted && state.judged_notes() > 0 {
                if let Some(p) = persist.as_ref() {
                    p.persist(ScoreRecord::from_state(&state));
                }
                persisted = true;
            }
            exit::exit(0);
        }
    })?;
    Ok(())
}

/// Compute the music-time at the moment a winit/evdev input event arrived,
/// using the event's monotonic `ts` rather than `Instant::now()`. We extrapolate
/// by taking the current audio position and subtracting the interval between
/// `ts` and "right now" — so the older the event, the further back the music
/// time.
fn music_time_from_ts(audio: &Audio, ts: Instant) -> f64 {
    let now = Instant::now();
    let age = now.saturating_duration_since(ts).as_secs_f64() * 1000.0;
    audio.position_ms() - age
}

pub(crate) fn log_final(state: &GameState) {
    let mean_off = state
        .mean_input_offset_ms()
        .map(|m| format!("{:+.1}ms", m))
        .unwrap_or_else(|| "n/a".to_string());
    let accuracy = state.accuracy_pct().unwrap_or(0.0);
    let suggestion = match state.recommended_audio_offset_ms() {
        Some(off) => format!(" | suggest --audio-offset-ms {off}"),
        None => String::new(),
    };
    tracing::info!(
        target: "juballer::rhythm",
        "session end: score={} acc={:.1}% max_combo={} P={} GT={} GD={} PO={} M={} | mean input offset {}{}",
        state.score,
        accuracy,
        state.max_combo,
        state.count(Grade::Perfect),
        state.count(Grade::Great),
        state.count(Grade::Good),
        state.count(Grade::Poor),
        state.count(Grade::Miss),
        mean_off,
        suggestion,
    );
}

/// Resolve the path to note_approach.wgsl at runtime. Checks (in order):
/// 1. `$CARGO_MANIFEST_DIR/examples/shaders/note_approach.wgsl` — dev builds.
/// 2. Alongside the current exe: `<exe_dir>/shaders/note_approach.wgsl` — packaged.
/// 3. CWD-relative fallback for ad-hoc runs.
fn resolve_shader_path() -> PathBuf {
    let candidates: Vec<PathBuf> = vec![
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/shaders/note_approach.wgsl"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .map(|d| d.join("shaders/note_approach.wgsl"))
            .unwrap_or_default(),
        PathBuf::from("examples/shaders/note_approach.wgsl"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    candidates.into_iter().next().unwrap()
}

/// Write `[rhythm] audio_offset_ms = <off>` into `deck_path`, preserving
/// every other key. Mirrors `settings_ui::write_rhythm_section` but
/// only touches the one field — the results-screen "APPLY GLOBALLY"
/// path doesn't have access to the full `SettingsState`.
fn write_global_audio_offset(deck_path: &Path, offset_ms: i32) -> Result<()> {
    let current = std::fs::read_to_string(deck_path)
        .map_err(|e| crate::Error::Config(format!("read {}: {e}", deck_path.display())))?;
    let mut doc: toml::Value = toml::from_str(&current)
        .map_err(|e| crate::Error::Config(format!("parse {}: {e}", deck_path.display())))?;
    let table = doc
        .as_table_mut()
        .ok_or_else(|| crate::Error::Config(format!("{} is not a table", deck_path.display())))?;
    let rhythm_entry = table
        .entry("rhythm".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let rhythm = rhythm_entry
        .as_table_mut()
        .ok_or_else(|| crate::Error::Config("[rhythm] is not a table".to_string()))?;
    rhythm.insert(
        "audio_offset_ms".into(),
        toml::Value::Integer(offset_ms.into()),
    );
    let serialized = toml::to_string_pretty(&doc)
        .map_err(|e| crate::Error::Config(format!("toml encode: {e}")))?;
    crate::config::atomic::atomic_write(deck_path, serialized.as_bytes())
        .map_err(|e| crate::Error::Config(format!("atomic_write: {e}")))?;
    Ok(())
}
