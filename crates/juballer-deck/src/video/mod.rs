//! Video source → per-tile texture sampler.
//!
//! A [`VideoSource`] owns a capture backend (v4l2 on Linux) producing RGBA frames,
//! maintains a wgpu texture that's updated on new frames, and draws a textured full-quad
//! into the tile's pixel viewport via a private pipeline.

use crate::{Error, Result};
use juballer_core::TileRawCtx;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
pub mod v4l2;

/// A single decoded frame ready for upload to a wgpu texture.
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly-packed RGBA8 (width*4 bytes per row, no padding).
    pub data: Vec<u8>,
    pub captured_at: std::time::Instant,
}

pub trait VideoBackend: Send {
    /// Pull the newest pending frame, if any. Returns `None` when no new frame has
    /// arrived since the previous call.
    fn try_recv_frame(&mut self) -> Option<VideoFrame>;
}

/// Shared blit pipeline: one per [`VideoRegistry`], reused across all video sources.
struct BlitPipeline {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bgl: wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
}

impl BlitPipeline {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video blit shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video blit bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video blit pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video blit pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video blit sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            sampler,
            bgl,
            format,
        }
    }
}

pub struct VideoSource {
    uri: String,
    backend: Box<dyn VideoBackend>,
    texture: Option<wgpu::Texture>,
    view: Option<wgpu::TextureView>,
    bind_group: Option<wgpu::BindGroup>,
    last_size: Option<(u32, u32)>,
    last_frame_at: Option<std::time::Instant>,
}

impl VideoSource {
    /// Parse a URI like `v4l2:///dev/video0` and construct the matching backend.
    pub fn from_uri(uri: &str) -> Result<Self> {
        if let Some(path) = uri.strip_prefix("v4l2://") {
            #[cfg(target_os = "linux")]
            {
                let backend = v4l2::V4l2Backend::open(path)
                    .map_err(|e| Error::Config(format!("v4l2 open {path}: {e}")))?;
                return Ok(Self {
                    uri: uri.to_string(),
                    backend: Box::new(backend),
                    texture: None,
                    view: None,
                    bind_group: None,
                    last_size: None,
                    last_frame_at: None,
                });
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = path;
                return Err(Error::Config(
                    "v4l2 video sources require Linux".to_string(),
                ));
            }
        }
        Err(Error::Config(format!(
            "unsupported video uri scheme: {uri}"
        )))
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    fn ensure_texture(&mut self, device: &wgpu::Device, blit: &BlitPipeline, w: u32, h: u32) {
        if self.last_size == Some((w, h)) && self.texture.is_some() {
            return;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("video texture"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video bind group"),
            layout: &blit.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&blit.sampler),
                },
            ],
        });
        self.texture = Some(tex);
        self.view = Some(view);
        self.bind_group = Some(bind_group);
        self.last_size = Some((w, h));
    }

    fn upload_frame(&mut self, queue: &wgpu::Queue, frame: &VideoFrame) {
        if let Some(tex) = self.texture.as_ref() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width * 4),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
            self.last_frame_at = Some(frame.captured_at);
        }
    }
}

/// Owns the blit pipeline + map of URI → VideoSource. `draw_tile` pulls new frames
/// and records the blit into the tile's viewport.
pub struct VideoRegistry {
    sources: HashMap<String, VideoSource>,
    blit: Option<BlitPipeline>,
    /// Sources that failed to open; we cache the error so we don't re-attempt every frame.
    failed: Arc<Mutex<HashMap<String, String>>>,
}

impl Default for VideoRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoRegistry {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            blit: None,
            failed: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Remove the cached source (if any) for `uri` so the next `draw_tile` reopens it.
    pub fn invalidate(&mut self, uri: &str) {
        self.sources.remove(uri);
        if let Ok(mut m) = self.failed.lock() {
            m.remove(uri);
        }
    }

    /// Check whether a source has an open-time error.
    pub fn last_error(&self, uri: &str) -> Option<String> {
        self.failed.lock().ok().and_then(|m| m.get(uri).cloned())
    }

    fn ensure_blit(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        let recreate = match &self.blit {
            Some(b) => b.format != format,
            None => true,
        };
        if recreate {
            self.blit = Some(BlitPipeline::new(device, format));
        }
    }

    fn ensure_source(&mut self, uri: &str) -> Option<&mut VideoSource> {
        if self.sources.contains_key(uri) {
            return self.sources.get_mut(uri);
        }
        if let Ok(m) = self.failed.lock() {
            if m.contains_key(uri) {
                return None;
            }
        }
        match VideoSource::from_uri(uri) {
            Ok(src) => {
                self.sources.insert(uri.to_string(), src);
                self.sources.get_mut(uri)
            }
            Err(e) => {
                tracing::warn!(target: "juballer::video", "open {uri}: {e}");
                if let Ok(mut m) = self.failed.lock() {
                    m.insert(uri.to_string(), e.to_string());
                }
                None
            }
        }
    }

