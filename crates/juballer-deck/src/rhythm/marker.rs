//! Per-grade animated tile effects for rhythm mode.
//!
//! Each marker set ships under `assets/markers/tap/<name>/` with a
//! `marker.json` + sprite sheets. The default set
//! (`assets/markers/tap/juballer_default/`) is produced by
//! `scripts/generate_markers.sh` via ImageMagick — no third-party art.
//!
//! Each marker folder has a `marker.json` declaring:
//!   * `approach`: 16-frame lead-in animation. Negative offsets relative to
//!     the note's hit time; e.g. at `-16/fps` the first frame is visible, at
//!     `0` the note reaches hit time and sprite dispatch switches to the
//!     grade or miss animation.
//!   * `{perfect,great,good,poor,miss}`: 9-frame post-judgment animation
//!     played from `music_time - judged_at_time` in positive seconds.
//!
//! Time → frame: `floor(offset_s * fps)`. Each sheet is a regular grid of
//! `columns × rows` frames; we pick the `frame`-th one and map to a UV rect.

use crate::{Error, Result};
use egui::{ColorImage, Context, Rect, TextureHandle, TextureOptions};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct MarkerJson {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    size: u32,
    fps: f64,
    approach: AnimSpec,
    miss: AnimSpec,
    poor: AnimSpec,
    good: AnimSpec,
    great: AnimSpec,
    perfect: AnimSpec,
}

#[derive(Debug, Deserialize, Clone)]
struct AnimSpec {
    sprite_sheet: String,
    count: usize,
    columns: usize,
    rows: usize,
    /// Optional per-anim override for playback rate. Falls back to the
    /// marker's root `fps` when absent. Lets us play approach at a slower
    /// rate (16 frames over 1s lead-in) while keeping grade bursts snappy.
    #[serde(default)]
    fps: Option<f64>,
}

pub struct Animation {
    pub texture: TextureHandle,
    pub count: usize,
    pub columns: usize,
    pub rows: usize,
    pub fps: f64,
}

impl Animation {
    /// UV rect (0..1 in both axes) for the `frame`-th cell in the sheet.
    pub fn frame_uv(&self, frame: usize) -> Rect {
        let col = (frame % self.columns) as f32;
        let row = (frame / self.columns) as f32;
        let w = 1.0 / self.columns as f32;
        let h = 1.0 / self.rows as f32;
        Rect::from_min_size(egui::pos2(col * w, row * h), egui::vec2(w, h))
    }
}

pub struct Markers {
    pub fps: f64,
    pub approach: Animation,
    pub perfect: Animation,
    pub great: Animation,
    pub good: Animation,
    pub poor: Animation,
    pub miss: Animation,
}

#[derive(Clone, Copy)]
pub enum GradePhase {
    Perfect,
    Great,
    Good,
    Poor,
    Miss,
}

impl Markers {
    pub fn load(ctx: &Context, dir: &Path) -> Result<Self> {
        let bytes = std::fs::read(dir.join("marker.json"))
            .map_err(|e| Error::Config(format!("marker.json: {e}")))?;
        let meta: MarkerJson = serde_json::from_slice(&bytes)
            .map_err(|e| Error::Config(format!("marker.json parse: {e}")))?;
        let root_fps = meta.fps;
        let load_anim = |s: &AnimSpec| -> Result<Animation> {
            let p = dir.join(&s.sprite_sheet);
            let img =
                image::open(&p).map_err(|e| Error::Config(format!("load {}: {e}", p.display())))?;
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width() as usize, rgba.height() as usize);
            let color = ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            let texture = ctx.load_texture(
                format!("marker/{}", s.sprite_sheet),
                color,
                TextureOptions::LINEAR,
            );
            Ok(Animation {
                texture,
                count: s.count,
                columns: s.columns,
                rows: s.rows,
                fps: s.fps.unwrap_or(root_fps),
            })
        };
        Ok(Self {
            fps: meta.fps,
            approach: load_anim(&meta.approach)?,
            perfect: load_anim(&meta.perfect)?,
            great: load_anim(&meta.great)?,
            good: load_anim(&meta.good)?,
            poor: load_anim(&meta.poor)?,
            miss: load_anim(&meta.miss)?,
        })
    }

    /// Approach phase — `offset_ms` is `music_time - hit_time` and should be
    /// negative during lead-in. Returns the texture + UV for the frame that
    /// should be visible now, or `None` if we're outside the lead-in window
    /// (offset_ms >= 0, i.e. note has already hit).
    pub fn approach_frame(&self, offset_ms: f64) -> Option<(&TextureHandle, Rect)> {
        let raw = (offset_ms / 1000.0 * self.approach.fps).floor() as i32;
        if raw >= 0 {
            return None;
        }
        let frame = (raw + self.approach.count as i32) as usize;
        if frame >= self.approach.count {
            return None;
        }
        Some((&self.approach.texture, self.approach.frame_uv(frame)))
    }

    /// Post-judgment phase — `offset_ms` is `music_time - judged_at_ms`
    /// (should be >= 0). Returns None once the animation completes.
    pub fn grade_frame(&self, phase: GradePhase, offset_ms: f64) -> Option<(&TextureHandle, Rect)> {
        let anim = match phase {
            GradePhase::Perfect => &self.perfect,
            GradePhase::Great => &self.great,
            GradePhase::Good => &self.good,
            GradePhase::Poor => &self.poor,
            GradePhase::Miss => &self.miss,
        };
        let raw = (offset_ms / 1000.0 * anim.fps).floor() as i32;
        if raw < 0 {
            return None;
        }
        let frame = raw as usize;
        if frame >= anim.count {
            return None;
        }
        Some((&anim.texture, anim.frame_uv(frame)))
    }

    /// Tweened approach picker. `offset_ms` is `music_time - hit_time` (negative
    /// during lead-in). Returns `(texture, uv_a, uv_b, t)` for a crossfade
    /// between adjacent sprite frames. `None` once past hit moment.
    pub fn approach_frame_tweened(
        &self,
        offset_ms: f64,
    ) -> Option<(&TextureHandle, Rect, Rect, f32)> {
        // Approach uses "count minus index from the end" convention — raw goes
        // negative; frame index = raw + count. See approach_frame below.
        let fps = self.approach.fps;
        let count = self.approach.count;
        let raw = offset_ms / 1000.0 * fps;
        if raw >= 0.0 {
            return None;
        }
        let f = raw + count as f64;
        if f < 0.0 || f >= count as f64 {
            return None;
        }
        let frame_a = f.floor() as usize;
        let frame_b = (frame_a + 1).min(count - 1);
        let t = if frame_b == frame_a {
            0.0
        } else {
            (f - frame_a as f64) as f32
        };
        Some((
            &self.approach.texture,
            self.approach.frame_uv(frame_a),
            self.approach.frame_uv(frame_b),
            t.clamp(0.0, 1.0),
        ))
    }

    /// Tweened grade-burst picker. `offset_ms` is `music_time - judged_at_ms`
    /// (>= 0). Returns None once anim ends.
    pub fn grade_frame_tweened(
        &self,
        phase: GradePhase,
        offset_ms: f64,
    ) -> Option<(&TextureHandle, Rect, Rect, f32)> {
        let anim = match phase {
            GradePhase::Perfect => &self.perfect,
            GradePhase::Great => &self.great,
            GradePhase::Good => &self.good,
            GradePhase::Poor => &self.poor,
            GradePhase::Miss => &self.miss,
        };
        let offset_s = offset_ms / 1000.0;
        let (a, b, t) = tween_frame_at(offset_s, anim.fps, anim.count)?;
        Some((&anim.texture, anim.frame_uv(a), anim.frame_uv(b), t))
    }
}

