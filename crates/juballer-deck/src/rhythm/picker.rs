//! Chart-picker page. Scans a directory for `*.memon` files, shows up to 14
//! of them in the 4×4 grid (cells 0..=13) with title/artist overlays, and
//! on commit re-execs the deck with `play <chosen>` — simplest way to
//! cleanly transition from picker → rhythm mode without running two winit
//! event loops back-to-back in the same process.
//!
//! Not part of the main deck app; driven via `play <dir>` from the CLI.
//!
//! # UX (focus → cycle difficulty → PLAY → exit)
//!
//! The last two cells of the grid are reserved:
//!
//! - `(3, 2)` — **PLAY** (commit) cell. Only armed once a chart is focused.
//!   Launches with the currently-selected difficulty.
//! - `(3, 3)` — **EXIT** cell. Stops preview and quits the picker.
//!
//! The other 14 cells are chart cells. Interaction rules:
//!
//! 1. **First tap on a chart cell** → focus it. A looping audio snippet of
//!    the song starts playing (20% into the track for 15s by default, or
//!    the memon `metadata.preview` window if present). `selected_diff_idx`
//!    resets to 0 (the chart's first difficulty).
//! 2. **Same-cell press while focused, `difficulties.len() > 1`** → cycle
//!    `selected_diff_idx` to the next difficulty. Does NOT launch. Preview
//!    keeps playing.
//! 3. **Same-cell press while focused, single difficulty** → launch
//!    immediately (nothing to cycle through).
//! 4. **Different chart cell while focused** → refocus + reset
//!    `selected_diff_idx` to 0 (new chart → new difficulty list).
//! 5. **PLAY cell (3,2) while focused** → launch with
//!    `entry.difficulties[selected_diff_idx]`.
//! 6. **PLAY cell (3,2) without focus** → ignored (nothing to play).
//! 7. **EXIT cell (3,3)** → stop preview, quit.
//!
//! Intended flow: tap a chart cell → hear preview → tap same cell to cycle
//! difficulty → tap `(3, 2)` PLAY to commit.

use super::scores::ScoreBook;
use crate::rhythm::chart::{resolve_jacket_path, Preview};
use crate::{Error, Result};
use juballer_core::input::Event;
use juballer_core::{App, Color, Frame, PresentMode};
use juballer_egui::EguiOverlay;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::favorites::FavoriteBook;
use super::picker_view::{
    apply_filters, apply_sort, discover_packs, DifficultyFilter, FavoriteFilter, PickerView,
    SortDirection, SortMode,
};

/// Where in the song to start the preview snippet when the chart has no
/// `metadata.preview` hint. 0.20 = 20% of the track — enough to skip past
/// most intros but not so far that short songs run out of runway.
pub const PREVIEW_START_PCT: f64 = 0.20;

/// How long the preview snippet plays before looping, when no explicit
/// `metadata.preview.duration` is present.
pub const PREVIEW_LEN: Duration = Duration::from_secs(25);

/// Fade-in ramp at the start of every loop iteration. Stops loud
/// transient hits (kicks, cymbals) from snapping in cold when the
/// preview wraps.
pub const PREVIEW_FADE_IN: Duration = Duration::from_millis(450);

/// Fade-out ramp at the end of every loop iteration. Pairs with the
/// fade-in so the loop seam reads as a single soft swell rather than
/// a hard cut.
pub const PREVIEW_FADE_OUT: Duration = Duration::from_millis(700);

/// Quiet playback volume for the preview sink — loud enough to audition the
/// song, quiet enough that the user isn't blasted when they're just
/// scrolling through the grid.
pub const PREVIEW_VOLUME: f32 = 0.45;

/// One row in the picker grid.
#[derive(Debug, Clone)]
pub struct ChartEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub bpm: f64,
    pub note_count: usize,
    pub difficulties: Vec<String>,
    /// Absolute path of the audio file referenced by the memon `metadata.audio`
    /// field, resolved against the chart's directory. Used to play the
    /// song-preview snippet when the cell is focused.
    pub audio_path: PathBuf,
    /// memon-provided preview window (seconds). `None` means "use heuristic".
    pub preview: Option<Preview>,
    /// Absolute path to the chart's jacket/album-art PNG (320×320 cover).
    /// Resolved from `metadata.jacket` or a sibling `jacket.png` fallback.
    /// `None` when absent.
    pub jacket_path: Option<PathBuf>,
    /// Absolute path to the chart's banner PNG (typ. 160×160 song-select
    /// grid tile) auto-detected as `banner.png` next to the chart file.
    /// Picker cells prefer this over the larger jacket because it's
    /// authored at grid-tile scale.
    pub banner_path: Option<PathBuf>,
    /// Absolute path to the chart's mini banner (typ. 132×24 wordmark
    /// strip). Used in the picker's top-region preview strip when a
    /// chart is focused.
    pub mini_path: Option<PathBuf>,
}

/// Lazy, per-path egui texture cache for chart jackets. Negative caching:
/// a path that failed to load once maps to `None` so we don't hammer the
/// filesystem every frame. Owned separately by the picker and the HUD —
/// no shared state between them (see module docs in `render.rs`).
#[derive(Default)]
pub struct JacketCache {
    inner: HashMap<PathBuf, Option<egui::TextureHandle>>,
}

impl JacketCache {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Return a texture handle for `path`, loading it on first request.
    /// The `ctx` is only used to allocate the texture — subsequent calls
    /// return the cached handle directly. Decode / IO errors are logged
    /// once and cached as `None`.
    pub fn get_or_load(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Option<&egui::TextureHandle> {
        if !self.inner.contains_key(path) {
            let loaded = load_jacket_texture(ctx, path);
            self.inner.insert(path.to_path_buf(), loaded);
        }
        self.inner.get(path).and_then(|o| o.as_ref())
    }
}

fn load_jacket_texture(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                target: "juballer::rhythm::jacket",
                "cannot read {}: {e}",
                path.display()
            );
            return None;
        }
    };
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(
                target: "juballer::rhythm::jacket",
                "decode {}: {e}",
                path.display()
            );
            return None;
        }
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let uri = format!("jacket://{}", path.display());
    Some(ctx.load_texture(uri, color_img, egui::TextureOptions::LINEAR))
}

/// Maximum number of chart cells shown on one page of the 4x4 grid.
/// Four cells in row 3 are reserved: PREV, NEXT, PLAY, EXIT.
pub const CHART_CELLS_PER_PAGE: usize = 12;

/// Grid index of the PREV-page cell — row 3, col 0.
pub const PREV_CELL_IDX: usize = 3 * 4; // 12
/// Grid index of the NEXT-page cell — row 3, col 1.
pub const NEXT_CELL_IDX: usize = 3 * 4 + 1; // 13
/// Grid index of the dedicated PLAY (commit) cell — row 3, col 2.
pub const PLAY_CELL_IDX: usize = 3 * 4 + 2; // 14
/// Grid index of the dedicated EXIT cell — row 3, col 3.
pub const EXIT_CELL_IDX: usize = 3 * 4 + 3; // 15

/// Scan `dir` for `*.memon` files and parse each into a [`ChartEntry`].
/// Returns everything found (no truncation) so the caller can paginate
/// larger chart libraries via [`crate::rhythm::pagination::Paginator`].
pub fn scan(dir: &Path) -> Result<Vec<ChartEntry>> {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_charts(dir, &mut paths, 0)?;
    paths.sort();
    let mut entries = Vec::with_capacity(paths.len());
    for p in paths {
        match load_entry(&p) {
            Ok(e) => entries.push(e),
            Err(e) => tracing::warn!(
                target: "juballer::rhythm::picker",
                "skipping {}: {e}",
                p.display()
            ),
        }
    }
    Ok(entries)
}

/// Maximum recursion depth when scanning a charts directory. Enough to
/// walk into `<charts_dir>/<pack>/<song>/`, which matches community
/// bulk-dump layouts; stops the picker from wandering into arbitrarily
/// deep trees.
const MAX_SCAN_DEPTH: usize = 4;

/// Recursive walk collecting chart files. Per directory:
///   * If the dir contains a `song.memon` (multi-difficulty container),
///     that file alone represents the song — emit it and stop recursing.
///   * Otherwise emit any `*.memon` in the dir and descend into subdirs
///     up to `MAX_SCAN_DEPTH`.
///
/// This naturally handles both "flat dir of .memon files" and bulk dumps
/// laid out as `<pack>/<song>/song.memon` + per-difficulty siblings.
fn collect_charts(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    if depth > MAX_SCAN_DEPTH {
        return Ok(());
    }
    let read = std::fs::read_dir(dir)
        .map_err(|e| Error::Config(format!("picker: read {}: {e}", dir.display())))?;
    let mut entries: Vec<std::fs::DirEntry> = read
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| Error::Config(format!("picker: read entries in {}: {e}", dir.display())))?;
    entries.sort_by_key(|e| e.file_name());

    // Song-container short-circuit.
    let song_memon = dir.join("song.memon");
    if song_memon.is_file() {
        out.push(song_memon);
        return Ok(());
    }

    for e in entries {
        let p = e.path();
        if p.is_dir() {
            if let Err(e) = collect_charts(&p, out, depth + 1) {
                tracing::warn!(
                    target: "juballer::rhythm::picker",
                    "skipping subdir {}: {e}",
                    p.display()
                );
            }
        } else if p.extension().and_then(|s| s.to_str()) == Some("memon") {
            out.push(p);
        }
    }
    Ok(())
}

/// Parse the memon header without fully resolving notes (we only need title,
/// artist, first-BPM, and the difficulty key list for the grid).
fn load_entry(path: &Path) -> Result<ChartEntry> {
    let bytes = std::fs::read(path)?;
    let memon: super::chart::Memon = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
    if memon.version != "1.0.0" {
        return Err(Error::Config(format!(
            "unsupported version {}",
            memon.version
        )));
    }
    // Pick a difficulty to count notes from — prefer BSC if present, else
    // whichever sorts first.
    let diffs: Vec<String> = memon.data.keys().cloned().collect();
    let first_diff = diffs
        .iter()
        .find(|d| d.as_str() == "BSC")
        .cloned()
        .or_else(|| diffs.first().cloned())
        .ok_or_else(|| Error::Config("no difficulties".to_string()))?;
    let note_count = memon
        .data
        .get(&first_diff)
        .map(|d| d.notes.len())
        .unwrap_or(0);
    // BPM lookup: memon allows top-level OR per-difficulty timing
    // blocks. The chart loader falls back from chart-level → top-level
    // (`resolve_timing`); the picker mirrors that order so the side
    // panel doesn't show "0 BPM" for charts that only specify timing
    // inside their first difficulty block.
    let bpm = memon
        .timing
        .as_ref()
        .and_then(|t| t.bpms.first())
        .map(|b| b.bpm)
        .or_else(|| {
            memon.data.values().find_map(|d| {
                d.timing
                    .as_ref()
                    .and_then(|t| t.bpms.first())
                    .map(|b| b.bpm)
            })
        })
        .unwrap_or(0.0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let audio_path = parent.join(&memon.metadata.audio);
    let jacket_path = resolve_jacket_path(parent, memon.metadata.jacket.as_deref());
    let banner_path = ["banner.png", "banner.jpg", "banner.jpeg"]
        .iter()
        .map(|n| parent.join(n))
        .find(|p| p.is_file());
    let mini_path = ["mini.png", "mini.jpg", "thumb.png"]
        .iter()
        .map(|n| parent.join(n))
        .find(|p| p.is_file());
    Ok(ChartEntry {
        path: path.to_path_buf(),
        title: memon.metadata.title,
        artist: memon.metadata.artist,
        bpm,
        note_count,
        difficulties: diffs,
        audio_path,
        preview: memon.metadata.preview,
        jacket_path,
        banner_path,
        mini_path,
    })
}

/// Resolve the `(start, duration)` pair for a song-preview snippet, given the
/// chart's explicit memon `preview` metadata (if any) and the decoded track
/// length (if the decoder could determine it).
///
/// Priority:
/// 1. Explicit `preview` metadata wins outright — we trust the chart author.
/// 2. Otherwise, for a track of length `L`, start at `L * PREVIEW_START_PCT`
///    and play for `PREVIEW_LEN`.
/// 3. If the track length is unknown, start at 0 and play for `PREVIEW_LEN`
///    — better than nothing; the user hears the intro.
///
/// Split out of [`PreviewPlayer::start`] so it can be unit-tested without
/// decoding any audio.
pub fn preview_window(preview: Option<Preview>, total: Option<Duration>) -> (Duration, Duration) {
    if let Some(p) = preview {
        let start = Duration::from_secs_f64(p.start.max(0.0));
        let dur = Duration::from_secs_f64(p.duration.max(0.0));
        return (start, dur);
    }
    match total {
        Some(t) => {
            let start = Duration::from_secs_f64(t.as_secs_f64() * PREVIEW_START_PCT);
            (start, PREVIEW_LEN)
        }
        None => (Duration::ZERO, PREVIEW_LEN),
    }
}

/// Focus + difficulty-selection state carried across frames by the picker's
/// event loop. Kept as a plain data struct so the cycling/commit decision
/// logic can be written as a pure function (see [`press_cell`]) and
/// unit-tested without spinning up winit, egui, audio, or process-exec.
///
/// - `focused` is the grid index (0..[`CHART_CELLS_PER_PAGE`]) of the chart cell
///   whose preview is currently playing, or `None` when nothing is focused.
/// - `selected_diff_idx` is the position within the focused chart's
///   `difficulties` vector. Always resets to 0 on (re)focus. Only meaningful
///   when `focused.is_some()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PickerState {
    pub focused: Option<usize>,
    pub selected_diff_idx: usize,
}

