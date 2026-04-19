//! Input pipeline.

use std::time::Instant;

pub mod keymap;
pub use keymap::{KeyCode, Keymap};

use crate::calibration::Profile;

mod winit_backend;
pub use winit_backend::WinitInput;

mod ring;
pub use ring::EventRing;

#[cfg(all(target_os = "linux", feature = "raw-input"))]
pub mod raw_linux;

#[cfg(all(target_os = "windows", feature = "raw-input"))]
pub mod raw_windows;

#[derive(Debug, Clone)]
pub enum Event {
    KeyDown {
        row: u8,
        col: u8,
        key: KeyCode,
        ts: Instant,
    },
    KeyUp {
        row: u8,
        col: u8,
        key: KeyCode,
        ts: Instant,
    },
    Unmapped {
        key: KeyCode,
        ts: Instant,
    },
    CalibrationDone(Profile),
    WindowResized {
        w: u32,
        h: u32,
    },
    Quit,
}
