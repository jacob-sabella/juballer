//! Tutorial mode — a short scripted rhythm session with narration overlays.
//!
//! Builds an in-memory [`Chart`] against `assets/sample/metronome.ogg`
//! (shared with calibrate, so no separate tutorial track) and runs it
//! through the normal play loop with a [`NarrationHook`] wired up to
//! paint a translucent text strip under the HUD title.
//!
//! The chart is tiny — four phases over ~18s, ~10 notes total. The
//! narration script is the authoritative source for *when* each phase
//! starts: see [`narration_at`]. The chart generator places notes inside
//! each phase's time window so the player sees the lesson and the notes
//! together.
//!
//! Layout:
//! ```text
//!   phase 0  |  0.0s – 3.0s   — intro banner, no notes
//!   phase 1  |  3.0s – 8.0s   — 4 single taps in middle cells, ~1s apart
//!   phase 2  |  8.0s – 13.0s  — 4 faster taps, ~0.7s apart
//!   phase 3  | 13.0s – 17.0s  — one long (held) note, ~3s
//!   phase 4  | 17.0s – ...    — exit hint, no notes
//! ```
//!
//! Phase boundaries live in consts so the chart generator, narration
//! script, and tests share one source of truth.

use super::chart::{BpmEntry, BpmSchedule, Chart, Note};
use super::{play_chart_with_hook, GameState, NarrationHook};
use crate::{Error, Result};
use std::path::{Path, PathBuf};

// ── Phase boundaries (music time in ms) ─────────────────────────────────────
pub const PHASE1_START_MS: f64 = 3_000.0;
pub const PHASE2_START_MS: f64 = 8_000.0;
pub const PHASE3_START_MS: f64 = 13_000.0;
pub const PHASE4_START_MS: f64 = 17_000.0;

// ── Narration labels (exact strings the tests assert on) ───────────────────
pub const INTRO_LABEL: &str = "Welcome. Tap the middle cells.";
pub const PHASE1_LABEL: &str = "Tap as each tile flashes.";
pub const PHASE2_LABEL: &str = "Faster now — hit earlier or later shows timing.";
pub const PHASE3_LABEL: &str = "Hold until the bar drains.";
pub const PHASE4_LABEL: &str = "ESC or all four corners 3s = exit.";

/// Map a music-time (ms) onto the narration label for that moment.
///
/// Separated out so unit tests can verify the phase boundaries without
/// touching audio or rendering.
pub fn narration_at(music_ms: f64) -> &'static str {
    if music_ms < PHASE1_START_MS {
        INTRO_LABEL
    } else if music_ms < PHASE2_START_MS {
        PHASE1_LABEL
    } else if music_ms < PHASE3_START_MS {
        PHASE2_LABEL
    } else if music_ms < PHASE4_START_MS {
        PHASE3_LABEL
    } else {
        PHASE4_LABEL
    }
}

/// Build the scripted tutorial chart in memory. ~10 notes total:
///   * phase 1: 4 single taps in (1,1), (1,2), (2,1), (2,2)
///   * phase 2: 4 faster taps in the same middle cells
///   * phase 3: one long (held) note at (1,1) lasting ~3s
///
/// Timing is derived via a real [`BpmSchedule`] at 120 BPM so hit times match
/// what a disk-loaded chart produces (500 ms/beat grid). All notes land well
/// inside the metronome track's 12 seconds of audio tail — the loop extends
/// past the last note with the normal 2s MISS cutoff, covering phase 3+4.
pub fn generate_tutorial_chart() -> Chart {
    let bpm = 120.0;
    let resolution: u32 = 240;
    let schedule = BpmSchedule::new(&[BpmEntry { beat: 0, bpm }], resolution)
        .expect("bpm > 0 for tutorial chart");

    // Absolute hit times in ms, placed inside each phase window. We pick
    // values by hand rather than beat-grid math because the phases are not
    // beat-aligned (3.0s isn't 8 beats at 120 BPM exactly, it's 6 — but the
    // narrative is driven by wall-clock seconds, not beats).
    let mid_cells = [(1u8, 1u8), (1, 2), (2, 1), (2, 2)];

    let mut notes: Vec<Note> = Vec::with_capacity(10);

    // Phase 1 — 4 single taps, ~1s apart, starting 0.5s into the phase.
    let phase1_hits = [3_500.0, 4_500.0, 5_500.0, 6_500.0];
    for (i, t) in phase1_hits.iter().enumerate() {
        let (row, col) = mid_cells[i];
        notes.push(Note {
            hit_time_ms: *t,
            row,
            col,
            length_ms: 0.0,
            tail_row: row,
            tail_col: col,
        });
    }

    // Phase 2 — 4 taps, ~0.7s apart, starting 0.5s into the phase.
    let phase2_hits = [8_500.0, 9_200.0, 9_900.0, 10_600.0];
    for (i, t) in phase2_hits.iter().enumerate() {
        let (row, col) = mid_cells[i];
        notes.push(Note {
            hit_time_ms: *t,
            row,
            col,
            length_ms: 0.0,
            tail_row: row,
            tail_col: col,
        });
    }

    // Phase 3 — one long (held) note at (1,1), held for 3s.
    notes.push(Note {
        hit_time_ms: 13_500.0,
        row: 1,
        col: 1,
        length_ms: 3_000.0,
        tail_row: 1,
        tail_col: 1,
    });

    // Sanity check: the generated chart keeps the invariant that notes are
    // sorted by hit_time_ms ascending (the play loop assumes this).
    debug_assert!(notes
        .windows(2)
        .all(|w| w[0].hit_time_ms <= w[1].hit_time_ms));

    Chart {
        title: "Tutorial".to_string(),
        artist: "juballer".to_string(),
        audio_path: resolve_metronome_audio(),
        bpm,
        offset_ms: 0.0,
        notes,
        schedule,
        preview: None,
        jacket_path: None,
        banner_path: None,
        mini_path: None,
    }
}

