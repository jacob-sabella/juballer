//! memon v1.0.0 chart loader.
//!
//! Only the fields the rhythm game actually uses are materialised; everything
//! else (hakus, etc.) is silently ignored. Fractional `t` values (the
//! `[whole, frac_num, frac_den]` form) are resolved to
//! `whole + frac_num / frac_den` ticks and converted to ms via the
//! float-friendly [`BpmSchedule::tick_to_ms_float`] path.

use crate::{Error, Result};
use indexmap::IndexMap;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Normalized, game-ready chart data. Produced by [`load`]. All timing has already
/// been resolved (chart-specific timing wins over top-level) and notes are sorted
/// by `hit_time_ms` ascending.
#[derive(Debug, Clone)]
pub struct Chart {
    pub title: String,
    pub artist: String,
    pub audio_path: PathBuf,
    pub bpm: f64,
    pub offset_ms: f64,
    pub notes: Vec<Note>,
    /// Full BPM schedule so the HUD (and anything else that cares about the
    /// active tempo) can query the current BPM at an arbitrary music time.
    /// `bpm` above is just the initial segment's BPM, preserved for back-compat.
    pub schedule: BpmSchedule,
    /// Optional memon-provided preview window (chart picker snippet). See
    /// [`Metadata::preview`].
    pub preview: Option<Preview>,
    /// Absolute path to the jacket image referenced by `metadata.jacket`,
    /// resolved sibling-to-the-chart. `None` when the chart has no jacket
    /// or the path would be empty. The picker / HUD lazy-load + cache the
    /// texture themselves — this field is just the filesystem pointer.
    pub jacket_path: Option<PathBuf>,
    /// Wide banner image (typ. `banner.png`) — auto-detected sibling.
    /// Intended for song-select rows / fancy header treatments.
    pub banner_path: Option<PathBuf>,
    /// Small-thumbnail image (typ. `mini.png`) — auto-detected sibling.
    /// For compact / dense chart grids that can't afford a full jacket.
    pub mini_path: Option<PathBuf>,
}

/// A single note with its cell position and absolute hit time in ms from song start.
/// Long (held) notes set `length_ms > 0` — the player must hold the cell from
/// `hit_time_ms` until `hit_time_ms + length_ms`. Tap notes set `length_ms = 0`.
///
/// `tail_row` / `tail_col` are the arrow-tail cell for a long note, as decoded
/// from the memon `p` field. For tap notes and long notes without `p`, these
/// default to `row` / `col` (no arrow direction — zero-length vector in the
/// renderer, which falls back to the plain vertical tail hint).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub hit_time_ms: f64,
    pub row: u8,
    pub col: u8,
    pub length_ms: f64,
    pub tail_row: u8,
    pub tail_col: u8,
}

impl Note {
    pub fn is_long(&self) -> bool {
        self.length_ms > 0.0
    }

    pub fn release_time_ms(&self) -> f64 {
        self.hit_time_ms + self.length_ms
    }
}

// ── On-disk memon structures ────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Memon {
    pub version: String,
    pub metadata: Metadata,
    #[serde(default)]
    pub timing: Option<Timing>,
    pub data: IndexMap<String, ChartData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    pub title: String,
    /// Artist credit. Many chart dumps lack this; default to empty so the
    /// loader still accepts them.
    #[serde(default)]
    pub artist: String,
    /// Audio file (relative path). Some chart-only dumps omit it. When the
    /// string is empty or the resolved file is missing, playback runs
    /// silent (useful for chart review / practice).
    #[serde(default)]
    pub audio: String,
    /// Optional per-chart song-preview hint (memon v1.0.0 spec field). When
    /// present the chart picker uses it verbatim for its audio snippet;
    /// otherwise the picker falls back to a heuristic (see
    /// `picker::preview_window`). Both `start` and `duration` are seconds.
    #[serde(default)]
    pub preview: Option<Preview>,
    /// Optional album-art / jacket path relative to the memon file. Resolved
    /// to an absolute [`Chart::jacket_path`] by the loader. Absent or empty
    /// → no art in the picker / HUD.
    #[serde(default)]
    pub jacket: Option<String>,
}

