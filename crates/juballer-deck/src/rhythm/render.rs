//! Rhythm-mode renderer. Paints tile shaders for approaching/judged notes and an
//! egui HUD in the top region.

use super::judge::Grade;
use super::state::{render_slots, GameState, RENDER_TRAIL_MS};
use crate::shader::{ShaderPipelineCache, TileUniforms};
use juballer_core::{Color, Frame};
use juballer_egui::EguiOverlay;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Per-grade tile accent colors in linear-ish srgb. White means "approaching,
/// not yet judged". The per-grade entries activate the moment a note is judged
/// and fade with the freeze-frame factor.
pub fn grade_color(g: Option<Grade>) -> [f32; 4] {
    match g {
        None => [1.0, 1.0, 1.0, 1.0],
        Some(Grade::Perfect) => [0.40, 1.0, 0.55, 1.0],
        Some(Grade::Great) => [1.0, 0.95, 0.35, 1.0],
        Some(Grade::Good) => [1.0, 0.65, 0.25, 1.0],
        Some(Grade::Poor) => [1.0, 0.35, 0.35, 1.0],
        Some(Grade::Miss) => [0.70, 0.10, 0.10, 1.0],
    }
}

/// Map (music_time vs note hit_time) → shader approach value in [0, 1].
///
/// `lead_ms` is the full approach window in ms (set per-session from
/// `RhythmConfig.lead_in_ms`). 0.0 = note still `lead_ms` in the future;
/// 1.0 = note at hit time. For already-past notes the approach clamps at
/// 1.0 so the freeze-frame takes over.
pub fn approach_factor(music_ms: f64, hit_ms: f64, lead_ms: f64) -> f32 {
    let dt = hit_ms - music_ms;
    let normalized = 1.0 - (dt / lead_ms.max(1.0));
    // Full 0..1 range so the shader fades the note in smoothly from 0
    // rather than popping in at any non-zero floor.
    normalized.clamp(0.0, 1.0) as f32
}

/// Freeze factor for a judged note: 1.0 right at judgment, decaying over the trail.
pub fn freeze_factor(music_ms: f64, hit_ms: f64) -> f32 {
    let elapsed = (music_ms - hit_ms).max(0.0);
    let f = 1.0 - (elapsed / RENDER_TRAIL_MS);
    f.clamp(0.0, 1.0) as f32
}

/// Fill all 16 cells with the dark mantle tone so the note shader has something to
/// blend over. Called once per frame before the per-note draws.
pub fn paint_backgrounds(frame: &mut Frame) {
    // Paint a subtle dark bg so empty cells aren't pure black but shader output
    // still shows clearly. Alpha 0xB0 so wgpu shader draws below blend with it.
    let bg = Color::rgba(0x10, 0x10, 0x1a, 0xff);
    for r in 0..4u8 {
        for c in 0..4u8 {
            frame.grid_cell(r, c).fill(bg);
        }
    }
    let _ = frame;
}

/// Compute the cells along a memon long-note's tail path: the row or
/// column of cells between `head` and `tail`, **head-exclusive,
/// tail-inclusive**.
///
/// These are the cells that get arrow glyphs pointing back at the head —
/// the visual lane the player sees "feeding into" the head cell.
/// Returns up to 3 (row, col) pairs ordered head-adjacent → tail-end
/// (closest to the head first). Empty when head equals tail (no `p`, or
/// invalid).
pub fn long_tail_path(head_row: u8, head_col: u8, tail_row: u8, tail_col: u8) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    if head_row == tail_row && head_col == tail_col {
        return out;
    }
    if head_row == tail_row {
        // Horizontal tail: walk col direction.
        let step: i8 = if tail_col > head_col { 1 } else { -1 };
        let mut c = head_col as i8 + step;
        loop {
            out.push((head_row, c as u8));
            if c == tail_col as i8 {
                break;
            }
            c += step;
            if !(0..4).contains(&c) {
                break;
            }
        }
    } else if head_col == tail_col {
        let step: i8 = if tail_row > head_row { 1 } else { -1 };
        let mut r = head_row as i8 + step;
        loop {
            out.push((r as u8, head_col));
            if r == tail_row as i8 {
                break;
            }
            r += step;
            if !(0..4).contains(&r) {
                break;
            }
        }
    }
    out
}

