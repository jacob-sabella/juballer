mod builder;
mod mode;
pub(crate) mod profile_loader;
mod run;

pub use builder::{AppBuilder, PresentMode, RefreshTarget};
pub use mode::{closure_mode_with_switcher, Mode, ModeOutcome, Switcher};

use crate::{Profile, Result};

/// The top-level application handle. Owns the window, GPU surface, profile, and event loop.
pub struct App {
    pub(crate) cfg: AppBuilder,
    pub(crate) cfg_top_layout: Option<crate::layout::Node>,
    pub(crate) profile: Option<Profile>,
    pub(crate) debug: bool,
    pub(crate) force_calibration: bool,
    pub(crate) force_keymap: bool,
}

impl App {
    pub fn builder() -> AppBuilder {
        AppBuilder::default()
    }

    /// Set or replace the top-region layout. Solved once per call (not per frame).
    pub fn set_top_layout(&mut self, root: crate::layout::Node) {
        self.cfg_top_layout = Some(root);
    }

    /// Current calibration profile. Only populated after `run()` has started; returns `None`
    /// before then because profile load requires a live window + GPU.
    pub fn profile(&self) -> Option<&crate::Profile> {
        self.profile.as_ref()
    }

    /// Toggle the debug overlay that marks each grid cell with its top-left corner in magenta.
    ///
    /// Must be set before `run()`; mutations after are ignored.
    pub fn set_debug(&mut self, on: bool) {
        self.debug = on;
    }

    /// Force the next `run()` to start in full calibration mode (geometry then keymap).
    ///
    /// Mutations after `run()` begins are ignored.
    pub fn run_calibration(&mut self) -> Result<()> {
        self.force_calibration = true;
        Ok(())
    }

    /// Force the next `run()` to start in keymap-only auto-learn mode (skipping geometry).
    ///
    /// Mutations after `run()` begins are ignored.
    pub fn run_keymap_auto_learn(&mut self) -> Result<()> {
        self.force_keymap = true;
        Ok(())
    }
}

impl AppBuilder {
    /// Build the App. Opens the window and initializes wgpu lazily inside `App::run()`,
    /// so this constructor only validates configuration.
    pub fn build(self) -> Result<App> {
        Ok(App {
            cfg: self,
            cfg_top_layout: None,
            profile: None,
            debug: false,
            force_calibration: false,
            force_keymap: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults() {
        let app = App::builder()
            .title("smoke")
            .present_mode(PresentMode::Immediate)
            .controller_vid_pid(0x1234, 0x5678)
            .build()
            .unwrap();
        assert_eq!(app.cfg.title, "smoke");
        assert_eq!(app.cfg.present_mode, PresentMode::Immediate);
        assert_eq!(app.cfg.controller_vid, 0x1234);
        assert_eq!(app.cfg.controller_pid, 0x5678);
    }
}
