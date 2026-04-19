//! HUD top-bar background renderer.
//!
//! The rhythm HUD paints title/artist/combo/score/jacket into the
//! `top_region`. Behind all of that, a *background* can be rendered
//! from either:
//!   * a WGSL shader (full GPU, receives music + input as uniforms)
//!   * an image (PNG / JPG / JPEG — stretched to fill the rect)
//!
//! Entries are configured under `deck.toml`'s `[rhythm]` section:
//!
//! ```toml
//! [rhythm]
//! backgrounds = [
//!     "/some/path/bg_waves.wgsl",
//!     "/some/path/cover_collage.png",
//!     "/some/path/spectrum.wgsl",
//! ]
//! ```
//!
//! With N entries configured, we pick one per chart via
//! `hash(chart_path) % N` so each song gets a stable (but rotating
//! across the library) backdrop. Empty list = no background draw.
//!
//! # Shader convention
//!
//! Background shaders share the standard `struct Uniforms` block with
//! tile shaders (same bind group layout + standard vs_main) so WGSL
//! authors can reuse boilerplate. The field semantics differ:
//!
//! | field        | tile meaning       | background meaning                  |
//! |--------------|--------------------|-------------------------------------|
//! | resolution   | tile pixel size    | top-region pixel size               |
//! | time         | seconds since boot | seconds since boot                  |
//! | delta_time   | frame dt           | frame dt                            |
//! | cursor.x     | unused (0)         | music_ms / 1000.0                   |
//! | cursor.y     | unused (0)         | current BPM                         |
//! | kind         | action kind        | last grade idx (0=none..5=miss)     |
//! | bound        | bound flag         | 1 if any cell is held, else 0       |
//! | toggle_on    | toggle state       | beat_phase [0..1)                   |
//! | flash        | tile press flash   | last-hit-freshness [0..1]           |
//! | accent       | tile accent        | last-hit grade color (rgba)         |
//! | state.x      | custom             | life [0..1]                         |
//! | state.y      | custom             | combo (float)                       |
//! | state.z      | custom             | score / 10_000 (float)              |
//! | state.w      | custom             | held-cell mask as float (0..65535)  |

use crate::shader::{ShaderPipelineCache, TileUniforms};
use juballer_core::{Frame, Rect};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// One resolved background entry. Either a compiled shader path or a
/// preloaded image texture keyed by path.
#[derive(Debug, Clone)]
pub enum Background {
    Shader(PathBuf),
    Image(PathBuf),
}

impl Background {
    fn from_path(p: &Path) -> Option<Self> {
        let ext = p.extension().and_then(|s| s.to_str())?.to_ascii_lowercase();
        match ext.as_str() {
            "wgsl" => Some(Background::Shader(p.to_path_buf())),
            "png" | "jpg" | "jpeg" => Some(Background::Image(p.to_path_buf())),
            _ => None,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Background::Shader(p) | Background::Image(p) => p.as_path(),
        }
    }
}

