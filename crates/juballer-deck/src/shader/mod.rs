//! Per-tile custom WGSL shader pipeline cache.
//!
//! The cache compiles and memoizes a [`TilePipeline`] per unique WGSL source path.
//! `draw_tile` records a scissored full-quad draw into the caller's encoder, targeting
//! the tile's pixel viewport.

use crate::{Error, Result};
use bytemuck::{Pod, Zeroable};
use juballer_core::TileRawCtx;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub const TILE_UNIFORMS_SIZE: u64 = std::mem::size_of::<TileUniforms>() as u64;

/// Standard uniform block (group 0, binding 0) visible to every tile shader.
///
/// Layout is 16-byte aligned so it drops straight into a wgpu uniform buffer and
/// mirrors a WGSL `struct Uniforms { ... }`. The first 16 bytes (resolution, time,
/// delta_time) match the original v1 layout so existing shaders keep working.
///
/// Semantic fields:
/// - `kind`: 0=Action, 1=Nav, 2=Toggle (matches [`crate::action::ActionKind`]).
/// - `bound`: 1.0 if the cell has a bound action, else 0.0.
/// - `toggle_on`: 1.0 if this is a Toggle in its "on" state, else 0.0.
/// - `flash`: 1.0 at the instant of press, decays to 0.0 over the tile flash window.
/// - `accent`: primary per-kind accent color (rgba 0..1).
/// - `state`: `tile.state_color` if present, else the accent.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug, Default)]
pub struct TileUniforms {
    pub resolution: [f32; 2],
    pub time: f32,
    pub delta_time: f32,

    pub cursor: [f32; 2],
    pub kind: f32,
    pub bound: f32,

    pub toggle_on: f32,
    pub flash: f32,
    pub _pad0: [f32; 2],

    pub accent: [f32; 4],
    pub state: [f32; 4],

    /// Live-audio spectrum, 16 log-spaced bins (~60 Hz → Nyquist),
    /// smoothed with fast-attack / slow-release on the CPU side. Shaders
    /// read via `u.spectrum[i/4][i%4]`. Zero-filled when no audio tap
    /// is active (e.g. picker preview idle).
    pub spectrum: [[f32; 4]; 4],
}

/// A compiled tile pipeline; shared via Arc so many tiles can sample the same shader.
pub struct TilePipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    pub uniforms_buf: wgpu::Buffer,
    pub target_format: wgpu::TextureFormat,
}

/// An error recorded while compiling or loading a shader. Stored per-path so the renderer
/// can surface an overlay without crashing.
#[derive(Debug, Clone)]
pub struct ShaderError {
    pub path: PathBuf,
    pub message: String,
}

struct CacheEntry {
    pipeline: Option<Arc<TilePipeline>>,
    last_error: Option<ShaderError>,
    source_mtime: Option<SystemTime>,
}

pub struct ShaderPipelineCache {
    entries: HashMap<PathBuf, CacheEntry>,
    /// Paths that need a recompile attempt on the next `draw_tile`. Populated by
    /// [`ShaderPipelineCache::invalidate`] (called from the fs watcher).
    dirty: Arc<Mutex<Vec<PathBuf>>>,
}

