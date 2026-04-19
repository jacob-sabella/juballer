//! Short per-grade "hit" sound effects for rhythm mode.
//!
//! Each grade (Perfect/Great/Good/Poor/Miss) maps to a small ~30–80ms OGG
//! sample under `assets/sample/sfx/`. [`SfxBank`] owns a dedicated rodio
//! `OutputStream` + `OutputStreamHandle` — the SFX path is deliberately
//! **separate** from the song audio sink in [`super::audio`], so a burst of
//! hit sounds can never preempt or stall the music.
//!
//! The bank lazy-loads sample bytes on first use. Missing samples (or a
//! failed output device) collapse to a no-op + single WARN; the game keeps
//! playing without audio feedback. The `muted` flag is wired to the top-
//! level `--mute-sfx` CLI flag so devs can silence it entirely.
//!
//! Each [`play`](SfxBank::play) call spawns a fresh one-shot `Sink` on the
//! shared stream handle and appends a freshly-decoded source. Rodio frees
//! the sink once the source finishes, so bank state stays bounded.

use super::judge::Grade;
use crate::{Error, Result};
use rodio::source::Source;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, Instant};

/// Max concurrent hit-SFX voices. A 4-note chord fires up to 4 grades in
/// the same frame; we clamp at 3 so the layer never fills the whole
/// mixer and stays out of the song's way.
const MAX_ACTIVE_SINKS: usize = 3;

/// Per-grade dedup window. A chord of N notes judged in the same frame
/// produces up to N calls to `play` — all with the same grade when all
/// notes hit the same tier. We suppress any second (or later) call for
/// the same grade within this window so the listener hears one clean click
/// instead of a piled-up stack of identical sounds.
const COOLDOWN: Duration = Duration::from_millis(15);

/// Which grades we ship samples for. Kept here (not on [`Grade`]) so the
/// game's scoring logic doesn't have to know anything about audio.
const GRADES: [Grade; 5] = [
    Grade::Perfect,
    Grade::Great,
    Grade::Good,
    Grade::Poor,
    Grade::Miss,
];

/// Raw sample bytes keyed by [`Grade`]. `None` entries mean either the
/// sample file was missing on disk or decoding probe failed — both collapse
/// to a silent no-op in [`SfxBank::play`].
struct Sample {
    grade: Grade,
    /// Decoded-format-preserving OGG bytes. We re-decode per play so each
    /// burst gets an independent source (rodio's [`Decoder`] isn't
    /// clone-friendly and the samples are a few KB, so decode cost is
    /// negligible).
    bytes: Option<Vec<u8>>,
}

/// Per-grade loudness factor applied on top of the bank's master volume.
/// Shared soft tick for P/G/G, nominal Poor, slightly boosted Miss so it
/// pops above the song.
pub(crate) fn grade_volume_factor(grade: Grade) -> f32 {
    match grade {
        Grade::Perfect | Grade::Great | Grade::Good => 0.6,
        Grade::Poor => 1.0,
        Grade::Miss => 1.1,
    }
}

/// Audio output for hit sounds. Holds its own `OutputStream` distinct from
/// the song's — see module docs.
pub struct SfxBank {
    // OutputStream must stay alive for the handle to produce audio; we
    // never touch it directly after construction.
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    samples: Vec<Sample>,
    muted: bool,
    master_volume: f32,
    /// Bounded queue of in-flight sinks. Capped at [`MAX_ACTIVE_SINKS`];
    /// oldest is dropped (and thus stopped) when the cap is hit.
    active: VecDeque<Sink>,
    /// Per-grade timestamp of the last successful `play`. Used by
    /// [`cooldown_allows`](Self::cooldown_allows) to suppress duplicate sounds
    /// from chords where multiple notes hit the same grade in one frame.
    last_played: HashMap<Grade, Instant>,
    /// One-shot guard so we only WARN once on missing samples / no device,
    /// regardless of how many hits the player lands afterward.
    warned_once: Once,
}

