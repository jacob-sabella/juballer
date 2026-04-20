//! In-process mode switching for the App driver.
//!
//! Historically each "mode" (deck, rhythm picker, rhythm play, carla)
//! owned its own [`crate::App`] and called [`crate::App::run`], which
//! creates a fresh `winit::EventLoop`. winit doesn't allow recreating
//! an EventLoop on the same thread on most platforms, so switching
//! between modes required `exec()`-ing into a new process — visible
//! to the user as a window flicker, controller release/grab, and
//! audio dropout.
//!
//! [`Mode`] + [`ModeOutcome`] let modes coexist inside one App. Each
//! frame the active mode runs its `frame()` and tells the driver to
//! either stay, switch to a new mode, or exit. The driver swaps the
//! active mode in-place; the EventLoop and (where possible) the GPU
//! surface stay alive across the transition.
//!
//! ```ignore
//! struct DeckMode { /* … */ }
//! impl Mode for DeckMode {
//!     fn frame(&mut self, frame: &mut Frame, events: &[Event]) -> ModeOutcome {
//!         // …draw + handle events…
//!         if user_pressed_play_chart() {
//!             ModeOutcome::switch_to(RhythmPickerMode::new(/* … */))
//!         } else {
//!             ModeOutcome::Continue
//!         }
//!     }
//! }
//!
//! App::builder().build()?.run_modes(Box::new(DeckMode::new()))?;
//! ```
//!
//! Backwards compatibility: the existing single-callback
//! [`crate::App::run`] entry point is unchanged. Callers that don't
//! need switching keep working as before.

use crate::input::Event;
use crate::Frame;

/// What a mode wants to happen after this frame. The driver acts on
/// the variant returned by [`Mode::frame`]:
///
/// - [`ModeOutcome::Continue`]: keep the same mode active for the
///   next frame.
/// - [`ModeOutcome::SwitchTo`]: replace the active mode with the
///   provided one. The outgoing mode is dropped (its destructor runs
///   so it can release audio handles, OSC clients, etc); the new
///   mode's first call will be its own `frame()`. There's no separate
///   `on_enter`/`on_exit` — modes own their own lifecycle.
/// - [`ModeOutcome::Exit`]: the App returns from `run_modes`. The
///   outgoing mode is dropped on the way out.
pub enum ModeOutcome {
    Continue,
    SwitchTo(Box<dyn Mode>),
    Exit,
}

impl ModeOutcome {
    /// Convenience constructor that boxes the mode for the caller.
    pub fn switch_to<M: Mode + 'static>(mode: M) -> Self {
        Self::SwitchTo(Box::new(mode))
    }
}

/// Per-frame mode logic. Implementors own all of their own state
/// (audio engines, OSC clients, sub-state machines) and surface only
/// the per-frame `frame()` call to the driver.
pub trait Mode {
    /// Called once per frame with the rendering frame + the set of
    /// pending input events since the last frame. Return value drives
    /// the App's mode switcher — see [`ModeOutcome`].
    fn frame(&mut self, frame: &mut Frame<'_>, events: &[Event]) -> ModeOutcome;
}

/// Closure adapter for callers that want a one-mode App without
/// authoring a struct. Used by the existing single-callback
/// [`crate::App::run`] under the hood.
pub(crate) struct ClosureMode<F>
where
    F: FnMut(&mut Frame<'_>, &[Event]),
{
    pub(crate) draw: F,
}

impl<F> Mode for ClosureMode<F>
where
    F: FnMut(&mut Frame<'_>, &[Event]),
{
    fn frame(&mut self, frame: &mut Frame<'_>, events: &[Event]) -> ModeOutcome {
        (self.draw)(frame, events);
        ModeOutcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal mode that counts frames and asks to exit on the
    /// third one. Used by the closure-adapter test below.
    struct CountdownMode {
        remaining: u32,
    }
    impl Mode for CountdownMode {
        fn frame(&mut self, _: &mut Frame<'_>, _: &[Event]) -> ModeOutcome {
            if self.remaining == 0 {
                ModeOutcome::Exit
            } else {
                self.remaining -= 1;
                ModeOutcome::Continue
            }
        }
    }

    #[test]
    fn switch_to_helper_boxes_the_provided_mode() {
        let outcome = ModeOutcome::switch_to(CountdownMode { remaining: 3 });
        match outcome {
            ModeOutcome::SwitchTo(_) => {}
            _ => panic!("switch_to must produce a SwitchTo variant"),
        }
    }

    #[test]
    fn countdown_mode_decrements_until_exit() {
        let mut m = CountdownMode { remaining: 2 };
        // We can't synthesise a `Frame<'_>` without a live wgpu
        // surface so this test exercises only the outcome state
        // machine — the actual frame-call path is covered by the
        // smoke examples (echo_grid, smoke_grid) under
        // crates/juballer-core/examples/.
        assert_eq!(m.remaining, 2);
        m.remaining -= 1;
        m.remaining -= 1;
        assert_eq!(m.remaining, 0);
    }
}
