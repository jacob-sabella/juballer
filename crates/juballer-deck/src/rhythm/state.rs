//! Rhythm game state + hit algorithm.
//!
//! `GameState` is the pure model: notes + counters + latency stats. It has no
//! dependency on audio, rendering, or input backends. The caller (rhythm::play)
//! feeds it events + music-time on every frame and reads back the snapshot.

use super::chart::{Chart, Note};
use super::judge::{self, Grade, HIT_WINDOW_MS, MISS_WINDOW_MS};
use super::notes::{HeadPress, HitOutcome, ScheduledNote};
use std::collections::HashMap;

/// Tier ordering used for combining two grades. Lower = better.
fn grade_rank(g: Grade) -> u8 {
    match g {
        Grade::Perfect => 0,
        Grade::Great => 1,
        Grade::Good => 2,
        Grade::Poor => 3,
        Grade::Miss => 4,
    }
}

/// Pick the worse of two grades — used to combine a long note's head + release.
fn worst_grade(a: Grade, b: Grade) -> Grade {
    if grade_rank(a) >= grade_rank(b) {
        a
    } else {
        b
    }
}

/// How many ms ahead of its hit time a note becomes eligible to draw.
/// 470 ms is the reference 1.0× lead window (a 16-frame marker animation at
/// ~34 fps). Override per session via `RhythmConfig.lead_in_ms` (deck.toml
/// `[rhythm] lead_in_ms`).
pub const RENDER_LEAD_MS: f64 = 470.0;
/// How many ms past its hit time a note remains drawable (gives the judgment
/// flash room to animate before the cell clears).
pub const RENDER_TRAIL_MS: f64 = MISS_WINDOW_MS;
/// Tolerance window around a long-note's release_time_ms. A KeyUp inside this
/// window judges the release on the standard scale; outside it the release is
/// graded POOR (early release) or auto-MISS (held past the window).
pub const RELEASE_WINDOW_MS: f64 = MISS_WINDOW_MS;

/// Life-bar delta applied per judged note, indexed by grade. Perfect/Great
/// regenerate the bar, Poor/Miss drain it. Tuned so that a player averaging
/// ≥90% accuracy (mostly Perfect/Great with the odd Miss) never depletes the
/// bar, while a player around 50% accuracy depletes it quickly.
pub fn life_delta_for(grade: Grade) -> f32 {
    match grade {
        Grade::Perfect => 0.02,
        Grade::Great => 0.01,
        Grade::Good => 0.0,
        Grade::Poor => -0.04,
        Grade::Miss => -0.08,
    }
}

pub struct GameState {
    pub chart: Chart,
    pub notes: Vec<ScheduledNote>,
    pub music_time_ms: f64,
    pub score: u64,
    pub combo: u32,
    pub max_combo: u32,
    pub grade_counts: HashMap<Grade, u32>,
    /// Every judged non-MISS note contributes its `delta_ms`. Used at exit to
    /// report the mean offset so the player can tune `--audio-offset-ms`.
    pub input_offset_samples: Vec<f64>,
    pub finished: bool,
    /// Hard-fail life bar in [0.0, 1.0]. Starts full; drains on Poor/Miss
    /// (see `life_delta_for`). Hitting 0.0 flips `failed` to true and the
    /// caller is expected to bail out of the play loop.
    pub life: f32,
    /// True once `life` hit 0. Terminal — once set, stays set.
    pub failed: bool,
    /// Personal best score for this (chart, difficulty) loaded from the score
    /// book at session start. `None` means "never played" (or score book
    /// disabled). Displayed in the HUD alongside the live / final score.
    pub best_score: Option<u64>,
    /// No-fail mod: clamp life above 0 so the player can never see the
    /// FAILED banner during a run. Set once at session start from
    /// `RhythmConfig.mods.no_fail`; never mutated mid-session.
    pub no_fail: bool,
    /// Lead-in window in ms — how long before a note's `hit_time_ms` its
    /// approach visual starts drawing. Defaults to [`RENDER_LEAD_MS`] but
    /// rhythm mode can override it from `RhythmConfig.lead_in_ms` so the
    /// player can tune reaction-time difficulty.
    pub lead_in_ms: f64,
}

