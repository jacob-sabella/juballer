#![cfg(feature = "headless")]

use indexmap::IndexMap;
use juballer_core::calibration::Profile;
use juballer_core::layout::PaneId;
use juballer_core::{geometry, render, Color};

fn render_empty_grid(w: u32, h: u32) -> Vec<u8> {
    let p = Profile::default_for("a", "b", w, h);
    let cells = geometry::cell_rects(&p.grid);
    let pane_rects: IndexMap<PaneId, juballer_core::Rect> = IndexMap::new();
    pollster::block_on(render::headless::render_to_rgba(
        w,
        h,
        Color::rgb(0x0b, 0x0d, 0x12),
        &cells,
        &pane_rects,
        0.0,
        |_, _| {},
    ))
}

#[test]
fn snapshot_empty_grid_1080p_runs() {
    let pixels = render_empty_grid(1920, 1080);
    assert_eq!(pixels.len(), (1920 * 1080 * 4) as usize);
    // Top-left corner pixel should be the bg color (cleared first, composite doesn't fill
    // past source bounds). Tolerance ±2 for sRGB gamma conversions across drivers.
    let r = pixels[0];
    let g = pixels[1];
    let b = pixels[2];
    let a = pixels[3];
    assert!((r as i32 - 0x0b).abs() <= 2, "R got {r:#x}");
    assert!((g as i32 - 0x0d).abs() <= 2, "G got {g:#x}");
    assert!((b as i32 - 0x12).abs() <= 2, "B got {b:#x}");
    assert_eq!(a, 0xff, "A should be fully opaque");
}

#[test]
fn with_tile_raw_receives_clipped_viewport() {
    let w = 640u32;
    let h = 480u32;
    let p = Profile::default_for("a", "b", w, h);
    let cells = geometry::cell_rects(&p.grid);
    let pane_rects: IndexMap<PaneId, juballer_core::Rect> = IndexMap::new();
    let observed_ref = std::sync::Arc::new(std::sync::Mutex::new(None::<(f32, f32, f32, f32)>));
    let obs = observed_ref.clone();
    let _ = pollster::block_on(render::headless::render_to_rgba(
        w,
        h,
        Color::rgb(0x0b, 0x0d, 0x12),
        &cells,
        &pane_rects,
        0.0,
        move |frame, _| {
            frame.with_tile_raw(1, 2, |ctx| {
                *obs.lock().unwrap() = Some(ctx.viewport);
            });
        },
    ));
    let observed = *observed_ref.lock().unwrap();
    let (x, y, ww, hh) = observed.expect("with_tile_raw should run");
    let r = cells[4 + 2];
    assert!((x - r.x as f32).abs() < 0.5);
    assert!((y - r.y as f32).abs() < 0.5);
    assert!(ww > 0.0 && hh > 0.0);
    assert!(ww <= r.w as f32 + 0.5);
    assert!(hh <= r.h as f32 + 0.5);
}

#[test]
fn snapshot_filled_cell_has_bright_pixel() {
    let w = 1920u32;
    let h = 1080u32;
    let p = Profile::default_for("a", "b", w, h);
    let cells = geometry::cell_rects(&p.grid);
    let pane_rects: IndexMap<PaneId, juballer_core::Rect> = IndexMap::new();

    // Fill cell (0,0) bright white. Then sample the pixel inside that cell's interior.
    let pixels = pollster::block_on(render::headless::render_to_rgba(
        w,
        h,
        Color::rgb(0x0b, 0x0d, 0x12),
        &cells,
        &pane_rects,
        0.0,
        |frame, _| {
            frame.grid_cell(0, 0).fill(Color::rgb(0xff, 0xff, 0xff));
        },
    ));

    // Sample the middle of cell (0,0).
    let c = cells[0];
    let sx = c.x + (c.w as i32) / 2;
    let sy = c.y + (c.h as i32) / 2;
    let idx = ((sy as u32 * w + sx as u32) * 4) as usize;
    let r = pixels[idx];
    // The white fill should produce a near-255 red channel. Tolerate any sRGB/rounding
    // differences; anything brighter than bg is a pass signal.
    assert!(r > 0x80, "expected bright pixel in filled cell, got {r:#x}");
}