/// Top-level picker mode. Browse is the chart grid; Filter overlays a
/// 4×4 sort/filter editor on the same cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMode {
    Browse,
    Filter,
}

/// Min hold duration on a nav cell to trigger the alternate action
/// (PREV → enter Filter mode; NEXT → favorite focused chart). Below
/// this, the press counts as a normal page-nav tap on KeyUp.
pub const HOLD_THRESHOLD_MS: u64 = 500;

/// Decision returned by [`press_cell`] for the run loop to carry out.
///
/// `press_cell` is intentionally *pure* — it doesn't touch audio, stdout,
/// or the filesystem. The caller maps actions onto side effects:
///
/// - `Focus { idx }` — drop the current preview, start a new one for
///   `entries[idx]`.
/// - `Cycle` — update on-screen hint; no audio change.
/// - `Launch { idx, diff_idx }` — drop preview and exec into play.
/// - `Exit` — drop preview and `super::exit::exit(0)`.
/// - `Ignore` — do nothing (e.g. press on empty cell, PLAY cell with no
///   focus, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    /// (Re)focus the chart at `idx`. `selected_diff_idx` is implicitly 0.
    Focus { idx: usize },
    /// Advance `selected_diff_idx` by one within the currently-focused
    /// chart's difficulty list.
    Cycle,
    /// Commit — launch `entries[idx]` at `entries[idx].difficulties[diff_idx]`.
    Launch { idx: usize, diff_idx: usize },
    /// User tapped the exit cell.
    Exit,
    /// Nothing actionable (empty cell, orphan PLAY press, etc.).
    Ignore,
}

/// Pure state transition for a single cell press. Given the current
/// `state` (focus + selected difficulty), the pressed `(row, col)`, and a
/// `diff_count_for` resolver that returns `None` when the cell is unoccupied
/// or `Some(n)` for a chart cell with `n` difficulties, compute the
/// [`PickerAction`] the run loop should take AND the next [`PickerState`].
///
/// The resolver is a closure so tests can drive arbitrary fake grids
/// without constructing full `ChartEntry` values.
pub fn press_cell(
    state: PickerState,
    row: u8,
    col: u8,
    diff_count_for: impl Fn(usize) -> Option<usize>,
) -> (PickerAction, PickerState) {
    // EXIT cell always wins — takes precedence over everything else.
    if row == 3 && col == 3 {
        return (PickerAction::Exit, state);
    }
    let idx = (row as usize) * 4 + col as usize;

    // PLAY (commit) cell: only meaningful when a chart is focused.
    if idx == PLAY_CELL_IDX {
        match state.focused {
            Some(fidx) => (
                PickerAction::Launch {
                    idx: fidx,
                    diff_idx: state.selected_diff_idx,
                },
                state,
            ),
            None => (PickerAction::Ignore, state),
        }
    } else {
        // Chart cell (0..CHART_CELLS_PER_PAGE).
        let Some(n_diffs) = diff_count_for(idx) else {
            // Empty / unoccupied cell — ignore entirely.
            return (PickerAction::Ignore, state);
        };
        if state.focused == Some(idx) {
            // Same-cell re-press.
            if n_diffs > 1 {
                let next = (state.selected_diff_idx + 1) % n_diffs;
                let new_state = PickerState {
                    focused: state.focused,
                    selected_diff_idx: next,
                };
                (PickerAction::Cycle, new_state)
            } else {
                // Single difficulty → nothing to cycle, commit immediately.
                (PickerAction::Launch { idx, diff_idx: 0 }, state)
            }
        } else {
            // New focus (fresh or switching from another chart). Reset the
            // selected-difficulty index so we always start at the first
            // difficulty of the new chart.
            let new_state = PickerState {
                focused: Some(idx),
                selected_diff_idx: 0,
            };
            (PickerAction::Focus { idx }, new_state)
        }
    }
}

/// Audio snippet looped while a cell is focused in the picker. Owns its own
/// `OutputStream` + `Sink`, deliberately separate from the rhythm-mode
/// `Audio` so there's zero chance of the preview bleeding into gameplay
/// playback. `stop()` (or `Drop`) halts the sink; building a fresh
/// `PreviewPlayer` is how we "switch" previews — spin down the old one,
/// spin up a new one. Decoding runs on a worker thread so the picker UI
/// doesn't stall waiting for a 25 s slice of vorbis to come off disk.
///
/// State machine:
///   Loading(rx) — decode worker is running; nothing is playing yet
///   Playing(sink) — buffer arrived, sink is appending the looping audio
///
/// `poll` is cheap and idempotent; the picker calls it every frame and
/// flips Loading → Playing the moment the worker delivers a buffer.
pub struct PreviewPlayer {
    state: PreviewState,
    handle: OutputStreamHandle,
    spectrum: Option<super::spectrum::SharedSpectrum>,
}

enum PreviewState {
    Loading(std::sync::mpsc::Receiver<std::io::Result<PreviewBuf>>),
    Playing(Sink),
}

/// What the worker thread sends back: pre-decoded sample buffers + the
/// stream's native rate/channels so the main thread can wrap them in a
/// `SamplesBuffer` without re-querying the decoder.
struct PreviewBuf {
    first: Vec<i16>,
    looping: Vec<i16>,
    sample_rate: u32,
    channels: u16,
}

impl PreviewPlayer {
    /// Spawn an async preview — returns immediately.
    ///
    /// The decode + fade envelope work runs on a background thread; the
    /// sink doesn't get built (and audio doesn't start) until the worker
    /// delivers the buffer. Caller polls [`Self::poll`] each frame to flip
    /// Loading → Playing.
    ///
    /// `handle` is the long-lived stream handle owned by the picker —
    /// keeping one [`OutputStream`] alive across the whole picker run
    /// avoids the ~10-30 ms cold-open cost on every chart switch.
    pub fn start(
        audio_path: &Path,
        preview: Option<Preview>,
        spectrum: Option<super::spectrum::SharedSpectrum>,
        handle: OutputStreamHandle,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let path = audio_path.to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(decode_preview(&path, preview));
        });
        Self {
            state: PreviewState::Loading(rx),
            handle,
            spectrum,
        }
    }

    /// Pull-poll the worker. If the buffer is ready and we're still in
    /// Loading, build the sink and start playback. Cheap when the
    /// worker hasn't delivered yet (one `try_recv` per call).
    pub fn poll(&mut self) {
        use rodio::buffer::SamplesBuffer;
        let buf = match &self.state {
            PreviewState::Playing(_) => return,
            PreviewState::Loading(rx) => match rx.try_recv() {
                Ok(Ok(b)) => b,
                Ok(Err(e)) => {
                    tracing::warn!(target: "juballer::rhythm::picker",
                        "preview decode failed: {e}");
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            },
        };
        let sink = match Sink::try_new(&self.handle) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(target: "juballer::rhythm::picker",
                    "preview sink create failed: {e}");
                return;
            }
        };
        sink.set_volume(PREVIEW_VOLUME);
        let first_buf = SamplesBuffer::new(buf.channels, buf.sample_rate, buf.first);
        let loop_buf =
            SamplesBuffer::new(buf.channels, buf.sample_rate, buf.looping).repeat_infinite();
        match self.spectrum.clone() {
            Some(shared) => {
                sink.append(super::spectrum::SampleTap::new(first_buf, shared.clone()));
                sink.append(super::spectrum::SampleTap::new(loop_buf, shared));
            }
            None => {
                sink.append(first_buf);
                sink.append(loop_buf);
            }
        }
        sink.play();
        self.state = PreviewState::Playing(sink);
    }

    /// Best-effort silence: drops the sink's queue and parks it. Equivalent
    /// in effect to dropping the player, but exposed for callers that want
    /// to stop *before* losing the value.
    pub fn stop(&self) {
        if let PreviewState::Playing(sink) = &self.state {
            sink.stop();
        }
    }
}

impl Drop for PreviewPlayer {
    fn drop(&mut self) {
        if let PreviewState::Playing(sink) = &self.state {
            sink.stop();
        }
        // Loading state has no sink to stop; the worker thread will
        // finish its decode and discard the buffer when it tries to
        // send into the dropped channel.
    }
}

/// Decode the preview window into in-memory sample buffers + bake the
/// fade envelopes. Called from the worker thread spawned by
/// [`PreviewPlayer::start`].
fn decode_preview(audio_path: &Path, preview: Option<Preview>) -> std::io::Result<PreviewBuf> {
    let file = File::open(audio_path)?;
    let mut decoder = Decoder::new(BufReader::new(file))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    // Skip total_duration(): vorbis walks the whole file to compute it,
    // adding 100-300 ms cold for big songs. preview_window's heuristic
    // copes fine with None.
    let (start, dur) = preview_window(preview, None);
    let sample_rate = decoder.sample_rate();
    let channels = decoder.channels();
    let _ = decoder.try_seek(start); // O(1) on vorbis
    let want_samples = (dur.as_secs_f64() * sample_rate as f64 * channels as f64) as usize;
    let mut samples: Vec<i16> = Vec::with_capacity(want_samples);
    for s in decoder.by_ref().take(want_samples) {
        samples.push(s);
    }
    if samples.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "preview decode returned 0 samples",
        ));
    }
    // Linear fade envelope sample counts (× channels). First iteration
    // is fade-out only (instant start when user picks). Loop iterations
    // get both fades for a soft seam.
    let fade_in_n = ((PREVIEW_FADE_IN.as_secs_f64() * sample_rate as f64 * channels as f64)
        as usize)
        .min(samples.len() / 2);
    let fade_out_n = ((PREVIEW_FADE_OUT.as_secs_f64() * sample_rate as f64 * channels as f64)
        as usize)
        .min(samples.len() / 2);
    let mut first = samples.clone();
    for (k, s) in first.iter_mut().rev().take(fade_out_n).enumerate() {
        let g = k as f32 / fade_out_n.max(1) as f32;
        *s = (*s as f32 * g) as i16;
    }
    for (i, s) in samples.iter_mut().take(fade_in_n).enumerate() {
        let g = i as f32 / fade_in_n.max(1) as f32;
        *s = (*s as f32 * g) as i16;
    }
    for (k, s) in samples.iter_mut().rev().take(fade_out_n).enumerate() {
        let g = k as f32 / fade_out_n.max(1) as f32;
        *s = (*s as f32 * g) as i16;
    }
    Ok(PreviewBuf {
        first,
        looping: samples,
        sample_rate,
        channels,
    })
}