impl GameState {
    pub fn new(chart: Chart) -> Self {
        let notes = chart
            .notes
            .iter()
            .copied()
            .map(ScheduledNote::new)
            .collect();
        Self {
            chart,
            notes,
            music_time_ms: 0.0,
            score: 0,
            combo: 0,
            max_combo: 0,
            grade_counts: HashMap::new(),
            input_offset_samples: Vec::new(),
            finished: false,
            life: 1.0,
            failed: false,
            best_score: None,
            no_fail: false,
            lead_in_ms: RENDER_LEAD_MS,
        }
    }

    /// Apply a keypress at (row, col) that occurred at music-time `press_ms`.
    /// Returns the grade awarded (if any). For tap notes that's the final
    /// grade; for long notes it's the *head* grade — the note isn't fully
    /// judged until the matching release. "None" means the press didn't match
    /// any unfinalized note at that cell within ±HIT_WINDOW_MS.
    pub fn on_press(&mut self, row: u8, col: u8, press_ms: f64) -> Option<Grade> {
        // Find the unjudged, not-yet-pressed note at this cell closest (by
        // absolute delta) to the press. Prefer the earliest hit_time_ms on ties
        // so dense stacks resolve deterministically.
        let mut best: Option<(usize, f64)> = None;
        for (i, sn) in self.notes.iter().enumerate() {
            if sn.is_judged() || sn.head_press.is_some() {
                continue;
            }
            if sn.note.row != row || sn.note.col != col {
                continue;
            }
            let delta = press_ms - sn.note.hit_time_ms;
            if delta.abs() > HIT_WINDOW_MS {
                continue;
            }
            match best {
                None => best = Some((i, delta)),
                Some((_, bd)) if delta.abs() < bd.abs() => best = Some((i, delta)),
                _ => {}
            }
        }
        let (idx, delta) = best?;
        let grade = judge::judge(delta).unwrap_or(Grade::Poor);
        if self.notes[idx].note.is_long() {
            // Stash the head press; final grade is decided on release.
            self.notes[idx].head_press = Some(HeadPress {
                press_ms,
                head_grade: grade,
                head_delta_ms: delta,
            });
        } else {
            self.apply_grade(idx, grade, Some(delta));
        }
        Some(grade)
    }

    /// Apply a key release at (row, col) that occurred at music-time
    /// `release_ms`. Only meaningful for long notes whose head was already
    /// pressed — looks up the held note at this cell and finalizes it. Returns
    /// the final combined grade if a held note was finalized.
    pub fn on_release(&mut self, row: u8, col: u8, release_ms: f64) -> Option<Grade> {
        let idx = self
            .notes
            .iter()
            .position(|sn| sn.is_holding() && sn.note.row == row && sn.note.col == col)?;
        Some(self.finalize_long_release(idx, release_ms))
    }

    /// Called each frame after event processing. Advances the game clock and
    /// resolves anything that's run out of time:
    ///   * Untouched tap notes past their MISS window → MISS.
    ///   * Untouched long-note heads past their MISS window → MISS (whole note).
    ///   * Held long notes still down past `release_time + RELEASE_WINDOW` →
    ///     auto-finalized (the player overheld; counts as if released late).
    pub fn tick(&mut self, music_time_ms: f64) {
        self.music_time_ms = music_time_ms;
        let head_cutoff = music_time_ms - MISS_WINDOW_MS;
        let release_cutoff = music_time_ms - RELEASE_WINDOW_MS;
        for i in 0..self.notes.len() {
            if self.notes[i].is_judged() {
                continue;
            }
            // Held long note that overshot its release window → auto-finalize
            // as if released at `release_time + RELEASE_WINDOW`.
            if self.notes[i].is_holding() {
                if self.notes[i].note.release_time_ms() < release_cutoff {
                    let auto_release_ms = self.notes[i].note.release_time_ms() + RELEASE_WINDOW_MS;
                    self.finalize_long_release(i, auto_release_ms);
                }
                continue;
            }
            // Untouched note: if its head time is past the MISS window, MISS.
            if self.notes[i].note.hit_time_ms < head_cutoff {
                self.apply_grade(i, Grade::Miss, None);
            }
        }
    }