/// Pick a stable background for `chart_path` out of `list`, honouring
/// an optional fixed index. Returns `None` if the list is empty or the
/// picked entry has an unrecognised extension.
///
/// Selection order:
///   1. `fixed_idx = Some(i)` with `i < list.len()` → always pin to
///      `list[i]`. Think of this as the "specific" / "pinned" mode.
///   2. Otherwise hash `chart_path` into the list (the "mix" mode).
///
/// Out-of-range fixed indices fall back to mix with a `warn!` log so
/// the user sees that their config value is stale after editing the
/// list.
pub fn pick_for_chart(
    chart_path: &Path,
    list: &[PathBuf],
    fixed_idx: Option<usize>,
) -> Option<Background> {
    if list.is_empty() {
        return None;
    }
    if let Some(i) = fixed_idx {
        if i < list.len() {
            return Background::from_path(&list[i]);
        }
        tracing::warn!(
            target: "juballer::rhythm::background",
            "background_index {i} out of range for {} entries — falling back to mix",
            list.len()
        );
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    chart_path.hash(&mut h);
    let idx = (h.finish() as usize) % list.len();
    Background::from_path(&list[idx])
}

/// Per-frame inputs the renderer packs into `TileUniforms` for
/// background shaders. Rhythm loop fills this in and hands it to
/// [`draw`]. Image-mode backgrounds ignore everything.
#[derive(Debug, Clone, Copy)]
pub struct BackgroundInputs {
    pub music_ms: f64,
    pub bpm: f64,
    pub beat_phase: f32,
    pub combo: u32,
    pub score: u64,
    pub life: f32,
    pub held_mask: u16,
    pub last_grade: f32,
    pub last_hit_elapsed_ms: f64,
    pub last_hit_accent: [f32; 4],
    /// 16 log-spaced FFT bins. All zeros when no audio tap is active
    /// (picker preview, mods screen, etc.). Populated each frame from
    /// [`crate::rhythm::spectrum::SharedSpectrum::snapshot`].
    pub spectrum: [f32; 16],
}

impl Default for BackgroundInputs {
    fn default() -> Self {
        Self {
            music_ms: 0.0,
            bpm: 120.0,
            beat_phase: 0.0,
            combo: 0,
            score: 0,
            life: 1.0,
            held_mask: 0,
            last_grade: 0.0,
            last_hit_elapsed_ms: 1e9,
            last_hit_accent: [1.0, 1.0, 1.0, 1.0],
            spectrum: [0.0; 16],
        }
    }
}

impl BackgroundInputs {
    fn into_uniforms(self, resolution: [f32; 2], boot_secs: f32, dt: f32) -> TileUniforms {
        // Flash decays linearly over 600ms post-hit — same scale the tile
        // shader uses for grade freeze. Clamped to 1.0 max.
        let flash = if self.last_hit_elapsed_ms >= 0.0 && self.last_hit_elapsed_ms < 600.0 {
            (1.0 - (self.last_hit_elapsed_ms / 600.0)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let bound = if self.held_mask != 0 { 1.0 } else { 0.0 };
        TileUniforms {
            resolution,
            time: boot_secs,
            delta_time: dt,
            cursor: [(self.music_ms as f32) / 1000.0, self.bpm as f32],
            kind: self.last_grade,
            bound,
            toggle_on: self.beat_phase,
            flash,
            _pad0: [0.0, 0.0],
            accent: self.last_hit_accent,
            state: [
                self.life,
                self.combo as f32,
                (self.score as f32) / 10_000.0,
                self.held_mask as f32,
            ],
            spectrum: [
                [
                    self.spectrum[0],
                    self.spectrum[1],
                    self.spectrum[2],
                    self.spectrum[3],
                ],
                [
                    self.spectrum[4],
                    self.spectrum[5],
                    self.spectrum[6],
                    self.spectrum[7],
                ],
                [
                    self.spectrum[8],
                    self.spectrum[9],
                    self.spectrum[10],
                    self.spectrum[11],
                ],
                [
                    self.spectrum[12],
                    self.spectrum[13],
                    self.spectrum[14],
                    self.spectrum[15],
                ],
            ],
        }
    }
}

/// Image-path → egui texture cache. Identical pattern to
/// `rhythm::render::HudJacketCache` but scoped here so background +
/// jacket don't race for each other's handles. Negative caching via
/// `None` keeps failed loads from re-trying every frame.
#[derive(Default)]
pub struct BackgroundImageCache {
    inner: HashMap<PathBuf, Option<egui::TextureHandle>>,
}

impl BackgroundImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_load(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Option<&egui::TextureHandle> {
        if !self.inner.contains_key(path) {
            self.inner
                .insert(path.to_path_buf(), load_texture(ctx, path));
        }
        self.inner.get(path).and_then(|o| o.as_ref())
    }
}

fn load_texture(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(target: "juballer::rhythm::background", "read {}: {e}", path.display());
            return None;
        }
    };
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(target: "juballer::rhythm::background", "decode {}: {e}", path.display());
            return None;
        }
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(
        format!("bg://{}", path.display()),
        color,
        egui::TextureOptions::LINEAR,
    ))
}

/// Draw a shader-mode background into `top_rect`. Must be called
/// BEFORE any egui overlay for the same frame or the HUD text will
/// render underneath the shader. Image-mode backgrounds use
/// [`draw_image_overlay`] from inside an egui overlay instead.
pub fn draw_shader(
    frame: &mut Frame,
    background: &Background,
    top_rect: Rect,
    inputs: BackgroundInputs,
    cache: &mut ShaderPipelineCache,
    boot_secs: f32,
    dt: f32,
) {
    let Background::Shader(path) = background else {
        return;
    };
    let path = path.clone();
    frame.with_region_raw(top_rect, |mut ctx| {
        let uniforms = inputs.into_uniforms([ctx.viewport.2, ctx.viewport.3], boot_secs, dt);
        cache.draw_tile(&mut ctx, &path, &uniforms);
    });
}