/// Paint chevron arrows on every cell of each approaching long note's
/// tail path, pointing toward the head cell. This is the missing
/// "lane preview" — without it the head's directional shader hint
/// reads as arbitrary noise. With it the player sees a clear chain of
/// arrows feeding into the head.
///
/// Visuals:
///   * each tail cell gets one chevron pointing at the head;
///   * baseline alpha tracks `approach_factor` (fades in over lead-in);
///   * a pulsing "wave" highlights one cell at a time, sliding from
///     the tail end toward the head as approach goes 0 → 1, so the
///     direction of motion is unambiguous;
///   * arrows fade out once the head is being held or has been judged
///     (their job is to telegraph the upcoming hit, not the hold).
pub fn draw_long_tail_arrows(frame: &mut Frame, overlay: &mut EguiOverlay, state: &GameState) {
    let cell_rects = *frame.cell_rects();
    let viewport_w = frame.viewport_w() as f32;
    let viewport_h = frame.viewport_h() as f32;
    let music = state.music_time_ms;
    let lead_ms = state.lead_in_ms;

    // Collect renderable long notes once so we don't iterate state's
    // notes vec inside the egui closure (the iterator borrows state).
    struct Pending {
        head_row: u8,
        head_col: u8,
        tail_row: u8,
        tail_col: u8,
        approach: f32,
    }
    let mut pending: Vec<Pending> = Vec::new();
    for sn in state.renderable_notes() {
        let n = &sn.note;
        if !n.is_long() {
            continue;
        }
        // Suppress arrows once head's been judged (hit or auto-miss) or
        // we're past hit_time and the player is in the hold phase.
        if sn.is_judged() || sn.is_holding() {
            continue;
        }
        if music >= n.hit_time_ms {
            continue;
        }
        if n.tail_row == n.row && n.tail_col == n.col {
            continue;
        }
        pending.push(Pending {
            head_row: n.row,
            head_col: n.col,
            tail_row: n.tail_row,
            tail_col: n.tail_col,
            approach: approach_factor(music, n.hit_time_ms, lead_ms),
        });
    }
    if pending.is_empty() {
        return;
    }

    overlay.draw(frame, |rc| {
        egui::Area::new(egui::Id::new("rhythm_long_tail_arrows_root"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(viewport_w);
                ui.set_height(viewport_h);
                let painter = ui.painter();
                for p in &pending {
                    let path = long_tail_path(p.head_row, p.head_col, p.tail_row, p.tail_col);
                    if path.is_empty() {
                        continue;
                    }
                    let head_idx = (p.head_row as usize) * 4 + p.head_col as usize;
                    let head_rect = cell_rects[head_idx];
                    let head_cx = head_rect.x as f32 + head_rect.w as f32 * 0.5;
                    let head_cy = head_rect.y as f32 + head_rect.h as f32 * 0.5;

                    // Single static chevron per cell (anchored at center).
                    // A static hint is enough to telegraph direction; the
                    // held cell's bright cyan square is the unambiguous
                    // "active" marker. Animated chevrons read as "this
                    // note is active" on unpressed long notes whose hit
                    // landed at the same instant as a pressed neighbour.
                    let phase = 0.5_f32; // center-of-cell, no slide
                    let _ = music; // unused without animation
                    for &(cr, cc) in path.iter() {
                        let idx = (cr as usize) * 4 + cc as usize;
                        let cell = cell_rects[idx];
                        let cx = cell.x as f32 + cell.w as f32 * 0.5;
                        let cy = cell.y as f32 + cell.h as f32 * 0.5;
                        // Direction unit vector from this cell toward head.
                        let dx = head_cx - cx;
                        let dy = head_cy - cy;
                        let len = (dx * dx + dy * dy).sqrt().max(1.0);
                        let ux = dx / len;
                        let uy = dy / len;
                        // Perpendicular for the chevron's two trailing arms.
                        let px = -uy;
                        let py = ux;
                        let cell_size = cell.w.min(cell.h) as f32;
                        let arm = cell_size * 0.28;
                        // Travel range = ±cell_size * 0.45 from cell
                        // center. Loop bounds permit a second offset arrow
                        // (half a period later) without restructuring.
                        let travel = cell_size * 0.45;
                        for k in 0..1 {
                            let local_phase = ((phase + k as f32 * 0.5) + 1.0) % 1.0;
                            // Map phase 0 → tail-edge (-travel), 1 → head-edge (+travel).
                            let along = (local_phase * 2.0 - 1.0) * travel;
                            // Fade arrow in/out at the cycle boundaries so
                            // it doesn't pop visibly when wrapping.
                            let edge_fade =
                                (local_phase * std::f32::consts::PI).sin().clamp(0.0, 1.0);
                            let acx = cx + ux * along;
                            let acy = cy + uy * along;
                            let tip = egui::pos2(acx + ux * arm, acy + uy * arm);
                            let back_l = egui::pos2(
                                acx - ux * arm * 0.6 + px * arm * 1.1,
                                acy - uy * arm * 0.6 + py * arm * 1.1,
                            );
                            let back_r = egui::pos2(
                                acx - ux * arm * 0.6 - px * arm * 1.1,
                                acy - uy * arm * 0.6 - py * arm * 1.1,
                            );
                            let base = (p.approach * 0.45 + 0.45).clamp(0.45, 1.0);
                            let alpha = ((base * edge_fade) * 255.0).clamp(0.0, 255.0) as u8;
                            if alpha == 0 {
                                continue;
                            }
                            let fill = egui::Color32::from_rgba_unmultiplied(120, 220, 255, alpha);
                            let outline = egui::Color32::from_rgba_unmultiplied(10, 30, 50, alpha);
                            painter.add(egui::Shape::convex_polygon(
                                vec![tip, back_l, back_r],
                                fill,
                                egui::Stroke::new(cell_size * 0.035, outline),
                            ));
                        }
                    }
                }
            });
    });
}

pub fn draw_hit_rings(frame: &mut Frame, overlay: &mut EguiOverlay, state: &GameState) {
    let slots = render_slots(state);
    let cell_rects = *frame.cell_rects();
    let viewport_w = frame.viewport_w() as f32;
    let viewport_h = frame.viewport_h() as f32;
    let music = state.music_time_ms;
    let lead_ms = state.lead_in_ms;

    overlay.draw(frame, |rc| {
        egui::Area::new(egui::Id::new("rhythm_hit_rings_root"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(viewport_w);
                ui.set_height(viewport_h);
                let painter = ui.painter();
                for (idx, slot) in slots.iter().enumerate() {
                    let Some(slot) = slot else { continue };
                    // Only draw the target while the note is still pending.
                    if slot.hit.is_some() {
                        continue;
                    }
                    // Long notes have their own shader-based visual
                    // (`draw_notes`); skip here so their cells don't get
                    // a target ring stacked on top.
                    if slot.note.is_long() {
                        continue;
                    }
                    let rect_core = cell_rects[idx];
                    let cx = rect_core.x as f32 + rect_core.w as f32 * 0.5;
                    let cy = rect_core.y as f32 + rect_core.h as f32 * 0.5;
                    let radius = rect_core.w.min(rect_core.h) as f32 * 0.5 * 0.35;
                    let approach = approach_factor(music, slot.note.hit_time_ms, lead_ms);
                    let alpha = (approach * 0.9 * 255.0).clamp(0.0, 255.0) as u8;
                    if alpha == 0 {
                        continue;
                    }
                    let center = egui::pos2(cx, cy);
                    // Inner low-alpha disc gives the ring a backing plate so
                    // it reads as a target over bright shader backgrounds.
                    painter.circle_filled(
                        center,
                        radius,
                        egui::Color32::from_rgba_unmultiplied(20, 40, 60, 20),
                    );
                    painter.circle_stroke(
                        center,
                        radius,
                        egui::Stroke::new(
                            2.0,
                            egui::Color32::from_rgba_unmultiplied(94, 232, 255, alpha),
                        ),
                    );
                }
            });
    });
}