    /// Combine a long note's head press + a release time into one final grade
    /// and apply it. Worse of {head, release} wins so a sloppy head + perfect
    /// release still counts as sloppy. Early release (release_ms before the
    /// note's release_time_ms) is treated as MISS-tier on the release side.
    fn finalize_long_release(&mut self, idx: usize, release_ms: f64) -> Grade {
        let head = self.notes[idx]
            .head_press
            .expect("finalize requires head_press");
        let release_target = self.notes[idx].note.release_time_ms();
        let release_delta = release_ms - release_target;
        // Inside ±RELEASE_WINDOW_MS the release grades on the normal scale.
        // Outside, the release is treated as MISS on its side.
        let release_grade = judge::judge(release_delta).unwrap_or(Grade::Miss);
        let final_grade = worst_grade(head.head_grade, release_grade);
        // We sample the *head* delta for offset calibration — it's the
        // intentional press timing the player controls; release timing is
        // contaminated by hold-duration error.
        self.apply_grade(idx, final_grade, Some(head.head_delta_ms));
        final_grade
    }

    fn apply_grade(&mut self, idx: usize, grade: Grade, delta: Option<f64>) {
        // Anchor the judgment-freeze visual at the actual moment of judgment,
        // not at the note's hit_time. For real presses that's press_ms =
        // hit_time + delta; for auto-misses it's the current music clock,
        // which has just crossed past hit_time + MISS_WINDOW.
        let judged_at_ms = match delta {
            Some(d) => self.notes[idx].note.hit_time_ms + d,
            None => self.music_time_ms,
        };
        self.notes[idx].hit = Some(HitOutcome {
            grade,
            delta_ms: delta,
            judged_at_ms,
        });
        *self.grade_counts.entry(grade).or_insert(0) += 1;
        self.score += grade.base_score() as u64;
        if grade.keeps_combo() {
            self.combo += 1;
            self.max_combo = self.max_combo.max(self.combo);
        } else {
            self.combo = 0;
        }
        if let Some(d) = delta {
            if grade != Grade::Miss {
                self.input_offset_samples.push(d);
            }
        }
        // Update life bar. Clamp to [0, 1]; flip `failed` when it hits 0.
        // `failed` is terminal — don't let a later hit regen it back above 0.
        // With the no-fail mod, life floor is 0.0 but `failed` never flips,
        // so the player keeps playing regardless of Misses.
        if !self.failed {
            self.life = (self.life + life_delta_for(grade)).clamp(0.0, 1.0);
            if self.life <= 0.0 {
                self.life = 0.0;
                if !self.no_fail {
                    self.failed = true;
                }
            }
        }
    }

    /// Mean signed delta of non-MISS keypresses, in ms. `None` when no samples yet.
    /// Positive = the player was on average late (→ set `--audio-offset-ms` negative).
    pub fn mean_input_offset_ms(&self) -> Option<f64> {
        if self.input_offset_samples.is_empty() {
            None
        } else {
            let sum: f64 = self.input_offset_samples.iter().sum();
            Some(sum / self.input_offset_samples.len() as f64)
        }
    }

    /// Suggested `--audio-offset-ms` for the next run, rounded to the nearest
    /// ms. The sign is the negative of the mean input offset so that if the
    /// player was on average late by +12ms, they should pass `-12` to make the
    /// audio arrive 12ms earlier relative to their presses. `None` when fewer
    /// than 8 samples were collected — below that, the mean is too noisy to
    /// recommend acting on.
    pub fn recommended_audio_offset_ms(&self) -> Option<i32> {
        if self.input_offset_samples.len() < 8 {
            return None;
        }
        let mean = self.mean_input_offset_ms()?;
        Some((-mean).round() as i32)
    }

