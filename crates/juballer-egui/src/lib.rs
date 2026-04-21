//! egui overlay scoped to juballer-core regions.

pub mod textfx;

use egui::Context;
use egui_wgpu::Renderer;
use indexmap::IndexMap;
use juballer_core::layout::PaneId;
use juballer_core::{Frame, Rect};

pub struct EguiOverlay {
    ctx: Context,
    renderer: Option<Renderer>,
    pixels_per_point: f32,
}

impl Default for EguiOverlay {
    fn default() -> Self {
        let ctx = Context::default();
        install_emoji_font(&ctx);
        Self {
            ctx,
            renderer: None,
            pixels_per_point: 1.0,
        }
    }
}

/// Bundled outline fonts registered as proportional-family fallbacks.
///
/// egui's text rasterizer (via `ab_glyph`) can only render outline glyphs; it does NOT
/// understand color-bitmap (`CBDT`) or `COLR` glyph tables, so a font like
/// `NotoColorEmoji.ttf` renders as blank tofu regardless of how we register it. The
/// fonts bundled here are all outline-only variants:
///
/// - `NotoSansSymbols2-Regular.ttf` — symbols, arrows, dingbats (≈656 KB)
/// - `NotoEmoji-Variable.ttf` — monochrome outline emoji (≈1.9 MB)
///
/// Bundling them in-crate (via `include_bytes!`) makes rendering deterministic — we no
/// longer depend on what the user happens to have installed under `/usr/share/fonts`.
///
/// Registration order matters: NotoSansSymbols2 is pushed before NotoEmoji so its
/// cleaner, more uniform monochrome glyphs win for the Unicode ranges both fonts
/// cover (arrows, geometric shapes, dingbats).
const BUNDLED_FONTS: &[(&str, &[u8])] = &[
    (
        "noto_symbols2",
        include_bytes!("../fonts/NotoSansSymbols2-Regular.ttf"),
    ),
    (
        "noto_emoji",
        include_bytes!("../fonts/NotoEmoji-Variable.ttf"),
    ),
];

/// System-font fallback paths. Used as a LAST resort after bundled fonts. Keeps
/// coverage for codepoints the bundled fonts miss (CJK, etc.) when the host happens
/// to have broader fonts installed.
const SYSTEM_FONT_FALLBACKS: &[&str] = &[
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/Symbola.ttf",
    "/usr/share/fonts/symbola/Symbola.ttf",
    "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
    "/usr/share/fonts/TTF/NotoSansSymbols-Regular.ttf",
    // CJK coverage — required for Japanese chart titles / artist names
    // that ship with memon packs. TTC face 0 is the JP regular sub-font
    // in Noto CJK, which is what we want.
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/TTF/NotoSansCJK-Regular.ttc",
];

/// Install bundled outline emoji + symbol fonts as Proportional family fallbacks,
/// then append any available system fonts as a last-resort tier.
fn install_emoji_font(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Tier 1 — bundled outline fonts (always present, deterministic).
    for (name, bytes) in BUNDLED_FONTS {
        fonts.font_data.insert(
            (*name).to_string(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
        if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            prop.push((*name).to_string());
        }
        if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            mono.push((*name).to_string());
        }
    }

    // Tier 2 — opportunistic system fonts for codepoints the bundled set misses.
    for path in SYSTEM_FONT_FALLBACKS {
        if let Ok(bytes) = std::fs::read(path) {
            let name = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("system_fallback")
                .to_string();
            if fonts.font_data.contains_key(&name) {
                continue;
            }
            fonts.font_data.insert(
                name.clone(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                prop.push(name.clone());
            }
            if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                mono.push(name);
            }
        }
    }

    ctx.set_fonts(fonts);
}

impl EguiOverlay {
    /// Build an overlay with no renderer yet.
    ///
    /// The `egui_wgpu::Renderer` is created lazily on the first `draw` call, since that
    /// is the first point at which a `Frame` exposes the device and format.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_pixels_per_point(&mut self, ppp: f32) {
        self.pixels_per_point = ppp;
    }

    fn ensure_renderer(&mut self, frame: &Frame<'_>) {
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new(
                frame.device(),
                frame.format(),
                egui_wgpu::RendererOptions {
                    msaa_samples: 1,
                    depth_stencil_format: None,
                    dithering: false,
                    predictable_texture_filtering: false,
                },
            ));
        }
    }

    /// Run egui for this frame. The `builder` closure receives a `RegionCtx` that provides
    /// `in_top_pane` / `in_grid_cell` helpers for scoping UI to juballer regions, backed by
    /// `egui::Area` instances placed at the pixel positions computed by juballer-core's layout.
    pub fn draw<F: FnOnce(&mut RegionCtx<'_>)>(&mut self, frame: &mut Frame<'_>, builder: F) {
        self.ensure_renderer(frame);
        let renderer = self.renderer.as_mut().expect("renderer ensured");

        // Snapshot rect data (cheap copies) before taking the mutable GPU borrow.
        // cell_rects is [Rect; 16] = 256 bytes; pane_rects clone is O(number of panes).
        let cell_rects: [Rect; 16] = *frame.cell_rects();
        let pane_rects: IndexMap<PaneId, Rect> = frame.pane_rects().clone();
        let viewport_w = frame.viewport_w();
        let viewport_h = frame.viewport_h();

        // Build the egui frame (pure — no GPU access needed).
        let raw_input = egui::RawInput::default();
        // `ctx.run` takes `FnMut`; wrap the `FnOnce` builder in an Option and `take()` it
        // so it runs exactly once while still satisfying the `FnMut` bound.
        let mut builder_opt = Some(builder);
        let full_output = self.ctx.run(raw_input, |ctx| {
            if let Some(b) = builder_opt.take() {
                let mut rc = RegionCtx {
                    ctx,
                    cell_rects: &cell_rects,
                    pane_rects: &pane_rects,
                };
                b(&mut rc);
            }
        });
        let paint_jobs = self
            .ctx
            .tessellate(full_output.shapes, self.pixels_per_point);

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [viewport_w, viewport_h],
            pixels_per_point: self.pixels_per_point,
        };

        // Borrow device, queue, and encoder together via a single mutable borrow of
        // `frame` so the borrow checker accepts simultaneous access to all three.
        let (device, queue, encoder) = frame.gpu_resources();

        // Update textures.
        for (id, image_delta) in &full_output.textures_delta.set {
            renderer.update_texture(device, queue, *id, image_delta);
        }

        // Update buffers using the frame's encoder.
        renderer.update_buffers(device, queue, encoder, &paint_jobs, &screen_descriptor);

        // Record the egui render pass into the offscreen FB.
        {
            let pass = frame.begin_overlay_pass();
            renderer.render(&mut pass.forget_lifetime(), &paint_jobs, &screen_descriptor);
        }

        // Free textures.
        for id in &full_output.textures_delta.free {
            renderer.free_texture(id);
        }
    }
}