/// Draw an image-mode background stretched to `top_rect`. Called from
/// inside an egui overlay closure (the HUD reuses the same overlay for
/// the background + text to keep draw-order correct).
pub fn draw_image(
    rc: &mut juballer_egui::RegionCtx<'_>,
    background: &Background,
    top_rect: Rect,
    cache: &mut BackgroundImageCache,
) {
    let Background::Image(path) = background else {
        return;
    };
    let Some(tex) = cache.get_or_load(rc.ctx(), path) else {
        return;
    };
    let painter = rc.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new(("rhythm_bg", path.clone())),
    ));
    let rect = egui::Rect::from_min_size(
        egui::pos2(top_rect.x as f32, top_rect.y as f32),
        egui::vec2(top_rect.w as f32, top_rect.h as f32),
    );
    painter.image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_is_deterministic() {
        let list = vec![
            PathBuf::from("/bg/a.wgsl"),
            PathBuf::from("/bg/b.wgsl"),
            PathBuf::from("/bg/c.png"),
        ];
        let chart = Path::new("/charts/song_01.memon");
        let a = pick_for_chart(chart, &list, None).unwrap();
        let b = pick_for_chart(chart, &list, None).unwrap();
        assert_eq!(a.path(), b.path(), "same chart → same pick");
    }

    #[test]
    fn pick_varies_across_charts() {
        // Deterministic but not constant: two different chart paths
        // should not always hash to the same slot.
        let list: Vec<PathBuf> = (0..8)
            .map(|i| PathBuf::from(format!("/bg/{i}.wgsl")))
            .collect();
        let picked_paths: Vec<_> = (0..16)
            .map(|i| {
                let chart = PathBuf::from(format!("/charts/song_{i:02}.memon"));
                pick_for_chart(&chart, &list, None)
                    .unwrap()
                    .path()
                    .to_path_buf()
            })
            .collect();
        let unique: std::collections::HashSet<_> = picked_paths.iter().collect();
        assert!(unique.len() > 1, "expected some variety, got {unique:?}");
    }

    #[test]
    fn empty_list_returns_none() {
        assert!(pick_for_chart(Path::new("/x.memon"), &[], None).is_none());
    }

    #[test]
    fn unknown_extension_returns_none() {
        let list = vec![PathBuf::from("/bg/weird.xyz")];
        assert!(pick_for_chart(Path::new("/x.memon"), &list, None).is_none());
    }

    #[test]
    fn shader_vs_image_classification() {
        assert!(matches!(
            Background::from_path(Path::new("a.wgsl")).unwrap(),
            Background::Shader(_)
        ));
        assert!(matches!(
            Background::from_path(Path::new("a.PNG")).unwrap(),
            Background::Image(_)
        ));
        assert!(Background::from_path(Path::new("a.bin")).is_none());
    }

    #[test]
    fn into_uniforms_encodes_expected_channels() {
        let i = BackgroundInputs {
            music_ms: 1500.0,
            bpm: 140.0,
            beat_phase: 0.25,
            combo: 42,
            score: 12_345,
            life: 0.75,
            held_mask: 0b101,
            last_grade: 1.0,
            last_hit_elapsed_ms: 120.0,
            last_hit_accent: [0.4, 1.0, 0.5, 1.0],
            spectrum: [0.0; 16],
        };
        let u = i.into_uniforms([1920.0, 120.0], 0.0, 0.016);
        assert_eq!(u.resolution, [1920.0, 120.0]);
        assert!((u.cursor[0] - 1.5).abs() < 1e-6);
        assert!((u.cursor[1] - 140.0).abs() < 1e-6);
        assert_eq!(u.kind, 1.0);
        assert_eq!(u.bound, 1.0);
        assert!((u.toggle_on - 0.25).abs() < 1e-6);
        // flash ramp: 1 - (120/600) = 0.8
        assert!((u.flash - 0.8).abs() < 1e-6);
        assert_eq!(u.accent, [0.4, 1.0, 0.5, 1.0]);
        assert_eq!(u.state[0], 0.75);
        assert_eq!(u.state[1], 42.0);
        assert!((u.state[2] - 12345.0 / 10_000.0).abs() < 1e-6);
        assert_eq!(u.state[3], 5.0); // 0b101
    }
}