    /// Accuracy percentage: judged non-MISS notes over the total chart. 100.0
    /// means every note landed at least POOR. `None` for empty charts.
    pub fn accuracy_pct(&self) -> Option<f64> {
        let total = self.notes.len();
        if total == 0 {
            return None;
        }
        let hits: usize = self
            .notes
            .iter()
            .filter(|sn| matches!(sn.hit.map(|h| h.grade), Some(g) if g != Grade::Miss))
            .count();
        Some((hits as f64 / total as f64) * 100.0)
    }

    pub fn total_notes(&self) -> usize {
        self.notes.len()
    }

    pub fn judged_notes(&self) -> usize {
        self.notes.iter().filter(|n| n.is_judged()).count()
    }

    /// Filter notes visible to the renderer. A note is renderable from
    /// `hit_time - RENDER_LEAD_MS` until `release_time + RENDER_TRAIL_MS`
    /// (where `release_time = hit_time + length_ms`, so tap notes collapse to
    /// `hit_time + TRAIL`). Judged notes get an extra TRAIL of grace so their
    /// freeze/fade animation has room to play out.
    pub fn renderable_notes(&self) -> impl Iterator<Item = &ScheduledNote> {
        let now = self.music_time_ms;
        self.notes.iter().filter(move |sn| {
            let start = sn.note.hit_time_ms - self.lead_in_ms;
            let mut end = sn.note.release_time_ms() + RENDER_TRAIL_MS;
            if sn.is_judged() {
                end += RENDER_TRAIL_MS;
            }
            (start..=end).contains(&now)
        })
    }

    pub fn count(&self, g: Grade) -> u32 {
        *self.grade_counts.get(&g).unwrap_or(&0)
    }
}