/// Marker-sprite note rendering for tap notes.
///
/// Long notes stay on the shader path for the hold-state fuel bar; tap
/// notes use the per-grade PNG animations here. Markers are lazy-loaded
/// on the first frame (textures need an egui Context). Load failure logs
/// a warn and skips drawing; the rest of the rhythm loop keeps running.
pub fn draw_notes_markers(
    frame: &mut Frame,
    overlay: &mut EguiOverlay,
    markers: &mut Option<super::marker::Markers>,
    marker_dir: &std::path::Path,
    state: &GameState,
) {
    let slots = render_slots(state);
    let music = state.music_time_ms;
    let cell_rects = *frame.cell_rects();
    let viewport_w = frame.viewport_w() as f32;
    let viewport_h = frame.viewport_h() as f32;
    overlay.draw(frame, |rc| {
        if markers.is_none() {
            match super::marker::Markers::load(rc.ctx(), marker_dir) {
                Ok(m) => *markers = Some(m),
                Err(e) => {
                    tracing::warn!(
                        target: "juballer::rhythm::marker",
                        "marker load failed from {}: {e}",
                        marker_dir.display()
                    );
                    return;
                }
            }
        }
        let Some(m) = markers.as_ref() else { return };
        // Uses a dedicated EguiOverlay from play_chart — sharing the HUD's
        // overlay clobbers the marker pass's draw buffers (each
        // EguiOverlay::draw owns a single egui_wgpu::Renderer).
        egui::Area::new(egui::Id::new("rhythm_markers_root"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(viewport_w);
                ui.set_height(viewport_h);
                let painter = ui.painter();
                for (idx, slot) in slots.iter().enumerate() {
                    let Some(slot) = slot else { continue };
                    if slot.note.is_long() {
                        continue;
                    }
                    let rect_core = cell_rects[idx];
                    let tile = egui::Rect::from_min_size(
                        egui::pos2(rect_core.x as f32, rect_core.y as f32),
                        egui::vec2(rect_core.w as f32, rect_core.h as f32),
                    );
                    let pick = match slot.hit {
                        Some(h) => {
                            let phase = super::marker::grade_to_phase(h.grade);
                            let since = music - h.judged_at_ms;
                            m.grade_frame_tweened(phase, since)
                        }
                        None => {
                            let offset = music - slot.note.hit_time_ms;
                            m.approach_frame_tweened(offset)
                        }
                    };
                    if let Some((tex, uv_a, uv_b, t)) = pick {
                        // Crossfade between adjacent sprite frames. Linear texture sampling on
                        // the marker atlases means the alpha blend is bit-safe.
                        let alpha_a = ((1.0 - t) * 255.0).round() as u8;
                        let alpha_b = (t * 255.0).round() as u8;
                        if alpha_a > 0 {
                            painter.image(
                                tex.id(),
                                tile,
                                uv_a,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha_a),
                            );
                        }
                        if alpha_b > 0 {
                            painter.image(
                                tex.id(),
                                tile,
                                uv_b,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha_b),
                            );
                        }
                    }
                }
            });
    });
}

/// Draw the per-tile note-approach shader into each cell that currently hosts a
/// renderable note. `shader_path` points at note_approach.wgsl.
pub fn draw_notes(
    frame: &mut Frame,
    state: &GameState,
    shader_cache: &mut ShaderPipelineCache,
    shader_path: &Path,
    boot_secs: f32,
    delta_time: f32,
) {
    let slots = render_slots(state);
    let music = state.music_time_ms;
    let active_count = slots.iter().filter(|s| s.is_some()).count();
    if active_count > 0 {
        tracing::trace!("draw_notes: music_ms={music} active={active_count}");
    }
    for (idx, slot) in slots.iter().enumerate() {
        let Some(slot) = slot else {
            continue;
        };
        // Tap notes render via the marker sprite path; this shader path
        // only owns long-note hold visuals.
        if !slot.note.is_long() {
            continue;
        }
        let r = (idx / 4) as u8;
        let c = (idx % 4) as u8;
        let approach = approach_factor(music, slot.note.hit_time_ms, state.lead_in_ms);
        let (freeze, accent) = match slot.hit {
            Some(out) => (
                // Anchor freeze to the actual judgment moment (press_ms for
                // real hits, tick's music_ms for auto-misses). Anchoring to
                // release_time would clamp the freeze burst for auto-misses,
                // since they're judged at release_time + ε.
                freeze_factor(music, out.judged_at_ms),
                grade_color(Some(out.grade)),
            ),
            None => (0.0, grade_color(None)),
        };
        // For long notes the shader needs to know two extra things:
        //   * is_long  (state.z != 0): swap visual to "held bar" mode
        //   * hold_progress (state.y in 0..1): 1.0 at press, 0.0 at release
        let is_long = if slot.note.is_long() { 1.0 } else { 0.0 };
        let hold_progress = if slot.note.is_long() {
            hold_progress(music, slot.note.hit_time_ms, slot.note.release_time_ms())
        } else {
            0.0
        };
        let holding = if slot.holding { 1.0 } else { 0.0 };
        let stack = slot.stack_count as f32;
        // `kind` selects the shader's phase branch:
        //   0 = approach (unjudged), 1 = Perfect, 2 = Great, 3 = Good,
        //   4 = Poor, 5 = Miss.
        let kind = match slot.hit.map(|h| h.grade) {
            None => 0.0,
            Some(Grade::Perfect) => 1.0,
            Some(Grade::Great) => 2.0,
            Some(Grade::Good) => 3.0,
            Some(Grade::Poor) => 4.0,
            Some(Grade::Miss) => 5.0,
        };
        // Arrow direction (head → tail) in tile-local UV space. UV has +y
        // pointing *down* (matches the fragment shader's `uv`), so a tail
        // one row below the head maps to dy = +1. We pass the raw grid-cell
        // delta here; the shader normalizes it. All-zero = no direction
        // (tap note or long note without `p`) → shader falls back to the
        // existing vertical tail hint.
        let dx = slot.note.tail_col as f32 - slot.note.col as f32;
        let dy = slot.note.tail_row as f32 - slot.note.row as f32;
        frame.with_tile_raw(r, c, |mut ctx| {
            let uniforms = TileUniforms {
                resolution: [ctx.viewport.2, ctx.viewport.3],
                time: boot_secs,
                delta_time,
                cursor: [dx, dy],
                kind,
                bound: 1.0,
                toggle_on: holding,
                flash: approach,
                _pad0: [0.0, 0.0],
                accent,
                state: [freeze, hold_progress, is_long, stack],
                spectrum: [[0.0; 4]; 4],
            };
            shader_cache.draw_tile(&mut ctx, shader_path, &uniforms);
        });
    }
}