impl Default for ShaderPipelineCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ShaderPipelineCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            dirty: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Shared handle to the dirty queue. The config watcher pushes paths into this on
    /// file-change notifications; the renderer drains it at the top of each frame.
    pub fn dirty_handle(&self) -> Arc<Mutex<Vec<PathBuf>>> {
        self.dirty.clone()
    }

    /// Mark the entry for `path` dirty; next `draw_tile` attempts a recompile.
    pub fn invalidate(&self, path: &Path) {
        if let Ok(mut q) = self.dirty.lock() {
            q.push(path.to_path_buf());
        }
    }

    fn drain_dirty(&mut self) {
        let mut paths: Vec<PathBuf> = Vec::new();
        if let Ok(mut q) = self.dirty.lock() {
            std::mem::swap(&mut paths, &mut q);
        }
        for p in paths {
            self.entries.remove(&p);
        }
    }

    /// Return the most recent error for `path`, if any.
    pub fn last_error(&self, path: &Path) -> Option<&ShaderError> {
        self.entries.get(path).and_then(|e| e.last_error.as_ref())
    }

    /// Ensure a pipeline exists for `path`; compile if absent, out-of-date, or its
    /// target format no longer matches.
    pub fn get_or_compile(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        path: &Path,
    ) -> Result<Arc<TilePipeline>> {
        self.drain_dirty();
        if let Some(entry) = self.entries.get(path) {
            if let Some(p) = &entry.pipeline {
                if p.target_format == format {
                    let current_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
                    if current_mtime == entry.source_mtime {
                        return Ok(p.clone());
                    }
                    tracing::info!(
                        target: "juballer::shader",
                        "recompiling {} (mtime changed)",
                        path.display()
                    );
                }
            }
        }
        self.compile_into_cache(device, format, path)
    }

    fn compile_into_cache(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        path: &Path,
    ) -> Result<Arc<TilePipeline>> {
        let src = std::fs::read_to_string(path).map_err(|e| {
            let err = ShaderError {
                path: path.to_path_buf(),
                message: format!("read failed: {e}"),
            };
            self.entries.insert(
                path.to_path_buf(),
                CacheEntry {
                    pipeline: None,
                    last_error: Some(err.clone()),
                    source_mtime: None,
                },
            );
            Error::Config(format!("shader {}: {}", path.display(), e))
        })?;

        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let prepared = preprocess_wgsl(&src);
        match compile_pipeline(device, format, path, &prepared) {
            Ok(pipeline) => {
                let arc = Arc::new(pipeline);
                self.entries.insert(
                    path.to_path_buf(),
                    CacheEntry {
                        pipeline: Some(arc.clone()),
                        last_error: None,
                        source_mtime: mtime,
                    },
                );
                Ok(arc)
            }
            Err(e) => {
                let err = ShaderError {
                    path: path.to_path_buf(),
                    message: e.clone(),
                };
                self.entries.insert(
                    path.to_path_buf(),
                    CacheEntry {
                        pipeline: None,
                        last_error: Some(err),
                        source_mtime: mtime,
                    },
                );
                Err(Error::Config(format!("shader {}: {}", path.display(), e)))
            }
        }
    }

    /// Compile (if needed) and record a draw for the tile. Errors are logged + stored
    /// for later retrieval via `last_error`; they never propagate to the render loop.
    pub fn draw_tile(&mut self, ctx: &mut TileRawCtx<'_>, path: &Path, uniforms: &TileUniforms) {
        let pipe = match self.get_or_compile(ctx.device, ctx.surface_format, path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(target: "juballer::shader", "{}", e);
                return;
            }
        };

        ctx.queue
            .write_buffer(&pipe.uniforms_buf, 0, bytemuck::bytes_of(uniforms));

        let (x, y, w, h) = ctx.viewport;
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tile shader pass"),
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
        pass.set_pipeline(&pipe.pipeline);
        pass.set_bind_group(0, &pipe.bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}

/// If the user's WGSL does not declare a `vs_main` entry point, prepend a standard
/// full-quad vertex function + a `Uniforms` struct declaration so fragment-only shaders
/// just work. If the source already has either, we leave it alone.
pub fn preprocess_wgsl(src: &str) -> String {
    let has_vs = src.contains("vs_main");
    let has_uniform_struct = src.contains("struct Uniforms");
    let mut out = String::new();
    if !has_uniform_struct {
        out.push_str(STANDARD_UNIFORMS_WGSL);
        out.push('\n');
    }
    if !has_vs {
        out.push_str(STANDARD_VS_WGSL);
        out.push('\n');
    }
    out.push_str(src);
    out
}

const STANDARD_UNIFORMS_WGSL: &str = r#"struct Uniforms {
    resolution: vec2<f32>,
    time: f32,
    delta_time: f32,

    cursor: vec2<f32>,
    kind: f32,
    bound: f32,

    toggle_on: f32,
    flash: f32,
    _pad0: vec2<f32>,

    accent: vec4<f32>,
    state: vec4<f32>,

    // 16 log-spaced FFT bins (vec4[0].x = lowest, vec4[3].w = highest).
    // Live-audio driven when a song is playing; zero-filled otherwise.
    spectrum: array<vec4<f32>, 4>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

// Sample a [0..1] amplitude for frequency-position `x` (0 = lowest bin,
// 1 = highest). Linear-interps between the nearest two log-spaced bins.
fn game_audio(x: f32) -> f32 {
    let fx = clamp(x, 0.0, 1.0) * 15.0;
    let i = u32(floor(fx));
    let j = min(i + 1u, 15u);
    let t = fract(fx);
    let a = u.spectrum[i >> 2u][i & 3u];
    let b = u.spectrum[j >> 2u][j & 3u];
    return mix(a, b, t);
}
"#;

