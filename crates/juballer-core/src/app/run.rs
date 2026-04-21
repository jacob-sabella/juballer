use crate::app::mode::{ClosureMode, Mode, ModeOutcome};
use crate::render::{gpu::Gpu, window::open_fullscreen};
use crate::{App, Color, Rect, Result};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

struct Runtime {
    cfg: crate::AppBuilder,
    cfg_top_layout: Option<crate::layout::Node>,
    profile: Option<crate::Profile>,
    window: Option<Arc<winit::window::Window>>,
    gpu: Option<Gpu>,
    /// Active mode. The driver hands every frame to `mode.frame(...)`
    /// and acts on the returned outcome — `Continue` keeps rendering,
    /// `SwitchTo` swaps the box in place, `Exit` breaks the event loop.
    mode: Box<dyn Mode>,
    cell_rects: [Rect; 16],
    pane_rects: indexmap::IndexMap<crate::layout::PaneId, Rect>,
    pending_events: Vec<crate::input::Event>,
    debug: bool,
    winit_input: crate::input::WinitInput,
    keymap: crate::input::Keymap,
    force_calibration: bool,
    force_keymap: bool,
    cal_state: Option<crate::calibration::CalibrationState>,
    shift_down: bool,
    #[cfg(feature = "raw-input")]
    raw_ring: Option<std::sync::Arc<crate::input::EventRing>>,
}

impl ApplicationHandler for Runtime {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = open_fullscreen(
            event_loop,
            &self.cfg.title,
            self.cfg.monitor_desc.as_deref(),
        )
        .expect("open_fullscreen");
        let gpu = pollster::block_on(Gpu::new(
            window.clone(),
            self.cfg.present_mode,
            self.cfg.swapchain_buffers,
        ))
        .expect("Gpu::new");

        // Resolve ids + load/create profile.
        let size = window.inner_size();
        let ctrl =
            super::profile_loader::controller_id(self.cfg.controller_vid, self.cfg.controller_pid);
        let mon = super::profile_loader::monitor_id(&window);
        let profile =
            super::profile_loader::load_or_create_profile(&ctrl, &mon, size.width, size.height);

        // Compute cell + pane rects from profile + layout.
        self.cell_rects = crate::geometry::cell_rects(&profile.grid);
        let top_outer =
            crate::geometry::top_region_rect(&profile.grid, &profile.top, size.width, size.height);
        self.pane_rects = match &self.cfg_top_layout {
            Some(root) => crate::layout::solve(root, top_outer),
            None => indexmap::IndexMap::new(),
        };

        // Build keymap from profile so keyboard events can be translated.
        self.keymap = crate::input::Keymap::from_profile(&profile);

        // Spawn platform-specific raw-input backend when feature is enabled and VID:PID is set.
        #[cfg(feature = "raw-input")]
        {
            let vid = self.cfg.controller_vid;
            let pid = self.cfg.controller_pid;
            if vid != 0 || pid != 0 {
                let ring = std::sync::Arc::new(crate::input::EventRing::new(256));
                let keymap = self.keymap.clone();

                #[cfg(target_os = "linux")]
                {
                    match crate::input::raw_linux::RawInputLinux::spawn(
                        vid,
                        pid,
                        keymap,
                        ring.clone(),
                    ) {
                        Ok(_handle) => {
                            log::info!(
                                "raw-input: Linux evdev thread started for {:04x}:{:04x}",
                                vid,
                                pid
                            );
                            self.raw_ring = Some(ring);
                        }
                        Err(e) => {
                            log::warn!(
                                "raw-input: Linux spawn failed: {}; falling back to winit path",
                                e
                            );
                        }
                    }
                }

                #[cfg(target_os = "windows")]
                {
                    match crate::input::raw_windows::RawInputWindows::spawn(
                        vid,
                        pid,
                        keymap,
                        ring.clone(),
                    ) {
                        Ok(_handle) => {
                            log::info!("raw-input: Windows RawInput thread started");
                            self.raw_ring = Some(ring);
                        }
                        Err(e) => {
                            log::warn!(
                                "raw-input: Windows spawn failed: {}; falling back to winit path",
                                e
                            );
                        }
                    }
                }
            } else {
                log::info!("raw-input: no controller VID:PID configured; using winit backend");
            }
        }