/// Compute the label + per-step fade-phase for a given `ms_until_start` tick
/// of the pre-song countdown. Pure function — extracted so tests can exercise
/// it without an egui context.
///
/// Returns `(label, phase)` where:
/// - `label` is `"3"`, `"2"`, `"1"`, or `"GO!"`.
/// - `phase` is in `[0.0, 1.0]`, **monotonically increasing** inside each step:
///   `0.0` right when the step begins, `1.0` right before it flips to the next.
///   Used to drive the pulse animation (fade-out + shrink).
///
/// Input semantics match `-music_ms`: at the top of a 3s countdown the caller
/// passes `3000.0`, ramping down to `0.0` at kickoff. Values `<= 0.0` collapse
/// to the "GO!" frame; values `> 3000.0` clamp to the "3" frame at phase 0.
pub fn countdown_label_phase(ms_until_start: f64) -> (&'static str, f32) {
    // Clamp into the 4-step window: [3000, 2000) → "3", [2000, 1000) → "2",
    // [1000, 0) → "1", <= 0 → "GO!".
    if ms_until_start <= 0.0 {
        return ("GO!", 1.0);
    }
    let clamped = ms_until_start.min(3000.0);
    let (label, step_start_ms) = if clamped > 2000.0 {
        ("3", 3000.0)
    } else if clamped > 1000.0 {
        ("2", 2000.0)
    } else {
        ("1", 1000.0)
    };
    // phase = 0.0 at the very top of the step, 1.0 just before it flips.
    // step_start_ms is the *larger* ms-until-start value, so phase ramps as
    // clamped decreases.
    let phase = ((step_start_ms - clamped) / 1000.0).clamp(0.0, 1.0) as f32;
    (label, phase)
}

/// Draw the pre-song countdown overlay. Renders a pulsing
/// "3 → 2 → 1 → GO!" banner inside the top HUD region — the only
/// area that's guaranteed to be visible above the physical GAMO2
/// controller (which sits over the 4×4 grid below). `ms_until_start`
/// is what `-music_ms` would be at this frame — positive during the
/// countdown, non-positive once the song has started.
///
/// Caller is responsible for only invoking this while the countdown is active
/// (e.g. `music_ms < 0.0` *or* a small grace window after 0 so "GO!" shows).
pub fn draw_countdown(frame: &mut Frame, overlay: &mut EguiOverlay, ms_until_start: f64) {
    let (label, phase) = countdown_label_phase(ms_until_start);
    // Fade: opaque at start of step, fading as phase → 1.0.
    let alpha = ((1.0 - phase) * 255.0).clamp(20.0, 255.0) as u8;
    // "GO!" pops green; number steps pop white-ish.
    let color = if label == "GO!" {
        egui::Color32::from_rgba_unmultiplied(120, 255, 140, alpha)
    } else {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha)
    };

    // Paint into the top HUD region — that's the slice of the window
    // that sits above the physical controller and is actually visible
    // to the player. Glyph height is sized off the region height so a
    // tall HUD gets a tall countdown without overflowing.
    let top_rect = frame.top_region_rect();
    let top_w = top_rect.w as f32;
    let top_h = top_rect.h as f32;
    if top_w <= 0.0 || top_h <= 0.0 {
        return;
    }
    // Glyph height capped well under the HUD height to leave vertical
    // breathing room. Holds steady through the step (no per-frame size
    // tweening — that read as jitter against the surrounding HUD).
    let size = if label == "GO!" {
        (top_h * 0.55).clamp(36.0, 72.0)
    } else {
        (top_h * 0.60).clamp(40.0, 80.0)
    };
    // Centered chip sized to the glyph so the backdrop doesn't paint
    // over the score / life HUD widgets to either side.
    let chip_w = (size * 1.6).min(top_w * 0.4);
    let chip_h = (size * 1.2).min(top_h * 0.85);
    let chip_x = top_rect.x as f32 + (top_w - chip_w) * 0.5;
    let chip_y = top_rect.y as f32 + (top_h - chip_h) * 0.5;

    overlay.draw(frame, |rc| {
        let id = egui::Id::new("rhythm_countdown_root");
        egui::Area::new(id)
            .fixed_pos(egui::pos2(chip_x, chip_y))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(chip_w);
                ui.set_height(chip_h);
                let painter = ui.painter();
                let rect = ui.max_rect();
                let backdrop_alpha = (alpha / 2).min(110);
                painter.rect_filled(
                    rect,
                    egui::Rounding::same(8),
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, backdrop_alpha),
                );
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(size),
                    color,
                );
            });
    });
}

/// For long notes, returns 1.0 at hit_time (start of hold) → 0.0 at
/// release_time. Before hit_time it stays 1.0 (note hasn't started yet);
/// after release_time it stays 0.0.
pub fn hold_progress(music_ms: f64, hit_ms: f64, release_ms: f64) -> f32 {
    if music_ms <= hit_ms {
        return 1.0;
    }
    if music_ms >= release_ms {
        return 0.0;
    }
    let total = (release_ms - hit_ms).max(1.0);
    let elapsed = music_ms - hit_ms;
    (1.0 - (elapsed / total)).clamp(0.0, 1.0) as f32
}