/// Resolve the default marker directory shipped with the project. Probes
/// `CARGO_MANIFEST_DIR/../../assets/...` for dev builds and CWD-relative
/// for ad-hoc runs.
pub fn default_marker_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dev = manifest.join("../../assets/markers/tap/juballer_default");
    if dev.exists() {
        return dev;
    }
    PathBuf::from("assets/markers/tap/juballer_default")
}

/// Map a `Grade` from our judge into the marker animation variant. MISS has
/// its own animation; everything else gets the matching grade sheet.
pub fn grade_to_phase(g: super::judge::Grade) -> GradePhase {
    use super::judge::Grade;
    match g {
        Grade::Perfect => GradePhase::Perfect,
        Grade::Great => GradePhase::Great,
        Grade::Good => GradePhase::Good,
        Grade::Poor => GradePhase::Poor,
        Grade::Miss => GradePhase::Miss,
    }
}

/// Pure helper: given `offset_s`, `fps`, and `count`, return
/// (frame_a, frame_b, t) matching the tween contract:
///  - frame_a in [0, count)
///  - frame_b = min(frame_a + 1, count - 1)
///  - t in [0, 1]; at the last frame t clamps to 0 so the anim holds
fn tween_frame_at(offset_s: f64, fps: f64, count: usize) -> Option<(usize, usize, f32)> {
    if count == 0 {
        return None;
    }
    let f = offset_s * fps;
    if f < 0.0 {
        return None;
    }
    let frame_a = f.floor() as usize;
    if frame_a >= count {
        return None;
    }
    let frame_b = (frame_a + 1).min(count - 1);
    let raw_t = (f - frame_a as f64) as f32;
    let t = if frame_b == frame_a {
        0.0
    } else {
        raw_t.clamp(0.0, 1.0)
    };
    Some((frame_a, frame_b, t))
}

#[cfg(test)]
mod tween_tests {
    use super::tween_frame_at;

    #[test]
    fn frame_boundary_yields_zero_t() {
        let (a, b, t) = tween_frame_at(2.0 / 30.0, 30.0, 16).unwrap();
        assert_eq!(a, 2);
        assert_eq!(b, 3);
        assert!(t.abs() < 1e-5);
    }

    #[test]
    fn midway_yields_half_t() {
        let (a, b, t) = tween_frame_at(2.5 / 30.0, 30.0, 16).unwrap();
        assert_eq!(a, 2);
        assert_eq!(b, 3);
        assert!((t - 0.5).abs() < 1e-5);
    }

    #[test]
    fn last_frame_clamps_b_and_zero_t() {
        let (a, b, t) = tween_frame_at((15.0 + 0.7) / 30.0, 30.0, 16).unwrap();
        assert_eq!(a, 15);
        assert_eq!(b, 15, "frame_b must clamp to count - 1 at the tail");
        assert!(
            t.abs() < 1e-5,
            "t must be 0 at the tail to hold the last frame"
        );
    }

    #[test]
    fn past_end_returns_none() {
        assert!(tween_frame_at(1.0, 30.0, 16).is_none()); // 30 frames > count 16
    }

    #[test]
    fn negative_offset_returns_none() {
        assert!(tween_frame_at(-0.1, 30.0, 16).is_none());
    }
}