/// What the run loop should do after a filter-mode cell press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterAction {
    /// View was mutated (cycled), but no further action.
    Stay,
    /// Persist view + rebuild paginator + return to Browse.
    Apply,
    /// Discard view changes (reload from disk) + return to Browse.
    Back,
    /// Reset view to defaults (still on the screen).
    Reset,
    /// Quit the picker entirely.
    Exit,
}

/// 4×4 layout for filter mode. Rows 0–2 are sort/filter dimensions
/// that cycle through their options on each tap; row 3 is the action
/// bar (BACK / RESET / APPLY / EXIT).
///
/// (0,0) sort dimension       (0,1) sort direction
/// (0,2) pack filter          (0,3) difficulty filter
/// (1,0) favorites filter     (1,1..3) reserved for future filters
/// (2,0..3) reserved
/// (3,0) BACK                 (3,1) RESET
/// (3,2) APPLY                (3,3) EXIT
fn handle_filter_press(
    row: u8,
    col: u8,
    view: &mut PickerView,
    all_packs: &[String],
) -> FilterAction {
    match (row, col) {
        (0, 0) => {
            view.sort = view.sort.next();
            FilterAction::Stay
        }
        (0, 1) => {
            view.direction = view.direction.flip();
            FilterAction::Stay
        }
        (0, 2) => {
            view.pack_filter = view.pack_filter.next(all_packs);
            FilterAction::Stay
        }
        (0, 3) => {
            view.difficulty_filter = view.difficulty_filter.next();
            FilterAction::Stay
        }
        (1, 0) => {
            view.favorite_filter = view.favorite_filter.next();
            FilterAction::Stay
        }
        (3, 0) => FilterAction::Back,
        (3, 1) => FilterAction::Reset,
        (3, 2) => FilterAction::Apply,
        (3, 3) => FilterAction::Exit,
        _ => FilterAction::Stay,
    }
}

/// Apply the active filter + sort to `all_entries` and wrap the result
/// in a fresh paginator. Called at picker init and again any time the
/// view (sort/filter) or favorites change so the on-screen grid stays
/// in sync with the user's selections.
fn build_paginator(
    all_entries: &[ChartEntry],
    view: &PickerView,
    favs: &FavoriteBook,
) -> crate::rhythm::pagination::Paginator<ChartEntry> {
    let mut filtered = apply_filters(all_entries, view, favs);
    apply_sort(&mut filtered, view);
    crate::rhythm::pagination::Paginator::new(filtered, CHART_CELLS_PER_PAGE)
}