impl SfxBank {
    /// Construct a bank with no samples and no output stream. `play` on this
    /// instance is always a no-op. Used in tests and as the fallback when
    /// the audio backend isn't available.
    pub fn new_empty() -> Self {
        Self {
            _stream: None,
            handle: None,
            samples: GRADES
                .iter()
                .map(|g| Sample {
                    grade: *g,
                    bytes: None,
                })
                .collect(),
            muted: false,
            master_volume: 0.35,
            active: VecDeque::with_capacity(4),
            last_played: HashMap::new(),
            warned_once: Once::new(),
        }
    }

    /// Open a rodio output stream and eagerly load all five samples from
    /// `dir`. Missing files are tolerated — they simply won't play. If the
    /// output stream can't open (no audio device, tests, headless CI) the
    /// bank collapses to [`Self::new_empty`] semantics.
    pub fn load_from_dir(dir: &Path) -> Self {
        let samples: Vec<Sample> = GRADES
            .iter()
            .map(|g| Sample {
                grade: *g,
                bytes: read_sample(dir, *g),
            })
            .collect();

        match OutputStream::try_default() {
            Ok((stream, handle)) => Self {
                _stream: Some(stream),
                handle: Some(handle),
                samples,
                muted: false,
                master_volume: 0.35,
                active: VecDeque::with_capacity(4),
                last_played: HashMap::new(),
                warned_once: Once::new(),
            },
            Err(e) => {
                tracing::warn!(
                    target: "juballer::rhythm::sfx",
                    "sfx: no audio output device ({e}); hit sounds disabled"
                );
                Self {
                    _stream: None,
                    handle: None,
                    samples,
                    muted: false,
                    master_volume: 0.35,
                    active: VecDeque::with_capacity(4),
                    last_played: HashMap::new(),
                    warned_once: Once::new(),
                }
            }
        }
    }

    /// Load the bank from the standard asset directory resolved by
    /// [`resolve_sfx_dir`]. Always succeeds; on a missing directory the bank
    /// is silent (play is a no-op) with a single WARN emitted on first hit.
    pub fn load_default() -> Self {
        let dir = resolve_sfx_dir();
        Self::load_from_dir(&dir)
    }

    /// Globally silence the bank. Useful for `--mute-sfx` and tests.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }

    pub fn set_master_volume(&mut self, v: f32) {
        self.master_volume = v.clamp(0.0, 1.0);
    }

    /// True if this bank has a live output handle AND at least one loaded
    /// sample. Mainly for tests — callers should just call [`play`] and let
    /// the bank decide what to do.
    pub fn is_ready(&self) -> bool {
        self.handle.is_some() && self.samples.iter().any(|s| s.bytes.is_some())
    }

    /// Returns `true` if no sound for `grade` has been played within the
    /// [`COOLDOWN`] window. Used to deduplicate chord pileup: a 4-note chord
    /// judged in a single frame produces up to 4 identical `play` calls, but
    /// only the first passes this gate.
    fn cooldown_allows(&self, grade: Grade) -> bool {
        match self.last_played.get(&grade) {
            None => true,
            Some(t) => t.elapsed() >= COOLDOWN,
        }
    }

    /// Record the current instant as the last-played time for `grade`.
    fn mark_cooldown_now(&mut self, grade: Grade) {
        self.last_played.insert(grade, Instant::now());
    }

