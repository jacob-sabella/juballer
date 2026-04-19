//! Audio-offset calibration mode.
//!
//! Generates a short metronome chart in memory, runs it through the normal
//! rhythm loop, and prints a recommended `--audio-offset-ms` value on exit.
//! The user taps along with the click track; `GameState`'s existing
//! `recommended_audio_offset_ms()` does the math.
//!
//! Not intended to replace ear-tuning — it's a starting point. 12 seconds of
//! 120 BPM → 24 samples, well above the 8-sample minimum the recommender
//! requires.

use super::chart::{BpmEntry, BpmSchedule, Chart, Note};
use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// Build an in-memory calibration chart. Produces `beats` notes, one per beat,
/// all at `cell = (row, col)`. Timing is derived via the real [`BpmSchedule`]
/// so any future BPM-aware downstream code sees identical ms values to what
/// a disk-loaded chart would.
///
/// `beats = 0` returns an empty note list without panicking. `bpm` must be
/// positive — the underlying [`BpmSchedule::new`] rejects zero/negative.
pub fn generate_chart(bpm: f64, beats: usize, cell: (u8, u8)) -> Chart {
    let resolution: u32 = 240;
    let schedule = BpmSchedule::new(&[BpmEntry { beat: 0, bpm }], resolution)
        .expect("bpm > 0 for metronome chart");
    let (row, col) = cell;
    let mut notes = Vec::with_capacity(beats);
    for i in 0..beats {
        // Tick for beat `i` under the resolution we fed BpmSchedule.
        let tick = (i as i64) * (resolution as i64);
        let hit_time_ms = schedule.tick_to_ms(tick);
        notes.push(Note {
            hit_time_ms,
            row,
            col,
            length_ms: 0.0,
            tail_row: row,
            tail_col: col,
        });
    }
    Chart {
        title: "Calibration — 120 BPM metronome".to_string(),
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

/// Entry point for the `calibrate-audio` subcommand. Builds the metronome
/// chart, runs it through the normal rhythm loop at `user_offset_ms`, and
/// prints the recommended value (plus the exact command to paste) on exit.
pub fn run(user_offset_ms: i32) -> Result<()> {
    let bpm = 120.0;
    // 12s track at 120 BPM → 24 beats. Drop the very last one: the audio
    // tail + sink drain sometimes finalises the loop before the final beat's
    // MISS window closes, which the user sees as a "phantom miss" on the
    // banner. 23 beats leaves a half-second tail of silence.
    let beats = 23;
    let chart = generate_chart(bpm, beats, (1, 1));
    if !chart.audio_path.exists() {
        return Err(Error::Config(format!(
            "calibrate: metronome audio not found at {} (expected assets/sample/metronome.ogg)",
            chart.audio_path.display()
        )));
    }
    tracing::info!(
        target: "juballer::rhythm::calibrate",
        "starting audio calibration: {:.0} BPM × {} beats, tap cell (1,1) every beat",
        bpm,
        beats
    );
    println!("[calibrate] Tap cell (1,1) on every click. {beats} beats @ {bpm:.0} BPM.");
    println!("[calibrate] Recommended offset will be printed when the track ends.");
    // Calibration is pure timing; per-grade hit sounds would add another
    // latency signal that confuses the measurement. Always mute SFX here.
    //
    // `countdown_ms = 0` skips the pre-song countdown that gameplay normally
    // shows — the metronome should click immediately so the user tapping
    // along doesn't eat dead air, and the measurement window isn't shifted.
    super::play_chart_opts(
        chart,
        user_offset_ms,
        /*mute_sfx=*/ true,
        /*sfx_volume=*/ None,
        0,
    )
}

/// Resolve the metronome audio path. Mirrors `resolve_shader_path` in mod.rs —
/// prefer the in-tree asset (`CARGO_MANIFEST_DIR/../../assets/...`), then
/// exe-relative, then CWD as a last resort.
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
    // Fall through to the canonical location; caller will error with a clear
    // message if the file is genuinely absent.
    candidates.into_iter().next().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_chart_120_bpm_10_beats_cell_0_0() {
        let chart = generate_chart(120.0, 10, (0, 0));
        assert_eq!(chart.notes.len(), 10);
        // 120 BPM → 500ms per beat. Hit times 0, 500, 1000, …, 4500.
        for (i, n) in chart.notes.iter().enumerate() {
            let expected = (i as f64) * 500.0;
            assert!(
                (n.hit_time_ms - expected).abs() < 1e-6,
                "beat {i}: expected {expected}ms, got {}",
                n.hit_time_ms
            );
            assert_eq!(n.row, 0);
            assert_eq!(n.col, 0);
            assert_eq!(n.length_ms, 0.0);
        }
        assert!((chart.bpm - 120.0).abs() < 1e-6);
    }

    #[test]
    fn generate_chart_zero_beats_is_empty() {
        let chart = generate_chart(120.0, 0, (2, 3));
        assert_eq!(chart.notes.len(), 0);
        // The chart itself is still well-formed.
        assert!((chart.bpm - 120.0).abs() < 1e-6);
    }

    #[test]
    fn generate_chart_honours_cell_argument() {
        let chart = generate_chart(90.0, 4, (3, 2));
        assert_eq!(chart.notes.len(), 4);
        for n in &chart.notes {
            assert_eq!(n.row, 3);
            assert_eq!(n.col, 2);
        }
    }

    #[test]
    fn generate_chart_handles_non_120_bpm() {
        // 60 BPM → 1000ms/beat. 4 beats → 0, 1000, 2000, 3000.
        let chart = generate_chart(60.0, 4, (0, 0));
        let expected = [0.0, 1000.0, 2000.0, 3000.0];
        for (i, n) in chart.notes.iter().enumerate() {
            assert!((n.hit_time_ms - expected[i]).abs() < 1e-6);
        }
    }
}