/// memon v1.0.0 song-preview window. Units are seconds. The memon spec does
/// not mandate a default; we treat absence as "no hint, use heuristic".
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct Preview {
    pub start: f64,
    pub duration: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Timing {
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "default_resolution")]
    pub resolution: u32,
    #[serde(default)]
    pub bpms: Vec<BpmEntry>,
}

fn default_resolution() -> u32 {
    240
}

#[derive(Debug, Clone, Deserialize)]
pub struct BpmEntry {
    #[serde(default)]
    pub beat: i64,
    pub bpm: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChartData {
    #[serde(default)]
    pub level: serde_json::Value,
    #[serde(default)]
    pub resolution: Option<u32>,
    #[serde(default)]
    pub timing: Option<Timing>,
    #[serde(default)]
    pub notes: Vec<NoteRaw>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NoteRaw {
    pub n: u8,
    pub t: serde_json::Value,
    /// Length in ticks (long notes). Absent or 0 means a tap note.
    #[serde(default)]
    pub l: Option<i64>,
    /// Arrow tail-position encoding (0..5) — visual only. Only the head
    /// cell + length matter for gameplay.
    #[serde(default)]
    pub p: Option<u8>,
}

// ── Loader ─────────────────────────────────────────────────────────────────

/// Parse `path` as memon v1.0.0 and produce a normalized [`Chart`] for `difficulty`.
/// Returns `Err(Error::Config)` if the file can't be read, version mismatches, the
/// difficulty is absent, timing info is missing, or no valid notes remain.
pub fn load(path: &Path, difficulty: &str) -> Result<Chart> {
    let bytes = std::fs::read(path)?;
    let memon: Memon = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Config(format!("{}: memon parse: {e}", path.display())))?;
    if memon.version != "1.0.0" {
        return Err(Error::Config(format!(
            "{}: unsupported memon version '{}', expected 1.0.0",
            path.display(),
            memon.version
        )));
    }
    let chart_data = memon.data.get(difficulty).ok_or_else(|| {
        let keys: Vec<&str> = memon.data.keys().map(String::as_str).collect();
        Error::Config(format!(
            "{}: difficulty '{difficulty}' not present; available: {keys:?}",
            path.display()
        ))
    })?;

    // Resolution + BPM-schedule + offset precedence per spec: chart-level
    // timing wins over top-level.
    let (schedule, offset_s) = resolve_timing(&memon.timing, chart_data)?;
    let bpm = schedule.initial_bpm();
    let hit_times = build_notes(&chart_data.notes, &schedule, offset_s);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let audio_path = parent.join(&memon.metadata.audio);
    let jacket_path = resolve_jacket_path(parent, memon.metadata.jacket.as_deref());
    let banner_path = find_sibling(parent, &["banner.png", "banner.jpg", "banner.jpeg"]);
    let mini_path = find_sibling(parent, &["mini.png", "mini.jpg", "thumb.png"]);

    Ok(Chart {
        title: memon.metadata.title,
        artist: memon.metadata.artist,
        audio_path,
        bpm,
        offset_ms: offset_s * 1000.0,
        notes: hit_times,
        schedule,
        preview: memon.metadata.preview,
        jacket_path,
        banner_path,
        mini_path,
    })
}

/// First existing file among `candidates` (evaluated in order) resolved
/// sibling-to-`parent`. Used by the loader to auto-detect conventional
/// artwork filenames (banner / mini / jacket fallbacks) that most chart
/// dumps ship but don't declare in `metadata`.
fn find_sibling(parent: &Path, candidates: &[&str]) -> Option<PathBuf> {
    for name in candidates {
        let p = parent.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Resolve the optional `metadata.jacket` string to an absolute path under
/// the chart's directory. Empty strings are treated as absent so a chart
/// author can leave the key as `""` without producing a bogus path.
pub(crate) fn resolve_jacket_path(parent: &Path, jacket: Option<&str>) -> Option<PathBuf> {
    // Explicit metadata wins.
    if let Some(raw) = jacket.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(parent.join(raw));
    }
    // Fallback: auto-detect a conventional sibling file. Chart dumps
    // usually drop `jacket.png` (square cover) alongside each song without
    // declaring it in metadata.jacket. We probe a small list of standard
    // names in priority order and pick the first one that exists.
    const CANDIDATES: &[&str] = &[
        "jacket.png",
        "jacket.jpg",
        "jacket.jpeg",
        "cover.png",
        "cover.jpg",
    ];
    for name in CANDIDATES {
        let p = parent.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn resolve_timing(top: &Option<Timing>, chart: &ChartData) -> Result<(BpmSchedule, f64)> {
    // Per-difficulty timing wins ONLY if it actually carries bpms — some
    // chart dumps include a present-but-empty `timing: {}` on every
    // difficulty, and an `or(top)` chain that stopped at the empty Some
    // would error out with "timing.bpms is empty". Fall through to the
    // top-level timing whenever the per-difficulty bpms list is missing
    // or empty.
    let chart_has_bpms = chart
        .timing
        .as_ref()
        .map(|t| !t.bpms.is_empty())
        .unwrap_or(false);
    let timing = if chart_has_bpms {
        chart.timing.as_ref()
    } else {
        top.as_ref()
    }
    .ok_or_else(|| Error::Config("chart has no timing block (nor top-level)".to_string()))?;

    if timing.bpms.is_empty() {
        return Err(Error::Config("timing.bpms is empty".to_string()));
    }

    let resolution = chart
        .timing
        .as_ref()
        .filter(|t| !t.bpms.is_empty())
        .map(|t| t.resolution)
        .or(chart.resolution)
        .unwrap_or(timing.resolution);

    if resolution == 0 {
        return Err(Error::Config("resolution must be > 0".to_string()));
    }

    let schedule = BpmSchedule::new(&timing.bpms, resolution)?;
    Ok((schedule, timing.offset))
}

/// Pre-computed BPM segment table so ticks can be converted to ms in O(log n)
/// (linear scan in practice — charts rarely have more than a handful of BPM
/// changes). Each segment covers a half-open range `[start_tick, next_start)`.
#[derive(Debug, Clone)]
pub struct BpmSchedule {
    segments: Vec<BpmSegment>,
    resolution: u32,
}

#[derive(Debug, Clone, Copy)]
struct BpmSegment {
    start_tick: i64,
    bpm: f64,
    ms_per_tick: f64,
    /// Cumulative music-time in ms at `start_tick` (before applying the
    /// chart-level offset). Segment N's `ms_at_start` = segment N-1's
    /// `ms_at_start + (start_tick_N - start_tick_{N-1}) * ms_per_tick_{N-1}`.
    ms_at_start: f64,
}

impl BpmSchedule {
    /// Build a schedule from a memon `bpms` list. The list doesn't need to be
    /// sorted — we sort by `beat` ascending here. If the earliest entry isn't
    /// at beat 0, it's treated as if it were (the first BPM applies from the
    /// song start).
    pub fn new(bpms: &[BpmEntry], resolution: u32) -> Result<Self> {
        if bpms.is_empty() {
            return Err(Error::Config("bpms is empty".to_string()));
        }
        if resolution == 0 {
            return Err(Error::Config("resolution must be > 0".to_string()));
        }
        let mut sorted: Vec<BpmEntry> = bpms.to_vec();
        sorted.sort_by_key(|b| b.beat);
        // Force the first segment to start at beat 0 — otherwise ticks before
        // the earliest BPM entry would have no applicable segment.
        if sorted[0].beat != 0 {
            let first = sorted[0].clone();
            sorted.insert(
                0,
                BpmEntry {
                    beat: 0,
                    bpm: first.bpm,
                },
            );
        }
        for b in &sorted {
            if b.bpm <= 0.0 {
                return Err(Error::Config(format!("bpm must be > 0 (got {})", b.bpm)));
            }
        }
        let mut segments = Vec::with_capacity(sorted.len());
        let mut ms_acc = 0.0;
        let mut prev: Option<BpmSegment> = None;
        for b in sorted {
            let start_tick = b.beat * resolution as i64;
            let ms_per_tick = (60_000.0 / b.bpm) / (resolution as f64);
            if let Some(p) = prev {
                ms_acc = p.ms_at_start + (start_tick - p.start_tick) as f64 * p.ms_per_tick;
            }
            let seg = BpmSegment {
                start_tick,
                bpm: b.bpm,
                ms_per_tick,
                ms_at_start: ms_acc,
            };
            segments.push(seg);
            prev = Some(seg);
        }
        Ok(Self {
            segments,
            resolution,
        })
    }

    /// BPM at the start of the song — shown in the HUD.
    pub fn initial_bpm(&self) -> f64 {
        self.segments[0].bpm
    }

    pub fn resolution(&self) -> u32 {
        self.resolution
    }

    /// Convert a tick count (from song start) to ms (before offset). Walks
    /// backward through segments to find the active one for this tick. Ticks
    /// before the first segment are treated as if they fell into segment 0.
    pub fn tick_to_ms(&self, tick: i64) -> f64 {
        let seg = self
            .segments
            .iter()
            .rfind(|s| s.start_tick <= tick)
            .unwrap_or(&self.segments[0]);
        seg.ms_at_start + (tick - seg.start_tick) as f64 * seg.ms_per_tick
    }

    /// Like [`Self::tick_to_ms`] but accepts a fractional tick position —
    /// needed for memon's `[whole, frac_num, frac_den]` note-time form.
    /// Segment lookup uses the floor of `tick` so sub-tick positions stay in
    /// the same BPM segment as the whole part.
    pub fn tick_to_ms_float(&self, tick: f64) -> f64 {
        let floor_tick = tick.floor() as i64;
        let seg = self
            .segments
            .iter()
            .rfind(|s| s.start_tick <= floor_tick)
            .unwrap_or(&self.segments[0]);
        seg.ms_at_start + (tick - seg.start_tick as f64) * seg.ms_per_tick
    }

    /// BPM active at music-time `ms` (pre-offset; segment boundaries are
    /// stored relative to tick 0). Segments are half-open `[start, next_start)`
    /// — a query exactly on a boundary returns the *later* segment's BPM.
    /// Times before the first segment fall back to the initial BPM.
    pub fn bpm_at(&self, ms: f64) -> f64 {
        let seg = self
            .segments
            .iter()
            .rfind(|s| s.ms_at_start <= ms)
            .unwrap_or(&self.segments[0]);
        seg.bpm
    }
}

/// Resolve memon's `t` field to `(tick_as_f64, floor_tick_as_i64)`. Returns
/// `None` (after a `warn!`) for malformed values so the caller can skip the
/// note. Accepts both the integer form (`t: 480`) and the fractional form
/// (`t: [whole, frac_num, frac_den]` → `whole + frac_num / frac_den`).
fn parse_note_time(t: &serde_json::Value, n: u8) -> Option<(f64, i64)> {
    match t {
        serde_json::Value::Number(num) => match num.as_i64() {
            Some(v) => Some((v as f64, v)),
            None => {
                tracing::warn!(
                    target: "juballer::rhythm",
                    "note.t non-integer number at n={n}; skipping"
                );
                None
            }
        },
        serde_json::Value::Array(arr) if arr.len() == 3 => {
            let (w, frac_n, frac_d) = (arr[0].as_i64(), arr[1].as_i64(), arr[2].as_i64());
            match (w, frac_n, frac_d) {
                // Standard memon: frac_n ≥ 0 and frac_d > 0. Negative frac_n is
                // out-of-spec (every example in memon v1.0.0 uses non-negative).
                (Some(w), Some(fn_), Some(fd)) if fd > 0 && fn_ >= 0 => {
                    let tick_f = w as f64 + fn_ as f64 / fd as f64;
                    Some((tick_f, tick_f.floor() as i64))
                }
                _ => {
                    tracing::warn!(
                        target: "juballer::rhythm",
                        "note.t fractional form invalid at n={n} (got {t}); skipping"
                    );
                    None
                }
            }
        }
        _ => {
            tracing::warn!(
                target: "juballer::rhythm",
                "note.t unsupported type at n={n}; skipping"
            );
            None
        }
    }
}

/// Pick the `idx`-th value in `0..4` skipping `skip`. Used by
/// [`resolve_tail`] to enumerate the 3 cells in the head's row (or
/// column) that are *not* the head itself.
fn nth_excluding(skip: u8, idx: usize) -> u8 {
    let mut count = 0;
    for v in 0u8..4 {
        if v == skip {
            continue;
        }
        if count == idx {
            return v;
        }
        count += 1;
    }
    skip
}

/// Decode memon v1.0.0 long-note tail position from `p`.
///
/// Per the spec (`docs/source/schema.md` in Stepland/memon):
///   p ∈ 0..=2 → horizontal tail; the 3 cells in the head's row
///       excluding the head itself, ordered left→right.
///   p ∈ 3..=5 → vertical tail; the 3 cells in the head's column
///       excluding the head itself, ordered top→bottom.
fn resolve_tail(row: u8, col: u8, p: Option<u8>) -> (u8, u8) {
    match p {
        Some(p) if p <= 2 => (row, nth_excluding(col, p as usize)),
        Some(p) if p <= 5 => (nth_excluding(row, (p - 3) as usize), col),
        _ => (row, col),
    }
}

fn build_notes(raw: &[NoteRaw], schedule: &BpmSchedule, offset_s: f64) -> Vec<Note> {
    let offset_ms = offset_s * 1000.0;
    let mut out = Vec::with_capacity(raw.len());
    for nr in raw {
        if nr.n >= 16 {
            tracing::warn!(
                target: "juballer::rhythm",
                "note.n out of range (got {}); skipping",
                nr.n
            );
            continue;
        }
        let Some((t_tick_f, t_tick_floor)) = parse_note_time(&nr.t, nr.n) else {
            continue;
        };
        let hit_ms = schedule.tick_to_ms_float(t_tick_f);
        let hit_time_ms = offset_ms + hit_ms;
        // Length of a long note must be measured across whatever BPM segments
        // it spans — take the end-tick's ms minus the start-tick's ms rather
        // than a single ms_per_tick multiplication. `l` is always integer in
        // memon, so we anchor the end at floor(t) + l using the integer path.
        let length_ms = match nr.l {
            Some(l) if l > 0 => {
                schedule.tick_to_ms(t_tick_floor + l) - schedule.tick_to_ms(t_tick_floor)
            }
            _ => 0.0,
        };
        let row = nr.n / 4;
        let col = nr.n % 4;
        // Only long notes use `p`; ignore it for taps (tail stays at head so
        // the renderer gets a zero-length arrow and falls back to the plain
        // vertical tail hint).
        let (tail_row, tail_col) = if length_ms > 0.0 {
            resolve_tail(row, col, nr.p)
        } else {
            (row, col)
        };
        out.push(Note {
            hit_time_ms,
            row,
            col,
            length_ms,
            tail_row,
            tail_col,
        });
    }
    // Stable sort by time so the scheduler always sees notes in ascending order.
    out.sort_by(|a, b| a.hit_time_ms.partial_cmp(&b.hit_time_ms).unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "version": "1.0.0",
        "metadata": {"title": "T", "artist": "A", "audio": "a.ogg"},
        "timing": {
            "offset": 0.0,
            "resolution": 240,
            "bpms": [{"beat": 0, "bpm": 120}]
        },
        "data": {
            "BSC": {
                "level": 5,
                "notes": [
                    {"n": 0, "t": 0},
                    {"n": 5, "t": 480}
                ]
            }
        }
    }"#;

    fn write(tmp: &tempfile::TempDir, name: &str, body: &str) -> std::path::PathBuf {
        let p = tmp.path().join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn parse_minimal_memon() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", SAMPLE);
        let chart = load(&path, "BSC").unwrap();
        assert_eq!(chart.title, "T");
        assert_eq!(chart.artist, "A");
        assert_eq!(chart.notes.len(), 2);
        // 120 BPM => 500 ms/beat; resolution 240 => ms_per_tick = 2.0833...
        // n=0 at t=0 ticks => 0.0ms. n=5 at t=480 ticks => 480 * (500/240) = 1000.0 ms.
        assert!((chart.notes[0].hit_time_ms - 0.0).abs() < 1e-6);
        assert!((chart.notes[1].hit_time_ms - 1000.0).abs() < 1e-6);
        // n=5 => row 1 col 1.
        assert_eq!(chart.notes[1].row, 1);
        assert_eq!(chart.notes[1].col, 1);
    }

    #[test]
    fn fractional_tick_resolves_correctly() {
        // 120 BPM, resolution 240 → ms_per_tick = 500/240 ≈ 2.0833…
        // t = [480, 1, 2] → tick = 480.5 → ms ≈ 480.5 * 500/240 ≈ 1001.04166…
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title": "T", "artist": "A", "audio": "a.ogg"},
            "timing": {"offset": 0, "resolution": 240, "bpms": [{"beat": 0, "bpm": 120}]},
            "data": {"BSC": {"level": 1, "notes": [{"n": 0, "t": [480, 1, 2]}]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        assert_eq!(chart.notes.len(), 1);
        let expected = 480.5 * (500.0 / 240.0);
        assert!(
            (chart.notes[0].hit_time_ms - expected).abs() < 1e-6,
            "got {}, expected {expected}",
            chart.notes[0].hit_time_ms
        );
    }

    #[test]
    fn fractional_tick_zero_denominator_skipped_with_warn() {
        // Zero denominator must not crash (no div-by-zero, no panic); note is
        // skipped with a warn so the rest of the chart still loads.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title": "T", "artist": "A", "audio": "a.ogg"},
            "timing": {"offset": 0, "resolution": 240, "bpms": [{"beat": 0, "bpm": 120}]},
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 0, "t": [480, 1, 0]},
                {"n": 3, "t": 60}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        // Bad fractional skipped, integer note survives.
        assert_eq!(chart.notes.len(), 1);
        assert_eq!(chart.notes[0].row, 0);
        assert_eq!(chart.notes[0].col, 3);
    }

    #[test]
    fn rejects_wrong_version() {
        let body =
            r#"{"version":"0.3.0","metadata":{"title":"t","artist":"a","audio":"x"},"data":{}}"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let err = load(&path, "BSC").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("1.0.0"), "{msg}");
    }

    #[test]
    fn missing_difficulty_error_lists_available() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", SAMPLE);
        let err = load(&path, "EXT").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("BSC"), "{msg}");
    }

    #[test]
    fn sample_chart_loads() {
        // The packaged sample under assets/sample/ doubles as a smoke check for
        // the chart format — if this test breaks, the file is malformed.
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sample/test.memon");
        let chart = load(&path, "BSC").expect("sample chart loads");
        assert!(!chart.notes.is_empty(), "sample has notes");
        assert!((chart.bpm - 120.0).abs() < 0.001);
        assert_eq!(chart.title, "Sine Tone Sample");
        // All cells stay within the 4×4 grid.
        for n in &chart.notes {
            assert!(n.row < 4 && n.col < 4, "out-of-grid note {n:?}");
        }
    }

    #[test]
    fn parses_long_note_length_field() {
        // 120 BPM, resolution 240 → 1 beat = 240 ticks = 500 ms; 1 tick ≈ 2.083 ms.
        // l=480 ticks → 1000 ms hold. l=0 / missing → tap (length_ms = 0).
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 0, "t": 0},
                {"n": 5, "t": 240, "l": 480, "p": 2},
                {"n": 10, "t": 1200, "l": 0}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        assert_eq!(chart.notes.len(), 3);
        // Note 0: tap.
        assert_eq!(chart.notes[0].length_ms, 0.0);
        assert!(!chart.notes[0].is_long());
        // Note 1: long with 480-tick length → 1000 ms.
        assert!((chart.notes[1].length_ms - 1000.0).abs() < 1e-6);
        assert!(chart.notes[1].is_long());
        assert!((chart.notes[1].release_time_ms() - 1500.0).abs() < 1e-6);
        // Note 2: l=0 explicit → tap.
        assert_eq!(chart.notes[2].length_ms, 0.0);
    }

    #[test]
    fn bpm_changes_shift_later_note_times() {
        // Resolution 240. BPM 120 from beat 0 (500 ms/beat), BPM 240 from
        // beat 4 (250 ms/beat). Notes:
        //   t=0        → 0 ms        (beat 0 at 120)
        //   t=240*4    → 2000 ms     (beat 4 boundary)
        //   t=240*4+240 → 2250 ms    (1 beat into the 240 BPM segment)
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {
                "offset": 0,
                "resolution": 240,
                "bpms": [{"beat": 0, "bpm": 120}, {"beat": 4, "bpm": 240}]
            },
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 0, "t": 0},
                {"n": 1, "t": 960},
                {"n": 2, "t": 1200}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        assert!((chart.notes[0].hit_time_ms - 0.0).abs() < 1e-6);
        assert!((chart.notes[1].hit_time_ms - 2000.0).abs() < 1e-6);
        assert!((chart.notes[2].hit_time_ms - 2250.0).abs() < 1e-6);
        // Initial BPM stays 120 in the chart struct for the HUD.
        assert!((chart.bpm - 120.0).abs() < 1e-6);
    }

    #[test]
    fn long_note_length_spans_bpm_change() {
        // Long note starts at tick 720 (1.5 s @ 120) and runs 720 ticks.
        // First 240 ticks @ 120 = 500ms; next 480 ticks @ 240 = 500ms.
        // Total length_ms should be ~1000ms, NOT naive 1500 or 750.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {
                "offset": 0,
                "resolution": 240,
                "bpms": [{"beat": 0, "bpm": 120}, {"beat": 4, "bpm": 240}]
            },
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 0, "t": 720, "l": 720}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        assert!((chart.notes[0].hit_time_ms - 1500.0).abs() < 1e-6);
        assert!(
            (chart.notes[0].length_ms - 1000.0).abs() < 1e-6,
            "got {}",
            chart.notes[0].length_ms
        );
    }

    #[test]
    fn bpm_at_returns_segment_bpm_with_boundary_convention() {
        // Pick BPMs that yield an exact float boundary: BPM 125 at resolution 240
        // gives exactly 2.0 ms/tick, so beat 4 → tick 960 → boundary at 1920.0 ms.
        // Second segment runs at 250 BPM from beat 4 onward.
        let bpms = vec![
            BpmEntry {
                beat: 0,
                bpm: 125.0,
            },
            BpmEntry {
                beat: 4,
                bpm: 250.0,
            },
        ];
        let sched = BpmSchedule::new(&bpms, 240).unwrap();
        let boundary_ms = sched.tick_to_ms(4 * 240); // exact boundary in ms
        assert!((boundary_ms - 1920.0).abs() < 1e-9);

        // First-segment query returns segment 0's BPM.
        assert!((sched.bpm_at(0.0) - 125.0).abs() < 1e-9);
        assert!((sched.bpm_at(1000.0) - 125.0).abs() < 1e-9);
        // Query after the second segment boundary returns segment 1's BPM.
        assert!((sched.bpm_at(boundary_ms + 100.0) - 250.0).abs() < 1e-9);
        // Exactly on the boundary → later segment wins (half-open [start, next_start)).
        assert!((sched.bpm_at(boundary_ms) - 250.0).abs() < 1e-9);
        // Just before the boundary is still in the first segment.
        assert!((sched.bpm_at(boundary_ms - 1e-6) - 125.0).abs() < 1e-9);
    }

    #[test]
    fn long_note_p_field_resolves_tail_cell() {
        // Head at n=5 → (row 1, col 1). Per memon v1.0.0 6-notation:
        // p=0..2 enumerates the 3 horizontal cells in head's row,
        // excluding head, ordered left→right. For col=1 those cells
        // are col=0,2,3 → p=2 lands at col=3.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 5, "t": 0, "l": 240, "p": 2}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        assert_eq!(chart.notes.len(), 1);
        let n = &chart.notes[0];
        assert_eq!((n.row, n.col), (1, 1));
        assert_eq!((n.tail_row, n.tail_col), (1, 3));
        assert!(n.is_long());
    }

    #[test]
    fn long_note_p_vertical_tail() {
        // Head at n=5 (row 1, col 1) with p=4. p=3..5 enumerates the 3
        // vertical cells in head's column excluding head, top→bottom.
        // For row=1 those rows are 0,2,3 → p=3→0, p=4→2, p=5→3.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 5, "t": 0, "l": 240, "p": 4}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        let n = &chart.notes[0];
        assert_eq!((n.tail_row, n.tail_col), (2, 1));
    }

    #[test]
    fn long_note_without_p_has_tail_equal_to_head() {
        // l > 0 but no p → tail defaults to head cell (renderer falls back).
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 5, "t": 0, "l": 240}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        let n = &chart.notes[0];
        assert_eq!((n.tail_row, n.tail_col), (n.row, n.col));
    }

    #[test]
    fn tap_note_tail_defaults_to_head_even_if_p_present() {
        // A tap (no l) that happens to carry a stray p should still report
        // tail = head — arrows only mean something on long notes.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 5, "t": 0, "p": 2}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        let n = &chart.notes[0];
        assert_eq!((n.tail_row, n.tail_col), (n.row, n.col));
    }

    #[test]
    fn long_note_p_first_horizontal_slot() {
        // Head at (0, 0) with p=0 → first horizontal cell in row 0
        // excluding col 0 → col 1. (Old "compass + clamp" semantics
        // returned (0, 0); 6-notation never lets the tail collide with
        // the head, so the first slot is always a real neighbour.)
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
            "data": {"BSC": {"level": 1, "notes": [
                {"n": 0, "t": 0, "l": 240, "p": 0}
            ]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        let n = &chart.notes[0];
        assert_eq!((n.tail_row, n.tail_col), (0, 1));
    }

    #[test]
    fn metadata_jacket_resolves_to_sibling_path() {
        // `metadata.jacket: "cover.png"` should surface as an absolute
        // `Chart.jacket_path` sibling to the chart file on disk.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x","jacket":"cover.png"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
            "data": {"BSC": {"level": 1, "notes": [{"n":0,"t":0}]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        let jp = chart.jacket_path.expect("jacket_path resolved");
        assert_eq!(jp, tmp.path().join("cover.png"));
    }

    #[test]
    fn absent_jacket_auto_detects_sibling_png() {
        // No `metadata.jacket` declared, but a sibling `jacket.png` exists
        // (common in bulk chart dumps). Loader should auto-resolve it.
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", SAMPLE);
        std::fs::write(tmp.path().join("jacket.png"), &[0x89, 0x50, 0x4e, 0x47]).unwrap();
        let chart = load(&path, "BSC").unwrap();
        let jp = chart.jacket_path.expect("auto-detected jacket.png");
        assert_eq!(jp, tmp.path().join("jacket.png"));
    }

    #[test]
    fn absent_jacket_yields_none_path() {
        // Default case — existing charts with no `jacket` key must still
        // load and report `jacket_path = None`.
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", SAMPLE);
        let chart = load(&path, "BSC").unwrap();
        assert!(chart.jacket_path.is_none(), "no jacket → None");
    }

    #[test]
    fn empty_jacket_string_yields_none_path() {
        // Defensive: `"jacket":""` shouldn't resolve to a dir-pointing path.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x","jacket":""},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
            "data": {"BSC": {"level": 1, "notes": [{"n":0,"t":0}]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        assert!(chart.jacket_path.is_none());
    }

    #[test]
    fn chart_timing_overrides_top_level() {
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title":"t","artist":"a","audio":"x"},
            "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":60}]},
            "data": {
                "BSC": {
                    "level": 1,
                    "timing": {"offset": 0, "resolution": 240, "bpms":[{"beat":0,"bpm":120}]},
                    "notes": [{"n":0,"t":240}]
                }
            }
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        // 120 BPM => beat = 500ms. tick 240 = 1 beat = 500ms. (Not 1000ms which would be 60 BPM.)
        assert!((chart.notes[0].hit_time_ms - 500.0).abs() < 1e-6);
    }
}