/// Which note each grid cell should currently render (if any). Picks the
/// earliest unjudged note at that cell within the render window, and also
/// counts how many *other* unfinalized notes at that cell are waiting behind
/// it — that count becomes `stack_count` so the renderer can show a
/// "more coming" badge on the tile. Returns a 16-element array indexed by
/// `row * 4 + col`.
pub fn render_slots(state: &GameState) -> [Option<RenderSlot>; 16] {
    const NONE: Option<RenderSlot> = None;
    let mut out: [Option<RenderSlot>; 16] = [NONE; 16];
    for sn in state.renderable_notes() {
        let idx = (sn.note.row as usize) * 4 + sn.note.col as usize;
        let cand = RenderSlot {
            note: sn.note,
            hit: sn.hit,
            holding: sn.is_holding(),
            stack_count: 0,
        };
        match &out[idx] {
            None => out[idx] = Some(cand),
            Some(cur) => {
                // Prefer unjudged over judged; within the same judged-status, pick
                // the earliest hit_time_ms so dense stacks resolve deterministically.
                let cur_unjudged = cur.hit.is_none();
                let cand_unjudged = cand.hit.is_none();
                let replace = match (cand_unjudged, cur_unjudged) {
                    (true, false) => true,
                    (false, true) => false,
                    _ => cand.note.hit_time_ms < cur.note.hit_time_ms,
                };
                if replace {
                    out[idx] = Some(cand);
                }
            }
        }
    }
    // Second pass: count pending siblings per cell. A "pending sibling" is
    // another renderable note at the same cell that's not yet judged — i.e.
    // still tappable, so the player needs to know it's coming after the
    // currently-displayed note clears.
    for (idx, slot) in out.iter_mut().enumerate() {
        let Some(slot) = slot else { continue };
        let r = (idx / 4) as u8;
        let c = (idx % 4) as u8;
        let mut count: u32 = 0;
        for sn in state.renderable_notes() {
            if sn.note.row != r || sn.note.col != c {
                continue;
            }
            if sn.is_judged() {
                continue;
            }
            // Don't count the slot's own note.
            if (sn.note.hit_time_ms - slot.note.hit_time_ms).abs() < 1e-6
                && sn.note.length_ms == slot.note.length_ms
            {
                continue;
            }
            count += 1;
        }
        slot.stack_count = count.min(9) as u8;
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub struct RenderSlot {
    pub note: Note,
    pub hit: Option<HitOutcome>,
    /// True for long notes whose head was pressed but not yet released — the
    /// renderer should show the cell in "held" state (sustained color, trail
    /// shrinking toward release_time).
    pub holding: bool,
    /// How many other pending (unfinalized) notes live at this cell right now,
    /// excluding the one this slot renders. Capped at 9; drives the stack
    /// indicator badge.
    pub stack_count: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhythm::chart::{BpmEntry, BpmSchedule, Chart};

    fn chart_from_notes(notes: Vec<Note>) -> Chart {
        let schedule = BpmSchedule::new(
            &[BpmEntry {
                beat: 0,
                bpm: 120.0,
            }],
            240,
        )
        .expect("single-segment schedule");
        Chart {
            title: "T".into(),
            artist: "A".into(),
            audio_path: std::path::PathBuf::new(),
            bpm: 120.0,
            offset_ms: 0.0,
            notes,
            schedule,
            preview: None,
            jacket_path: None,
            banner_path: None,
            mini_path: None,
        }
    }

    fn tap(t: f64, row: u8, col: u8) -> Note {
        Note {
            hit_time_ms: t,
            row,
            col,
            length_ms: 0.0,
            tail_row: row,
            tail_col: col,
        }
    }

    fn long(t: f64, row: u8, col: u8, length_ms: f64) -> Note {
        Note {
            hit_time_ms: t,
            row,
            col,
            length_ms,
            tail_row: row,
            tail_col: col,
        }
    }

    #[test]
    fn hit_marks_nearest_note() {
        // Three notes at (0,0): 1000ms, 1500ms, 2000ms.
        let notes = vec![tap(1000.0, 0, 0), tap(1500.0, 0, 0), tap(2000.0, 0, 0)];
        let mut st = GameState::new(chart_from_notes(notes));
        // Press at 1480ms — 20ms early on the middle note. Should pick that one (not 1000).
        let g = st.on_press(0, 0, 1480.0).unwrap();
        assert_eq!(g, Grade::Perfect);
        assert!(!st.notes[0].is_judged());
        assert!(st.notes[1].is_judged());
        assert!(!st.notes[2].is_judged());
    }

    #[test]
    fn press_at_wrong_cell_misses() {
        let notes = vec![tap(500.0, 0, 0)];
        let mut st = GameState::new(chart_from_notes(notes));
        let g = st.on_press(1, 1, 500.0);
        assert_eq!(g, None);
        assert!(!st.notes[0].is_judged());
    }

    #[test]
    fn press_outside_window_returns_none() {
        let notes = vec![tap(500.0, 0, 0)];
        let mut st = GameState::new(chart_from_notes(notes));
        let g = st.on_press(0, 0, 800.0); // 300ms late — outside ±200.
        assert_eq!(g, None);
    }

    #[test]
    fn tick_auto_misses_old_notes() {
        let notes = vec![tap(100.0, 0, 0), tap(5000.0, 0, 0)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.tick(500.0);
        assert!(st.notes[0].is_judged());
        assert_eq!(st.notes[0].hit.unwrap().grade, Grade::Miss);
        assert!(!st.notes[1].is_judged());
    }

    #[test]
    fn combo_breaks_on_miss() {
        let notes = vec![tap(100.0, 0, 0), tap(200.0, 0, 1), tap(1000.0, 0, 2)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.on_press(0, 0, 100.0).unwrap();
        st.on_press(0, 1, 200.0).unwrap();
        assert_eq!(st.combo, 2);
        st.tick(2000.0);
        assert_eq!(st.combo, 0);
        assert_eq!(st.max_combo, 2);
    }

    #[test]
    fn long_note_held_to_end_grades_perfect() {
        // Hold from 1000 to 2000ms (1s). Press at 1000 (Perfect head), release at 2000 (Perfect tail).
        let notes = vec![long(1000.0, 0, 0, 1000.0)];
        let mut st = GameState::new(chart_from_notes(notes));
        let head = st.on_press(0, 0, 1000.0).unwrap();
        assert_eq!(head, Grade::Perfect);
        // Note isn't finalized yet.
        assert!(!st.notes[0].is_judged());
        assert!(st.notes[0].is_holding());
        // Release on the dot.
        let final_g = st.on_release(0, 0, 2000.0).unwrap();
        assert_eq!(final_g, Grade::Perfect);
        assert!(st.notes[0].is_judged());
        assert!(!st.notes[0].is_holding());
    }

    #[test]
    fn long_note_early_release_misses_release_side() {
        // Hold from 1000 to 2000. Press at 1000 (Perfect), release way early at 1100.
        let notes = vec![long(1000.0, 0, 0, 1000.0)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.on_press(0, 0, 1000.0).unwrap();
        // Release 900ms too early — way outside the release window.
        let final_g = st.on_release(0, 0, 1100.0).unwrap();
        // Release timing was MISS-tier; combined with Perfect head → MISS.
        assert_eq!(final_g, Grade::Miss);
    }

    #[test]
    fn long_note_overhold_auto_finalizes_via_tick() {
        // Hold from 1000 to 2000. Press perfectly at 1000, never release.
        let notes = vec![long(1000.0, 0, 0, 1000.0)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.on_press(0, 0, 1000.0).unwrap();
        assert!(st.notes[0].is_holding());
        // Far past release+window, tick should auto-finalize.
        st.tick(3000.0);
        assert!(st.notes[0].is_judged());
        assert!(!st.notes[0].is_holding());
    }

    #[test]
    fn long_note_head_missed_auto_misses_whole_note() {
        // Long note 1000..2000 never pressed at all.
        let notes = vec![long(1000.0, 0, 0, 1000.0)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.tick(1500.0); // past head MISS window (1000+200=1200)
        assert!(st.notes[0].is_judged());
        assert_eq!(st.notes[0].hit.unwrap().grade, Grade::Miss);
    }

    #[test]
    fn release_without_held_note_returns_none() {
        let notes = vec![tap(500.0, 0, 0)];
        let mut st = GameState::new(chart_from_notes(notes));
        // No long note, no held state; on_release should be a no-op.
        assert_eq!(st.on_release(0, 0, 500.0), None);
    }

    #[test]
    fn render_slots_counts_pending_stack() {
        // Three taps at (0,0) packed within the render-lead window so all
        // three are eligible to draw at music_time_ms == first hit time.
        // Spacing = RENDER_LEAD_MS / 3 keeps the chain inside the window
        // regardless of the tuned default.
        let step = RENDER_LEAD_MS / 3.0;
        let t0 = 1000.0;
        let notes = vec![
            tap(t0, 0, 0),
            tap(t0 + step, 0, 0),
            tap(t0 + 2.0 * step, 0, 0),
        ];
        let mut st = GameState::new(chart_from_notes(notes));
        st.tick(t0);
        let slots = render_slots(&st);
        let slot = slots[0].expect("cell (0,0) should have a slot");
        assert!((slot.note.hit_time_ms - t0).abs() < 1e-6);
        assert_eq!(slot.stack_count, 2);
        // After the first is hit, count drops.
        st.on_press(0, 0, t0).unwrap();
        let slots2 = render_slots(&st);
        let slot2 = slots2[0].expect("second tap still renderable");
        assert!((slot2.note.hit_time_ms - (t0 + step)).abs() < 1e-6);
        assert_eq!(slot2.stack_count, 1);
    }

    #[test]
    fn recommended_offset_is_negated_mean_after_enough_samples() {
        // 8 notes at 100ms apart, player consistently 10ms late.
        let mut notes = Vec::new();
        for i in 0..8 {
            notes.push(tap(1000.0 + i as f64 * 100.0, 0, i));
        }
        let mut st = GameState::new(chart_from_notes(notes));
        for i in 0..8 {
            st.on_press(0, i, 1010.0 + i as f64 * 100.0).unwrap();
        }
        assert!((st.mean_input_offset_ms().unwrap() - 10.0).abs() < 1e-6);
        // Player late by +10 → recommend -10 next run.
        assert_eq!(st.recommended_audio_offset_ms(), Some(-10));
    }

    #[test]
    fn recommended_offset_needs_min_samples() {
        // Only 2 samples — too noisy to recommend.
        let notes = vec![tap(100.0, 0, 0), tap(200.0, 0, 1)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.on_press(0, 0, 105.0).unwrap();
        st.on_press(0, 1, 205.0).unwrap();
        assert_eq!(st.recommended_audio_offset_ms(), None);
    }

    #[test]
    fn accuracy_counts_non_miss() {
        let notes = vec![tap(100.0, 0, 0), tap(200.0, 0, 1), tap(300.0, 0, 2)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.on_press(0, 0, 100.0).unwrap(); // Perfect
        st.on_press(0, 1, 300.0); // 100ms late → Good
        st.tick(600.0); // third auto-MISS
        let a = st.accuracy_pct().unwrap();
        // 2 of 3 non-miss → ~66.67.
        assert!((a - (200.0 / 3.0)).abs() < 1e-6);
    }

    #[test]
    fn life_starts_full() {
        let notes = vec![tap(100.0, 0, 0)];
        let st = GameState::new(chart_from_notes(notes));
        assert_eq!(st.life, 1.0);
        assert!(!st.failed);
    }

    #[test]
    fn life_delta_matches_spec() {
        assert!((life_delta_for(Grade::Perfect) - 0.02).abs() < 1e-6);
        assert!((life_delta_for(Grade::Great) - 0.01).abs() < 1e-6);
        assert!((life_delta_for(Grade::Good) - 0.0).abs() < 1e-6);
        assert!((life_delta_for(Grade::Poor) - (-0.04)).abs() < 1e-6);
        assert!((life_delta_for(Grade::Miss) - (-0.08)).abs() < 1e-6);
    }

    #[test]
    fn life_clamps_at_one_on_all_perfects() {
        // 16 perfect taps should not push life over 1.0.
        let mut notes = Vec::new();
        for i in 0..16 {
            notes.push(tap(100.0 + i as f64 * 100.0, 0, (i % 4) as u8));
        }
        let mut st = GameState::new(chart_from_notes(notes.clone()));
        for (i, n) in notes.iter().enumerate() {
            st.on_press(n.row, n.col, 100.0 + i as f64 * 100.0).unwrap();
        }
        assert_eq!(st.life, 1.0);
        assert!(!st.failed);
    }

    #[test]
    fn life_fails_after_enough_misses() {
        // 20 notes, all missed. 1.0 / 0.08 = 12.5 → 13 misses are enough.
        let mut notes = Vec::new();
        for i in 0..20 {
            notes.push(tap(100.0 + i as f64 * 100.0, 0, (i % 4) as u8));
        }
        let mut st = GameState::new(chart_from_notes(notes));
        // Tick past all miss windows.
        st.tick(10_000.0);
        assert!(st.failed);
        assert_eq!(st.life, 0.0);
    }

    #[test]
    fn life_miss_drain_is_correct() {
        let notes = vec![tap(100.0, 0, 0)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.tick(10_000.0); // auto-MISS
        assert!((st.life - 0.92).abs() < 1e-6);
        assert!(!st.failed);
    }

    #[test]
    fn life_stays_failed_after_hits() {
        // Force failure, then land a Perfect — life/failed should not revert.
        let mut notes = Vec::new();
        for i in 0..15 {
            notes.push(tap(100.0 + i as f64 * 10.0, 0, (i % 4) as u8));
        }
        // One extra note we can hit cleanly after failure.
        notes.push(tap(20_000.0, 0, 0));
        let mut st = GameState::new(chart_from_notes(notes));
        // Tick past the miss window for the first 15.
        st.tick(1_000.0);
        assert!(st.failed);
        // Now land a Perfect on the last note.
        st.on_press(0, 0, 20_000.0).unwrap();
        assert!(st.failed);
        assert_eq!(st.life, 0.0);
    }

    #[test]
    fn mean_offset_excludes_misses() {
        let notes = vec![tap(100.0, 0, 0), tap(200.0, 0, 1)];
        let mut st = GameState::new(chart_from_notes(notes));
        st.on_press(0, 0, 110.0).unwrap(); // +10ms
        st.on_press(0, 1, 220.0).unwrap(); // +20ms
        let m = st.mean_input_offset_ms().unwrap();
        assert!((m - 15.0).abs() < 1e-6);
    }
}
