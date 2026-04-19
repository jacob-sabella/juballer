//! Forces full calibration flow every launch on the AOC monitor.
//!
//! ## Phase 1 — Geometry (single-cell + gap calibration)
//!
//! Align the top-left cell to the physical TL button, then tune the inter-cell gap:
//!
//! | key                 | action                                    |
//! |---------------------|-------------------------------------------|
//! | Arrow keys          | move TL cell origin by 1 px               |
//! | Shift + Arrow keys  | move TL cell origin by 10 px              |
//! | `[` / `]`           | shrink / grow cell width & height         |
//! | `-` / `=`           | shrink / grow both gaps (symmetric)       |
//! | `h` / `j`           | shrink / grow `gap_x` (left-right spacing)|
//! | `k` / `l`           | shrink / grow `gap_y` (top-bottom spacing)|
//! | `,` / `.`           | rotate left / right by 0.25°              |
//!
//! Top-region (egui overlay area) adjustment — same phase:
//!
//! | key       | action                              |
//! |-----------|-------------------------------------|
//! | `p` / `o` | grow / shrink `edge_padding_top`    |
//! | `x` / `z` | grow / shrink `edge_padding_x`      |
//! | `t` / `y` | grow / shrink `cutoff_bottom`       |
//!
//! Shift applies a 10× step multiplier to all of the above.
//! A translucent teal rect shows the current top-region bounds.
//!
//! `Enter` commits Phase 1 → Phase 2. `Escape` cancels the whole flow.
//!
//! ## Phase 2 — Keymap
//!
//! Press each physical button in the order prompted by the orange marker. Duplicate
//! key codes are rejected. Completing cell (3,3) saves the profile atomically.

use juballer_core::input::Event;
use juballer_core::{App, Color};

fn main() -> juballer_core::Result<()> {
    env_logger::init();
    let mut app = App::builder()
        .title("juballer calibration_dance")
        .on_monitor("AOC")
        .controller_vid_pid(0x1973, 0x0011)
        .bg_color(Color::rgb(0x0b, 0x0d, 0x12))
        .build()?;
    app.run_calibration()?;
    app.run(|_frame, events| {
        for e in events {
            if let Event::CalibrationDone(_) = e {
                println!("calibration saved");
            }
        }
    })
}
