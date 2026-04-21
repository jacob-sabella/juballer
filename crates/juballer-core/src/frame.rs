use crate::layout::PaneId;
use crate::{Color, Rect};
use indexmap::IndexMap;

/// Per-frame context handed to the user draw callback. Provides scoped GPU access
/// to each grid cell and each top-region pane.
pub struct Frame<'a> {
    pub(crate) device: &'a wgpu::Device,
    pub(crate) queue: &'a wgpu::Queue,
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
    pub(crate) offscreen_view: &'a wgpu::TextureView,
    pub(crate) cell_rects: &'a [Rect; 16],
    pub(crate) pane_rects: &'a IndexMap<PaneId, Rect>,
    pub(crate) top_region_rect: Rect,
    pub(crate) viewport_w: u32,
    pub(crate) viewport_h: u32,
    pub(crate) fill_pipeline: &'a crate::render::FillPipeline,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) pending_top_layout: &'a mut Option<Option<crate::layout::Node>>,
}

impl<'a> Frame<'a> {
    // ── Accessors for out-of-crate users (e.g. juballer-egui) ──────────────

    pub fn device(&self) -> &wgpu::Device {
        self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        self.queue
    }

    pub fn offscreen_view(&self) -> &wgpu::TextureView {
        self.offscreen_view
    }

    pub fn viewport_w(&self) -> u32 {
        self.viewport_w
    }

    pub fn viewport_h(&self) -> u32 {
        self.viewport_h
    }

    pub fn cell_rects(&self) -> &[crate::Rect; 16] {
        self.cell_rects
    }

    pub fn pane_rects(&self) -> &IndexMap<PaneId, crate::Rect> {
        self.pane_rects
    }

    /// The clipped rect that bounds the top region (egui overlay area). Respects
    /// `edge_padding_top`, `edge_padding_x`, and `cutoff_bottom` from the active
    /// profile's `[top_region]` section. Zero-height when the profile's cutoff
    /// would push the bottom above the top padding.
    pub fn top_region_rect(&self) -> crate::Rect {
        self.top_region_rect
    }

    /// The color format of the offscreen framebuffer (matches the swapchain format).
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Replace the top-region layout. The new layout is solved against the current viewport
    /// after the draw callback returns; pane rects update before the next frame.
    /// Pass `None` to clear (no top region drawn).
    pub fn set_top_layout(&mut self, root: Option<crate::layout::Node>) {
        *self.pending_top_layout = Some(root);
    }

    /// Mutable access to the per-frame command encoder. Callers may record their own
    /// render passes. Use with care — do not hold the returned borrow across other Frame
    /// method calls.
    pub fn encoder_mut(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder
    }

    /// Borrow device, queue, and encoder together in a single call, avoiding the
    /// overlapping-borrow problem that arises when callers hold `device()`/`queue()`
    /// refs while also calling `encoder_mut()`.
    pub fn gpu_resources(&mut self) -> (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder) {
        (self.device, self.queue, self.encoder)
    }

    /// Open a render pass writing into the offscreen FB with `LoadOp::Load`.
    /// Caller may record draw calls and then drop the pass. This helper avoids
    /// an overlapping-borrow problem when the caller needs both the encoder and
    /// the offscreen view at the same time.
    pub fn begin_overlay_pass(&mut self) -> wgpu::RenderPass<'_> {
        self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frame overlay pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.offscreen_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    // ── Per-region draw helpers ─────────────────────────────────────────────

