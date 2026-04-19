//! Cell fills bright while pressed, dim when released. Visual input smoke test.

use juballer_core::input::Event;
use juballer_core::{App, Color};
use std::collections::HashSet;

fn main() -> juballer_core::Result<()> {
    env_logger::init();
    let mut held: HashSet<(u8, u8)> = HashSet::new();
    App::builder()
        .title("juballer echo_grid")
        .on_monitor("AOC")
        .controller_vid_pid(0x1973, 0x0011)
        .bg_color(Color::rgb(0x0b, 0x0d, 0x12))
        .build()?
        .run(move |frame, events| {
            for e in events {
                match e {
                    Event::KeyDown { row, col, .. } => {
                        held.insert((*row, *col));
                    }
                    Event::KeyUp { row, col, .. } => {
                        held.remove(&(*row, *col));
                    }
                    _ => {}
                }
            }
            for r in 0..4 {
                for c in 0..4 {
                    let shade = if held.contains(&(r, c)) { 0xe0 } else { 0x22 };
                    frame.grid_cell(r, c).fill(Color::rgb(shade, shade, shade));
                }
            }
        })
}
