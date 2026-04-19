use crate::calibration::Profile;
use std::collections::HashMap;

/// Opaque keycode string (e.g. `"KEY_W"` on Linux, `"VK_W"` on Windows). The default
/// `winit` backend converts winit `Key` to a stable string; the raw-input backend uses
/// `evdev::KeyCode` (Linux) or Windows VKs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCode(pub String);

impl KeyCode {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// Reverse-lookup table built from a profile's `[keymap]` section.
#[derive(Debug, Default, Clone)]
pub struct Keymap {
    by_keycode: HashMap<String, (u8, u8)>,
}

impl Keymap {
    pub fn from_profile(p: &Profile) -> Self {
        let mut m = HashMap::with_capacity(16);
        for r in 0..4 {
            for c in 0..4 {
                let key = format!("{},{}", r, c);
                if let Some(kc) = p.keymap.get(&key) {
                    m.insert(kc.clone(), (r as u8, c as u8));
                }
            }
        }
        Self { by_keycode: m }
    }

    pub fn lookup(&self, key: &str) -> Option<(u8, u8)> {
        self.by_keycode.get(key).copied()
    }

    pub fn is_complete(&self) -> bool {
        self.by_keycode.len() == 16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_full_profile() -> Profile {
        let mut p = Profile::default_for("a", "b", 1920, 1080);
        for r in 0..4 {
            for c in 0..4 {
                p.keymap
                    .insert(format!("{},{}", r, c), format!("KEY_{}_{}", r, c));
            }
        }
        p
    }

    #[test]
    fn lookup_round_trip() {
        let p = make_full_profile();
        let m = Keymap::from_profile(&p);
        assert_eq!(m.lookup("KEY_2_3"), Some((2, 3)));
        assert_eq!(m.lookup("KEY_DOES_NOT_EXIST"), None);
        assert!(m.is_complete());
    }

    #[test]
    fn empty_profile_is_incomplete() {
        let p = Profile::default_for("a", "b", 1920, 1080);
        let m = Keymap::from_profile(&p);
        assert!(!m.is_complete());
    }
}