    /// Fire a one-shot hit sound for `grade`. No-op if the bank is muted,
    /// has no output device, or the sample for that grade is missing. Never
    /// panics and never blocks — decode failures at play time are logged at
    /// debug level and swallowed.
    pub fn play(&mut self, grade: Grade) {
        if self.muted {
            return;
        }
        if !self.cooldown_allows(grade) {
            return;
        }
        let Some(handle) = self.handle.as_ref() else {
            self.warn_once("no audio output handle");
            return;
        };
        let Some(bytes) = self
            .samples
            .iter()
            .find(|s| s.grade == grade)
            .and_then(|s| s.bytes.as_ref())
        else {
            self.warn_once("one or more sample files missing under assets/sample/sfx/");
            return;
        };

        // GC finished sinks before adding a new one so the cap reflects live
        // voices only, not voices that already drained their source.
        self.active.retain(|s| !s.empty());

        // Decode into a fresh source per play. Samples are tiny (~3 KB OGG
        // ≈ a few ms of decode), so this is cheaper than maintaining a pool.
        let cursor = Cursor::new(bytes.clone());
        let source = match Decoder::new(cursor) {
            Ok(d) => d,
            Err(e) => {
                tracing::debug!(target: "juballer::rhythm::sfx", "decode failed for {:?}: {e}", grade);
                return;
            }
        };
        let sink = match Sink::try_new(handle) {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!(target: "juballer::rhythm::sfx", "sink create failed: {e}");
                return;
            }
        };
        let vol = self.master_volume * grade_volume_factor(grade);
        sink.set_volume(vol);
        // Small guard on total duration: force-stop after 500ms so an
        // accidentally-long sample can't accumulate latency.
        sink.append(source.take_duration(std::time::Duration::from_millis(500)));
        // Enqueue instead of detach: voice-cap logic in enqueue_sink drops the
        // oldest sink (stopping its source) if the queue is at MAX_ACTIVE_SINKS.
        self.enqueue_sink(sink);
        self.mark_cooldown_now(grade);
    }

    #[cfg(test)]
    pub fn active_sink_count(&self) -> usize {
        self.active.len()
    }

    /// Push a new `Sink` into the active queue, dropping the oldest if the
    /// queue is at capacity. Dropping a rodio `Sink` stops its currently-
    /// playing source — exactly the voice-steal we want.
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

    fn warn_once(&self, reason: &str) {
        self.warned_once.call_once(|| {
            tracing::warn!(
                target: "juballer::rhythm::sfx",
                "sfx: {reason} — hit sounds will be silent for this session"
            );
        });
    }
}

/// Read the OGG bytes for `grade` from `dir`. Returns `None` if the file is
/// missing or unreadable; callers treat that as "no sample for this grade".
fn read_sample(dir: &Path, grade: Grade) -> Option<Vec<u8>> {
    let name = sample_filename(grade);
    let path = dir.join(name);
    std::fs::read(&path).ok()
}

/// Canonical on-disk filename for each grade's sample.
///
/// Perfect / Great / Good share a single quiet click — hit feedback is
/// minimal so the song carries the rhythm. Poor and Miss remain distinct
/// for clear near-miss and miss feedback. `const fn` so callers can
/// reference it in tests without `match` duplication.
pub const fn sample_filename(grade: Grade) -> &'static str {
    match grade {
        Grade::Perfect | Grade::Great | Grade::Good => "tick.ogg",
        Grade::Poor => "poor.ogg",
        Grade::Miss => "miss.ogg",
    }
}

