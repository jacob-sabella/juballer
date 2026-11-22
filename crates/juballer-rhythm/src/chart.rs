//! memon chart loader: v1.0.0, plus v0.x and pre-semver files upgraded on read.
//!
//! Only the fields the rhythm game actually uses are materialised; everything
//! else (hakus, etc.) is silently ignored. Symbolic times are either an integer
//! number of ticks or a `[whole, num, den]` mixed number of *beats*. Decimals
//! may be JSON numbers or strings.

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
    #[serde(default)]
    pub metadata: Metadata,
    #[serde(default)]
    pub timing: Option<Timing>,
    pub data: IndexMap<String, ChartData>,
}

impl Memon {
    /// Parse a memon file of any supported version into the v1.0.0 shape.
    pub fn parse(bytes: &[u8]) -> std::result::Result<Self, String> {
        #[derive(Deserialize)]
        struct Header {
            #[serde(default)]
            version: Option<String>,
        }
        let header: Header = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        match header.version.as_deref() {
            Some("1.0.0") => serde_json::from_slice(bytes).map_err(|e| e.to_string()),
            // No version key at all is the pre-semver format.
            None | Some("0.1.0" | "0.2.0" | "0.3.0") => {
                let root: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
                serde_json::from_value(upgrade_v0(root)?).map_err(|e| e.to_string())
            }
            Some(v) => Err(format!(
                "unsupported memon version '{v}', expected 1.0.0 or 0.x"
            )),
        }
    }

    /// Difficulty names in the spec's display order: BSC, ADV, EXT, then the
    /// rest alphabetically.
    pub fn ordered_difficulties(&self) -> Vec<&str> {
        let rank = |d: &str| match d {
            "BSC" => 0,
            "ADV" => 1,
            "EXT" => 2,
            _ => 3,
        };
        let mut names: Vec<&str> = self.data.keys().map(String::as_str).collect();
        names.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
        names
    }

    /// The BPM events that apply to `difficulty`, after timing fallback.
    /// Empty means the schema default of 120 BPM.
    pub fn bpms_for(&self, difficulty: &str) -> &[BpmEntry] {
        self.data
            .get(difficulty)
            .and_then(|c| c.timing.as_ref())
            .filter(|t| !t.bpms.is_empty())
            .or(self.timing.as_ref())
            .map(|t| t.bpms.as_slice())
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Metadata {
    /// Song title. Optional in the spec; loaders fall back to the file name.
    #[serde(default)]
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
    /// Optional per-chart song-preview hint. When present the chart picker
    /// uses it verbatim for its audio snippet; otherwise the picker falls back
    /// to a heuristic (see `picker::preview_window`). Both `start` and
    /// `duration` are seconds. The spec's other form, a path to a BMS-style
    /// preview file, reads as `None`.
    #[serde(default, deserialize_with = "de_preview")]
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
    #[serde(deserialize_with = "de_decimal")]
    pub start: f64,
    #[serde(deserialize_with = "de_decimal")]
    pub duration: f64,
}

/// Every key is optional: each one falls back separately from the chart's
/// timing object to the root's, then to the schema default.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Timing {
    #[serde(default, deserialize_with = "de_opt_decimal")]
    pub offset: Option<f64>,
    #[serde(default)]
    pub resolution: Option<u32>,
    #[serde(default)]
    pub bpms: Vec<BpmEntry>,
}

const DEFAULT_RESOLUTION: u32 = 240;
const DEFAULT_BPM: f64 = 120.0;

/// A memon time point or duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolicTime {
    /// Ticks of the enclosing resolution.
    Ticks(u64),
    /// `whole + num / den` beats.
    Mixed(u64, u64, u64),
}

impl SymbolicTime {
    fn from_json(v: &serde_json::Value) -> Option<Self> {
        match v {
            serde_json::Value::Number(n) => n.as_u64().map(Self::Ticks),
            serde_json::Value::Array(a) if a.len() == 3 => {
                let (w, n, d) = (a[0].as_u64()?, a[1].as_u64()?, a[2].as_u64()?);
                (d > 0).then_some(Self::Mixed(w, n, d))
            }
            _ => None,
        }
    }

