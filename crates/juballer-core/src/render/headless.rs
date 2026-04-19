//! Headless render: build wgpu device with no surface, render one frame into an offscreen
//! framebuffer, copy to a CPU-readable buffer, return RGBA bytes.

use crate::{layout, Color, Rect};
use indexmap::IndexMap;

pub async fn render_to_rgba<F>(
    width: u32,
    height: u32,
    bg: Color,
    cell_rects: &[Rect; 16],
    pane_rects: &IndexMap<layout::PaneId, Rect>,
    rotation_deg: f32,
    mut draw: F,
) -> Vec<u8>
where
    F: FnMut(&mut crate::Frame, &[crate::input::Event]),
{
    // Mirror the windowed-path backend restriction (see render/gpu.rs)
    // so headless tests don't trip the same NVIDIA EGL/Wayland probe.
    let backends = if cfg!(target_os = "linux") {
        wgpu::Backends::VULKAN
    } else {
        wgpu::Backends::PRIMARY
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..Default::default()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .expect("adapter");
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default(), None)
        .await
        .expect("device");

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let offscreen = super::OffscreenFb::create(&device, format, width, height);

    let final_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("headless final"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let final_view = final_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let composite = super::CompositePass::new(&device, format);
    let fill = super::FillPipeline::new(&device, format);

    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

    // Clear offscreen.
    {
        let [r, g, b, a] = bg.as_linear_f32();
        let _ = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &offscreen.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: r as f64,
                        g: g as f64,
                        b: b as f64,
                        a: a as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    // User draw.
    {
        let mut pending_top_layout: Option<Option<crate::layout::Node>> = None;
        let mut frame = crate::Frame {
            device: &device,
            queue: &queue,
            encoder: &mut enc,
            offscreen_view: &offscreen.view,
            cell_rects,
            pane_rects,
            top_region_rect: Rect::new(0, 0, width, 0),
            viewport_w: width,
            viewport_h: height,
            fill_pipeline: &fill,
            format,
            pending_top_layout: &mut pending_top_layout,
        };
        draw(&mut frame, &[]);
    }

    // Composite offscreen → final.
    composite.record(
        &device,
        &queue,
        &mut enc,
        &offscreen.view,
        &final_view,
        width,
        height,
        rotation_deg,
    );

    // Copy final_tex → buffer.
    let bytes_per_row = (width * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &final_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buf,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(enc.finish()));

    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();

    let mapped = slice.get_mapped_range();
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let src_off = (y * bytes_per_row) as usize;
        let dst_off = (y * width * 4) as usize;
        out[dst_off..dst_off + (width * 4) as usize]
            .copy_from_slice(&mapped[src_off..src_off + (width * 4) as usize]);
    }
    drop(mapped);
    buf.unmap();
    out
}
