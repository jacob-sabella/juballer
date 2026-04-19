//! Note scheduling primitives. Pure data — no GPU or audio.

use super::chart::Note;
use super::judge::Grade;

/// A note tracked through the session.
///
/// For tap notes, `hit` is set the moment a keypress registers (or when the
/// note ages past MISS) and the score is counted right then.
///
/// For long notes the flow is two-stage: head press sets `head_press`
/// (recording the head grade + delta) but does NOT finalize the note. The
/// player must continue holding the cell until `release_time_ms`. The release
/// — whether it's a real KeyUp, an early-release downgrade, or an auto-MISS
/// from never letting go — is what eventually fills `hit` and counts the
/// note for score.
#[derive(Debug, Clone, Copy)]
pub struct ScheduledNote {
    pub note: Note,
    pub hit: Option<HitOutcome>,
    pub head_press: Option<HeadPress>,
}

#[derive(Debug, Clone, Copy)]
pub struct HitOutcome {
    pub grade: Grade,
    /// Delta relative to the note's hit_time_ms (positive = player late).
    /// `None` for MISS — no keypress happened.
    pub delta_ms: Option<f64>,
    /// music_time_ms at the moment this note was finalized. For real presses
    /// this is the press_ms; for auto-misses it's the tick that moved the
    /// note past the MISS window. Used by the renderer to anchor the
    /// judgment-freeze visual so every grade (including Miss) gets a burst.
    pub judged_at_ms: f64,
}

/// Recorded head-press for a long note that's still in its hold phase. When
/// the player eventually releases (or runs out the clock) this gets combined
/// with the release timing to produce the final HitOutcome.
#[derive(Debug, Clone, Copy)]
pub struct HeadPress {
    pub press_ms: f64,
    pub head_grade: Grade,
    pub head_delta_ms: f64,
}

impl ScheduledNote {
    pub fn new(note: Note) -> Self {
        Self {
            note,
            hit: None,
            head_press: None,
        }
    }

    /// True once the note has been finalized (score-counted).
    pub fn is_judged(&self) -> bool {
        self.hit.is_some()
    }

    /// True if a long note's head was pressed and we're now in the hold phase
    /// (not yet finalized via release).
    pub fn is_holding(&self) -> bool {
        self.head_press.is_some() && self.hit.is_none()
    }
}