    pub fn beats(self, resolution: u32) -> f64 {
        match self {
            Self::Ticks(t) => t as f64 / resolution as f64,
            Self::Mixed(w, n, d) => w as f64 + n as f64 / d as f64,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BpmEntry {
    #[serde(deserialize_with = "de_symbolic")]
    pub beat: SymbolicTime,
    #[serde(deserialize_with = "de_decimal")]
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

impl ChartData {
    /// The chart's level, whether stored as a number or a string.
    pub fn level_value(&self) -> Option<f64> {
        decimal(&self.level)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NoteRaw {
    pub n: u8,
    pub t: serde_json::Value,
    /// Symbolic length (long notes). Absent or 0 means a tap note.
    #[serde(default)]
    pub l: Option<serde_json::Value>,
    /// Arrow tail-position encoding (0..5) — visual only. Only the head
    /// cell + length matter for gameplay.
    #[serde(default)]
    pub p: Option<u8>,
}

// ── Lenient field readers ──────────────────────────────────────────────────

fn decimal(v: &serde_json::Value) -> Option<f64> {
    let x = match v {
        serde_json::Value::Number(n) => n.as_f64()?,
        serde_json::Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    x.is_finite().then_some(x)
}

fn de_decimal<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<f64, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    decimal(&v).ok_or_else(|| serde::de::Error::custom(format!("expected a decimal, got {v}")))
}

fn de_opt_decimal<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<f64>, D::Error> {
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Null => Ok(None),
        v => decimal(&v)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom(format!("expected a decimal, got {v}"))),
    }
}

fn de_symbolic<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<SymbolicTime, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    SymbolicTime::from_json(&v)
        .ok_or_else(|| serde::de::Error::custom(format!("expected a symbolic time, got {v}")))
}

fn de_preview<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Preview>, D::Error> {
    match serde_json::Value::deserialize(d)? {
        v @ serde_json::Value::Object(_) => serde_json::from_value(v)
            .map(Some)
            .map_err(serde::de::Error::custom),
        _ => Ok(None),
    }
}

// ── v0.x upgrade ───────────────────────────────────────────────────────────

/// A v0.x long-note `p` (12-notation: direction `p % 4`, distance `p / 4 + 1`)
/// as 6-notation. `None` if the tail would fall off the grid.
fn p12_to_p6(n: u64, p: u64) -> Option<u64> {
    let dist = (p / 4 + 1) as i64;
    let (dx, dy) = match p % 4 {
        0 => (0, -dist),
        1 => (dist, 0),
        2 => (0, dist),
        _ => (-dist, 0),
    };
    if p > 11 || n > 15 {
        return None;
    }
    let (col, row) = ((n % 4) as i64, (n / 4) as i64);
    let (tail_col, tail_row) = (col + dx, row + dy);
    if !(0..4).contains(&tail_col) || !(0..4).contains(&tail_row) {
        return None;
    }
    // Position among the three cells that aren't the head.
    let slot = |tail: i64, head: i64| (if tail < head { tail } else { tail - 1 }) as u64;
    Some(if dy == 0 {
        slot(tail_col, col)
    } else {
        3 + slot(tail_row, row)
    })
}

fn upgrade_v0_chart(chart: &mut serde_json::Value) {
    let Some(notes) = chart.get_mut("notes").and_then(|n| n.as_array_mut()) else {
        return;
    };
    for note in notes.iter_mut().filter_map(|n| n.as_object_mut()) {
        if note.get("l").and_then(|l| l.as_u64()).unwrap_or(0) == 0 {
            note.remove("l");
            note.remove("p");
            continue;
        }
        let n = note.get("n").and_then(|v| v.as_u64());
        let p = note.get("p").and_then(|v| v.as_u64());
        match n.zip(p).and_then(|(n, p)| p12_to_p6(n, p)) {
            Some(p6) => {
                note.insert("p".into(), p6.into());
            }
            None => {
                note.remove("p");
            }
        }
    }
}

/// Rewrite a v0.x or pre-semver file as v1.0.0. The old `offset` is the
/// opposite of the new one, and old `p` values use 12-notation.
fn upgrade_v0(mut root: serde_json::Value) -> std::result::Result<serde_json::Value, String> {
    use serde_json::{Map, Value};
    let obj = root.as_object_mut().ok_or("memon root is not an object")?;
    let mut old = match obj.remove("metadata") {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let mut meta = Map::new();
    for (from, to) in [
        ("song title", "title"),
        ("artist", "artist"),
        ("music path", "audio"),
        ("album cover path", "jacket"),
        ("jacket path", "jacket"),
    ] {
        if let Some(v) = old.remove(from) {
            meta.insert(to.into(), v);
        }
    }
    if let Some(Value::Object(mut pv)) = old.remove("preview") {
        let mut new = Map::new();
        for (from, to) in [("position", "start"), ("length", "duration")] {
            if let Some(v) = pv.remove(from) {
                new.insert(to.into(), v);
            }
        }
        meta.insert("preview".into(), Value::Object(new));
    }

    let mut timing = Map::new();
    if let Some(offset) = old.get("offset").and_then(decimal) {
        timing.insert("offset".into(), (-offset).into());
    }
    if let Some(bpm) = old.remove("BPM") {
        timing.insert("bpms".into(), serde_json::json!([{"beat": 0, "bpm": bpm}]));
    }

    let mut data = match obj.remove("data") {
        Some(Value::Object(m)) => m,
        // Pre-semver: an array of charts, each naming its own difficulty.
        Some(Value::Array(charts)) => {
            let mut m = Map::new();
            for mut c in charts {
                let name = c
                    .as_object_mut()
                    .and_then(|o| o.remove("dif_name"))
                    .and_then(|v| v.as_str().map(str::to_string))
                    .ok_or("pre-semver chart has no dif_name")?;
                m.insert(name, c);
            }
            m
        }
        _ => return Err("memon file has no data".to_string()),
    };
    data.values_mut().for_each(upgrade_v0_chart);
    Ok(serde_json::json!({
        "version": "1.0.0",
        "metadata": meta,
        "timing": timing,
        "data": data,
    }))
}

// ── Loader ─────────────────────────────────────────────────────────────────

/// Parse the memon file at `path` and produce a normalized [`Chart`] for `difficulty`.
/// Returns `Err(Error::Config)` if the file can't be read, its version is
/// unsupported, the difficulty is absent, or its timing is invalid.
pub fn load(path: &Path, difficulty: &str) -> Result<Chart> {
    let bytes = std::fs::read(path)?;
    let memon = Memon::parse(&bytes)
        .map_err(|e| Error::Config(format!("{}: memon parse: {e}", path.display())))?;
    let title = display_title(&memon, path);
    let chart_data = memon.data.get(difficulty).ok_or_else(|| {
        let keys: Vec<&str> = memon.data.keys().map(String::as_str).collect();
        Error::Config(format!(
            "{}: difficulty '{difficulty}' not present; available: {keys:?}",
            path.display()
        ))
    })?;

    // Each timing key falls back separately: chart, then root, then default.
    let (schedule, offset_s) = resolve_timing(&memon.timing, chart_data)?;
    let bpm = schedule.initial_bpm();
    let hit_times = build_notes(&chart_data.notes, &schedule, offset_s);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let audio_path = parent.join(&memon.metadata.audio);
    let jacket_path = resolve_jacket_path(parent, memon.metadata.jacket.as_deref());
    let banner_path = find_sibling(parent, &["banner.png", "banner.jpg", "banner.jpeg"]);
    let mini_path = find_sibling(parent, &["mini.png", "mini.jpg", "thumb.png"]);

    Ok(Chart {
        title,
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

/// The song title, or the file's name when the chart doesn't have one.
pub fn display_title(memon: &Memon, path: &Path) -> String {
    let title = memon.metadata.title.trim();
    if title.is_empty() {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        title.to_string()
    }
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
pub fn resolve_jacket_path(parent: &Path, jacket: Option<&str>) -> Option<PathBuf> {
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
    // Spec: every key is searched separately, chart timing first, then root
    // timing, then the schema default. A present-but-empty `bpms` counts as
    // missing, since some chart dumps carry `timing: {}` on every difficulty.
    let own = chart.timing.as_ref();
    let root = top.as_ref();
    let offset = own
        .and_then(|t| t.offset)
        .or(root.and_then(|t| t.offset))
        .unwrap_or(0.0);
    let timing_resolution = own
        .and_then(|t| t.resolution)
        .or(root.and_then(|t| t.resolution))
        .unwrap_or(DEFAULT_RESOLUTION);
    let default_bpms = [BpmEntry {
        beat: SymbolicTime::Ticks(0),
        bpm: DEFAULT_BPM,
    }];
    let bpms = own
        .filter(|t| !t.bpms.is_empty())
        .or(root.filter(|t| !t.bpms.is_empty()))
        .map(|t| t.bpms.as_slice())
        .unwrap_or(&default_bpms);
    // Notes count ticks in the chart's own resolution, BPM events in the
    // timing object's.
    let note_resolution = chart.resolution.unwrap_or(DEFAULT_RESOLUTION);
    let schedule = BpmSchedule::with_resolutions(bpms, timing_resolution, note_resolution)?;
    Ok((schedule, offset))
}

/// Pre-computed BPM segment table so beats can be converted to ms (linear scan
/// in practice — charts rarely have more than a handful of BPM changes). Each
/// segment covers a half-open range `[start_beat, next_start)`.
#[derive(Debug, Clone)]
pub struct BpmSchedule {
    segments: Vec<BpmSegment>,
    resolution: u32,
}

#[derive(Debug, Clone, Copy)]
struct BpmSegment {
    start_beat: f64,
    bpm: f64,
    ms_per_beat: f64,
    /// Cumulative music-time in ms at `start_beat` (before applying the
    /// chart-level offset).
    ms_at_start: f64,
}

impl BpmSchedule {
    /// A schedule whose BPM events and notes share one resolution.
    pub fn new(bpms: &[BpmEntry], resolution: u32) -> Result<Self> {
        Self::with_resolutions(bpms, resolution, resolution)
    }

    /// Build a schedule from a memon `bpms` list, whose integer beats count
    /// ticks of `timing_resolution`. `note_resolution` is what
    /// [`Self::tick_to_ms`] counts in. The list doesn't need to be sorted. If
    /// the earliest entry isn't at beat 0, its BPM applies from the song
    /// start. When several entries share a beat, the last one wins.
    pub fn with_resolutions(
        bpms: &[BpmEntry],
        timing_resolution: u32,
        note_resolution: u32,
    ) -> Result<Self> {
        if bpms.is_empty() {
            return Err(Error::Config("bpms is empty".to_string()));
        }
        if timing_resolution == 0 || note_resolution == 0 {
            return Err(Error::Config("resolution must be > 0".to_string()));
        }
        for b in bpms {
            if b.bpm <= 0.0 {
                return Err(Error::Config(format!("bpm must be > 0 (got {})", b.bpm)));
            }
        }
        let mut sorted: Vec<(f64, f64)> = bpms
            .iter()
            .map(|b| (b.beat.beats(timing_resolution), b.bpm))
            .collect();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0)); // stable: file order within a beat
        let mut events: Vec<(f64, f64)> = Vec::with_capacity(sorted.len() + 1);
        for e in sorted {
            match events.last_mut() {
                Some(last) if last.0 == e.0 => *last = e,
                _ => events.push(e),
            }
        }
        if events[0].0 != 0.0 {
            events.insert(0, (0.0, events[0].1));
        }
        let mut segments: Vec<BpmSegment> = Vec::with_capacity(events.len());
        for (start_beat, bpm) in events {
            let ms_at_start = segments
                .last()
                .map(|p| p.ms_at_start + (start_beat - p.start_beat) * p.ms_per_beat)
                .unwrap_or(0.0);
            segments.push(BpmSegment {
                start_beat,
                bpm,
                ms_per_beat: 60_000.0 / bpm,
                ms_at_start,
            });
        }
        Ok(Self {
            segments,
            resolution: note_resolution,
        })
    }

    /// BPM at the start of the song — shown in the HUD.
    pub fn initial_bpm(&self) -> f64 {
        self.segments[0].bpm
    }

    pub fn resolution(&self) -> u32 {
        self.resolution
    }

    /// Convert a beat position to ms (before offset).
    pub fn beat_to_ms(&self, beat: f64) -> f64 {
        let seg = self
            .segments
            .iter()
            .rfind(|s| s.start_beat <= beat)
            .unwrap_or(&self.segments[0]);
        seg.ms_at_start + (beat - seg.start_beat) * seg.ms_per_beat
    }

    /// Convert a tick count (from song start) to ms (before offset).
    pub fn tick_to_ms(&self, tick: i64) -> f64 {
        self.tick_to_ms_float(tick as f64)
    }

    /// Like [`Self::tick_to_ms`] but accepts a fractional tick position.
    pub fn tick_to_ms_float(&self, tick: f64) -> f64 {
        self.beat_to_ms(tick / self.resolution as f64)
    }

    /// BPM active at music-time `ms` (pre-offset; segment boundaries are
    /// stored relative to beat 0). Segments are half-open `[start, next_start)`
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
    let resolution = schedule.resolution();
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
        let Some(start) = SymbolicTime::from_json(&nr.t) else {
            tracing::warn!(
                target: "juballer::rhythm",
                "note.t invalid at n={} (got {}); skipping",
                nr.n,
                nr.t
            );
            continue;
        };
        let start_beat = start.beats(resolution);
        let hit_ms = schedule.beat_to_ms(start_beat);
        let hit_time_ms = offset_ms + hit_ms;
        // A long note can span BPM changes, so measure it as end minus start.
        let length_beats =
            nr.l.as_ref()
                .and_then(SymbolicTime::from_json)
                .map_or(0.0, |l| l.beats(resolution));
        let length_ms = if length_beats > 0.0 {
            schedule.beat_to_ms(start_beat + length_beats) - hit_ms
        } else {
            0.0
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
    out.sort_by(|a, b| a.hit_time_ms.total_cmp(&b.hit_time_ms));
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
        // The array form counts beats: [2, 1, 2] is 2.5 beats, 1250 ms at 120 BPM.
        let body = r#"{
            "version": "1.0.0",
            "metadata": {"title": "T", "artist": "A", "audio": "a.ogg"},
            "timing": {"offset": 0, "resolution": 240, "bpms": [{"beat": 0, "bpm": 120}]},
            "data": {"BSC": {"level": 1, "notes": [{"n": 0, "t": [2, 1, 2]}]}}
        }"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "a.memon", body);
        let chart = load(&path, "BSC").unwrap();
        assert_eq!(chart.notes.len(), 1);
        let expected = 1250.0;
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
            r#"{"version":"2.0.0","metadata":{"title":"t","artist":"a","audio":"x"},"data":{}}"#;
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
                "bpms": [{"beat": 0, "bpm": 120}, {"beat": [4, 0, 1], "bpm": 240}]
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
                "bpms": [{"beat": 0, "bpm": 120}, {"beat": [4, 0, 1], "bpm": 240}]
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
                beat: SymbolicTime::Ticks(0),
                bpm: 125.0,
            },
            BpmEntry {
                beat: SymbolicTime::Mixed(4, 0, 1),
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

    fn load_body(body: &str, diff: &str) -> Chart {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(&tmp, "song.memon", body);
        load(&path, diff).unwrap()
    }

    #[test]
    fn decimals_may_be_strings() {
        let c = load_body(
            r#"{"version":"1.0.0",
                "metadata":{"title":"t","preview":{"start":"56.7","duration":"22.7"}},
                "timing":{"offset":"0.031","bpms":[{"beat":0,"bpm":"130.000"}]},
                "data":{"EXT":{"level":"10.8","notes":[{"n":0,"t":240}]}}}"#,
            "EXT",
        );
        assert!((c.bpm - 130.0).abs() < 1e-9);
        assert!((c.offset_ms - 31.0).abs() < 1e-9);
        assert_eq!(c.preview.unwrap().start, 56.7);
        assert!((c.notes[0].hit_time_ms - (31.0 + 60_000.0 / 130.0)).abs() < 1e-6);
    }

    #[test]
    fn metadata_timing_and_title_are_optional() {
        let c = load_body(
            r#"{"version":"1.0.0","data":{"BSC":{"notes":[{"n":0,"t":240}]}}}"#,
            "BSC",
        );
        assert_eq!(c.title, "song");
        assert_eq!(c.bpm, 120.0);
        assert!((c.notes[0].hit_time_ms - 500.0).abs() < 1e-6);
    }

    #[test]
    fn preview_path_string_is_no_window() {
        let c = load_body(
            r#"{"version":"1.0.0","metadata":{"preview":"preview.ogg"},
                "data":{"BSC":{"notes":[]}}}"#,
            "BSC",
        );
        assert!(c.preview.is_none());
    }

    #[test]
    fn timing_falls_back_key_by_key() {
        // The spec's own example: BSC takes only its bpms from its timing.
        let body = r#"{"version":"1.0.0","timing":{"offset":0.84},"data":{
            "BSC":{"timing":{"bpms":[{"beat":0,"bpm":200}]},"notes":[]},
            "ADV":{"timing":{"offset":0.31,"bpms":[{"beat":0,"bpm":100}]},"notes":[]}}}"#;
        let bsc = load_body(body, "BSC");
        assert_eq!((bsc.bpm, bsc.offset_ms), (200.0, 840.0));
        let adv = load_body(body, "ADV");
        assert_eq!((adv.bpm, adv.offset_ms), (100.0, 310.0));
    }

    #[test]
    fn integer_bpm_beat_counts_timing_ticks() {
        // Tick 480 at timing resolution 120 is beat 4; notes use their own 240.
        let c = load_body(
            r#"{"version":"1.0.0",
                "timing":{"resolution":120,"bpms":[{"beat":0,"bpm":120},{"beat":480,"bpm":240}]},
                "data":{"BSC":{"notes":[{"n":0,"t":960},{"n":1,"t":1200}]}}}"#,
            "BSC",
        );
        assert!((c.notes[0].hit_time_ms - 2000.0).abs() < 1e-6);
        assert!((c.notes[1].hit_time_ms - 2250.0).abs() < 1e-6);
    }

    #[test]
    fn duplicate_bpm_keeps_the_last() {
        let c = load_body(
            r#"{"version":"1.0.0","timing":{"bpms":[{"beat":0,"bpm":120},{"beat":0,"bpm":124}]},
                "data":{"BSC":{"notes":[]}}}"#,
            "BSC",
        );
        assert_eq!(c.bpm, 124.0);
    }

