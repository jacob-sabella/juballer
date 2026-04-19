use crate::{Error, PresentMode, Result};
use std::sync::Arc;

/// Owns the wgpu device, queue, surface, and offscreen color view. Created once,
/// reconfigured on resize.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub offscreen: OffscreenFb,
    pub composite: super::composite::CompositePass,
    pub fill: super::fill::FillPipeline,
}

pub struct OffscreenFb {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub w: u32,
    pub h: u32,
}

impl Gpu {
    pub async fn new(
        window: Arc<winit::window::Window>,
        present_mode: PresentMode,
        swapchain_buffers: u8,
    ) -> Result<Self> {
        // NVIDIA's libEGL_nvidia.so + Wayland surface init is unstable: the GLES probe path
        // segfaults in libwayland-client during wl_proxy_marshal when a subprocess re-execs
        // and brings up its own window. Vulkan works fine on the same driver, so on Linux
        // we restrict the instance to Vulkan-only and skip the GL probe entirely. Other
        // platforms keep the default (PRIMARY) backend set.
        let backends = if cfg!(target_os = "linux") {
            wgpu::Backends::VULKAN
        } else {
            wgpu::Backends::PRIMARY
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| Error::GpuInit(format!("create_surface: {e}")))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| Error::GpuInit("no compatible adapter".into()))?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("juballer-core device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| Error::GpuInit(format!("request_device: {e}")))?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: match present_mode {
                PresentMode::Fifo => wgpu::PresentMode::Fifo,
                PresentMode::Mailbox => wgpu::PresentMode::Mailbox,
                PresentMode::Immediate => wgpu::PresentMode::Immediate,
            },
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: swapchain_buffers as u32,
        };
        surface.configure(&device, &surface_config);

        let offscreen =
            OffscreenFb::create(&device, format, surface_config.width, surface_config.height);

        let composite = super::composite::CompositePass::new(&device, surface_config.format);
        let fill = super::fill::FillPipeline::new(&device, surface_config.format);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface,
            surface_config,
            offscreen,
            composite,
            fill,
        })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.surface_config.width = w.max(1);
        self.surface_config.height = h.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.offscreen = OffscreenFb::create(
            &self.device,
            self.surface_config.format,
            self.surface_config.width,
            self.surface_config.height,
        );
    }
}

impl OffscreenFb {
    pub fn create(device: &wgpu::Device, format: wgpu::TextureFormat, w: u32, h: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("juballer offscreen FB"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            format,
            w,
            h,
        }
    }
}
