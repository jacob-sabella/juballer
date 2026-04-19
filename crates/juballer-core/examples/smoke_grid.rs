//! Opens fullscreen on the AOC monitor and renders 16 progressively-lighter grey squares.

use juballer_core::{App, Color};

fn main() -> juballer_core::Result<()> {
    env_logger::init();
    App::builder()
        .title("juballer smoke_grid")
        .on_monitor("AOC")
        .controller_vid_pid(0x1973, 0x0011)
        .bg_color(Color::rgb(0x0b, 0x0d, 0x12))
        .build()?
        .run(|frame, _events| {
            for r in 0..4 {
                for c in 0..4 {
                    let shade = 0x20 + (r * 4 + c) * 8;
                    frame.grid_cell(r, c).fill(Color::rgb(shade, shade, shade));
                }
            }
        })
}