/// Resolve the SFX asset directory at runtime. Mirrors the shader resolver
/// in [`super::resolve_shader_path`] — probes `CARGO_MANIFEST_DIR` first for
/// dev builds, falls back to the exe-relative layout for packaged installs,
/// and finally a CWD-relative path for ad-hoc runs.
pub fn resolve_sfx_dir() -> PathBuf {
    let candidates: Vec<PathBuf> = vec![
        // 1. Dev: <crate>/../../assets/sample/sfx (the workspace root has
        //    `assets/`, so walk up two levels from juballer-deck's manifest).
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/sample/sfx")
            .to_path_buf(),
        // 2. Packaged install: <exe_dir>/assets/sample/sfx
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .map(|d| d.join("assets/sample/sfx"))
            .unwrap_or_default(),
        // 3. CWD fallback.
        PathBuf::from("assets/sample/sfx"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    candidates.into_iter().next().unwrap()
}

/// Error-returning load that distinguishes "dir missing" from "everything's
/// fine". Used by tests; the game loop uses [`SfxBank::load_default`].
pub fn load_or_error(dir: &Path) -> Result<SfxBank> {
    if !dir.exists() {
        return Err(Error::Config(format!(
            "sfx: directory not found at {}",
            dir.display()
        )));
    }
    Ok(SfxBank::load_from_dir(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The empty bank must construct without doing I/O or opening an
    /// audio device — critical for headless CI where rodio can't find an
    /// output stream. `play` must be a no-op in every grade.
    #[test]
    fn new_empty_constructs_and_play_is_noop() {
        let mut bank = SfxBank::new_empty();
        assert!(!bank.is_ready());
        // Hitting play on every grade must not panic, log-spam, or block.
        for g in [
            Grade::Perfect,
            Grade::Great,
            Grade::Good,
            Grade::Poor,
            Grade::Miss,
        ] {
            bank.play(g);
        }
    }

    /// Muting is independent of the output stream — muted bank with samples
    /// still doesn't play.
    #[test]
    fn muted_bank_plays_nothing() {
        let mut bank = SfxBank::new_empty();
        bank.set_muted(true);
        assert!(bank.is_muted());
        bank.play(Grade::Perfect);
    }

    /// Loading from a nonexistent directory must not panic — samples just
    /// resolve to `None` and play becomes a no-op.
    #[test]
    fn load_from_missing_dir_is_silent_no_panic() {
        let mut bank = SfxBank::load_from_dir(Path::new("/nonexistent/juballer/sfx"));
        // With no device OR no samples, is_ready should be false.
        assert!(!bank.is_ready());
        bank.play(Grade::Miss);
    }

    /// `resolve_sfx_dir` must probe `CARGO_MANIFEST_DIR` just like the shader
    /// resolver. Asserting the candidate list includes that path matters —
    /// on dev machines it's what makes hit sounds work out of the box.
    #[test]
    fn resolve_sfx_dir_probes_cargo_manifest_dir() {
        // The concrete dev candidate should be the crate's manifest dir
        // joined with the standard asset subpath.
        let expected_dev = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/sample/sfx")
            .to_path_buf();
        let resolved = resolve_sfx_dir();
        // Either the resolved path is the dev candidate (common case), or
        // it falls back to one of the known alternatives — but it must
        // never be empty.
        assert!(
            !resolved.as_os_str().is_empty(),
            "resolver returned empty path"
        );
        // If the dev path exists (running from a checkout), the resolver
        // must pick it over the fallbacks.
        if expected_dev.exists() {
            assert_eq!(
                resolved.canonicalize().ok(),
                expected_dev.canonicalize().ok(),
                "resolver should prefer CARGO_MANIFEST_DIR dev path when it exists"
            );
        }
    }

    /// Perfect / Great / Good share a single tick.ogg; Poor and Miss remain
    /// distinct. This collapsed model reduces audio layer bloat while keeping
    /// feedback for near-miss (Poor) and completely-missed cases clear.
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

    /// Smoke-check: when the real dev asset dir exists, load_from_dir finds
    /// at least the perfect.ogg sample. Skipped if the layout is different
    /// (e.g. running from an installed binary with a different root).
    #[test]
    fn load_from_dev_dir_reads_samples_when_present() {
        let dev_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sample/sfx");
        if !dev_dir.exists() {
            eprintln!("skip: dev asset dir not present at {}", dev_dir.display());
            return;
        }
        let bank = SfxBank::load_from_dir(&dev_dir);
        // We can't rely on is_ready() (no audio device on CI) but samples
        // themselves should be populated.
        let have_any = bank.samples.iter().any(|s| s.bytes.is_some());
        assert!(
            have_any,
            "expected at least one loaded sample in {:?}",
            dev_dir
        );
    }

    #[test]
    fn load_or_error_rejects_missing_dir() {
        // SfxBank doesn't impl Debug (it owns a rodio OutputStream which
        // doesn't either), so we manually destructure instead of expect_err.
        let res = load_or_error(Path::new("/definitely/not/here/sfx"));
        let err = match res {
            Ok(_) => panic!("expected Err for missing dir"),
            Err(e) => e,
        };
        let msg = format!("{err}");
        assert!(msg.contains("sfx"), "error should mention sfx: {msg}");
    }

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

    #[test]
    fn voice_cap_keeps_at_most_three_active_sinks() {
        // Exercise with an empty bank: play() is a no-op on missing samples, so
        // drive the queue bookkeeping via the test-only push_dummy_sink_for_test
        // helper. Keeps the test deterministic across environments without audio.
        let mut bank = SfxBank::new_empty();
        for _ in 0..10 {
            bank.push_dummy_sink_for_test();
        }
        assert!(bank.active_sink_count() <= 3, "voice cap must clamp to 3");
    }

    #[test]
    fn cooldown_suppresses_rapid_same_grade_plays() {
        let mut bank = SfxBank::new_empty();
        assert!(bank.cooldown_allows(Grade::Perfect));
        bank.mark_cooldown_now(Grade::Perfect);
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
        sleep(Duration::from_millis(20));
        assert!(bank.cooldown_allows(Grade::Great));
    }
}