/// Narration hook bound to the tutorial script. Implemented as a concrete
/// struct so we can write a direct unit test against it without going
/// through the blanket `FnMut` impl.
struct TutorialNarrator;

impl NarrationHook for TutorialNarrator {
    fn narrate(&mut self, _state: &GameState, music_ms: f64) -> Option<String> {
        Some(narration_at(music_ms).to_string())
    }
}

/// Entry point for `juballer-deck tutorial`. Runs the scripted chart with
/// narration wired in. Hit SFX are muted so the metronome stays audible
/// (same rationale as `calibrate::run`).
pub fn run_tutorial(user_offset_ms: i32) -> Result<()> {
    let chart = generate_tutorial_chart();
    if !chart.audio_path.exists() {
        return Err(Error::Config(format!(
            "tutorial: metronome audio not found at {} (expected assets/sample/metronome.ogg)",
            chart.audio_path.display()
        )));
    }
    tracing::info!(
        target: "juballer::rhythm::tutorial",
        "starting tutorial ({} notes over ~{:.1}s)",
        chart.notes.len(),
        chart.notes.last().map(|n| (n.hit_time_ms + n.length_ms) / 1000.0).unwrap_or(0.0),
    );
    println!(
        "[tutorial] Follow the on-screen prompts. ESC or hold all four corners for 3s to exit."
    );
    play_chart_with_hook(
        chart,
        user_offset_ms,
        /*mute_sfx=*/ true,
        /*sfx_volume=*/ None,
        TutorialNarrator,
    )
}

