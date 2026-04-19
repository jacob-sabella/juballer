//! Timing windows + grading.
//!
//! Windows match rhythm-game defaults (approximate): Perfect ±42ms, Great ±82ms,
//! Good ±125ms, Poor ±200ms, otherwise MISS.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Grade {
    Perfect,
    Great,
    Good,
    Poor,
    Miss,
}

impl Grade {
    /// Score contribution; combo multiplier is applied elsewhere.
    pub fn base_score(self) -> u32 {
        match self {
            Grade::Perfect => 1000,
            Grade::Great => 500,
            Grade::Good => 100,
            Grade::Poor => 50,
            Grade::Miss => 0,
        }
    }

    /// True if this grade keeps (or extends) the combo. MISS and POOR break combo.
    pub fn keeps_combo(self) -> bool {
        matches!(self, Grade::Perfect | Grade::Great | Grade::Good)
    }

    pub fn label(self) -> &'static str {
        match self {
            Grade::Perfect => "PERFECT",
            Grade::Great => "GREAT",
            Grade::Good => "GOOD",
            Grade::Poor => "POOR",
            Grade::Miss => "MISS",
        }
    }
}

/// Classify a keypress whose music-time lies `delta_ms` away from the target note's
/// `hit_time_ms` (positive = late). Returns `None` when the delta falls outside
/// every window — i.e. too early/too late for this note to claim it at all.
pub fn judge(delta_ms: f64) -> Option<Grade> {
    let a = delta_ms.abs();
    if a <= 42.0 {
        Some(Grade::Perfect)
    } else if a <= 82.0 {
        Some(Grade::Great)
    } else if a <= 125.0 {
        Some(Grade::Good)
    } else if a <= 200.0 {
        Some(Grade::Poor)
    } else {
        None
    }
}

/// Outer MISS deadline in ms past the note's hit_time_ms. Anything beyond this is
/// auto-MISSed by the game loop (no input arrived in time).
pub const MISS_WINDOW_MS: f64 = 200.0;

/// Inclusive window used for binding a keypress to the closest unjudged note. Matches
/// the MISS window — a press >200 ms early or late won't claim any note.
pub const HIT_WINDOW_MS: f64 = 200.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_windows() {
        // Perfect: edges 0 and ±42 inclusive.
        assert_eq!(judge(0.0), Some(Grade::Perfect));
        assert_eq!(judge(42.0), Some(Grade::Perfect));
        assert_eq!(judge(-42.0), Some(Grade::Perfect));
        // Just past Perfect.
        assert_eq!(judge(42.1), Some(Grade::Great));
        assert_eq!(judge(-42.1), Some(Grade::Great));
        // Great upper bound.
        assert_eq!(judge(82.0), Some(Grade::Great));
        assert_eq!(judge(82.1), Some(Grade::Good));
        // Good upper bound.
        assert_eq!(judge(125.0), Some(Grade::Good));
        assert_eq!(judge(125.1), Some(Grade::Poor));
        // Poor upper bound.
        assert_eq!(judge(200.0), Some(Grade::Poor));
        // Past the widest window.
        assert_eq!(judge(200.01), None);
        assert_eq!(judge(-500.0), None);
    }

    #[test]
    fn combo_semantics() {
        assert!(Grade::Perfect.keeps_combo());
        assert!(Grade::Great.keeps_combo());
        assert!(Grade::Good.keeps_combo());
        assert!(!Grade::Poor.keeps_combo());
        assert!(!Grade::Miss.keeps_combo());
    }

    #[test]
    fn scoring_monotonic() {
        assert!(Grade::Perfect.base_score() > Grade::Great.base_score());
        assert!(Grade::Great.base_score() > Grade::Good.base_score());
        assert!(Grade::Good.base_score() > Grade::Poor.base_score());
        assert!(Grade::Poor.base_score() > Grade::Miss.base_score());
    }
}