    pub fn grid_cell(&mut self, row: u8, col: u8) -> RegionDraw<'_> {
        debug_assert!(row < 4 && col < 4, "grid_cell out of range: ({row},{col})");
        let viewport = self.cell_rects[(row as usize) * 4 + col as usize];
        self.region(viewport)
    }

    /// Run a closure with raw wgpu access scoped to a specific grid cell. The ctx exposes
    /// device/queue/encoder/target view plus the pixel viewport of the tile. The caller is
    /// responsible for opening a render pass with appropriate scissor + viewport to avoid
    /// bleeding outside the tile. Intended for per-tile custom shaders / video textures.
    pub fn with_tile_raw<F>(&mut self, row: u8, col: u8, f: F)
    where
        F: FnOnce(TileRawCtx<'_>),
    {
        debug_assert!(
            row < 4 && col < 4,
            "with_tile_raw out of range: ({row},{col})"
        );
        let viewport = self.cell_rects[(row as usize) * 4 + col as usize];
        let r_x = viewport.x.max(0) as u32;
        let r_y = viewport.y.max(0) as u32;
        let r_w = viewport.w.min(self.viewport_w.saturating_sub(r_x));
        let r_h = viewport.h.min(self.viewport_h.saturating_sub(r_y));
        if r_w == 0 || r_h == 0 {
            return;
        }
        let ctx = TileRawCtx {
            device: self.device,
            queue: self.queue,
            encoder: self.encoder,
            target_view: self.offscreen_view,
            viewport: (r_x as f32, r_y as f32, r_w as f32, r_h as f32),
            surface_format: self.format,
        };
        f(ctx);
    }

    pub fn top_pane(&mut self, id: PaneId) -> RegionDraw<'_> {
        let rect = self.pane_rects.get(&id).copied().unwrap_or(Rect::ZERO);
        self.region(rect)
    }

    /// Like [`Frame::with_tile_raw`] but targets an arbitrary rect (e.g. the top region).
    /// Useful for driving a single shader across a whole region without carving it into tiles.
    pub fn with_region_raw<F>(&mut self, rect: Rect, f: F)
    where
        F: FnOnce(TileRawCtx<'_>),
    {
        let r_x = rect.x.max(0) as u32;
        let r_y = rect.y.max(0) as u32;
        let r_w = rect.w.min(self.viewport_w.saturating_sub(r_x));
        let r_h = rect.h.min(self.viewport_h.saturating_sub(r_y));
        if r_w == 0 || r_h == 0 {
            return;
        }
        let ctx = TileRawCtx {
            device: self.device,
            queue: self.queue,
            encoder: self.encoder,
            target_view: self.offscreen_view,
            viewport: (r_x as f32, r_y as f32, r_w as f32, r_h as f32),
            surface_format: self.format,
        };
        f(ctx);
    }

    fn region(&mut self, viewport: Rect) -> RegionDraw<'_> {
        RegionDraw {
            viewport,
            encoder: self.encoder,
            gpu: GpuCtx {
                device: self.device,
                queue: self.queue,
                view: self.offscreen_view,
            },
            viewport_w: self.viewport_w,
            viewport_h: self.viewport_h,
            fill_pipeline: self.fill_pipeline,
        }
    }
}

/// Raw wgpu context for per-tile custom rendering passes. Produced by
/// [`Frame::with_tile_raw`]. The caller records into `encoder`, targets the
/// `target_view` (the offscreen framebuffer the deck's egui overlay later draws onto),
/// and MUST apply scissor + viewport matching `viewport` to avoid bleeding past the tile.
pub struct TileRawCtx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub target_view: &'a wgpu::TextureView,
    /// (x, y, w, h) in framebuffer pixels, pre-clipped to the surface.
    pub viewport: (f32, f32, f32, f32),
    pub surface_format: wgpu::TextureFormat,
}

pub struct GpuCtx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub view: &'a wgpu::TextureView,
}

pub struct RegionDraw<'a> {
    pub viewport: Rect,
    encoder: &'a mut wgpu::CommandEncoder,
    pub gpu: GpuCtx<'a>,
    viewport_w: u32,
    viewport_h: u32,
    pub(crate) fill_pipeline: &'a crate::render::FillPipeline,
}

impl<'a> RegionDraw<'a> {
    /// Solid-fill the region with `color`.
    pub fn fill(&mut self, color: Color) {
        if self.viewport.is_empty() {
            return;
        }
        let r_x = self.viewport.x.max(0) as u32;
        let r_y = self.viewport.y.max(0) as u32;
        let r_w = self.viewport.w.min(self.viewport_w.saturating_sub(r_x));
        let r_h = self.viewport.h.min(self.viewport_h.saturating_sub(r_y));
        if r_w == 0 || r_h == 0 {
            return;
        }
        self.fill_pipeline
            .write_color(self.gpu.queue, color.as_linear_f32());
        let mut pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("region fill"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.gpu.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_scissor_rect(r_x, r_y, r_w, r_h);
        pass.set_viewport(r_x as f32, r_y as f32, r_w as f32, r_h as f32, 0.0, 1.0);
        pass.set_pipeline(self.fill_pipeline.pipeline());
        pass.set_bind_group(0, self.fill_pipeline.bind(), &[]);
        pass.draw(0..3, 0..1);
    }

    /// Begin a render pass scoped (scissor + viewport) to this region. Caller may set its own
    /// pipeline + draw calls. The returned pass writes into the offscreen FB with `LoadOp::Load`.
    pub fn render_pass(&mut self) -> wgpu::RenderPass<'_> {
        let r_x = self.viewport.x.max(0) as u32;
        let r_y = self.viewport.y.max(0) as u32;
        let r_w = self.viewport.w.min(self.viewport_w.saturating_sub(r_x));
        let r_h = self.viewport.h.min(self.viewport_h.saturating_sub(r_y));
        let mut pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("region render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.gpu.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_scissor_rect(r_x, r_y, r_w, r_h);
        pass.set_viewport(r_x as f32, r_y as f32, r_w as f32, r_h as f32, 0.0, 1.0);
        pass
    }
}