/// Linear interpolate between green (1.0), yellow (0.5), red (0.0) for the
/// life bar fill color. Pure green at full life, pure red at empty.
pub fn life_bar_color(life: f32) -> egui::Color32 {
    let t = life.clamp(0.0, 1.0);
    let (r, g) = if t >= 0.5 {
        // green → yellow as t drops from 1.0 to 0.5
        let k = (1.0 - t) * 2.0; // 0..1 over 1.0..0.5
        (k, 1.0)
    } else {
        // yellow → red as t drops from 0.5 to 0.0
        let k = t * 2.0; // 0..1 over 0.0..0.5
        (1.0, k)
    };
    egui::Color32::from_rgb((r * 255.0).round() as u8, (g * 255.0).round() as u8, 20)
}

/// Lazy texture cache for the HUD's album-art / jacket tile.
///
/// Mirrors the picker's `JacketCache` but kept distinct so the two never
/// share a texture handle (different egui contexts: picker spawns its own
/// app, rhythm-mode has its own, and exec() tears them down individually).
/// Negative caching: a path that fails to load resolves to `None` and is
/// not retried in this process.
#[derive(Default)]
pub struct HudJacketCache {
    inner: HashMap<PathBuf, Option<egui::TextureHandle>>,
}

impl HudJacketCache {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Return the texture for `path`, loading on first request. Errors
    /// are logged once; the key stays in the map as `None` so we don't
    /// retry the same broken file every frame.
    pub fn get_or_load(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Option<&egui::TextureHandle> {
        if !self.inner.contains_key(path) {
            let loaded = load_hud_jacket(ctx, path);
            self.inner.insert(path.to_path_buf(), loaded);
        }
        self.inner.get(path).and_then(|o| o.as_ref())
    }
}

fn load_hud_jacket(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                target: "juballer::rhythm::hud",
                "jacket read {}: {e}",
                path.display()
            );
            return None;
        }
    };
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(
                target: "juballer::rhythm::hud",
                "jacket decode {}: {e}",
                path.display()
            );
            return None;
        }
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let uri = format!("hud-jacket://{}", path.display());
    Some(ctx.load_texture(uri, color_img, egui::TextureOptions::LINEAR))
}

pub fn draw_hud(
    frame: &mut Frame,
    overlay: &mut EguiOverlay,
    state: &GameState,
    finished: bool,
    jackets: &mut HudJacketCache,
) {
    draw_hud_with_narration(frame, overlay, state, finished, jackets, None, None);
}