pub struct RegionCtx<'a> {
    ctx: &'a Context,
    cell_rects: &'a [Rect; 16],
    pane_rects: &'a IndexMap<PaneId, Rect>,
}

impl<'a> RegionCtx<'a> {
    /// Direct access to the underlying egui `Context`.
    ///
    /// Useful for callers that need to place their own `egui::Area` outside the
    /// regions covered by `in_top_pane` / `in_grid_cell` (e.g. a full-width HUD
    /// banner in rhythm mode).
    pub fn ctx(&self) -> &egui::Context {
        self.ctx
    }

    /// Scope egui UI to a top-region pane identified by `id`.
    ///
    /// An `egui::Area` is placed at the pane's pixel-exact position as computed by the
    /// juballer-core layout engine. If `id` is not present in the layout, a zero-sized area
    /// at the origin is used.
    pub fn in_top_pane<R>(&mut self, id: PaneId, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let rect = self.pane_rects.get(id).copied().unwrap_or(Rect::ZERO);
        self.in_rect(egui::Id::new(("juballer_top_pane", id)), rect, add)
    }

    /// Scope egui UI to grid cell `(row, col)` (both 0-indexed, range 0–3).
    ///
    /// An `egui::Area` is placed at the cell's pixel-exact position. Panics (debug) if
    /// `row` or `col` is ≥ 4.
    pub fn in_grid_cell<R>(&mut self, row: u8, col: u8, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        debug_assert!(row < 4 && col < 4, "grid_cell out of range: ({row},{col})");
        let rect = self.cell_rects[(row as usize) * 4 + col as usize];
        self.in_rect(egui::Id::new(("juballer_cell", row, col)), rect, add)
    }

    /// Open an `egui::Area` fixed at `rect`'s pixel origin and sized to `rect`'s
    /// dimensions, then run `add` inside it.
    fn in_rect<R>(&mut self, id: egui::Id, rect: Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        egui::Area::new(id)
            .fixed_pos(egui::pos2(rect.x as f32, rect.y as f32))
            .order(egui::Order::Foreground)
            .show(self.ctx, |ui| {
                ui.set_width(rect.w as f32);
                ui.set_height(rect.h as f32);
                add(ui)
            })
            .inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct an `EguiOverlay` and verify the Proportional family covers every
    /// codepoint we care about after bundled fonts are installed.
    ///
    /// Guards against false positives by also asserting that a deliberately unassigned
    /// Private Use Area codepoint (U+E000) is NOT claimed to be covered — if `has_glyph`
    /// returned `true` for everything unconditionally, this would fail.
    #[test]
    fn bundled_fonts_cover_key_codepoints() {
        let overlay = EguiOverlay::new();
        // Drive the context once so fonts get loaded (set_fonts is lazy).
        let _ = overlay.ctx.run(egui::RawInput::default(), |_| {});

        let font_id = egui::FontId::proportional(16.0);
        let expected_covered: &[(char, &str)] = &[
            ('\u{1F9EA}', "test tube"),
            ('\u{1F916}', "robot"),
            ('\u{1F4AC}', "speech balloon"),
            ('\u{1F6D1}', "stop sign"),
            ('\u{2B06}', "upwards arrow"),
            ('\u{2B07}', "downwards arrow"),
            ('\u{2B05}', "leftwards arrow"),
            ('\u{2794}', "heavy arrow"),
            ('\u{2191}', "up arrow"),
            ('\u{2193}', "down arrow"),
            ('\u{1F4BB}', "laptop"),
            ('\u{1F9D1}', "person"),
            ('\u{1F49C}', "purple heart"),
        ];

        let mut missing: Vec<(char, &str)> = Vec::new();
        overlay.ctx.fonts_mut(|fonts| {
            for &(c, name) in expected_covered {
                if !fonts.has_glyph(&font_id, c) {
                    missing.push((c, name));
                }
            }
        });
        assert!(
            missing.is_empty(),
            "bundled fonts missing coverage for: {:?}",
            missing
                .iter()
                .map(|(c, n)| format!("U+{:04X} ({})", *c as u32, n))
                .collect::<Vec<_>>()
        );

        // False-positive guard: U+E000 is Private Use Area start, none of the bundled
        // fonts should advertise coverage for it. If this ever starts returning true we
        // want to know — likely means `has_glyph` semantics changed.
        let pua_covered = overlay
            .ctx
            .fonts_mut(|fonts| fonts.has_glyph(&font_id, '\u{E000}'));
        assert!(
            !pua_covered,
            "Private Use Area U+E000 was reported as covered — test logic is unsound (false positives possible)"
        );
    }
}
