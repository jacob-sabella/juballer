//! Per-cell tile render state. Actions mutate a `TileHandle`; render layer reads the
//! underlying `TileState` at frame time.

use juballer_core::Color;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum IconRef {
    /// Relative path to an asset (profile assets/ resolved by render layer) or absolute.
    Path(PathBuf),
    /// Single emoji / short text rendered as icon.
    Emoji(String),
    /// Named icon baked into the binary.
    Builtin(&'static str),
}

/// Optional per-tile content source drawn UNDER the egui overlay.
#[derive(Debug, Clone, PartialEq)]
pub enum TileShaderSource {
    /// User-authored WGSL file. `params` is reserved for push-constant/uniform
    /// extension and is currently ignored.
    CustomShader {
        wgsl_path: PathBuf,
        params: HashMap<String, f32>,
    },
    /// Video URI; currently `v4l2:///dev/videoN` is the only supported scheme.
    Video { uri: String },
}

#[derive(Debug, Clone, Default)]
pub struct TileState {
    pub icon: Option<IconRef>,
    pub label: Option<String>,
    pub bg: Option<Color>,
    pub state_color: Option<Color>,
    /// If Some, tile is currently flashing until that instant.
    pub flash_until: Option<std::time::Instant>,
    /// Optional raw-wgpu content drawn beneath the egui overlay.
    pub shader: Option<TileShaderSource>,
}

/// Handle given to actions during a callback. Owns a &mut to the tile state slot,
/// so mutations land immediately.
pub struct TileHandle<'a> {
    state: &'a mut TileState,
}

impl<'a> TileHandle<'a> {
    pub fn new(state: &'a mut TileState) -> Self {
        Self { state }
    }

    pub fn set_icon(&mut self, icon: IconRef) {
        self.state.icon = Some(icon);
    }
    pub fn set_label(&mut self, text: impl Into<String>) {
        self.state.label = Some(text.into());
    }
    pub fn set_bg(&mut self, color: Color) {
        self.state.bg = Some(color);
    }
    pub fn set_state_color(&mut self, color: Color) {
        self.state.state_color = Some(color);
    }
    pub fn flash(&mut self, ms: u16) {
        self.state.flash_until =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(ms as u64));
    }
    pub fn set_shader(&mut self, src: TileShaderSource) {
        self.state.shader = Some(src);
    }
    pub fn clear_shader(&mut self) {
        self.state.shader = None;
    }
    pub fn state(&self) -> &TileState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_mutates_state() {
        let mut s = TileState::default();
        {
            let mut h = TileHandle::new(&mut s);
            h.set_label("hi");
            h.set_icon(IconRef::Emoji("▶".into()));
        }
        assert_eq!(s.label.as_deref(), Some("hi"));
        match s.icon.unwrap() {
            IconRef::Emoji(e) => assert_eq!(e, "▶"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn flash_sets_future_instant() {
        let mut s = TileState::default();
        let now = std::time::Instant::now();
        {
            let mut h = TileHandle::new(&mut s);
            h.flash(200);
        }
        assert!(s.flash_until.unwrap() > now);
    }
}