    #[test]
    fn long_note_length_may_be_mixed_beats() {
        let c = load_body(
            r#"{"version":"1.0.0","data":{"BSC":{"notes":[
                {"n":5,"t":[1,0,1],"l":[1,1,2],"p":2}]}}}"#,
            "BSC",
        );
        assert!((c.notes[0].hit_time_ms - 500.0).abs() < 1e-6);
        assert!((c.notes[0].length_ms - 750.0).abs() < 1e-6);
        assert_eq!((c.notes[0].tail_row, c.notes[0].tail_col), (1, 3));
    }

    #[test]
    fn p12_converts_to_p6() {
        // Head n=5 (row 1, col 1). Up 1 → row 0 → p6 3; right 2 → col 3 → p6 2;
        // down 2 → row 3 → p6 5; left 1 → col 0 → p6 0.
        assert_eq!(p12_to_p6(5, 0), Some(3));
        assert_eq!(p12_to_p6(5, 5), Some(2));
        assert_eq!(p12_to_p6(5, 6), Some(5));
        assert_eq!(p12_to_p6(5, 3), Some(0));
        // Off the grid, or not a 12-notation value.
        assert_eq!(p12_to_p6(0, 0), None);
        assert_eq!(p12_to_p6(5, 12), None);
    }

    #[test]
    fn v0_file_upgrades() {
        let c = load_body(
            r#"{"version":"0.2.0",
                "metadata":{"song title":"Old","artist":"a","music path":"m.ogg",
                    "album cover path":"j.png","BPM":120,"offset":-0.25,
                    "preview":{"position":10,"length":5}},
                "data":{"EXT":{"level":10,"resolution":240,"notes":[
                    {"n":0,"t":0,"l":0,"p":0},
                    {"n":5,"t":240,"l":240,"p":5}]}}}"#,
            "EXT",
        );
        assert_eq!(c.title, "Old");
        assert!(c.audio_path.ends_with("m.ogg"));
        assert!(c.jacket_path.unwrap().ends_with("j.png"));
        assert_eq!(
            c.preview,
            Some(Preview {
                start: 10.0,
                duration: 5.0
            })
        );
        // The old offset is the opposite of the new one.
        assert_eq!(c.offset_ms, 250.0);
        assert!(!c.notes[0].is_long());
        assert!((c.notes[1].hit_time_ms - 750.0).abs() < 1e-6);
        assert_eq!((c.notes[1].tail_row, c.notes[1].tail_col), (1, 3));
    }

    #[test]
    fn pre_semver_chart_array_upgrades() {
        let c = load_body(
            r#"{"metadata":{"song title":"Older","artist":"a","BPM":120,"offset":0},
                "data":[{"dif_name":"ADV","level":5,"resolution":240,
                    "notes":[{"n":1,"t":240,"l":0,"p":0}]}]}"#,
            "ADV",
        );
        assert_eq!(c.title, "Older");
        assert_eq!(c.notes.len(), 1);
    }

    #[test]
    fn difficulties_sort_in_spec_order() {
        let m = Memon::parse(
            br#"{"version":"1.0.0","data":{"Zed":{"notes":[]},"EXT":{"notes":[]},
                "Alt":{"notes":[]},"BSC":{"notes":[]},"ADV":{"notes":[]}}}"#,
        )
        .unwrap();
        assert_eq!(
            m.ordered_difficulties(),
            ["BSC", "ADV", "EXT", "Alt", "Zed"]
        );
    }
}