        // Enter calibration mode if: user forced it OR profile keymap is incomplete.
        let need_cal = self.force_calibration || self.force_keymap || !profile.keymap_complete();
        if need_cal {
            use crate::calibration::CalibrationState;
            let mut state = CalibrationState::new(profile.clone());
            if self.force_keymap && !self.force_calibration {
                // Skip Geometry; go straight to keymap.
                state.confirm_geometry();
            }
            self.cal_state = Some(state);
        }

        self.profile = Some(profile);
        self.window = Some(window);
        self.gpu = Some(gpu);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.shift_down = mods.state().shift_key();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(state) = self.cal_state.as_mut() {
                    use crate::calibration::Phase;
                    use winit::event::ElementState;
                    use winit::keyboard::{Key, NamedKey};

                    // Only react to key press events in calibration, not release.
                    if event.state != ElementState::Pressed {
                        return;
                    }

                    // Geometry-phase nudge keys. New rects are computed into locals so the
                    // `state` borrow ends before writing back to `self`.
                    //
                    // Arrow keys move the TL cell origin (1 px; 10 px when Shift is held).
                    // `[` / `]` shrink/grow cell width; `-` / `=` shrink/grow cell height.
                    // `h` / `j` ± `gap_x`; `k` / `l` ± `gap_y` (single-axis gap control).
                    if matches!(state.phase, Phase::Geometry) {
                        let step = if self.shift_down { 10 } else { 1 };
                        let g = &mut state.draft.grid;
                        let mut mutated = false;
                        match &event.logical_key {
                            Key::Named(NamedKey::ArrowLeft) => {
                                g.origin_px.x -= step;
                                mutated = true;
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                g.origin_px.x += step;
                                mutated = true;
                            }
                            Key::Named(NamedKey::ArrowUp) => {
                                g.origin_px.y -= step;
                                mutated = true;
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                g.origin_px.y += step;
                                mutated = true;
                            }
                            Key::Character(s) => match s.as_str() {
                                "[" => {
                                    g.cell_size_px.w = g.cell_size_px.w.saturating_sub(4);
                                    g.cell_size_px.h = g.cell_size_px.h.saturating_sub(4);
                                    mutated = true;
                                }
                                "]" => {
                                    g.cell_size_px.w = g.cell_size_px.w.saturating_add(4);
                                    g.cell_size_px.h = g.cell_size_px.h.saturating_add(4);
                                    mutated = true;
                                }
                                "-" => {
                                    g.gap_x_px = g.gap_x_px.saturating_sub(1);
                                    g.gap_y_px = g.gap_y_px.saturating_sub(1);
                                    mutated = true;
                                }
                                "=" | "+" => {
                                    g.gap_x_px = g.gap_x_px.saturating_add(1);
                                    g.gap_y_px = g.gap_y_px.saturating_add(1);
                                    mutated = true;
                                }
                                "h" | "H" => {
                                    g.gap_x_px = g.gap_x_px.saturating_sub(1);
                                    mutated = true;
                                }
                                "j" | "J" => {
                                    g.gap_x_px = g.gap_x_px.saturating_add(1);
                                    mutated = true;
                                }
                                "k" | "K" => {
                                    g.gap_y_px = g.gap_y_px.saturating_sub(1);
                                    mutated = true;
                                }
                                "l" | "L" => {
                                    g.gap_y_px = g.gap_y_px.saturating_add(1);
                                    mutated = true;
                                }
                                "," => {
                                    g.rotation_deg -= 0.25;
                                    mutated = true;
                                }
                                "." => {
                                    g.rotation_deg += 0.25;
                                    mutated = true;
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                        // Top-region nudges run in a separate match so `g` isn't borrowed.
                        if !mutated {
                            let t = &mut state.draft.top;
                            if let Key::Character(s) = &event.logical_key {
                                match s.as_str() {
                                    "t" | "T" => {
                                        t.cutoff_bottom =
                                            t.cutoff_bottom.saturating_add(step as u16);
                                        mutated = true;
                                    }
                                    "y" | "Y" => {
                                        t.cutoff_bottom =
                                            t.cutoff_bottom.saturating_sub(step as u16);
                                        mutated = true;
                                    }
                                    "p" | "P" => {
                                        t.edge_padding_top =
                                            t.edge_padding_top.saturating_add(step as u16);
                                        mutated = true;
                                    }
                                    "o" | "O" => {
                                        t.edge_padding_top =
                                            t.edge_padding_top.saturating_sub(step as u16);
                                        mutated = true;
                                    }
                                    "x" | "X" => {
                                        t.edge_padding_x =
                                            t.edge_padding_x.saturating_add(step as u16);
                                        mutated = true;
                                    }
                                    "z" | "Z" => {
                                        t.edge_padding_x =
                                            t.edge_padding_x.saturating_sub(step as u16);
                                        mutated = true;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if mutated {
                            // Capture from the draft into locals so NLL ends the `state`
                            // borrow before `self` is mutated below.
                            let new_cell_rects = crate::geometry::cell_rects(g);
                            let grid_snap = *g;
                            let top_snap = state.draft.top;
                            self.cell_rects = new_cell_rects;
                            if let Some(gpu) = &self.gpu {
                                let size = (gpu.surface_config.width, gpu.surface_config.height);
                                let top_outer = crate::geometry::top_region_rect(
                                    &grid_snap, &top_snap, size.0, size.1,
                                );
                                self.pane_rects = match &self.cfg_top_layout {
                                    Some(root) => crate::layout::solve(root, top_outer),
                                    None => indexmap::IndexMap::new(),
                                };
                            }
                            return;
                        }
                    }

                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            state.cancel();
                        }
                        Key::Named(NamedKey::Enter) => {
                            if matches!(state.phase, Phase::Geometry) {
                                state.confirm_geometry();
                            }
                        }
                        _ if matches!(state.phase, Phase::Keymap { .. }) => {
                            let code = key_to_code_for_calibration(&event.logical_key);
                            state.record_key(&code);
                        }
                        _ => {}
                    }

                    // If calibration ended, finalize.
                    let ended = matches!(state.phase, Phase::Done | Phase::Cancelled);
                    if ended {
                        match &state.phase {
                            Phase::Done => {
                                let profile = state.draft.clone();
                                let path = crate::calibration::default_profile_path();
                                if let Err(e) = profile.save(&path) {
                                    log::warn!("failed to save profile after calibration: {}", e);
                                }
                                self.pending_events
                                    .push(crate::input::Event::CalibrationDone(profile.clone()));
                                // Refresh keymap + rects.
                                self.keymap = crate::input::Keymap::from_profile(&profile);
                                if let Some(gpu) = &self.gpu {
                                    let size =
                                        (gpu.surface_config.width, gpu.surface_config.height);
                                    self.cell_rects = crate::geometry::cell_rects(&profile.grid);
                                    let top_outer = crate::geometry::top_region_rect(
                                        &profile.grid,
                                        &profile.top,
                                        size.0,
                                        size.1,
                                    );
                                    self.pane_rects = match &self.cfg_top_layout {
                                        Some(root) => crate::layout::solve(root, top_outer),
                                        None => indexmap::IndexMap::new(),
                                    };
                                }
                                self.profile = Some(profile);
                            }
                            Phase::Cancelled => {
                                log::info!("calibration cancelled");
                            }
                            _ => {}
                        }
                        self.cal_state = None;
                    }
                    return;
                }

                // Suppress winit translation if the raw-input thread is authoritative.
                #[cfg(feature = "raw-input")]
                if self.raw_ring.is_some() {
                    return;
                }

                // Non-calibration path: normal input translate.
                self.winit_input.translate(
                    &event.logical_key,
                    event.state,
                    &self.keymap,
                    &mut self.pending_events,
                );
            }
            WindowEvent::Resized(sz) => {
                if let Some(g) = self.gpu.as_mut() {
                    g.resize(sz.width, sz.height);
                }
                if let Some(p) = &self.profile {
                    self.cell_rects = crate::geometry::cell_rects(&p.grid);
                    let top_outer =
                        crate::geometry::top_region_rect(&p.grid, &p.top, sz.width, sz.height);
                    self.pane_rects = match &self.cfg_top_layout {
                        Some(root) => crate::layout::solve(root, top_outer),
                        None => indexmap::IndexMap::new(),
                    };
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(gpu)) = (&self.window, self.gpu.as_mut()) {
                    // Drain raw-input ring into pending_events before rendering.
                    #[cfg(feature = "raw-input")]
                    if let Some(ring) = &self.raw_ring {
                        ring.drain_into(&mut self.pending_events);
                    }

                    let rotation = self
                        .profile
                        .as_ref()
                        .map(|p| p.grid.rotation_deg)
                        .unwrap_or(0.0);
                    let border_px = self.profile.as_ref().map(|p| p.grid.border_px).unwrap_or(4);
                    let cal_phase = self.cal_state.as_ref().map(|s| &s.phase);
                    // Compute the current top-region rect (uses calibration draft when
                    // active so live edits are reflected). Fed to both the user callback
                    // (via Frame::top_region_rect) and the calibration overlay.
                    let (grid_for_top, top_for_top) = match (&self.cal_state, &self.profile) {
                        (Some(s), _) => (s.draft.grid, s.draft.top),
                        (None, Some(p)) => (p.grid, p.top),
                        (None, None) => (
                            crate::calibration::GridGeometry {
                                origin_px: crate::calibration::PointPx { x: 0, y: 0 },
                                cell_size_px: crate::calibration::SizePx { w: 0, h: 0 },
                                gap_x_px: 0,
                                gap_y_px: 0,
                                border_px: 0,
                                rotation_deg: 0.0,
                            },
                            crate::calibration::TopGeometry {
                                edge_padding_top: 0,
                                edge_padding_x: 0,
                                cutoff_bottom: 0,
                            },
                        ),
                    };
                    let top_outer = crate::geometry::top_region_rect(
                        &grid_for_top,
                        &top_for_top,
                        gpu.surface_config.width,
                        gpu.surface_config.height,
                    );
                    let cal_top_rect = if self.cal_state.is_some() {
                        Some(top_outer)
                    } else {
                        None
                    };
                    let mut pending_layout: Option<Option<crate::layout::Node>> = None;
                    let outcome = render_one_frame(
                        gpu,
                        self.cfg.bg_color,
                        &self.cell_rects,
                        &self.pane_rects,
                        rotation,
                        border_px,
                        self.debug,
                        cal_phase,
                        cal_top_rect.as_ref(),
                        top_outer,
                        self.mode.as_mut(),
                        &self.pending_events,
                        &mut pending_layout,
                    );
                    match outcome {
                        ModeOutcome::Continue => {}
                        ModeOutcome::SwitchTo(new_mode) => {
                            // Swap in the new mode for the next frame.
                            // The outgoing mode drops here so its
                            // destructor (audio handles, OSC clients,
                            // …) runs before the next frame begins.
                            self.mode = new_mode;
                        }
                        ModeOutcome::Exit => {
                            event_loop.exit();
                        }
                    }
                    if let Some(new_layout) = pending_layout {
                        self.cfg_top_layout = new_layout;
                        if let Some(p) = &self.profile {
                            let size = (gpu.surface_config.width, gpu.surface_config.height);
                            let top_outer =
                                crate::geometry::top_region_rect(&p.grid, &p.top, size.0, size.1);
                            self.pane_rects = match &self.cfg_top_layout {
                                Some(root) => crate::layout::solve(root, top_outer),
                                None => indexmap::IndexMap::new(),
                            };
                        }
                    }
                    self.pending_events.clear();
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_one_frame(
    gpu: &mut Gpu,
    bg: Color,
    cell_rects: &[crate::Rect; 16],
    pane_rects: &indexmap::IndexMap<crate::layout::PaneId, crate::Rect>,
    rotation_deg: f32,
    border_px: u16,
    debug: bool,
    cal_phase: Option<&crate::calibration::Phase>,
    cal_top_rect: Option<&crate::Rect>,
    top_outer: crate::Rect,
    mode: &mut dyn Mode,
    events: &[crate::input::Event],
    pending_top_layout: &mut Option<Option<crate::layout::Node>>,
) -> ModeOutcome {
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame encoder"),
        });

    // 1. Clear offscreen FB to bg color.
    {
        let [r, g, b, a] = bg.as_linear_f32();
        let _ = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear offscreen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &gpu.offscreen.view,
                depth_slice: None,
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
            multiview_mask: None,
        });
    }

    // 2. Active mode's frame callback. The returned outcome is
    // bubbled out so the driver can swap modes / exit between frames.
    let outcome;
    {
        let mut frame = crate::Frame {
            device: &gpu.device,
            queue: &gpu.queue,
            encoder: &mut enc,
            offscreen_view: &gpu.offscreen.view,
            cell_rects,
            pane_rects,
            top_region_rect: top_outer,
            viewport_w: gpu.surface_config.width,
            viewport_h: gpu.surface_config.height,
            fill_pipeline: &gpu.fill,
            format: gpu.offscreen.format,
            pending_top_layout,
        };
        outcome = mode.frame(&mut frame, events);
    }

    // 2b. Lib-drawn borders (overlay user content so borders are always visible).
    draw_borders(
        &mut enc,
        gpu,
        cell_rects,
        border_px,
        Color::rgb(0x1f, 0x23, 0x30),
    );

    // 2c. Optional calibration overlay: orange cell markers + active-cell highlight.
    if let Some(phase) = cal_phase {
        draw_calibration_overlay(&mut enc, gpu, cell_rects, phase);
        if let Some(top_rect) = cal_top_rect {
            draw_top_region_overlay(&mut enc, gpu, *top_rect);
        }
    }

    // 2d. Optional debug overlay: magenta corner markers on each cell.
    if debug {
        draw_debug_corner_markers(&mut enc, gpu, cell_rects);
    }

    // 3. Composite to swapchain.
    let frame_tex = match gpu.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f)
        | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        // Surface acquisition failure (resize race, swapchain lost,
        // …): drop this frame but honour whatever outcome the mode
        // already produced so a SwitchTo / Exit doesn't get stuck.
        _ => return outcome,
    };
    let dst = frame_tex
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    gpu.composite.record(
        &gpu.device,
        &gpu.queue,
        &mut enc,
        &gpu.offscreen.view,
        &dst,
        gpu.surface_config.width,
        gpu.surface_config.height,
        rotation_deg,
    );
    gpu.queue.submit(Some(enc.finish()));
    frame_tex.present();
    outcome
}

fn draw_borders(
    enc: &mut wgpu::CommandEncoder,
    gpu: &Gpu,
    cell_rects: &[crate::Rect; 16],
    border_px: u16,
    color: Color,
) {
    if border_px == 0 {
        return;
    }
    gpu.fill.write_color(&gpu.queue, color.as_linear_f32());
    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("borders"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &gpu.offscreen.view,
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
    pass.set_pipeline(gpu.fill.pipeline());
    pass.set_bind_group(0, gpu.fill.bind(), &[]);
    let bp = border_px as i32;
    for r in cell_rects {
        // Top, bottom, left, right edges as four scissor passes.
        let edges = [
            (r.x, r.y, r.w as i32, bp),             // top
            (r.x, r.bottom() - bp, r.w as i32, bp), // bottom
            (r.x, r.y, bp, r.h as i32),             // left
            (r.right() - bp, r.y, bp, r.h as i32),  // right
        ];
        for (x, y, w, h) in edges {
            if w <= 0 || h <= 0 {
                continue;
            }
            let xx = x.max(0) as u32;
            let yy = y.max(0) as u32;
            let mut ww = w as u32;
            let mut hh = h as u32;
            let max_w = gpu.surface_config.width.saturating_sub(xx);
            let max_h = gpu.surface_config.height.saturating_sub(yy);
            ww = ww.min(max_w);
            hh = hh.min(max_h);
            if ww == 0 || hh == 0 {
                continue;
            }
            pass.set_scissor_rect(xx, yy, ww, hh);
            pass.set_viewport(xx as f32, yy as f32, ww as f32, hh as f32, 0.0, 1.0);
            pass.draw(0..3, 0..1);
        }
    }
}

fn draw_debug_corner_markers(
    enc: &mut wgpu::CommandEncoder,
    gpu: &Gpu,
    cell_rects: &[crate::Rect; 16],
) {
    // 4-px square in each cell's top-left corner, hot magenta. Easy to spot during calibration.
    gpu.fill.write_color(
        &gpu.queue,
        Color::rgba(0xff, 0x00, 0xff, 0x80).as_linear_f32(),
    );
    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("debug markers"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &gpu.offscreen.view,
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
    pass.set_pipeline(gpu.fill.pipeline());
    pass.set_bind_group(0, gpu.fill.bind(), &[]);
    let marker = 4u32;
    for r in cell_rects {
        let xx = r.x.max(0) as u32;
        let yy = r.y.max(0) as u32;
        let ww = marker.min(gpu.surface_config.width.saturating_sub(xx));
        let hh = marker.min(gpu.surface_config.height.saturating_sub(yy));
        if ww == 0 || hh == 0 {
            continue;
        }
        pass.set_scissor_rect(xx, yy, ww, hh);
        pass.set_viewport(xx as f32, yy as f32, ww as f32, hh as f32, 0.0, 1.0);
        pass.draw(0..3, 0..1);
    }
}

fn draw_calibration_overlay(
    enc: &mut wgpu::CommandEncoder,
    gpu: &Gpu,
    cell_rects: &[crate::Rect; 16],
    phase: &crate::calibration::Phase,
) {
    use crate::calibration::Phase;
    // Bright orange highlight for whatever cell/area is active.
    gpu.fill.write_color(
        &gpu.queue,
        Color::rgba(0xff, 0x80, 0x00, 0xa0).as_linear_f32(),
    );
    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("calibration overlay"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &gpu.offscreen.view,
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
    pass.set_pipeline(gpu.fill.pipeline());
    pass.set_bind_group(0, gpu.fill.bind(), &[]);

    // Always show a marker at every cell corner so the user can see the grid.
    // Highlight the active cell with a larger fill.
    let marker = 12u32;
    for (i, r) in cell_rects.iter().enumerate() {
        let row = (i / 4) as u8;
        let col = (i % 4) as u8;
        let active = match phase {
            Phase::Keymap { next_cell } => *next_cell == (row, col),
            _ => false,
        };
        let size = if active { marker * 3 } else { marker };
        let xx = r.x.max(0) as u32;
        let yy = r.y.max(0) as u32;
        let ww = size.min(gpu.surface_config.width.saturating_sub(xx));
        let hh = size.min(gpu.surface_config.height.saturating_sub(yy));
        if ww == 0 || hh == 0 {
            continue;
        }
        pass.set_scissor_rect(xx, yy, ww, hh);
        pass.set_viewport(xx as f32, yy as f32, ww as f32, hh as f32, 0.0, 1.0);
        pass.draw(0..3, 0..1);
    }
}

/// Translucent teal fill across the top-region rect so the user can see the effect of
/// `edge_padding_top`, `edge_padding_x`, and `cutoff_bottom` during geometry calibration.
fn draw_top_region_overlay(enc: &mut wgpu::CommandEncoder, gpu: &Gpu, rect: crate::Rect) {
    if rect.is_empty() {
        return;
    }
    gpu.fill.write_color(
        &gpu.queue,
        Color::rgba(0x20, 0xc0, 0xa0, 0x50).as_linear_f32(),
    );
    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("top-region overlay"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &gpu.offscreen.view,
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
    pass.set_pipeline(gpu.fill.pipeline());
    pass.set_bind_group(0, gpu.fill.bind(), &[]);
    let xx = rect.x.max(0) as u32;
    let yy = rect.y.max(0) as u32;
    let max_w = gpu.surface_config.width.saturating_sub(xx);
    let max_h = gpu.surface_config.height.saturating_sub(yy);
    let ww = rect.w.min(max_w);
    let hh = rect.h.min(max_h);
    if ww == 0 || hh == 0 {
        return;
    }
    pass.set_scissor_rect(xx, yy, ww, hh);
    pass.set_viewport(xx as f32, yy as f32, ww as f32, hh as f32, 0.0, 1.0);
    pass.draw(0..3, 0..1);
}

fn key_to_code_for_calibration(k: &winit::keyboard::Key) -> String {
    use winit::keyboard::Key;
    match k {
        Key::Character(s) => format!("CHAR_{}", s.to_uppercase()),
        Key::Named(n) => format!("NAMED_{:?}", n),
        Key::Unidentified(_) => "UNIDENTIFIED".into(),
        Key::Dead(_) => "DEAD".into(),
    }
}

impl App {
    /// Run the app with a user draw callback receiving a `Frame` and
    /// pending `Event` slice. Backwards-compatible shorthand for the
    /// single-mode case — internally wraps the closure in
    /// [`crate::app::Mode`] machinery via [`crate::app::ClosureMode`]
    /// so it shares a code path with [`Self::run_modes`].
    pub fn run<F>(self, draw: F) -> Result<()>
    where
        F: FnMut(&mut crate::Frame<'_>, &[crate::input::Event]) + 'static,
    {
        self.run_modes(Box::new(ClosureMode { draw }))
    }

    /// Run the app driving an arbitrary [`Mode`]. Modes can request
    /// [`ModeOutcome::SwitchTo`] mid-run to swap themselves out — the
    /// `winit` EventLoop and (where the platform allows) the GPU
    /// surface stay alive across the transition, so deck → rhythm →
    /// deck no longer needs the historical exec() bounce.
    pub fn run_modes(self, initial: Box<dyn Mode>) -> Result<()> {
        let event_loop = winit::event_loop::EventLoop::new()?;
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        let mut runtime = Runtime {
            cfg: self.cfg,
            cfg_top_layout: self.cfg_top_layout,
            profile: None,
            window: None,
            gpu: None,
            mode: initial,
            cell_rects: [Rect::ZERO; 16],
            pane_rects: indexmap::IndexMap::new(),
            pending_events: Vec::new(),
            debug: self.debug,
            winit_input: crate::input::WinitInput::default(),
            keymap: crate::input::Keymap::default(),
            force_calibration: self.force_calibration,
            force_keymap: self.force_keymap,
            cal_state: None,
            shift_down: false,
            #[cfg(feature = "raw-input")]
            raw_ring: None,
        };
        event_loop.run_app(&mut runtime)?;
        Ok(())
    }
}