/// Like [`draw_hud`] but paints an optional translucent narration strip
/// directly below the title block. Used by tutorial mode to overlay lesson
/// text. `narration = None` is equivalent to [`draw_hud`].
///
/// `offset_applied_at` powers the "saved!" toast on the results screen
/// after the player taps APPLY GLOBAL / APPLY SONG. None = no toast.
pub fn draw_hud_with_narration(
    frame: &mut Frame,
    overlay: &mut EguiOverlay,
    state: &GameState,
    finished: bool,
    jackets: &mut HudJacketCache,
    narration: Option<&str>,
    offset_applied_at: Option<std::time::Instant>,
) {
    let cell_rects = *frame.cell_rects();
    let title = state.chart.title.clone();
    let artist = state.chart.artist.clone();
    let music_ms = state.music_time_ms;
    // Live tempo — follows mid-song BPM changes instead of pinning to the chart's initial BPM.
    let bpm = state.chart.schedule.bpm_at(music_ms);
    let combo = state.combo;
    let max_combo = state.max_combo;
    let score = state.score;
    let perfect = state.count(Grade::Perfect);
    let great = state.count(Grade::Great);
    let good = state.count(Grade::Good);
    let poor = state.count(Grade::Poor);
    let miss = state.count(Grade::Miss);
    let total = state.total_notes();
    let judged = state.judged_notes();
    let life = state.life;
    let failed = state.failed;
    let narration_owned: Option<String> = narration.map(|s| s.to_string());

    // If the chart has a jacket, load it once (cheap after first frame) and
    // reserve a 120×120 slot on the right side of the HUD — the BPM/time/
    // judged text shifts left to avoid it. When no jacket is present the
    // layout is unchanged.
    let jacket_path = state.chart.jacket_path.clone();
    let banner_path = state.chart.mini_path.clone();
    let top_rect = frame.top_region_rect();
    overlay.draw(frame, |rc| {
        let hud_jacket_tex = jacket_path
            .as_deref()
            .and_then(|p| jackets.get_or_load(rc.ctx(), p))
            .cloned();
        // mini.png is the wide song-select banner (typ. ~340×64, 5:1).
        // Reuses the same texture cache as jacket — keys are full paths
        // so there's no collision risk.
        let hud_banner_tex = banner_path
            .as_deref()
            .and_then(|p| jackets.get_or_load(rc.ctx(), p))
            .cloned();
        // Top-region painter: we don't use panes here, just place a single Area at the
        // top region's pixel rect and draw directly into it.
        let id = egui::Id::new("rhythm_hud_root");
        egui::Area::new(id)
            .fixed_pos(egui::pos2(top_rect.x as f32, top_rect.y as f32))
            .order(egui::Order::Foreground)
            .show(rc.ctx(), |ui| {
                ui.set_width(top_rect.w as f32);
                ui.set_height(top_rect.h as f32);
                let painter = ui.painter();
                let rect = ui.max_rect();
                // Translucent backdrop so text stays legible over any background.
                painter.rect_filled(
                    rect,
                    egui::Rounding::same(6),
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
                );

                // Banner strip — wide song-select image (mini.png, ~5:1)
                // laid across the top of the HUD above the title text.
                // Scales to fit whatever horizontal space is left after
                // the jacket slot on the right, height-capped so it stays
                // subordinate to the title.
                let banner_h = 34.0_f32;
                let banner_y_pad = 6.0_f32;
                let banner_right_limit = if hud_jacket_tex.is_some() {
                    rect.right() - 120.0 - 10.0 - 10.0 // match jacket block
                } else {
                    rect.right() - 14.0
                };
                let banner_left = rect.left() + 14.0;
                let banner_avail_w = (banner_right_limit - banner_left).max(0.0);
                if let Some(tex) = hud_banner_tex.as_ref() {
                    let tex_size = tex.size_vec2();
                    let aspect = if tex_size.y > 0.0 { tex_size.x / tex_size.y } else { 5.0 };
                    let want_w = banner_h * aspect;
                    let drawn_w = want_w.min(banner_avail_w);
                    let drawn_h = drawn_w / aspect.max(0.01);
                    let banner_rect = egui::Rect::from_min_size(
                        egui::pos2(banner_left, rect.top() + banner_y_pad),
                        egui::vec2(drawn_w, drawn_h),
                    );
                    painter.image(
                        tex.id(),
                        banner_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                // Title + artist shift down below the banner strip when
                // one is painted (18px padding from HUD top → 44px).
                // Outlined so they stay legible overlapping the banner
                // image's bright regions.
                let title_y = if hud_banner_tex.is_some() { 44.0 } else { 10.0 };
                super::textfx::text_outlined(
                    &painter,
                    rect.left_top() + egui::vec2(14.0, title_y),
                    egui::Align2::LEFT_TOP,
                    &title,
                    egui::FontId::proportional(22.0),
                    egui::Color32::WHITE,
                );
                super::textfx::text_outlined(
                    &painter,
                    rect.left_top() + egui::vec2(14.0, title_y + 30.0),
                    egui::Align2::LEFT_TOP,
                    &artist,
                    egui::FontId::proportional(14.0),
                    egui::Color32::LIGHT_GRAY,
                );

                // Optional narration strip — painted below the title block
                // (above the life bar) when tutorial / coaching mode supplies
                // a hook that returns Some. Translucent backdrop so it's
                // visible over both banner and regular HUD states.
                if let Some(text) = narration_owned.as_deref() {
                    let n_pad_x = 14.0_f32;
                    let n_top = rect.top() + 58.0;
                    let n_h = 22.0_f32;
                    let n_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.left() + n_pad_x, n_top),
                        egui::pos2(rect.right() - n_pad_x, n_top + n_h),
                    );
                    painter.rect_filled(
                        n_rect,
                        egui::Rounding::same(4),
                        egui::Color32::from_rgba_unmultiplied(20, 30, 50, 200),
                    );
                    painter.text(
                        n_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        text,
                        egui::FontId::proportional(14.0),
                        egui::Color32::from_rgb(220, 230, 255),
                    );
                }

                // Life bar — sits below the title/banner row and the
                // top-right BPM/time/judged stack. Right edge clips short
                // of the jacket tile (when present) so it doesn't run
                // under the album art and so the "judged/total" counter
                // doesn't paint on top of the bar fill.
                let life_pad_x = 14.0_f32;
                let life_h = 8.0_f32;
                let life_y = rect.top() + 80.0;
                let life_right_clear = if hud_jacket_tex.is_some() {
                    120.0_f32 + 10.0 + 6.0 // jacket size + jacket pad + breathing room
                } else {
                    life_pad_x
                };
                let life_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + life_pad_x, life_y),
                    egui::pos2(rect.right() - life_right_clear, life_y + life_h),
                );
                painter.rect_filled(
                    life_rect,
                    egui::Rounding::same(3),
                    egui::Color32::from_rgba_unmultiplied(30, 30, 30, 220),
                );
                let fill_w = life_rect.width() * life.clamp(0.0, 1.0);
                if fill_w > 0.5 {
                    let fill_rect = egui::Rect::from_min_max(
                        life_rect.min,
                        egui::pos2(life_rect.min.x + fill_w, life_rect.max.y),
                    );
                    painter.rect_filled(
                        fill_rect,
                        egui::Rounding::same(3),
                        life_bar_color(life),
                    );
                }
                painter.rect_stroke(
                    life_rect,
                    egui::Rounding::same(3),
                    egui::Stroke::new(1.0, egui::Color32::from_gray(180)), egui::StrokeKind::Middle);

                // Jacket tile (top-right) — 120×120 when present. Painted
                // before the BPM/time/judged block so the text overlays
                // look correct relative to the jacket's left edge.
                let jacket_size = 120.0_f32;
                let jacket_pad = 10.0_f32;
                let text_right_inset = if hud_jacket_tex.is_some() {
                    // Text column ends to the *left* of the jacket.
                    jacket_size + jacket_pad + 14.0
                } else {
                    14.0
                };
                if let Some(tex) = hud_jacket_tex.as_ref() {
                    let top_right = egui::pos2(
                        rect.right() - jacket_pad - jacket_size,
                        rect.top() + jacket_pad,
                    );
                    let jacket_rect = egui::Rect::from_min_size(
                        top_right,
                        egui::vec2(jacket_size, jacket_size),
                    );
                    painter.image(
                        tex.id(),
                        jacket_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

                // BPM + time + counter row — laid out horizontally
                // *below* the life bar so they never collide with it
                // or with the jacket tile. Anchored to the right edge
                // of the HUD (text_right_inset already accounts for
                // jacket-clear when present).
                let _ = text_right_inset; // retained for layout symmetry
                let secs = (music_ms.max(0.0) / 1000.0) as i64;
                let mm = secs / 60;
                let ss = secs % 60;
                let info_y = life_y + life_h + 6.0;
                let info_right = rect.right() - 14.0;
                painter.text(
                    egui::pos2(info_right, info_y),
                    egui::Align2::RIGHT_TOP,
                    format!("{judged}/{total}"),
                    egui::FontId::monospace(13.0),
                    egui::Color32::LIGHT_GRAY,
                );
                let counter_w = 64.0_f32;
                painter.text(
                    egui::pos2(info_right - counter_w, info_y),
                    egui::Align2::RIGHT_TOP,
                    format!("{mm:02}:{ss:02}"),
                    egui::FontId::monospace(16.0),
                    egui::Color32::WHITE,
                );
                let time_w = 70.0_f32;
                painter.text(
                    egui::pos2(info_right - counter_w - time_w, info_y),
                    egui::Align2::RIGHT_TOP,
                    format!("{bpm:.0} BPM"),
                    egui::FontId::proportional(15.0),
                    egui::Color32::LIGHT_YELLOW,
                );

                // Combo (center, huge).
                let combo_label = if combo > 0 {
                    format!("{combo}")
                } else {
                    "—".to_string()
                };
                painter.text(
                    rect.center() - egui::vec2(0.0, 6.0),
                    egui::Align2::CENTER_CENTER,
                    combo_label,
                    egui::FontId::proportional(44.0),
                    if combo >= 20 {
                        egui::Color32::GOLD
                    } else {
                        egui::Color32::WHITE
                    },
                );
                painter.text(
                    rect.center() + egui::vec2(0.0, 28.0),
                    egui::Align2::CENTER_CENTER,
                    "combo",
                    egui::FontId::proportional(12.0),
                    egui::Color32::GRAY,
                );

                // Score + grade counts (bottom row).
                let score_text = format!("SCORE {score}");
                painter.text(
                    rect.left_bottom() + egui::vec2(14.0, -10.0),
                    egui::Align2::LEFT_BOTTOM,
                    score_text,
                    egui::FontId::proportional(18.0),
                    egui::Color32::from_rgb(120, 220, 255),
                );
                let grades = format!(
                    "P {perfect}  GT {great}  GD {good}  PO {poor}  M {miss}   MAX {max_combo}"
                );
                painter.text(
                    rect.right_bottom() + egui::vec2(-14.0, -10.0),
                    egui::Align2::RIGHT_BOTTOM,
                    grades,
                    egui::FontId::monospace(13.0),
                    egui::Color32::WHITE,
                );

                if failed {
                    let banner = egui::Color32::from_rgba_unmultiplied(40, 6, 6, 235);
                    painter.rect_filled(rect, egui::Rounding::same(6), banner);
                    painter.text(
                        rect.center() - egui::vec2(0.0, 16.0),
                        egui::Align2::CENTER_CENTER,
                        "FAILED",
                        egui::FontId::proportional(60.0),
                        egui::Color32::from_rgb(255, 60, 60),
                    );
                    painter.text(
                        rect.center() + egui::vec2(0.0, 30.0),
                        egui::Align2::CENTER_CENTER,
                        format!("SCORE {score}   {judged}/{total}"),
                        egui::FontId::proportional(16.0),
                        egui::Color32::LIGHT_GRAY,
                    );
                    painter.text(
                        rect.center() + egui::vec2(0.0, 54.0),
                        egui::Align2::CENTER_CENTER,
                        "exiting…",
                        egui::FontId::proportional(12.0),
                        egui::Color32::GRAY,
                    );
                } else if finished {
                    let banner = egui::Color32::from_rgba_unmultiplied(10, 10, 10, 230);
                    painter.rect_filled(rect, egui::Rounding::same(6), banner);

                    let accuracy = state.accuracy_pct().unwrap_or(0.0);
                    let mean_off = state.mean_input_offset_ms();
                    let suggest = state.recommended_audio_offset_ms();

                    // Row 1 — final score, gold. When a personal best is
                    // known, show it to the right so the player can see
                    // whether they topped it. "(new best!)" marker appears
                    // when score ≥ best and best is non-zero.
                    painter.text(
                        rect.center() - egui::vec2(0.0, 46.0),
                        egui::Align2::CENTER_CENTER,
                        format!("FINAL  {score}"),
                        egui::FontId::proportional(34.0),
                        egui::Color32::GOLD,
                    );
                    let best_text = match state.best_score {
                        Some(b) if b > 0 && score >= b => format!("BEST: {b}  (new best!)"),
                        Some(b) => format!("BEST: {b}"),
                        None => "BEST: —".to_string(),
                    };
                    painter.text(
                        rect.center() - egui::vec2(0.0, 72.0),
                        egui::Align2::CENTER_CENTER,
                        best_text,
                        egui::FontId::proportional(14.0),
                        egui::Color32::from_rgb(200, 200, 255),
                    );
                    // Row 2 — accuracy + hit/total.
                    painter.text(
                        rect.center() - egui::vec2(0.0, 12.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{accuracy:.1}% accuracy   {judged}/{total} hit"),
                        egui::FontId::proportional(16.0),
                        egui::Color32::WHITE,
                    );
                    // Row 3 — grade counts.
                    painter.text(
                        rect.center() + egui::vec2(0.0, 10.0),
                        egui::Align2::CENTER_CENTER,
                        format!(
                            "P {perfect}  GT {great}  GD {good}  PO {poor}  M {miss}   MAX {max_combo}"
                        ),
                        egui::FontId::monospace(13.0),
                        egui::Color32::LIGHT_GRAY,
                    );
                    // Row 4 — calibration hint.
                    let hint = match (mean_off, suggest) {
                        (Some(m), Some(off)) => {
                            format!("mean offset {m:+.1}ms  →  next run: --audio-offset-ms {off}")
                        }
                        (Some(m), None) => format!("mean offset {m:+.1}ms  (need more samples to suggest offset)"),
                        _ => "no offset samples collected".to_string(),
                    };
                    let hint_color = if suggest.is_some() {
                        egui::Color32::from_rgb(140, 220, 255)
                    } else {
                        egui::Color32::from_rgb(170, 170, 170)
                    };
                    painter.text(
                        rect.center() + egui::vec2(0.0, 30.0),
                        egui::Align2::CENTER_CENTER,
                        hint,
                        egui::FontId::proportional(13.0),
                        hint_color,
                    );
                    // Row 5 — bottom-row button hint (matches the cell
                    // labels painted into the grid below).
                    let bottom_hint = if suggest.is_some() {
                        "(3,0) APPLY GLOBAL    (3,1) APPLY THIS SONG    (any other) CONTINUE"
                    } else {
                        "tap any cell to continue"
                    };
                    painter.text(
                        rect.center() + egui::vec2(0.0, 52.0),
                        egui::Align2::CENTER_CENTER,
                        bottom_hint,
                        egui::FontId::proportional(12.0),
                        egui::Color32::from_rgb(180, 200, 220),
                    );
                    // Transient "saved!" toast confirming the write — the
                    // suggested offset value itself doesn't change after
                    // applying, so we just acknowledge the action.
                    if let Some(t) = offset_applied_at {
                        let age = t.elapsed().as_secs_f32();
                        if age < 1.5 {
                            let alpha = ((1.5 - age) / 1.5 * 255.0).clamp(0.0, 255.0) as u8;
                            painter.text(
                                rect.center() + egui::vec2(0.0, 70.0),
                                egui::Align2::CENTER_CENTER,
                                "✓ offset saved",
                                egui::FontId::proportional(14.0),
                                egui::Color32::from_rgba_unmultiplied(120, 240, 140, alpha),
                            );
                        }
                    }

                    // Paint the three action buttons directly into the
                    // grid cells (3,0)/(3,1)/(3,2) so the player sees
                    // *where* on the controller to tap. Drawn here (not
                    // inside the HUD's own rect) by reaching out via
                    // `cell_rects` captured at the top of this fn.
                    if let Some(off) = suggest {
                        let buttons = [
                            (3usize * 4, "APPLY", &format!("global {off:+}ms")[..],
                             egui::Color32::from_rgb(120, 220, 255)),
                            (3 * 4 + 1, "APPLY", &format!("this song {off:+}ms")[..],
                             egui::Color32::from_rgb(180, 240, 140)),
                            (3 * 4 + 2, "CONTINUE", "(any cell)",
                             egui::Color32::from_rgb(240, 220, 140)),
                        ];
                        for (idx, label, sub, fg) in buttons {
                            let cr = cell_rects[idx];
                            let er = egui::Rect::from_min_size(
                                egui::pos2(cr.x as f32, cr.y as f32),
                                egui::vec2(cr.w as f32, cr.h as f32),
                            );
                            painter.rect_filled(
                                er,
                                egui::Rounding::same(6),
                                egui::Color32::from_rgba_unmultiplied(20, 22, 30, 220),
                            );
                            painter.rect_stroke(
                                er,
                                egui::Rounding::same(6),
                                egui::Stroke::new(1.5, fg), egui::StrokeKind::Middle);
                            super::textfx::text_outlined(
                                &painter,
                                er.center() + egui::vec2(0.0, -10.0),
                                egui::Align2::CENTER_CENTER,
                                label,
                                egui::FontId::proportional(20.0),
                                fg,
                            );
                            super::textfx::text_outlined(
                                &painter,
                                er.center() + egui::vec2(0.0, 14.0),
                                egui::Align2::CENTER_CENTER,
                                sub,
                                egui::FontId::proportional(11.0),
                                egui::Color32::from_rgb(200, 210, 220),
                            );
                        }
                    } else {
                        // No suggested offset — only show CONTINUE.
                        let cr = cell_rects[3 * 4 + 2];
                        let er = egui::Rect::from_min_size(
                            egui::pos2(cr.x as f32, cr.y as f32),
                            egui::vec2(cr.w as f32, cr.h as f32),
                        );
                        painter.rect_filled(
                            er,
                            egui::Rounding::same(6),
                            egui::Color32::from_rgba_unmultiplied(20, 22, 30, 220),
                        );
                        super::textfx::text_outlined(
                            &painter,
                            er.center(),
                            egui::Align2::CENTER_CENTER,
                            "CONTINUE",
                            egui::FontId::proportional(20.0),
                            egui::Color32::from_rgb(240, 220, 140),
                        );
                    }
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::countdown_label_phase;

    #[test]
    fn countdown_labels_each_second() {
        // Top of countdown = 3000ms → "3". Just after = still "3". Crossing
        // into the 2000-1000 band flips to "2", etc.
        assert_eq!(countdown_label_phase(3000.0).0, "3");
        assert_eq!(countdown_label_phase(2500.0).0, "3");
        assert_eq!(countdown_label_phase(2000.0).0, "2");
        assert_eq!(countdown_label_phase(1500.0).0, "2");
        assert_eq!(countdown_label_phase(1000.0).0, "1");
        assert_eq!(countdown_label_phase(500.0).0, "1");
        assert_eq!(countdown_label_phase(1.0).0, "1");
        // At and past 0 we're in the "GO!" frame.
        assert_eq!(countdown_label_phase(0.0).0, "GO!");
        assert_eq!(countdown_label_phase(-250.0).0, "GO!");
    }

    #[test]
    fn countdown_phase_monotonic_within_step() {
        // Sampling a single step (3 → 2, 3000ms → 2000.001ms) should yield
        // a strictly non-decreasing phase value climbing toward 1.0.
        let samples = [3000.0, 2800.0, 2500.0, 2200.0, 2010.0];
        let mut prev = -1.0f32;
        let mut last_label = "";
        for ms in samples {
            let (label, phase) = countdown_label_phase(ms);
            assert_eq!(label, "3");
            assert!(phase >= prev, "phase regressed: {prev} → {phase} at {ms}ms");
            assert!((0.0..=1.0).contains(&phase), "phase out of range: {phase}");
            prev = phase;
            last_label = label;
        }
        assert_eq!(last_label, "3");
        assert!(
            prev > 0.9,
            "expected phase to approach 1.0 by end of step, got {prev}"
        );
    }

    #[test]
    fn countdown_phase_resets_at_step_boundary() {
        // Crossing from "3" → "2" → "1" should restart phase near 0 at each
        // step's opening. We can't hit exactly 0 on the boundary (the
        // boundary itself belongs to the next step), so sample just inside.
        let (l3, p3) = countdown_label_phase(2999.99);
        let (l2, p2) = countdown_label_phase(1999.99);
        let (l1, p1) = countdown_label_phase(999.99);
        assert_eq!(l3, "3");
        assert_eq!(l2, "2");
        assert_eq!(l1, "1");
        // Each step's opening phase should be very close to 0.
        for (lbl, p) in [(l3, p3), (l2, p2), (l1, p1)] {
            assert!(
                p < 0.01,
                "{lbl}: expected near-0 phase at step open, got {p}"
            );
        }
    }

    #[test]
    fn countdown_clamps_large_input_to_top_of_three() {
        // Garbage-large input (e.g. the song hasn't even *scheduled* yet)
        // should clamp to "3" at phase 0, not panic or wrap.
        let (label, phase) = countdown_label_phase(10_000.0);
        assert_eq!(label, "3");
        assert!(phase.abs() < 1e-4, "expected phase ~0, got {phase}");
    }
}