/// Run the picker.
///
/// Tapping an occupied cell focuses it and starts a looping audio preview;
/// tapping it again re-execs the current binary with `play <chart>` so the
/// rhythm event-loop starts fresh (avoiding winit's "one EventLoop per
/// process" constraint on some platforms). Tapping a *different* occupied
/// cell switches the preview. Back-cell (3,3) or ESC exits without
/// launching anything.
pub fn pick(
    dir: &Path,
    difficulty: &str,
    user_offset_ms: i32,
    mute_sfx: bool,
    sfx_volume: Option<f32>,
    backgrounds: Vec<PathBuf>,
    background_index: Option<usize>,
) -> Result<()> {
    let entries = scan(dir)?;
    if entries.is_empty() {
        return Err(Error::Config(format!(
            "no *.memon files found under {}",
            dir.display()
        )));
    }
    tracing::info!(
        target: "juballer::rhythm::picker",
        "{} charts available in {}",
        entries.len(),
        dir.display()
    );

    // Persistent view state (sort + filters) and favorites book. View
    // changes in the filter sub-screen are written back here on APPLY;
    // favorites toggle on long-hold of NEXT.
    let mut view = PickerView::load_default().unwrap_or_default();
    let mut favs = FavoriteBook::load_default().unwrap_or_default();
    let all_packs = discover_packs(&entries);
    let all_entries: Vec<ChartEntry> = entries; // keep the unfiltered library
                                                // Library can exceed the 12-slot grid; wrap in a paginator and reserve
                                                // (3,0)=PREV, (3,1)=NEXT for navigation on top of the existing
                                                // (3,2)=PLAY / (3,3)=EXIT buttons.
    let mut paginator = build_paginator(&all_entries, &view, &favs);

    let mut app = App::builder()
        .title("juballer — chart select")
        .present_mode(PresentMode::Fifo)
        .bg_color(Color::BLACK)
        .controller_vid_pid(0x1973, 0x0011)
        .build()?;
    app.set_debug(false);

    let mut overlay = EguiOverlay::new();
    // CLI-supplied difficulty is the *fallback default* when the focused
    // chart doesn't expose that difficulty (so single-diff charts still
    // work out-of-the-box). Per-chart cycling via the PLAY cell overrides
    // this at commit time.
    let exec_default_diff = difficulty.to_string();
    let exec_offset = user_offset_ms;
    let exec_mute_sfx = mute_sfx;
    let exec_sfx_volume = sfx_volume;
    let exec_exe =
        std::env::current_exe().map_err(|e| Error::Config(format!("current_exe: {e}")))?;

    // Load best scores per entry once up front, keyed by global index into
    // the paginator's full item list. Missing book = all None.
    let book = ScoreBook::load_default().unwrap_or_else(|e| {
        tracing::warn!(
            target: "juballer::rhythm::picker",
            "score book load failed: {e}"
        );
        ScoreBook::new()
    });
    let best_scores: Vec<Option<u64>> = (0..paginator.total())
        .map(|i| {
            let e = paginator.items_on_page(i / CHART_CELLS_PER_PAGE);
            let entry = &e[i % CHART_CELLS_PER_PAGE];
            book.best(&entry.path, difficulty).map(|r| r.score)
        })
        .collect();

    // Restore "last played" chart focus + page when present. Set by the
    // picker's exec into a song; consumed (unset) here so it only fires
    // for the immediate post-song return.
    let last_chart_env = std::env::var("JUBALLER_LAST_CHART").ok();
    if last_chart_env.is_some() {
        std::env::remove_var("JUBALLER_LAST_CHART");
    }
    let initial_focus_idx_global: Option<usize> = last_chart_env.as_deref().and_then(|p| {
        let target = std::path::Path::new(p);
        paginator.items().iter().position(|e| e.path == target)
    });
    if let Some(global_idx) = initial_focus_idx_global {
        let page = global_idx / CHART_CELLS_PER_PAGE;
        paginator.jump_to(page);
        tracing::info!(
            target: "juballer::rhythm::picker",
            "restored focus to last-played chart at global idx {global_idx} (page {page})"
        );
    }
    // `state` tracks focus + selected difficulty. `preview` is the live
    // audio sink; dropping it kills playback. Both are captured by the
    // `FnMut` closure below so they survive across frames.
    let mut state = PickerState {
        focused: initial_focus_idx_global.map(|gi| gi % CHART_CELLS_PER_PAGE),
        selected_diff_idx: 0,
    };
    let mut mode: PickerMode = PickerMode::Browse;
    // Hold-detection state. press_at is set on KeyDown; the per-frame
    // tick fires the hold action the instant elapsed crosses
    // HOLD_THRESHOLD_MS, then sets the corresponding *_fired flag so
    // KeyUp can distinguish "you held it" from "you tapped it".
    //
    // PREV hold → enter filter mode. NEXT hold → nothing (page nav only).
    // Chart-tile hold → toggle favorite on that tile.
    let mut prev_held_at: Option<Instant> = None;
    let mut prev_hold_fired = false;
    let mut chart_held_at: Option<(u8, u8, Instant)> = None;
    let mut chart_hold_fired = false;
    // Transient "★ added"/"★ removed" toast — set when favorites
    // toggles; HUD reads to flash a confirmation for ~1.2s.
    let mut fav_toast: Option<(Instant, bool)> = None;
    // One spectrum shared across every preview spin-up — its ring just
    // accumulates whatever sink is currently playing. Dropped previews
    // stop pushing new samples; new ones replace the data stream.
    let preview_spectrum = super::spectrum::SharedSpectrum::new();
    // One long-lived audio stream/handle for the whole picker run.
    // Reusing this across previews avoids the ~10-30 ms cold-open cost
    // per chart switch. Sinks are still per-preview (Sink::stop drops
    // queued audio on switch).
    let (_preview_stream, preview_handle) = OutputStream::try_default()
        .map_err(|e| Error::Config(format!("preview: no output device: {e}")))?;
    let mut preview: Option<PreviewPlayer> = None;
    let mut jackets = JacketCache::new();
    // HUD background plumbing — shader cache + image cache, same two
    // paths the rhythm loop uses. Picker builds `bg_inputs` from the
    // focused chart (or defaults when nothing focused) and draws the
    // background behind draw_overlay.
    let mut shader_cache = crate::shader::ShaderPipelineCache::new();
    let mut bg_img_cache = super::background::BackgroundImageCache::new();
    let boot = std::time::Instant::now();
    let mut last_frame = std::time::Instant::now();

    app.run(move |frame, events| {
        // Advance any active page transition so finished animations clear
        // cleanly before we process new input.
        paginator.tick();
        // Async preview: pull-poll the worker thread; flips Loading →
        // Playing the moment a buffer arrives. No-op when there's no
        // preview or the sink is already playing.
        if let Some(p) = preview.as_mut() {
            p.poll();
        }

        let current_entries = paginator.current_items().to_vec();
        let page_offset = paginator.current_page() * CHART_CELLS_PER_PAGE;
        let page_best_scores: Vec<Option<u64>> = (0..current_entries.len())
            .map(|i| best_scores.get(page_offset + i).copied().flatten())
            .collect();

        paint_backgrounds(
            frame,
            &current_entries,
            state.focused,
            paginator.current_page(),
            paginator.page_count(),
        );

        // Background for the picker's top-region preview header. Picks
        // from the user's `backgrounds` list using the focused chart's
        // path when something is focused, else the first entry's path
        // as a stable idle state. Shader / image mode split mirrors
        // play_chart_inner's handling — shader goes raw into the top
        // rect, image paints inside draw_overlay via bg_img_cache.
        // Hash the audio path, not the chart path: gameplay
        // (rhythm/mod.rs) keys `pick_for_chart` on `state.chart.audio_path`,
        // so the picker preview must do the same or the shader you see
        // hovering an entry won't match what you get once the song starts.
        let bg_key_path = state
            .focused
            .and_then(|i| current_entries.get(i))
            .map(|e| e.audio_path.clone())
            .or_else(|| backgrounds.first().cloned());
        let background = bg_key_path
            .as_deref()
            .and_then(|p| super::background::pick_for_chart(p, &backgrounds, background_index));
        let boot_secs = boot.elapsed().as_secs_f32();
        let dt = {
            let now = std::time::Instant::now();
            let d = now.duration_since(last_frame).as_secs_f32();
            last_frame = now;
            d
        };
        let focused_entry = state.focused.and_then(|i| current_entries.get(i));
        let bg_inputs = super::background::BackgroundInputs {
            bpm: focused_entry.map(|e| e.bpm).unwrap_or(120.0),
            // Live FFT of whatever the preview sink is playing; zero-
            // filled when no preview is active. Same plumbing as the
            // gameplay loop — the shader gets real audio bins, not the
            // synth fallback.
            spectrum: preview_spectrum.snapshot(),
            ..Default::default()
        };
        if let Some(bg) = &background {
            if matches!(bg, super::background::Background::Shader(_)) {
                let top_rect = frame.top_region_rect();
                super::background::draw_shader(
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

        // Browse vs Filter mode: each draws its own grid. They never
        // both run in the same frame — Filter completely replaces the
        // chart-cell layout, no scrim/overlay shenanigans, the
        // background shader keeps rendering underneath either way.
        match mode {
            PickerMode::Browse => draw_overlay(
                frame,
                &mut overlay,
                &current_entries,
                state,
                &page_best_scores,
                &mut jackets,
                paginator.current_page(),
                paginator.page_count(),
                paginator.transition().copied(),
                background.clone(),
                &mut bg_img_cache,
                &favs,
                fav_toast,
            ),
            PickerMode::Filter => {
                draw_filter_overlay(frame, &mut overlay, &view, &all_packs);
            }
        }

        // Per-frame hold tick — fires the hold action the instant the
        // press timer crosses HOLD_THRESHOLD_MS so the screen flips
        // *while* the player is still holding (rather than on release,
        // which felt laggy and unresponsive).
        let hold_threshold = Duration::from_millis(HOLD_THRESHOLD_MS);
        if mode == PickerMode::Browse {
            if !prev_hold_fired {
                if let Some(t0) = prev_held_at {
                    if t0.elapsed() >= hold_threshold {
                        prev_hold_fired = true;
                        tracing::info!(target: "juballer::rhythm::picker",
                            "PREV hold → entering Filter mode");
                        mode = PickerMode::Filter;
                    }
                }
            }
            // Chart-tile hold → toggle favorite. Short tap already did
            // whatever it does (focus / launch) on KeyDown; this fires
            // additionally after HOLD_THRESHOLD_MS, feeling like a
            // separate gesture on the same cell.
            if !chart_hold_fired {
                if let Some((r, c, t0)) = chart_held_at {
                    if t0.elapsed() >= hold_threshold {
                        chart_hold_fired = true;
                        let cell_idx = (r as usize) * 4 + (c as usize);
                        if cell_idx < CHART_CELLS_PER_PAGE {
                            if let Some(entry) = current_entries.get(cell_idx) {
                                let now_fav = favs.toggle(&entry.path);
                                if let Err(e) = favs.save_default() {
                                    tracing::warn!(target: "juballer::rhythm::picker",
                                        "favorites save failed: {e}");
                                }
                                fav_toast = Some((Instant::now(), now_fav));
                                tracing::info!(target: "juballer::rhythm::picker",
                                    "favorite {} → {}",
                                    entry.path.display(),
                                    if now_fav { "added" } else { "removed" });
                                if !now_fav
                                    && matches!(view.favorite_filter, FavoriteFilter::OnlyFavs)
                                {
                                    paginator = build_paginator(&all_entries, &view, &favs);
                                    state = PickerState::default();
                                }
                            }
                        }
                    }
                }
            }
        }

        for ev in events {
            match ev {
                Event::KeyDown { row, col, .. } => {
                    let idx = (*row as usize) * 4 + (*col as usize);
                    // ── Filter mode: route to the sub-screen handler.
                    if mode == PickerMode::Filter {
                        match handle_filter_press(*row, *col, &mut view, &all_packs) {
                            FilterAction::Stay => {}
                            FilterAction::Apply => {
                                let _ = view.save_default();
                                paginator = build_paginator(&all_entries, &view, &favs);
                                drop(preview.take());
                                state = PickerState::default();
                                mode = PickerMode::Browse;
                            }
                            FilterAction::Back => {
                                view = PickerView::load_default().unwrap_or_default();
                                mode = PickerMode::Browse;
                            }
                            FilterAction::Reset => {
                                view = PickerView::default();
                            }
                            FilterAction::Exit => {
                                drop(preview.take());
                                super::exit::exit(0);
                            }
                        }
                        continue;
                    }
                    // ── Browse mode: PREV starts a hold timer (enter
                    // filter mode on ≥500 ms hold). NEXT is tap-only
                    // (page nav). Chart cells also start a hold timer
                    // for favorite toggle.
                    if idx == PREV_CELL_IDX {
                        prev_held_at = Some(Instant::now());
                        prev_hold_fired = false;
                        continue;
                    }
                    if idx == NEXT_CELL_IDX {
                        let from = paginator.current_page();
                        let started =
                            paginator.next_page(crate::rhythm::pagination::DEFAULT_TRANSITION_MS);
                        tracing::info!(
                            target: "juballer::rhythm::picker",
                            "NEXT tap: from={from} started={started} → page={}",
                            paginator.current_page()
                        );
                        if started {
                            drop(preview.take());
                            state = PickerState::default();
                        }
                        continue;
                    }
                    // Chart cell: start hold timer ONLY. The
                    // focus/cycle action is deferred to KeyUp so a
                    // long-press for favorite doesn't *also* flip the
                    // selection. PLAY (3,2) + EXIT (3,3) still fire on
                    // KeyDown — they're not held for any alt action.
                    if idx < CHART_CELLS_PER_PAGE {
                        chart_held_at = Some((*row, *col, Instant::now()));
                        chart_hold_fired = false;
                        continue;
                    }
                    let diff_count =
                        |i: usize| current_entries.get(i).map(|e| e.difficulties.len());
                    let (action, next_state) = press_cell(state, *row, *col, diff_count);
                    state = next_state;
                    match action {
                        PickerAction::Exit => {
                            // Dropping `preview` before exit stops the audio
                            // callback thread cleanly; `std::process::exit`
                            // otherwise bypasses destructors.
                            drop(preview.take());
                            super::exit::exit(0);
                        }
                        PickerAction::Ignore => {
                            // No-op: empty cell, orphan PLAY, etc.
                        }
                        PickerAction::Cycle => {
                            if let Some(fidx) = state.focused {
                                if let Some(entry) = current_entries.get(fidx) {
                                    let diff = entry
                                        .difficulties
                                        .get(state.selected_diff_idx)
                                        .map(String::as_str)
                                        .unwrap_or("?");
                                    tracing::info!(
                                        target: "juballer::rhythm::picker",
                                        "cycled [{}] {} → difficulty [{}] {}",
                                        fidx,
                                        entry.path.display(),
                                        state.selected_diff_idx,
                                        diff,
                                    );
                                }
                            }
                        }
                        PickerAction::Focus { idx } => {
                            let entry = match current_entries.get(idx) {
                                Some(e) => e,
                                None => continue,
                            };
                            // Drop the previous player first so the outgoing
                            // sink stops immediately. PreviewPlayer::start
                            // is async — returns instantly; audio comes in
                            // once the worker thread finishes decoding
                            // (~50-150 ms typical).
                            drop(preview.take());
                            preview = Some(PreviewPlayer::start(
                                &entry.audio_path,
                                entry.preview,
                                Some(preview_spectrum.clone()),
                                preview_handle.clone(),
                            ));
                            tracing::info!(
                                target: "juballer::rhythm::picker",
                                "focused [{}] {} — preview decoding",
                                idx,
                                entry.path.display()
                            );
                        }
                        PickerAction::Launch { idx, diff_idx } => {
                            let entry = match current_entries.get(idx) {
                                Some(e) => e,
                                None => continue,
                            };
                            // Drop preview before exec so the audio fd isn't
                            // inherited by the replacement process.
                            drop(preview.take());
                            let diff = entry
                                .difficulties
                                .get(diff_idx)
                                .cloned()
                                .unwrap_or_else(|| exec_default_diff.clone());
                            tracing::info!(
                                target: "juballer::rhythm::picker",
                                "selected [{}] {} @ {}",
                                idx,
                                entry.path.display(),
                                diff,
                            );
                            // exec() replaces the process — nothing to clean
                            // up after this call returns (which on success
                            // it won't).
                            //
                            // Override the inherited RETURN_TO env so the
                            // song's exit lands back in the picker (this
                            // very pick() invocation, freshly re-exec'd)
                            // with the same chart pre-focused. Pre-focus
                            // is wired via JUBALLER_LAST_CHART, read at
                            // picker init.
                            let mut cmd = std::process::Command::new(&exec_exe);
                            cmd.arg("play")
                                .arg(&entry.path)
                                .arg("--difficulty")
                                .arg(&diff)
                                .arg("--audio-offset-ms")
                                .arg(exec_offset.to_string())
                                .env("JUBALLER_RETURN_TO", "picker")
                                .env("JUBALLER_LAST_CHART", entry.path.to_string_lossy().as_ref());
                            if exec_mute_sfx {
                                cmd.arg("--mute-sfx");
                            }
                            if let Some(v) = exec_sfx_volume {
                                cmd.arg("--sfx-volume").arg(format!("{v}"));
                            }
                            let err = cmd.exec();
                            tracing::error!(
                                target: "juballer::rhythm::picker",
                                "exec failed: {err}"
                            );
                            std::process::exit(1);
                        }
                    }
                }
                Event::KeyUp { row, col, .. } => {
                    let idx = (*row as usize) * 4 + (*col as usize);
                    if idx == PREV_CELL_IDX {
                        let was_pressed = prev_held_at.take().is_some();
                        let fired = prev_hold_fired;
                        prev_hold_fired = false;
                        // Hold already fired on the per-frame tick — the
                        // KeyUp is just cleanup. Otherwise (short tap)
                        // do the page nav now.
                        if was_pressed && !fired && mode == PickerMode::Browse {
                            let from = paginator.current_page();
                            let started = paginator
                                .prev_page(crate::rhythm::pagination::DEFAULT_TRANSITION_MS);
                            tracing::info!(
                                target: "juballer::rhythm::picker",
                                "PREV tap: from={from} started={started} → page={}",
                                paginator.current_page()
                            );
                            if started {
                                drop(preview.take());
                                state = PickerState::default();
                            }
                        }
                        continue;
                    }
                    // Chart cell release. If the hold action (favorite
                    // toggle) didn't fire, this was a short tap → run
                    // the focus/cycle dispatch now. Otherwise we were
                    // holding for fav and the tap should NOT also flip
                    // the selection.
                    if idx < CHART_CELLS_PER_PAGE {
                        let was_held = chart_held_at.take().is_some();
                        let fired = chart_hold_fired;
                        chart_hold_fired = false;
                        if was_held && !fired {
                            // Same dispatch as the KeyDown PLAY/EXIT
                            // branch — duplicated inline because
                            // PickerAction::Launch's exec() and
                            // PickerAction::Exit's process replacement
                            // are divergent calls that can't easily be
                            // factored into a closure that returns.
                            let diff_count =
                                |i: usize| current_entries.get(i).map(|e| e.difficulties.len());
                            let (action, next_state) = press_cell(state, *row, *col, diff_count);
                            state = next_state;
                            match action {
                                PickerAction::Exit => {
                                    drop(preview.take());
                                    super::exit::exit(0);
                                }
                                PickerAction::Ignore => {}
                                PickerAction::Cycle => {
                                    if let Some(fidx) = state.focused {
                                        if let Some(entry) = current_entries.get(fidx) {
                                            let diff = entry
                                                .difficulties
                                                .get(state.selected_diff_idx)
                                                .map(String::as_str)
                                                .unwrap_or("?");
                                            tracing::info!(
                                                target: "juballer::rhythm::picker",
                                                "cycled [{}] {} → difficulty [{}] {}",
                                                fidx,
                                                entry.path.display(),
                                                state.selected_diff_idx,
                                                diff,
                                            );
                                        }
                                    }
                                }
                                PickerAction::Focus { idx } => {
                                    let entry = match current_entries.get(idx) {
                                        Some(e) => e,
                                        None => continue,
                                    };
                                    drop(preview.take());
                                    preview = Some(PreviewPlayer::start(
                                        &entry.audio_path,
                                        entry.preview,
                                        Some(preview_spectrum.clone()),
                                        preview_handle.clone(),
                                    ));
                                    tracing::info!(
                                        target: "juballer::rhythm::picker",
                                        "focused [{}] {} — preview decoding",
                                        idx,
                                        entry.path.display()
                                    );
                                }
                                PickerAction::Launch { idx, diff_idx } => {
                                    let entry = match current_entries.get(idx) {
                                        Some(e) => e,
                                        None => continue,
                                    };
                                    drop(preview.take());
                                    let diff = entry
                                        .difficulties
                                        .get(diff_idx)
                                        .cloned()
                                        .unwrap_or_else(|| exec_default_diff.clone());
                                    tracing::info!(
                                        target: "juballer::rhythm::picker",
                                        "selected [{}] {} @ {}",
                                        idx,
                                        entry.path.display(),
                                        diff,
                                    );
                                    let mut cmd = std::process::Command::new(&exec_exe);
                                    cmd.arg("play")
                                        .arg(&entry.path)
                                        .arg("--difficulty")
                                        .arg(&diff)
                                        .arg("--audio-offset-ms")
                                        .arg(exec_offset.to_string())
                                        .env("JUBALLER_RETURN_TO", "picker")
                                        .env(
                                            "JUBALLER_LAST_CHART",
                                            entry.path.to_string_lossy().as_ref(),
                                        );
                                    if exec_mute_sfx {
                                        cmd.arg("--mute-sfx");
                                    }
                                    if let Some(v) = exec_sfx_volume {
                                        cmd.arg("--sfx-volume").arg(format!("{v}"));
                                    }
                                    let err = cmd.exec();
                                    tracing::error!(
                                        target: "juballer::rhythm::picker",
                                        "exec failed: {err}"
                                    );
                                    std::process::exit(1);
                                }
                            }
                        }
                    }
                }
                Event::Unmapped { key, .. } if key.0 == "NAMED_Escape" => {
                    drop(preview.take());
                    super::exit::exit(0);
                }
                Event::Quit => {
                    drop(preview.take());
                    super::exit::exit(0);
                }
                _ => {}
            }
        }
    })?;
    Ok(())
}

fn paint_backgrounds(
    frame: &mut Frame,
    entries: &[ChartEntry],
    focused: Option<usize>,
    _current_page: usize,
    page_count: usize,
) {
    // Cell background palette. Nav cells (PREV/NEXT) light up when there's
    // a sibling page in that direction. With wrap-around enabled, that's
    // equivalent to "more than one page exists".
    let occupied = Color::rgba(0x22, 0x26, 0x32, 0xff);
    let focused_tint = Color::rgba(0x2c, 0x4a, 0x78, 0xff);
    let empty = Color::rgba(0x10, 0x10, 0x1a, 0xff);
    let exit_tint = Color::rgba(0x40, 0x10, 0x14, 0xff);
    let play_armed_tint = Color::rgba(0x18, 0x50, 0x24, 0xff);
    let play_idle_tint = Color::rgba(0x18, 0x20, 0x18, 0xff);
    let nav_armed_tint = Color::rgba(0x20, 0x38, 0x50, 0xff);
    let nav_idle_tint = Color::rgba(0x18, 0x1c, 0x24, 0xff);

    // Wrap-around nav: PREV/NEXT are armed whenever there's more than one
    // page, because hitting them on the edge now wraps instead of no-op.
    let has_prev = page_count > 1;
    let has_next = page_count > 1;

    for r in 0..4u8 {
        for c in 0..4u8 {
            let idx = (r as usize) * 4 + c as usize;
            let color = if idx == EXIT_CELL_IDX {
                exit_tint
            } else if idx == PLAY_CELL_IDX {
                if focused.is_some() {
                    play_armed_tint
                } else {
                    play_idle_tint
                }
            } else if idx == PREV_CELL_IDX {
                if has_prev {
                    nav_armed_tint
                } else {
                    nav_idle_tint
                }
            } else if idx == NEXT_CELL_IDX {
                if has_next {
                    nav_armed_tint
                } else {
                    nav_idle_tint
                }
            } else if Some(idx) == focused {
                focused_tint
            } else if idx < entries.len() {
                occupied
            } else {
                empty
            };
            frame.grid_cell(r, c).fill(color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN_MEMON: &str = r#"{
        "version": "1.0.0",
        "metadata": {"title":"T","artist":"A","audio":"x.ogg"},
        "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":140}]},
        "data": {"BSC": {"level": 1, "notes": [{"n":0,"t":0}]}, "ADV": {"level": 5, "notes": []}}
    }"#;

    #[test]
    fn scan_reads_title_artist_bpm_and_difficulties() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.memon"), MIN_MEMON).unwrap();
        std::fs::write(tmp.path().join("b.memon"), MIN_MEMON).unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "not a chart").unwrap();
        let entries = scan(tmp.path()).unwrap();
        assert_eq!(entries.len(), 2); // .txt ignored
        assert_eq!(entries[0].title, "T");
        assert_eq!(entries[0].artist, "A");
        assert!((entries[0].bpm - 140.0).abs() < 1e-6);
        // Difficulties come through in the memon's insertion order.
        assert_eq!(entries[0].difficulties, vec!["BSC", "ADV"]);
        // Note count is taken from BSC when present.
        assert_eq!(entries[0].note_count, 1);
    }

    /// Sim against the user's *actual* chart library on disk. Loads
    /// the real `~/.config/juballer/rhythm/charts` dir, builds the
    /// paginator exactly as `pick()` does, and walks the next-page
    /// sequence through wrap-around. Skipped (passes) when the dir
    /// doesn't exist so CI on a clean checkout doesn't complain.
    ///
    /// Run: `cargo test -p juballer-deck --lib live_pagination -- --nocapture`
    #[test]
    fn live_pagination_simulation_against_real_charts_dir() {
        let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".config/juballer/rhythm/charts");
        if !dir.is_dir() {
            eprintln!("skip: {} not present", dir.display());
            return;
        }
        let entries = match scan(&dir) {
            Ok(e) => e,
            Err(e) => panic!("scan failed: {e}"),
        };
        eprintln!("loaded {} charts from {}", entries.len(), dir.display());
        let mut p = crate::rhythm::pagination::Paginator::new(entries, CHART_CELLS_PER_PAGE);
        eprintln!("page_count = {}", p.page_count());
        assert!(p.page_count() > 1, "need >1 page to test wrap; got 1");

        // Walk N+2 presses where N = page_count, so we cross the wrap
        // boundary at least once. Mirrors the real handler: tick before
        // each press, sleep > transition window so the prior nav has
        // cleared.
        let total = p.page_count() + 2;
        let mut visited = vec![p.current_page()];
        for i in 0..total {
            p.tick();
            std::thread::sleep(std::time::Duration::from_millis(
                crate::rhythm::pagination::DEFAULT_TRANSITION_MS as u64 + 30,
            ));
            p.tick();
            let from = p.current_page();
            let started = p.next_page(crate::rhythm::pagination::DEFAULT_TRANSITION_MS);
            let to = p.current_page();
            eprintln!(
                "press #{i}: {from} → {to} (started={started}, page_count={})",
                p.page_count()
            );
            visited.push(to);
            assert!(started, "press #{i} blocked");
        }
        // Wrap must have happened: at least one transition where new < old.
        let wrapped = visited.windows(2).any(|w| w[1] < w[0]);
        assert!(wrapped, "no wrap observed: {visited:?}");
    }

    #[test]
    fn scan_returns_all_found_charts_for_pagination() {
        // scan() itself no longer truncates; the caller wraps results in
        // a Paginator to slice per page. Cells (3,0..3) are reserved for
        // PREV / NEXT / PLAY / EXIT, leaving 12 chart cells per page.
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::write(tmp.path().join(format!("c{i:02}.memon")), MIN_MEMON).unwrap();
        }
        let entries = scan(tmp.path()).unwrap();
        assert_eq!(entries.len(), 20);
        assert_eq!(CHART_CELLS_PER_PAGE, 12);
        assert_eq!(PREV_CELL_IDX, 12);
        assert_eq!(NEXT_CELL_IDX, 13);
    }

    #[test]
    fn scan_resolves_audio_path_relative_to_chart_dir() {
        // `metadata.audio` in the memon is a bare filename; the picker
        // needs to resolve it against the *chart file's* parent so the
        // preview player can actually open it.
        let tmp = tempfile::tempdir().unwrap();
        let chart_path = tmp.path().join("a.memon");
        std::fs::write(&chart_path, MIN_MEMON).unwrap();
        let entries = scan(tmp.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].audio_path, tmp.path().join("x.ogg"));
        // No preview hint in MIN_MEMON → None, so preview_window falls
        // back to the heuristic.
        assert_eq!(entries[0].preview, None);
    }

    #[test]
    fn scan_populates_jacket_path_when_present() {
        // A chart with `metadata.jacket` should surface in ChartEntry with
        // the absolute sibling path (relative to the chart file).
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"T","artist":"A","audio":"x.ogg","jacket":"cover.png"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":140}]},
            "data": {"BSC": {"level": 1, "notes": [{"n":0,"t":0}]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.memon"), body).unwrap();
        let entries = scan(tmp.path()).unwrap();
        assert_eq!(
            entries[0].jacket_path.as_deref(),
            Some(tmp.path().join("cover.png").as_path())
        );
    }

    #[test]
    fn scan_jacket_path_none_when_absent() {
        // MIN_MEMON has no `jacket` key — ChartEntry.jacket_path must be None.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.memon"), MIN_MEMON).unwrap();
        let entries = scan(tmp.path()).unwrap();
        assert!(entries[0].jacket_path.is_none());
    }

    #[test]
    fn scan_reads_preview_metadata_when_present() {
        // Memon v1.0.0 `metadata.preview { start, duration }` (seconds)
        // should flow into ChartEntry.preview so the picker can honour
        // the chart author's choice.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"T","artist":"A","audio":"x.ogg",
                         "preview":{"start":42.5,"duration":12.0}},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":140}]},
            "data": {"BSC": {"level": 1, "notes": [{"n":0,"t":0}]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.memon"), body).unwrap();
        let entries = scan(tmp.path()).unwrap();
        let preview = entries[0].preview.expect("preview metadata parsed");
        assert!((preview.start - 42.5).abs() < 1e-9);
        assert!((preview.duration - 12.0).abs() < 1e-9);
    }

    #[test]
    fn preview_window_uses_heuristic_when_no_metadata() {
        // Given a track of L seconds and no `metadata.preview`, the
        // resolver must return (L * PREVIEW_START_PCT, PREVIEW_LEN).
        let total = Duration::from_secs(100);
        let (start, dur) = preview_window(None, Some(total));
        // 100 * 0.20 = 20.0 s.
        assert!((start.as_secs_f64() - 20.0).abs() < 1e-9);
        assert_eq!(dur, PREVIEW_LEN);
        // And for a very short song (5s): start at 1.0s.
        let (start_short, _) = preview_window(None, Some(Duration::from_secs(5)));
        assert!((start_short.as_secs_f64() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn preview_window_honours_explicit_metadata() {
        // Explicit preview wins even when a total duration is known.
        let total = Duration::from_secs(100);
        let preview = Some(Preview {
            start: 42.0,
            duration: 8.0,
        });
        let (start, dur) = preview_window(preview, Some(total));
        assert!((start.as_secs_f64() - 42.0).abs() < 1e-9);
        assert!((dur.as_secs_f64() - 8.0).abs() < 1e-9);
        // And it still wins when total is unknown (no crash on None).
        let (start2, dur2) = preview_window(preview, None);
        assert!((start2.as_secs_f64() - 42.0).abs() < 1e-9);
        assert!((dur2.as_secs_f64() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn preview_window_falls_back_to_zero_start_when_total_unknown() {
        // Some decoders can't report total_duration; we shouldn't crash
        // — fall back to (0, PREVIEW_LEN).
        let (start, dur) = preview_window(None, None);
        assert_eq!(start, Duration::ZERO);
        assert_eq!(dur, PREVIEW_LEN);
    }

    #[test]
    fn preview_window_clamps_negative_metadata_to_zero() {
        // Chart authors who set a negative start/duration (out-of-spec)
        // shouldn't break playback — clamp to zero and move on.
        let bogus = Some(Preview {
            start: -5.0,
            duration: -2.0,
        });
        let (start, dur) = preview_window(bogus, Some(Duration::from_secs(100)));
        assert_eq!(start, Duration::ZERO);
        assert_eq!(dur, Duration::ZERO);
    }

    // -------- press_cell / PickerState state-machine tests --------
    //
    // These exercise the pure decision logic without any winit / egui /
    // audio / process-exec involvement. The `diff_count_for` closure
    // stands in for `entries[i].difficulties.len()`.

    /// Fake grid helper: `None` marks an empty cell, `Some(n)` is a chart
    /// with `n` difficulties at that index. Returns a closure usable as
    /// the `diff_count_for` param.
    fn grid(slots: &[Option<usize>]) -> impl Fn(usize) -> Option<usize> + '_ {
        move |i: usize| slots.get(i).copied().flatten()
    }

    #[test]
    fn press_cell_exit_wins_from_any_state() {
        // EXIT cell (3,3) always exits, even when something is focused and
        // mid-cycle.
        let state = PickerState {
            focused: Some(2),
            selected_diff_idx: 3,
        };
        let g = grid(&[Some(3); 16]);
        let (action, next) = press_cell(state, 3, 3, &g);
        assert_eq!(action, PickerAction::Exit);
        assert_eq!(next, state); // Exit is side-effect-only, state unchanged.
    }

    #[test]
    fn press_cell_empty_cell_is_ignored_and_state_unchanged() {
        let state = PickerState::default();
        // Slot 0 is empty (None).
        let g = grid(&[None, None, None, None]);
        let (action, next) = press_cell(state, 0, 0, &g);
        assert_eq!(action, PickerAction::Ignore);
        assert_eq!(next, state);
    }

    #[test]
    fn press_cell_first_tap_on_chart_focuses_and_resets_diff_idx() {
        // Even if prior state had a stale diff idx (shouldn't normally,
        // but guard against regressions), focusing must zero it out.
        let state = PickerState {
            focused: None,
            selected_diff_idx: 5,
        };
        let slots: Vec<Option<usize>> = vec![Some(3), Some(2), Some(1)];
        let g = grid(&slots);
        let (action, next) = press_cell(state, 0, 1, &g); // idx 1
        assert_eq!(action, PickerAction::Focus { idx: 1 });
        assert_eq!(next.focused, Some(1));
        assert_eq!(next.selected_diff_idx, 0);
    }

    #[test]
    fn press_cell_same_cell_multidiff_cycles_without_launching() {
        // Chart with 3 difficulties; repeated presses on the same cell
        // advance selected_diff_idx through 0 → 1 → 2 → 0 (wrap).
        let mut state = PickerState {
            focused: Some(2),
            selected_diff_idx: 0,
        };
        let slots: Vec<Option<usize>> = vec![Some(1), Some(1), Some(3)];
        let g = grid(&slots);

        // Row 0 col 2 = idx 2.
        let (action, next) = press_cell(state, 0, 2, &g);
        assert_eq!(action, PickerAction::Cycle);
        assert_eq!(next.focused, Some(2));
        assert_eq!(next.selected_diff_idx, 1);
        state = next;

        let (action, next) = press_cell(state, 0, 2, &g);
        assert_eq!(action, PickerAction::Cycle);
        assert_eq!(next.selected_diff_idx, 2);
        state = next;

        // Third press wraps back to 0.
        let (action, next) = press_cell(state, 0, 2, &g);
        assert_eq!(action, PickerAction::Cycle);
        assert_eq!(next.selected_diff_idx, 0);
    }

    #[test]
    fn press_cell_same_cell_singlediff_launches_immediately() {
        // When the focused chart has only one difficulty there's nothing to
        // cycle through — the same-cell re-press should launch directly at
        // diff_idx = 0 rather than asking the user to go find the PLAY
        // cell for no reason.
        let state = PickerState {
            focused: Some(0),
            selected_diff_idx: 0,
        };
        let slots: Vec<Option<usize>> = vec![Some(1)];
        let g = grid(&slots);
        let (action, next) = press_cell(state, 0, 0, &g);
        assert_eq!(
            action,
            PickerAction::Launch {
                idx: 0,
                diff_idx: 0
            }
        );
        // Launch is a terminal action in practice (process exec), but the
        // pure function still returns a defined state.
        assert_eq!(next, state);
    }

    #[test]
    fn press_cell_different_chart_refocuses_and_resets_diff_idx() {
        // User is mid-cycle on chart 0 (at diff idx 2), then presses a
        // different chart. That should refocus + reset the diff idx to 0
        // — we're on a new chart with a new difficulty list.
        let state = PickerState {
            focused: Some(0),
            selected_diff_idx: 2,
        };
        let slots: Vec<Option<usize>> = vec![Some(3), Some(2), Some(4)];
        let g = grid(&slots);
        let (action, next) = press_cell(state, 0, 2, &g); // idx 2
        assert_eq!(action, PickerAction::Focus { idx: 2 });
        assert_eq!(next.focused, Some(2));
        assert_eq!(next.selected_diff_idx, 0);
    }

    #[test]
    fn press_cell_play_cell_without_focus_ignored() {
        // PLAY cell (3,2) is a no-op unless a chart is focused. Launching
        // out of nowhere would be user-hostile.
        let state = PickerState::default();
        let slots: Vec<Option<usize>> = vec![Some(2); 14];
        let g = grid(&slots);
        let (action, next) = press_cell(state, 3, 2, &g);
        assert_eq!(action, PickerAction::Ignore);
        assert_eq!(next, state);
    }

    #[test]
    fn press_cell_play_cell_with_focus_launches_at_selected_diff() {
        // Having cycled to difficulty idx 2 on chart 3, pressing PLAY
        // commits exactly that combo.
        let state = PickerState {
            focused: Some(3),
            selected_diff_idx: 2,
        };
        let slots: Vec<Option<usize>> = vec![Some(1), Some(1), Some(1), Some(3)];
        let g = grid(&slots);
        let (action, next) = press_cell(state, 3, 2, &g);
        assert_eq!(
            action,
            PickerAction::Launch {
                idx: 3,
                diff_idx: 2
            }
        );
        assert_eq!(next, state);
    }

    #[test]
    fn press_cell_does_not_focus_the_play_cell_as_a_chart() {
        // Sanity: (3,2) is reserved for PLAY even if the grid-resolver
        // accidentally claimed a chart there — press_cell treats it as a
        // commit cell unconditionally.
        let state = PickerState::default();
        // Pretend there's a "chart" at idx 14 with 2 difficulties. That
        // shouldn't happen given CHART_CELLS_PER_PAGE=14 but we want the
        // state machine to be robust against it.
        let mut slots: Vec<Option<usize>> = vec![Some(2); 14];
        slots.push(Some(2)); // idx 14 (= PLAY_CELL_IDX)
        let g = grid(&slots);
        let (action, next) = press_cell(state, 3, 2, &g);
        assert_eq!(action, PickerAction::Ignore);
        assert_eq!(next, state);
    }

    #[test]
    fn press_cell_cross_cell_press_then_cycle_on_new_cell() {
        // End-to-end sequence mirroring the intended UX:
        //   1. Tap chart A (idx 1, 3 diffs) → focus, diff_idx = 0.
        //   2. Tap chart B (idx 2, 2 diffs) → refocus, diff_idx = 0.
        //   3. Tap chart B again → cycle to diff_idx = 1.
        //   4. Tap PLAY (3,2) → launch B at diff_idx = 1.
        let slots: Vec<Option<usize>> = vec![None, Some(3), Some(2)];
        let g = grid(&slots);
        let mut state = PickerState::default();

        let (a, next) = press_cell(state, 0, 1, &g);
        assert_eq!(a, PickerAction::Focus { idx: 1 });
        state = next;

        let (a, next) = press_cell(state, 0, 2, &g);
        assert_eq!(a, PickerAction::Focus { idx: 2 });
        assert_eq!(next.selected_diff_idx, 0);
        state = next;

        let (a, next) = press_cell(state, 0, 2, &g);
        assert_eq!(a, PickerAction::Cycle);
        assert_eq!(next.selected_diff_idx, 1);
        state = next;

        let (a, _) = press_cell(state, 3, 2, &g);
        assert_eq!(
            a,
            PickerAction::Launch {
                idx: 2,
                diff_idx: 1
            }
        );
    }

    #[test]
    fn press_cell_play_cell_indexes_are_consistent() {
        // Guard against someone changing row/col conventions without
        // updating the constants.
        assert_eq!(PREV_CELL_IDX, 3 * 4);
        assert_eq!(NEXT_CELL_IDX, 3 * 4 + 1);
        assert_eq!(PLAY_CELL_IDX, 3 * 4 + 2);
        assert_eq!(EXIT_CELL_IDX, 3 * 4 + 3);
        // Chart-cell region is everything in rows 0..2 (12 cells).
        assert_eq!(CHART_CELLS_PER_PAGE, PREV_CELL_IDX);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_overlay(
    frame: &mut Frame,
    overlay: &mut EguiOverlay,
    entries: &[ChartEntry],
    state: PickerState,
    best_scores: &[Option<u64>],
    jackets: &mut JacketCache,
    current_page: usize,
    page_count: usize,
    transition: Option<crate::rhythm::pagination::Transition>,
    background: Option<super::background::Background>,
    bg_img_cache: &mut super::background::BackgroundImageCache,
    favs: &FavoriteBook,
    fav_toast: Option<(Instant, bool)>,
) {
    let focused = state.focused;
    let cell_rects = *frame.cell_rects();
    let top_rect_outer = frame.top_region_rect();

    // Horizontal slide offset for the chart-cell region during page
    // transitions. Forward nav slides incoming content in from the right
    // (+tile_w at eased=0 → 0 at eased=1); back nav from the left.
    // Nav + PLAY + EXIT cells in row 3 stay put so the user always has
    // something to tap.
    let slide_offset = transition
        .map(|t| {
            let approx_tile_w = cell_rects[0].w as f32;
            let sign = match t.direction() {
                crate::rhythm::pagination::Direction::Forward => 1.0,
                crate::rhythm::pagination::Direction::Back => -1.0,
            };
            (1.0 - t.eased()) * approx_tile_w * sign
        })
        .unwrap_or(0.0);

    // Wrap-around nav: PREV/NEXT are armed whenever there's more than one
    // page, because hitting them on the edge now wraps instead of no-op.
    let has_prev = page_count > 1;
    let has_next = page_count > 1;
    let top_rect = frame.top_region_rect();
    // Focused entry + its mini banner — used by the preview header at the
    // top of the picker region. Cloned so the closure below can own them
    // without holding a `&entries` borrow across the overlay draw.
    let focused_entry: Option<ChartEntry> = focused.and_then(|i| entries.get(i)).cloned();

    overlay.draw(frame, |rc| {
        // Image-mode background — painted at the bottom of the picker
        // overlay (below the preview header / grid overlays) so it sits
        // behind the rest of the picker UI. Shader-mode backgrounds
        // are drawn via frame.with_region_raw before this overlay runs,
        // so they stack the same way.
        if let Some(bg) = background.as_ref() {
            if matches!(bg, super::background::Background::Image(_)) {
                super::background::draw_image(rc, bg, top_rect_outer, bg_img_cache);
            }
        }
        // ── Preview header in the top region ────────────────────────────
        // Shows the focused chart's mini banner + title + artist + BPM.
        // When nothing is focused, a dim "tap to preview" hint instead.
        let hdr_id = egui::Id::new("picker_preview_header");
        egui::Area::new(hdr_id)
            .fixed_pos(egui::pos2(top_rect.x as f32, top_rect.y as f32))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(top_rect.w as f32);
                ui.set_height(top_rect.h as f32);
                let painter = ui.painter();
                let rect = ui.max_rect();
                painter.rect_filled(
                    rect,
                    egui::Rounding::same(6.0),
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
                );
                match focused_entry.as_ref() {
                    Some(entry) => {
                        use super::textfx::text_outlined;
                        // Right-side album-art slot. Square, padded into
                        // the corner. Prefer the jacket (richer art),
                        // fall back to mini-banner. The text column to
                        // the left auto-shrinks to clear the slot.
                        let art_pad = 10.0_f32;
                        let art_size = (rect.height() - 2.0 * art_pad).min(rect.width() * 0.35);
                        let art_path = entry
                            .jacket_path
                            .as_deref()
                            .or(entry.mini_path.as_deref())
                            .or(entry.banner_path.as_deref());
                        let mut text_right = rect.right() - 14.0;
                        if let Some(p) = art_path {
                            if let Some(tex) = jackets.get_or_load(rc.ctx(), p) {
                                let art_rect = egui::Rect::from_min_size(
                                    egui::pos2(
                                        rect.right() - art_pad - art_size,
                                        rect.top() + art_pad,
                                    ),
                                    egui::vec2(art_size, art_size),
                                );
                                // Cover-fit so non-square sources don't
                                // distort — crop the longer axis.
                                let sz = tex.size_vec2();
                                let ta = if sz.y > 0.0 { sz.x / sz.y } else { 1.0 };
                                let uv = if ta > 1.0 {
                                    let uw = 1.0 / ta;
                                    let u0 = (1.0 - uw) * 0.5;
                                    egui::Rect::from_min_max(
                                        egui::pos2(u0, 0.0),
                                        egui::pos2(u0 + uw, 1.0),
                                    )
                                } else {
                                    let vh = ta;
                                    let v0 = (1.0 - vh) * 0.5;
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, v0),
                                        egui::pos2(1.0, v0 + vh),
                                    )
                                };
                                painter.image(tex.id(), art_rect, uv, egui::Color32::WHITE);
                                painter.rect_stroke(
                                    art_rect,
                                    egui::Rounding::same(4.0),
                                    egui::Stroke::new(
                                        1.0,
                                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 80),
                                    ),
                                );
                                text_right = art_rect.left() - 12.0;
                            }
                        }
                        // Left column: song name on top, then artist,
                        // then BPM + notes stacked under it. All outlined
                        // for legibility over arbitrary backgrounds.
                        let left_x = rect.left() + 14.0;
                        let title_y = rect.top() + 14.0;
                        text_outlined(
                            painter,
                            egui::pos2(left_x, title_y),
                            egui::Align2::LEFT_TOP,
                            &entry.title,
                            egui::FontId::proportional(22.0),
                            egui::Color32::WHITE,
                        );
                        text_outlined(
                            painter,
                            egui::pos2(left_x, title_y + 28.0),
                            egui::Align2::LEFT_TOP,
                            &entry.artist,
                            egui::FontId::proportional(14.0),
                            egui::Color32::LIGHT_GRAY,
                        );
                        text_outlined(
                            painter,
                            egui::pos2(left_x, title_y + 52.0),
                            egui::Align2::LEFT_TOP,
                            &format!("{:.0} BPM", entry.bpm),
                            egui::FontId::proportional(16.0),
                            egui::Color32::LIGHT_YELLOW,
                        );
                        text_outlined(
                            painter,
                            egui::pos2(left_x, title_y + 74.0),
                            egui::Align2::LEFT_TOP,
                            &format!("{} notes", entry.note_count),
                            egui::FontId::monospace(13.0),
                            egui::Color32::from_rgb(180, 220, 255),
                        );
                        // ─ extra metadata row: ★ status · BEST · pack ─
                        // Pack = the immediate dir under charts/. With
                        // layout charts/<pack>/<song>/song.memon that's
                        // entry.path.parent().parent().file_name().
                        let is_fav = favs.is_favorite(&entry.path);
                        let best = focused
                            .and_then(|i| best_scores.get(i))
                            .and_then(|o| o.as_ref());
                        let pack = entry
                            .path
                            .parent()
                            .and_then(|p| p.parent())
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("(unknown)");
                        let star = if is_fav { "★" } else { "☆" };
                        let star_color = if is_fav {
                            egui::Color32::from_rgb(255, 215, 90)
                        } else {
                            egui::Color32::from_rgb(120, 130, 150)
                        };
                        text_outlined(
                            painter,
                            egui::pos2(left_x, title_y + 96.0),
                            egui::Align2::LEFT_TOP,
                            star,
                            egui::FontId::proportional(14.0),
                            star_color,
                        );
                        let best_label = match best {
                            Some(s) => format!("BEST {s}"),
                            None => "BEST —".to_string(),
                        };
                        text_outlined(
                            painter,
                            egui::pos2(left_x + 22.0, title_y + 96.0),
                            egui::Align2::LEFT_TOP,
                            &best_label,
                            egui::FontId::monospace(13.0),
                            egui::Color32::from_rgb(220, 200, 140),
                        );
                        text_outlined(
                            painter,
                            egui::pos2(left_x + 22.0, title_y + 116.0),
                            egui::Align2::LEFT_TOP,
                            &format!("pack: {pack}"),
                            egui::FontId::proportional(12.0),
                            egui::Color32::from_rgb(170, 190, 220),
                        );
                        let _ = text_right; // reserved if we add more
                                            // text-column content later
                                            // (e.g. difficulty selector).
                    }
                    None => {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "tap a chart cell to preview",
                            egui::FontId::proportional(18.0),
                            egui::Color32::from_rgb(140, 150, 170),
                        );
                    }
                }
                // Favorite-toggle confirmation toast — fades over 1.2s
                // so the player gets visual feedback when the long-hold
                // on NEXT actually fired (otherwise long-hold + no
                // change is indistinguishable from a missed input).
                if let Some((t, now_fav)) = fav_toast {
                    let age = t.elapsed().as_secs_f32();
                    if age < 1.2 {
                        let alpha = ((1.2 - age) / 1.2 * 255.0).clamp(0.0, 255.0) as u8;
                        let label = if now_fav { "★ added" } else { "☆ removed" };
                        super::textfx::text_outlined(
                            &painter,
                            rect.center_bottom() - egui::vec2(0.0, 14.0),
                            egui::Align2::CENTER_BOTTOM,
                            label,
                            egui::FontId::proportional(16.0),
                            egui::Color32::from_rgba_unmultiplied(255, 215, 90, alpha),
                        );
                    }
                }
            });

        for (idx, entry) in entries.iter().enumerate() {
            if idx >= CHART_CELLS_PER_PAGE {
                break;
            }
            let rect = cell_rects[idx];
            let is_focused = Some(idx) == focused;
            // Thumbnail path: prefer the wide banner (160×160 song-select
            // tile), fall back to the square jacket (320×320 cover). In
            // the new layout this image fills the *entire* tile behind
            // the text panel so the art is prominent; a dim overlay +
            // outlined text preserve legibility.
            let thumb_path = entry
                .banner_path
                .as_deref()
                .or(entry.jacket_path.as_deref());
            let tile_pad = 4.0_f32;
            let tile_rect = egui::Rect::from_min_size(
                egui::pos2(
                    rect.x as f32 + tile_pad + slide_offset,
                    rect.y as f32 + tile_pad,
                ),
                egui::vec2(
                    rect.w as f32 - 2.0 * tile_pad,
                    rect.h as f32 - 2.0 * tile_pad,
                ),
            );
            if let Some(thumb_path) = thumb_path {
                if let Some(tex) = jackets.get_or_load(rc.ctx(), thumb_path) {
                    let painter = rc.ctx().layer_painter(egui::LayerId::new(
                        egui::Order::Background,
                        egui::Id::new(("picker_thumb", idx, current_page)),
                    ));
                    // Cover-fit so the image fills the tile without
                    // stretching: sides or top/bottom get cropped as
                    // needed by sampling a matching UV sub-rect.
                    let sz = tex.size_vec2();
                    let tex_aspect = if sz.y > 0.0 { sz.x / sz.y } else { 1.0 };
                    let rect_aspect = tile_rect.width() / tile_rect.height().max(1.0);
                    let uv = if tex_aspect > rect_aspect {
                        let u_w = rect_aspect / tex_aspect;
                        let u0 = (1.0 - u_w) * 0.5;
                        egui::Rect::from_min_max(egui::pos2(u0, 0.0), egui::pos2(u0 + u_w, 1.0))
                    } else {
                        let v_h = tex_aspect / rect_aspect;
                        let v0 = (1.0 - v_h) * 0.5;
                        egui::Rect::from_min_max(egui::pos2(0.0, v0), egui::pos2(1.0, v0 + v_h))
                    };
                    // Mid-gray tint dims the art a touch so the text
                    // panel on top reads clearly on bright covers.
                    painter.image(
                        tex.id(),
                        tile_rect,
                        uv,
                        egui::Color32::from_rgba_unmultiplied(140, 140, 150, 255),
                    );
                    // Darkening wash — the "just barely blur" effect
                    // (no real blur pass; faked via dim + translucent
                    // panel).
                    painter.rect_filled(
                        tile_rect,
                        egui::Rounding::same(4.0),
                        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 90),
                    );
                }
            }
            let area_id = egui::Id::new(("picker_cell", idx, current_page));
            egui::Area::new(area_id)
                .fixed_pos(egui::pos2(
                    rect.x as f32 + 10.0 + slide_offset,
                    rect.y as f32 + 8.0,
                ))
                .order(egui::Order::Foreground)
                .show(rc.ctx(), |ui| {
                    // Text panel spans most of the tile width, padded.
                    // Reads as a frosted sub-window sitting on top of
                    // the art.
                    let panel_w = (rect.w as f32 - 20.0).max(60.0);
                    ui.set_width(panel_w);
                    let painter = ui.painter();
                    let anchor = ui.cursor().left_top();
                    let panel_pad = egui::vec2(8.0, 6.0);
                    let panel_rect = egui::Rect::from_min_size(
                        anchor - panel_pad,
                        egui::vec2(panel_w + 2.0 * panel_pad.x, rect.h as f32 - 20.0),
                    );
                    painter.rect_filled(
                        panel_rect,
                        egui::Rounding::same(5.0),
                        egui::Color32::from_rgba_unmultiplied(20, 22, 30, 170),
                    );
                    // ★ marker for favorited charts — top-right of the
                    // text panel so it doesn't fight the title.
                    if favs.is_favorite(&entry.path) {
                        super::textfx::text_outlined(
                            &painter,
                            egui::pos2(panel_rect.right() - 6.0, panel_rect.top() + 4.0),
                            egui::Align2::RIGHT_TOP,
                            "★",
                            egui::FontId::proportional(16.0),
                            egui::Color32::from_rgb(255, 215, 90),
                        );
                    }
                    painter.rect_stroke(
                        panel_rect,
                        egui::Rounding::same(5.0),
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 45),
                        ),
                    );
                    super::textfx::text_outlined(
                        &painter,
                        anchor + egui::vec2(0.0, 0.0),
                        egui::Align2::LEFT_TOP,
                        &entry.title,
                        egui::FontId::proportional(16.0),
                        egui::Color32::WHITE,
                    );
                    super::textfx::text_outlined(
                        &painter,
                        anchor + egui::vec2(0.0, 22.0),
                        egui::Align2::LEFT_TOP,
                        &entry.artist,
                        egui::FontId::proportional(12.0),
                        egui::Color32::from_rgb(220, 230, 240),
                    );
                    super::textfx::text_outlined(
                        &painter,
                        anchor + egui::vec2(0.0, 40.0),
                        egui::Align2::LEFT_TOP,
                        &format!("{:.0} BPM  {} notes", entry.bpm, entry.note_count),
                        egui::FontId::monospace(11.0),
                        egui::Color32::from_rgb(140, 220, 255),
                    );
                    // Difficulty list. When this cell is focused we bracket
                    // the currently-selected difficulty so the user can see
                    // what the PLAY button will launch with:
                    //
                    //     BSC [ADV] EXT       (ADV is selected)
                    let diffs_text = if is_focused {
                        entry
                            .difficulties
                            .iter()
                            .enumerate()
                            .map(|(i, d)| {
                                if i == state.selected_diff_idx {
                                    format!("[{d}]")
                                } else {
                                    d.to_string()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else {
                        entry.difficulties.join(" ")
                    };
                    let diffs_color = if is_focused {
                        egui::Color32::from_rgb(230, 240, 180)
                    } else {
                        egui::Color32::from_rgb(170, 200, 170)
                    };
                    super::textfx::text_outlined(
                        &painter,
                        anchor + egui::vec2(0.0, 58.0),
                        egui::Align2::LEFT_TOP,
                        &diffs_text,
                        egui::FontId::monospace(10.0),
                        diffs_color,
                    );
                    // Personal best line for the currently-selected difficulty.
                    let best_label = match best_scores.get(idx).copied().flatten() {
                        Some(s) => format!("BEST: {s}"),
                        None => "BEST: —".to_string(),
                    };
                    super::textfx::text_outlined(
                        &painter,
                        anchor + egui::vec2(0.0, 74.0),
                        egui::Align2::LEFT_TOP,
                        &best_label,
                        egui::FontId::monospace(11.0),
                        egui::Color32::from_rgb(220, 200, 140),
                    );
                    if is_focused {
                        // Context-sensitive hint: single-diff charts re-press
                        // to play directly, multi-diff charts re-press to
                        // cycle + commit via PLAY.
                        let hint = if entry.difficulties.len() > 1 {
                            "tap: cycle diff"
                        } else {
                            "tap to play"
                        };
                        super::textfx::text_outlined(
                            &painter,
                            anchor + egui::vec2(0.0, 92.0),
                            egui::Align2::LEFT_TOP,
                            hint,
                            egui::FontId::monospace(10.0),
                            egui::Color32::from_rgb(255, 210, 120),
                        );
                    }
                });
        }
        // PLAY (commit) cell (3,2). Label + hint flip colour based on
        // whether a chart is currently focused — i.e. whether PLAY is
        // armed.
        let play_rect = cell_rects[PLAY_CELL_IDX];
        let play_id = egui::Id::new("picker_play");
        let play_armed = focused.is_some();
        let play_label_color = if play_armed {
            egui::Color32::from_rgb(140, 240, 160)
        } else {
            egui::Color32::from_rgb(80, 100, 80)
        };
        let play_hint = if play_armed {
            if let Some(fidx) = focused {
                entries
                    .get(fidx)
                    .and_then(|e| e.difficulties.get(state.selected_diff_idx))
                    .cloned()
                    .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            "(focus a chart)".to_string()
        };
        egui::Area::new(play_id)
            .fixed_pos(egui::pos2(play_rect.x as f32, play_rect.y as f32))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(play_rect.w as f32);
                ui.set_height(play_rect.h as f32);
                let painter = ui.painter();
                let center = ui.max_rect().center();
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    "PLAY",
                    egui::FontId::proportional(22.0),
                    play_label_color,
                );
                if !play_hint.is_empty() {
                    // Clamp the hint to ~14 chars so a long difficulty
                    // label doesn't spill off the cell.
                    const MAX_HINT: usize = 14;
                    let shown = if play_hint.chars().count() > MAX_HINT {
                        let mut s: String = play_hint.chars().take(MAX_HINT - 1).collect();
                        s.push('…');
                        s
                    } else {
                        play_hint
                    };
                    painter.text(
                        center + egui::vec2(0.0, 22.0),
                        egui::Align2::CENTER_CENTER,
                        shown,
                        egui::FontId::monospace(11.0),
                        play_label_color,
                    );
                }
            });
        // PREV / NEXT nav cell labels — dimmer when there's no page in that
        // direction so the user can see the nav is currently a no-op.
        let nav_armed_color = egui::Color32::from_rgb(180, 220, 255);
        let nav_idle_color = egui::Color32::from_rgb(80, 90, 110);
        for (cell_idx, label, is_armed) in [
            (PREV_CELL_IDX, "◀ PREV", has_prev),
            (NEXT_CELL_IDX, "NEXT ▶", has_next),
        ] {
            let rect = cell_rects[cell_idx];
            let id = egui::Id::new(("picker_nav", cell_idx));
            let color = if is_armed {
                nav_armed_color
            } else {
                nav_idle_color
            };
            egui::Area::new(id)
                .fixed_pos(egui::pos2(rect.x as f32, rect.y as f32))
                .order(egui::Order::Foreground)
                .show(rc.ctx(), |ui| {
                    ui.set_width(rect.w as f32);
                    ui.set_height(rect.h as f32);
                    let painter = ui.painter();
                    let center = ui.max_rect().center();
                    painter.text(
                        center,
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(20.0),
                        color,
                    );
                    // Page counter under the arrows so the user always
                    // sees where they are in the library.
                    painter.text(
                        center + egui::vec2(0.0, 26.0),
                        egui::Align2::CENTER_CENTER,
                        format!("page {} / {}", current_page + 1, page_count),
                        egui::FontId::monospace(11.0),
                        egui::Color32::from_rgb(140, 150, 170),
                    );
                });
        }

        // Back-cell label (always (3,3)).
        let back_rect = cell_rects[EXIT_CELL_IDX];
        let back_id = egui::Id::new("picker_back");
        egui::Area::new(back_id)
            .fixed_pos(egui::pos2(back_rect.x as f32, back_rect.y as f32))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(back_rect.w as f32);
                ui.set_height(back_rect.h as f32);
                let painter = ui.painter();
                let center = ui.max_rect().center();
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    "EXIT",
                    egui::FontId::proportional(18.0),
                    egui::Color32::from_rgb(240, 120, 130),
                );
            });
    });
}