// Standard full-screen-quad VS. Emits a `@location(0) uv` varying so fragment
// shaders can consume tile-local UV directly — dividing `@builtin(position)`
// by `u.resolution` only produced tile-local UV for a tile at framebuffer
// origin (0,0), and garbage for every other tile (@builtin(position) is in
// absolute framebuffer pixels, not viewport-relative). UV origin is top-left.
const STANDARD_VS_WGSL: &str = r#"struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>( 1.0, -1.0), vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>( 1.0, -1.0), vec2<f32>( 1.0,  1.0),
    );
    let p = pos[vi];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 1.0 - (p.y * 0.5 + 0.5));
    return out;
}
"#;

fn compile_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    path: &Path,
    wgsl: &str,
) -> std::result::Result<TilePipeline, String> {
    let label = format!("tile shader {}", path.display());

    // Intercept validation errors so they don't crash the app. wgpu `push_error_scope`
    // returns a guard whose `.pop()` yields the first error inside the scope.
    let guard = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let err = pollster::block_on(guard.pop());
    if let Some(e) = err {
        return Err(e.to_string());
    }

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("tile shader bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("tile shader pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });

    let guard = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&label),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
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
    let err = pollster::block_on(guard.pop());
    if let Some(e) = err {
        return Err(e.to_string());
    }

    let uniforms_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tile shader uniforms"),
        size: TILE_UNIFORMS_SIZE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tile shader bind"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms_buf.as_entire_binding(),
        }],
    });

    Ok(TilePipeline {
        pipeline,
        bind_group,
        uniforms_buf,
        target_format: format,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocess_injects_missing_vs_main() {
        let src = "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }";
        let out = preprocess_wgsl(src);
        assert!(out.contains("fn vs_main"), "vs_main must be injected");
        assert!(out.contains("struct Uniforms"), "Uniforms struct injected");
        assert!(out.contains("fs_main"), "user source preserved");
    }

    #[test]
    fn preprocess_leaves_existing_vs_main_untouched() {
        let src = r#"struct Uniforms { resolution: vec2<f32>, time: f32, delta_time: f32, cursor: vec2<f32>, kind: f32, bound: f32, toggle_on: f32, flash: f32, _pad0: vec2<f32>, accent: vec4<f32>, state: vec4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
@vertex fn vs_main() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }
@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }"#;
        let out = preprocess_wgsl(src);
        let count = out.matches("fn vs_main").count();
        assert_eq!(count, 1, "no duplicate vs_main: {out}");
    }

    /// Exercise every preset shader in examples/shaders so that a typo in a
    /// WGSL file trips `cargo test` before it ships. We only run the preprocessor
    /// + ensure the result contains both `fs_main` and the new state uniform
    ///   fields — full wgpu compilation is covered by the headless tests.
    #[test]
    fn preset_shaders_preprocess_with_state_uniforms() {
        let presets = [
            "plasma.wgsl",
            "waves.wgsl",
            "matrix_rain.wgsl",
            "solid_time.wgsl",
            "nav_pulse.wgsl",
            "toggle_bar.wgsl",
            "press_ripple.wgsl",
            "ambient_warmth.wgsl",
            "kind_glow.wgsl",
            "empty_dotgrid.wgsl",
        ];
        for name in presets {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples/shaders")
                .join(name);
            let src = std::fs::read_to_string(&path).expect("read preset");
            let out = preprocess_wgsl(&src);
            assert!(out.contains("fn fs_main"), "{}: missing fs_main", name);
            assert!(out.contains("struct Uniforms"), "{}: Uniforms absent", name);
            // Every preset (after preprocess) must reference the new fields in
            // the struct declaration so the WGSL layout matches the Rust side.
            for field in ["kind", "bound", "toggle_on", "flash", "accent", "state"] {
                assert!(
                    out.contains(field),
                    "{}: Uniforms struct missing `{}`",
                    name,
                    field
                );
            }
        }
    }

    #[test]
    fn tile_uniforms_size_is_144_bytes() {
        // 16B (resolution+time+delta_time) + 16B (cursor+kind+bound)
        //   + 16B (toggle_on+flash+_pad0) + 16B (accent) + 16B (state)
        //   + 64B (spectrum: 4×vec4) = 144B.
        assert_eq!(std::mem::size_of::<TileUniforms>(), 144);
        // Uniform structs need 16-byte alignment in wgpu.
        assert_eq!(std::mem::size_of::<TileUniforms>() % 16, 0);
    }
}
