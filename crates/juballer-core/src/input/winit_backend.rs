use super::{Event, KeyCode, Keymap};
use std::collections::HashSet;
use std::time::Instant;
use winit::event::ElementState;
use winit::keyboard::Key;

/// Stateful translator: winit `KeyEvent` → `juballer_core::input::Event`. Holds the set of
/// currently-down keys so it can suppress OS key-repeat (a held key fires KeyDown only once).
#[derive(Default)]
pub struct WinitInput {
    held: HashSet<String>,
}

impl WinitInput {
    /// Translate a winit keyboard event into zero or more `Event`s appended to `out`.
    ///
    /// Takes `logical_key` and `state` separately (rather than the full `winit::event::KeyEvent`)
    /// so this method is unit-testable without constructing the private-field `KeyEvent` struct.
    /// The call site in `window_event` destructures the `KeyEvent` to extract these two fields.
    pub fn translate(
        &mut self,
        logical_key: &Key,
        state: ElementState,
        keymap: &Keymap,
        out: &mut Vec<Event>,
    ) {
        let code = key_to_code(logical_key);
        let ts = Instant::now();
        match state {
            ElementState::Pressed => {
                if !self.held.insert(code.clone()) {
                    return; // repeat — already in held set, ignore
                }
                match keymap.lookup(&code) {
                    Some((row, col)) => out.push(Event::KeyDown {
                        row,
                        col,
                        key: KeyCode(code),
                        ts,
                    }),
                    None => out.push(Event::Unmapped {
                        key: KeyCode(code),
                        ts,
                    }),
                }
            }
            ElementState::Released => {
                if !self.held.remove(&code) {
                    return; // was never pressed (e.g. synthetic release), ignore
                }
                if let Some((row, col)) = keymap.lookup(&code) {
                    out.push(Event::KeyUp {
                        row,
                        col,
                        key: KeyCode(code),
                        ts,
                    });
                }
            }
        }
    }
}

fn key_to_code(k: &Key) -> String {
    match k {
        Key::Character(s) => format!("CHAR_{}", s.to_uppercase()),
        Key::Named(n) => format!("NAMED_{n:?}"),
        Key::Unidentified(_) => "UNIDENTIFIED".into(),
        Key::Dead(_) => "DEAD".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn mapped_keymap(entries: &[(&str, (u8, u8))]) -> Keymap {
        let mut p = crate::calibration::Profile::default_for("a", "b", 1920, 1080);
        for (k, (r, c)) in entries {
            p.keymap.insert(format!("{},{}", r, c), (*k).into());
        }
        Keymap::from_profile(&p)
    }

    fn char_key(ch: &str) -> Key {
        Key::Character(SmolStr::new(ch))
    }

    #[test]
    fn pressed_then_released_emits_keydown_keyup() {
        let mut wi = WinitInput::default();
        let km = mapped_keymap(&[("CHAR_W", (0, 0))]);
        let mut out = Vec::new();
        wi.translate(&char_key("w"), ElementState::Pressed, &km, &mut out);
        wi.translate(&char_key("w"), ElementState::Released, &km, &mut out);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0], Event::KeyDown { row: 0, col: 0, .. }));
        assert!(matches!(out[1], Event::KeyUp { row: 0, col: 0, .. }));
    }

    #[test]
    fn repeat_pressed_is_suppressed() {
        let mut wi = WinitInput::default();
        let km = mapped_keymap(&[("CHAR_W", (0, 0))]);
        let mut out = Vec::new();
        wi.translate(&char_key("w"), ElementState::Pressed, &km, &mut out);
        wi.translate(&char_key("w"), ElementState::Pressed, &km, &mut out); // repeat
        wi.translate(&char_key("w"), ElementState::Pressed, &km, &mut out); // repeat
        wi.translate(&char_key("w"), ElementState::Released, &km, &mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn unmapped_keys_emit_unmapped_event() {
        let mut wi = WinitInput::default();
        let km = mapped_keymap(&[]);
        let mut out = Vec::new();
        wi.translate(&char_key("x"), ElementState::Pressed, &km, &mut out);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Event::Unmapped { .. }));
    }
}
