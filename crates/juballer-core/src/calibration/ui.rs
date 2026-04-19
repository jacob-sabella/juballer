use super::Profile;

/// Phase the calibration UI is currently in.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Geometry,
    Keymap { next_cell: (u8, u8) },
    Done,
    Cancelled,
}

#[derive(Debug)]
pub struct CalibrationState {
    pub phase: Phase,
    pub draft: Profile,
    pub original: Profile,
}

impl CalibrationState {
    pub fn new(profile: Profile) -> Self {
        Self {
            phase: Phase::Geometry,
            draft: profile.clone(),
            original: profile,
        }
    }

    /// Advance from Geometry → Keymap when the user confirms geometry.
    pub fn confirm_geometry(&mut self) {
        if matches!(self.phase, Phase::Geometry) {
            self.phase = Phase::Keymap { next_cell: (0, 0) };
            self.draft.keymap.clear();
        }
    }

    /// Record a keycode for the current cell and advance.
    pub fn record_key(&mut self, keycode: &str) {
        if let Phase::Keymap { next_cell } = self.phase {
            // Reject duplicates: if `keycode` already maps to a different cell, ignore.
            if self.draft.keymap.values().any(|v| v == keycode) {
                return;
            }
            let key = format!("{},{}", next_cell.0, next_cell.1);
            self.draft.keymap.insert(key, keycode.into());
            let next = match next_cell {
                (3, 3) => {
                    self.phase = Phase::Done;
                    return;
                }
                (r, 3) => (r + 1, 0),
                (r, c) => (r, c + 1),
            };
            self.phase = Phase::Keymap { next_cell: next };
        }
    }

    pub fn cancel(&mut self) {
        self.phase = Phase::Cancelled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::Profile;

    fn fresh() -> CalibrationState {
        CalibrationState::new(Profile::default_for("a", "b", 1920, 1080))
    }

    #[test]
    fn geometry_to_keymap_to_done() {
        let mut s = fresh();
        assert_eq!(s.phase, Phase::Geometry);
        s.confirm_geometry();
        assert_eq!(s.phase, Phase::Keymap { next_cell: (0, 0) });
        for i in 0..16u8 {
            s.record_key(&format!("KEY_{}", i));
        }
        assert_eq!(s.phase, Phase::Done);
        assert_eq!(s.draft.keymap.len(), 16);
    }

    #[test]
    fn duplicate_keycode_is_rejected() {
        let mut s = fresh();
        s.confirm_geometry();
        s.record_key("KEY_DUPE"); // (0,0) accepted
        s.record_key("KEY_DUPE"); // (0,1) rejected
        assert_eq!(s.phase, Phase::Keymap { next_cell: (0, 1) });
    }

    #[test]
    fn cancel_terminates() {
        let mut s = fresh();
        s.cancel();
        assert_eq!(s.phase, Phase::Cancelled);
    }

    #[test]
    fn row_advance() {
        let mut s = fresh();
        s.confirm_geometry();
        for i in 0..4 {
            s.record_key(&format!("K{}", i));
        }
        assert_eq!(s.phase, Phase::Keymap { next_cell: (1, 0) });
    }
}