    /// Pull the newest frame, upload if present, and record a blit into the tile.
    pub fn draw_tile(&mut self, ctx: &mut TileRawCtx<'_>, uri: &str) {
        self.ensure_blit(ctx.device, ctx.surface_format);
        let blit_fmt = self.blit.as_ref().map(|b| b.format);
        if blit_fmt != Some(ctx.surface_format) {
            return;
        }
        if self.ensure_source(uri).is_none() {
            return;
        }

        // Drain pending frames; keep the newest (avoid backlog).
        let latest = {
            let source = self.sources.get_mut(uri).expect("ensured above");
            let mut latest: Option<VideoFrame> = None;
            while let Some(f) = source.backend.try_recv_frame() {
                latest = Some(f);
            }
            latest
        };
        if let Some(frame) = latest {
            let blit = self.blit.as_ref().expect("blit ensured");
            let source = self.sources.get_mut(uri).expect("ensured above");
            source.ensure_texture(ctx.device, blit, frame.width, frame.height);
            source.upload_frame(ctx.queue, &frame);
        }

        let blit = self.blit.as_ref().expect("blit ensured");
        let source = self.sources.get(uri).expect("ensured above");
        let Some(bg) = source.bind_group.as_ref() else {
            return;
        };

        let (x, y, w, h) = ctx.viewport;
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("video blit pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target_view,
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
        pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        pass.set_pipeline(&blit.pipeline);
        pass.set_bind_group(0, bg, &[]);
        pass.draw(0..6, 0..1);
    }
}

/// YUYV (YUY2) → RGBA8 conversion. `data` is width*height*2 bytes.
/// Each 4-byte group is Y0 Cb Y1 Cr, producing two adjacent RGBA pixels.
#[cfg(target_os = "linux")]
pub fn yuyv_to_rgba(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let mut out = vec![0u8; w * h * 4];
    let stride = w * 2;
    for y in 0..h {
        let row_src = &data[y * stride..(y + 1) * stride];
        let row_dst = &mut out[y * w * 4..(y + 1) * w * 4];
        let mut sx = 0usize;
        let mut dx = 0usize;
        while sx + 3 < row_src.len() {
            let y0 = row_src[sx] as i32;
            let cb = row_src[sx + 1] as i32 - 128;
            let y1 = row_src[sx + 2] as i32;
            let cr = row_src[sx + 3] as i32 - 128;
            let (r0, g0, b0) = yuv_to_rgb(y0, cb, cr);
            let (r1, g1, b1) = yuv_to_rgb(y1, cb, cr);
            row_dst[dx] = r0;
            row_dst[dx + 1] = g0;
            row_dst[dx + 2] = b0;
            row_dst[dx + 3] = 0xff;
            row_dst[dx + 4] = r1;
            row_dst[dx + 5] = g1;
            row_dst[dx + 6] = b1;
            row_dst[dx + 7] = 0xff;
            sx += 4;
            dx += 8;
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn yuv_to_rgb(y: i32, cb: i32, cr: i32) -> (u8, u8, u8) {
    // ITU-R BT.601 full-range approximation. Sufficient for webcam preview.
    let c = y - 16;
    let d = cb;
    let e = cr;
    let r = (298 * c + 409 * e + 128) >> 8;
    let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
    let b = (298 * c + 516 * d + 128) >> 8;
    (
        r.clamp(0, 255) as u8,
        g.clamp(0, 255) as u8,
        b.clamp(0, 255) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn yuyv_converts_white() {
        let pixels = 4u32;
        let w = pixels;
        let h = 1u32;
        let mut buf = vec![0u8; (w * h * 2) as usize];
        for chunk in buf.chunks_mut(4) {
            chunk[0] = 235; // Y0 full-range white-ish
            chunk[1] = 128; // Cb neutral
            chunk[2] = 235;
            chunk[3] = 128;
        }
        let rgba = yuyv_to_rgba(&buf, w, h);
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        for px in rgba.chunks(4) {
            assert!(px[0] > 220, "r {}", px[0]);
            assert!(px[1] > 220);
            assert!(px[2] > 220);
            assert_eq!(px[3], 0xff);
        }
    }

    #[test]
    fn unsupported_scheme_errors() {
        let err = match VideoSource::from_uri("rtsp://example/stream") {
            Ok(_) => panic!("rtsp should not be supported"),
            Err(e) => e,
        };
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