/// Filter-mode tiles. Reuses the chart-tile visual language (frosted
/// panel + outlined text, no opaque overlay) so the screen feels like
/// the same UI in a different mode rather than a popup glued on top.
/// Background shader keeps running underneath.
fn draw_filter_overlay(
    frame: &mut Frame,
    overlay: &mut EguiOverlay,
    view: &PickerView,
    all_packs: &[String],
) {
    let cell_rects = *frame.cell_rects();
    let _ = all_packs;

    struct Tile<'a> {
        label: &'a str,
        value: String,
        accent: egui::Color32,
    }
    let blue = egui::Color32::from_rgb(140, 200, 255);
    let teal = egui::Color32::from_rgb(140, 240, 200);
    let gold = egui::Color32::from_rgb(255, 220, 120);
    let red = egui::Color32::from_rgb(255, 140, 140);
    let mut tiles: [Option<Tile>; 16] = Default::default();
    tiles[0] = Some(Tile {
        label: "SORT BY",
        value: view.sort.label().into(),
        accent: blue,
    });
    tiles[1] = Some(Tile {
        label: "DIRECTION",
        value: view.direction.label().into(),
        accent: blue,
    });
    tiles[2] = Some(Tile {
        label: "PACK",
        value: view.pack_filter.label(),
        accent: blue,
    });
    tiles[3] = Some(Tile {
        label: "DIFFICULTY",
        value: view.difficulty_filter.label().into(),
        accent: blue,
    });
    tiles[4] = Some(Tile {
        label: "FAVORITES",
        value: view.favorite_filter.label().into(),
        accent: teal,
    });
    tiles[12] = Some(Tile {
        label: "BACK",
        value: "discard".into(),
        accent: red,
    });
    tiles[13] = Some(Tile {
        label: "RESET",
        value: "defaults".into(),
        accent: red,
    });
    tiles[14] = Some(Tile {
        label: "APPLY",
        value: "save".into(),
        accent: gold,
    });
    tiles[15] = Some(Tile {
        label: "EXIT",
        value: "quit".into(),
        accent: red,
    });

    overlay.draw(frame, |rc| {
        // One Area per cell — same pattern as the Browse-mode chart
        // tile loop. No scrim, no full-viewport background fill, no
        // header banner. The picker's background shader is the
        // backdrop in both modes.
        for (idx, tile_opt) in tiles.iter().enumerate() {
            let Some(tile) = tile_opt else { continue };
            let cr = cell_rects[idx];
            let area_id = egui::Id::new(("picker_filter_cell", idx));
            egui::Area::new(area_id)
                .fixed_pos(egui::pos2(cr.x as f32 + 10.0, cr.y as f32 + 8.0))
                .order(egui::Order::Foreground)
                .show(rc.ctx(), |ui| {
                    let panel_w = (cr.w as f32 - 20.0).max(60.0);
                    ui.set_width(panel_w);
                    let painter = ui.painter();
                    let anchor = ui.cursor().left_top();
                    let panel_pad = egui::vec2(8.0, 6.0);
                    let panel_rect = egui::Rect::from_min_size(
                        anchor - panel_pad,
                        egui::vec2(panel_w + 2.0 * panel_pad.x, cr.h as f32 - 20.0),
                    );
                    // Identical aesthetic to a chart tile's frosted
                    // text panel — translucent dark fill + faint
                    // white-stroke. No second pass, no overdraw.
                    painter.rect_filled(
                        panel_rect,
                        egui::Rounding::same(5.0),
                        egui::Color32::from_rgba_unmultiplied(20, 22, 30, 170),
                    );
                    painter.rect_stroke(
                        panel_rect,
                        egui::Rounding::same(5.0),
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(
                                tile.accent.r(),
                                tile.accent.g(),
                                tile.accent.b(),
                                90,
                            ),
                        ),
                    );
                    super::textfx::text_outlined(
                        &painter,
                        panel_rect.left_top() + egui::vec2(8.0, 6.0),
                        egui::Align2::LEFT_TOP,
                        tile.label,
                        egui::FontId::proportional(13.0),
                        tile.accent,
                    );
                    super::textfx::text_outlined(
                        &painter,
                        panel_rect.left_top() + egui::vec2(8.0, 26.0),
                        egui::Align2::LEFT_TOP,
                        &tile.value,
                        egui::FontId::proportional(20.0),
                        egui::Color32::WHITE,
                    );
                });
        }
    });
    // Suppress unused-import warnings until more dimensions are wired.
    let _ = (SortMode::ALL, SortDirection::Asc, DifficultyFilter::ALL);
}
