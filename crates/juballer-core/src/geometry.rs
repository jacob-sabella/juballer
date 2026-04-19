use crate::calibration::{GridGeometry, TopGeometry};
use crate::Rect;

/// Compute the 16 cell rectangles for a calibrated grid (axis-aligned, ignoring rotation).
/// Cells are returned row-major: index = row * 4 + col.
///
/// With the single-cell schema, each cell has size `cell_size_px` and is offset from
/// the TL cell at `origin_px` by `(col * (cell_w + gap_x), row * (cell_h + gap_y))`.
pub fn cell_rects(grid: &GridGeometry) -> [Rect; 16] {
    let cw = grid.cell_size_px.w;
    let ch = grid.cell_size_px.h;
    let mut out = [Rect::ZERO; 16];
    for r in 0..4u32 {
        for c in 0..4u32 {
            let x = grid.origin_px.x + (c * (cw + grid.gap_x_px as u32)) as i32;
            let y = grid.origin_px.y + (r * (ch + grid.gap_y_px as u32)) as i32;
            out[(r * 4 + c) as usize] = Rect::new(x, y, cw, ch);
        }
    }
    out
}

/// The rect of a single cell. Pure math helper useful for tests + calibration overlays.
pub fn cell_rect(grid: &GridGeometry, row: u8, col: u8) -> Rect {
    let cw = grid.cell_size_px.w;
    let ch = grid.cell_size_px.h;
    let x = grid.origin_px.x + (col as u32 * (cw + grid.gap_x_px as u32)) as i32;
    let y = grid.origin_px.y + (row as u32 * (ch + grid.gap_y_px as u32)) as i32;
    Rect::new(x, y, cw, ch)
}

/// Compute the top-region outer rect for egui overlays.
///
/// Returns a rect inside `{edge_padding_x..monitor_w-edge_padding_x} × {edge_padding_top..grid_origin_y-cutoff_bottom}`.
/// `cutoff_bottom` exists because the GAMO2 FB9 physical controller's bezel covers the
/// area just above its button grid — widgets rendered there would be invisible under
/// the plastic. `monitor_h` is reserved for future clipping against the screen bottom.
pub fn top_region_rect(
    grid: &GridGeometry,
    top: &TopGeometry,
    monitor_w: u32,
    monitor_h: u32,
) -> Rect {
    let _ = monitor_h;
    let x = top.edge_padding_x as i32;
    let y = top.edge_padding_top as i32;
    let right_inset = 2u32 * top.edge_padding_x as u32;
    let w = monitor_w.saturating_sub(right_inset);
    let bottom = grid.origin_px.y - top.cutoff_bottom as i32;
    let h = (bottom - y).max(0) as u32;
    Rect::new(x, y, w, h)
}

/// 2x3 affine rotation matrix around `(cx, cy)` by `angle_deg`. Returns column-major
/// `[m00, m10, m01, m11, m02, m12]` so it can be uploaded to a shader as two `vec3` columns.
pub fn rotation_2x3(cx: f32, cy: f32, angle_deg: f32) -> [f32; 6] {
    let a = angle_deg.to_radians();
    let (s, c) = a.sin_cos();
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::{GridGeometry, PointPx, SizePx, TopGeometry};

    fn grid_247_with_gap_12() -> GridGeometry {
        GridGeometry {
            origin_px: PointPx { x: 100, y: 200 },
            cell_size_px: SizePx { w: 247, h: 247 },
            gap_x_px: 12,
            gap_y_px: 12,
            border_px: 4,
            rotation_deg: 0.0,
        }
    }

    #[test]
    fn cell_rects_are_equal_size_with_gaps() {
        let g = grid_247_with_gap_12();
        let cells = cell_rects(&g);
        for r in cells.iter() {
            assert_eq!(r.w, 247);
            assert_eq!(r.h, 247);
        }
        assert_eq!(cells[0].x, 100);
        assert_eq!(cells[0].y, 200);
        assert_eq!(cells[3].x, 100 + 3 * (247 + 12));
        assert_eq!(cells[15].x, 100 + 3 * (247 + 12));
        assert_eq!(cells[15].y, 200 + 3 * (247 + 12));
    }

    #[test]
    fn cell_rect_matches_cell_rects_entry() {
        let g = grid_247_with_gap_12();
        let all = cell_rects(&g);
        for r in 0..4u8 {
            for c in 0..4u8 {
                assert_eq!(cell_rect(&g, r, c), all[(r as usize) * 4 + c as usize]);
            }
        }
    }

    #[test]
    fn cell_rect_honors_asymmetric_gaps() {
        let mut g = grid_247_with_gap_12();
        g.gap_x_px = 8;
        g.gap_y_px = 20;
        let r00 = cell_rect(&g, 0, 0);
        let r01 = cell_rect(&g, 0, 1);
        let r10 = cell_rect(&g, 1, 0);
        assert_eq!(r01.x - r00.x, 247 + 8);
        assert_eq!(r10.y - r00.y, 247 + 20);
    }

    #[test]
    fn top_region_clips_between_padding_and_grid_minus_cutoff() {
        let g = grid_247_with_gap_12();
        let top = TopGeometry {
            edge_padding_top: 24,
            edge_padding_x: 16,
            cutoff_bottom: 40,
        };
        let r = top_region_rect(&g, &top, 1920, 1440);
        assert_eq!(r.x, 16);
        assert_eq!(r.y, 24);
        assert_eq!(r.w, 1920 - 32);
        // grid.origin_px.y=200, cutoff=40, padding_top=24 → height = (200-40)-24 = 136
        assert_eq!(r.h, 136);
    }

    #[test]
    fn top_region_collapses_to_zero_when_cutoff_exceeds_grid_y() {
        let g = grid_247_with_gap_12();
        let top = TopGeometry {
            edge_padding_top: 10,
            edge_padding_x: 0,
            cutoff_bottom: 500, // > grid.origin_px.y (=200)
        };
        let r = top_region_rect(&g, &top, 1920, 1440);
        assert_eq!(r.h, 0);
    }

    #[test]
    fn rotation_zero_is_identity_offset_zero() {
        let m = rotation_2x3(0.0, 0.0, 0.0);
        assert!((m[0] - 1.0).abs() < 1e-6);
        assert!((m[1] - 0.0).abs() < 1e-6);
        assert!((m[2] - 0.0).abs() < 1e-6);
        assert!((m[3] - 1.0).abs() < 1e-6);
        assert!((m[4] - 0.0).abs() < 1e-6);
        assert!((m[5] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn rotation_90_around_origin_maps_x_to_y() {
        let m = rotation_2x3(0.0, 0.0, 90.0);
        let x = 1.0;
        let y = 0.0;
        let xp = m[0] * x + m[2] * y + m[4];
        let yp = m[1] * x + m[3] * y + m[5];
        assert!(xp.abs() < 1e-5, "got {}", xp);
        assert!((yp - 1.0).abs() < 1e-5, "got {}", yp);
    }
}