/// Same asset resolution strategy as `calibrate::resolve_metronome_audio`,
/// duplicated here because the calibrate version is module-private.
fn resolve_metronome_audio() -> PathBuf {
    let candidates: Vec<PathBuf> = vec![
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sample/metronome.ogg"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .map(|d| d.join("assets/sample/metronome.ogg"))
            .unwrap_or_default(),
        PathBuf::from("assets/sample/metronome.ogg"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    candidates.into_iter().next().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_has_expected_note_count() {
        let chart = generate_tutorial_chart();
        // 4 (phase 1) + 4 (phase 2) + 1 (phase 3 long) = 9 notes.
        assert_eq!(chart.notes.len(), 9, "tutorial target is ~10 notes");
    }

    #[test]
    fn chart_notes_are_sorted_by_hit_time() {
        let chart = generate_tutorial_chart();
        for w in chart.notes.windows(2) {
            assert!(
                w[0].hit_time_ms <= w[1].hit_time_ms,
                "notes must be sorted: {} then {}",
                w[0].hit_time_ms,
                w[1].hit_time_ms,
            );
        }
    }

    #[test]
    fn every_note_is_inside_the_4x4_grid() {
        let chart = generate_tutorial_chart();
        for n in &chart.notes {
            assert!(n.row < 4, "row {} out of bounds", n.row);
            assert!(n.col < 4, "col {} out of bounds", n.col);
            assert!(n.tail_row < 4, "tail_row {} out of bounds", n.tail_row);
            assert!(n.tail_col < 4, "tail_col {} out of bounds", n.tail_col);
        }
    }

    #[test]
    fn phase1_has_four_taps_in_middle_cells() {
        let chart = generate_tutorial_chart();
        let phase1: Vec<&Note> = chart
            .notes
            .iter()
            .filter(|n| n.hit_time_ms >= PHASE1_START_MS && n.hit_time_ms < PHASE2_START_MS)
            .collect();
        assert_eq!(phase1.len(), 4, "phase 1 should contain 4 notes");
        for n in &phase1 {
            assert_eq!(n.length_ms, 0.0, "phase 1 notes are taps, not holds");
            assert!(
                (n.row == 1 || n.row == 2) && (n.col == 1 || n.col == 2),
                "phase 1 must be in middle cells, got ({},{})",
                n.row,
                n.col,
            );
        }
    }

    #[test]
    fn phase2_is_denser_than_phase1() {
        let chart = generate_tutorial_chart();
        let phase1: Vec<f64> = chart
            .notes
            .iter()
            .filter(|n| n.hit_time_ms >= PHASE1_START_MS && n.hit_time_ms < PHASE2_START_MS)
            .map(|n| n.hit_time_ms)
            .collect();
        let phase2: Vec<f64> = chart
            .notes
            .iter()
            .filter(|n| n.hit_time_ms >= PHASE2_START_MS && n.hit_time_ms < PHASE3_START_MS)
            .map(|n| n.hit_time_ms)
            .collect();
        assert_eq!(phase2.len(), 4, "phase 2 should contain 4 notes");
        let p1_spacing = phase1[1] - phase1[0];
        let p2_spacing = phase2[1] - phase2[0];
        assert!(
            p2_spacing < p1_spacing,
            "phase 2 ({p2_spacing}ms) must be tighter than phase 1 ({p1_spacing}ms)",
        );
    }

    #[test]
    fn phase3_has_one_long_note() {
        let chart = generate_tutorial_chart();
        let phase3: Vec<&Note> = chart
            .notes
            .iter()
            .filter(|n| n.hit_time_ms >= PHASE3_START_MS && n.hit_time_ms < PHASE4_START_MS)
            .collect();
        assert_eq!(phase3.len(), 1, "phase 3 is a single long note");
        assert!(phase3[0].is_long(), "phase 3 note must be a hold");
        assert!(
            phase3[0].length_ms >= 1_000.0,
            "phase 3 hold should be substantial, got {}ms",
            phase3[0].length_ms,
        );
    }

    #[test]
    fn narration_script_matches_phase_table() {
        // Walk the script at representative times inside each phase and
        // check the label. Guards against silent drift between the chart
        // generator and the narration text.
        assert_eq!(narration_at(0.0), INTRO_LABEL);
        assert_eq!(narration_at(2_999.0), INTRO_LABEL);
        assert_eq!(narration_at(PHASE1_START_MS), PHASE1_LABEL);
        assert_eq!(narration_at(5_000.0), PHASE1_LABEL);
        assert_eq!(narration_at(PHASE2_START_MS), PHASE2_LABEL);
        assert_eq!(narration_at(10_000.0), PHASE2_LABEL);
        assert_eq!(narration_at(PHASE3_START_MS), PHASE3_LABEL);
        assert_eq!(narration_at(15_000.0), PHASE3_LABEL);
        assert_eq!(narration_at(PHASE4_START_MS), PHASE4_LABEL);
        assert_eq!(narration_at(20_000.0), PHASE4_LABEL);
    }

    #[test]
    fn narration_hook_delegates_to_phase_table() {
        // Make sure the struct hook and the free function agree — they're
        // the same logic, but exercising both paths documents the contract
        // for anyone swapping in a custom hook.
        let chart = generate_tutorial_chart();
        let mut hook = TutorialNarrator;
        let state = GameState::new(chart);
        assert_eq!(hook.narrate(&state, 1_000.0).as_deref(), Some(INTRO_LABEL),);
        assert_eq!(hook.narrate(&state, 5_000.0).as_deref(), Some(PHASE1_LABEL),);
        assert_eq!(
            hook.narrate(&state, 14_000.0).as_deref(),
            Some(PHASE3_LABEL),
        );
    }

    #[test]
    fn chart_metadata_is_populated() {
        let chart = generate_tutorial_chart();
        assert_eq!(chart.title, "Tutorial");
        assert!(!chart.artist.is_empty());
        assert!(chart.bpm > 0.0);
    }
}
