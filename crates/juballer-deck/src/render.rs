//! Render glue: juballer-core frame + events → deck action dispatch + widget render.

use crate::app::DeckApp;
use crate::tile::TileHandle;
use juballer_core::input::Event;
use juballer_core::Color;

/// Per-frame entry point. Takes the DeckApp, a juballer-core Frame + events, handles input
/// dispatch and tile rendering.
pub fn on_frame(app: &mut DeckApp, frame: &mut juballer_core::Frame, events: &[Event]) {
    // 0. Drain bus events for deck nav + widget action requests.
    drain_bus(app);

    // 0b. Apply any pending top-layout swap before drawing.
    if let Some(layout) = app.pending_top_layout.take() {
        frame.set_top_layout(layout);
    }

    // 1. Handle input events → dispatch to bound actions.
    for ev in events {
        match ev {
            Event::KeyDown { row, col, ts, .. } => {
                if let Some(tx) = &app.editor_event_tx {
                    let _ = tx.send(crate::editor::server::EditorEvent::KeyPreview {
                        row: *row,
                        col: *col,
                        down: true,
                    });
                }
                if (*row, *col) == (0, 3) {
                    app.master_tr_down = Some(*ts);
                }
                if (*row, *col) == (3, 0) {
                    app.master_bl_down = Some(*ts);
                }
                dispatch_down(app, *row, *col);
            }
            Event::KeyUp { row, col, .. } => {
                if let Some(tx) = &app.editor_event_tx {
                    let _ = tx.send(crate::editor::server::EditorEvent::KeyPreview {
                        row: *row,
                        col: *col,
                        down: false,
                    });
                }
                if (*row, *col) == (0, 3) {
                    app.master_tr_down = None;
                }
                if (*row, *col) == (3, 0) {
                    app.master_bl_down = None;
                }
                dispatch_up(app, *row, *col);
            }
            _ => {}
        }
    }

    // 1b. Master-chord gesture: if both corners held >= 5s, jump to master page.
    if let (Some(tr), Some(bl)) = (app.master_tr_down, app.master_bl_down) {
        let earliest = tr.min(bl);
        if earliest.elapsed() >= std::time::Duration::from_secs(5) {
            let master = app
                .config
                .active_profile()
                .ok()
                .map(|p| p.meta.default_page.clone())
                .unwrap_or_else(|| "home".to_string());
            if app.active_page != master {
                app.page_history.push(app.active_page.clone());
                app.active_page = master;
                if app.bind_active_page().is_ok() {
                    app.queue_top_layout_for_active_page();
                    emit_page_appear(app);
                }
            }
            app.master_tr_down = None;
            app.master_bl_down = None;
        }
    }

    // 1c. Apply plugin-supplied named-tile overrides so icons/labels/state_color
    // pushed by plugins show up before the next paint. Runs after any scroll /
    // page switch so the mapping picks up the current physical location.
    app.apply_named_tile_overrides();

    // 2. Fill each tile's background color. Pick a theme-consistent default (mantle) so
    // the grid has its own subtle tone against the page base — not a hard-coded dark blue.
    // Indices here are PHYSICAL (0..4) — app.tiles is indexed by physical cell and is
    // rebuilt from the logical grid whenever scroll offset or page changes.
    let mantle = app.theme.mantle;
    let default_tile_bg = Color::rgba(mantle.r(), mantle.g(), mantle.b(), mantle.a());
    for r in 0..4u8 {
        for c in 0..4u8 {
            let tile_state = &app.tiles[(r as usize) * 4 + c as usize];
            let bg = tile_state.bg.unwrap_or(default_tile_bg);
            frame.grid_cell(r, c).fill(bg);
        }
    }

    // 2b. Raw-wgpu tile content (custom shaders / video). Draws into the offscreen FB
    // BEFORE the egui overlay so the icon/label/flash paint on top of this content.
    let now = std::time::Instant::now();
    let time = now.duration_since(app.boot_instant).as_secs_f32();
    let delta_time = app
        .last_frame_instant
        .map(|t| now.duration_since(t).as_secs_f32())
        .unwrap_or(0.0);
    app.last_frame_instant = Some(now);
    // Keep in sync with the egui paint_tile FLASH_MS constant below.
    const SHADER_FLASH_MS: f32 = 280.0;
    for r in 0..4u8 {
        for c in 0..4u8 {
            let idx = (r as usize) * 4 + c as usize;
            let shader_src = app.tiles[idx].shader.clone();
            let Some(src) = shader_src else { continue };

            // Derive per-tile state uniforms BEFORE we hand &mut self to with_tile_raw.
            let bound_opt = app.bound_actions.get(&(r, c));
            let kind = bound_opt
                .map(|b| b.action.kind())
                .unwrap_or(crate::action::ActionKind::Action);
            let kind_f = match kind {
                crate::action::ActionKind::Action => 0.0f32,
                crate::action::ActionKind::Nav => 1.0,
                crate::action::ActionKind::Toggle => 2.0,
            };
            let bound_f = if bound_opt.is_some() { 1.0 } else { 0.0 };
            let tile_state = &app.tiles[idx];
            let toggle_on_f = if matches!(kind, crate::action::ActionKind::Toggle)
                && tile_state.state_color.is_some()
            {
                1.0
            } else {
                0.0
            };
            let flash_f = match tile_state.flash_until {
                Some(until) => {
                    let now = std::time::Instant::now();
                    if until > now {
                        let remain_ms = until.duration_since(now).as_millis() as f32;
                        (remain_ms / SHADER_FLASH_MS).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                }
                None => 0.0,
            };
            let accent_rgba = match kind {
                crate::action::ActionKind::Nav => color32_to_rgba(app.theme.accent),
                crate::action::ActionKind::Toggle => color32_to_rgba(app.theme.ok),
                crate::action::ActionKind::Action => color32_to_rgba(app.theme.accent_alt),
            };
            let state_rgba = match tile_state.state_color {
                Some(sc) => core_color_to_rgba(sc),
                None => accent_rgba,
            };

            let shader_cache = &mut app.shader_cache;
            let video_registry = &mut app.video_registry;
            frame.with_tile_raw(r, c, |mut ctx| match src {
                crate::tile::TileShaderSource::CustomShader { wgsl_path, .. } => {
                    let uniforms = crate::shader::TileUniforms {
                        resolution: [ctx.viewport.2, ctx.viewport.3],
                        time,
                        delta_time,
                        cursor: [0.0, 0.0],
                        kind: kind_f,
                        bound: bound_f,
                        toggle_on: toggle_on_f,
                        flash: flash_f,
                        _pad0: [0.0, 0.0],
                        accent: accent_rgba,
                        state: state_rgba,
                        spectrum: [[0.0; 4]; 4],
                    };
                    shader_cache.draw_tile(&mut ctx, &wgsl_path, &uniforms);
                }
                crate::tile::TileShaderSource::Video { uri } => {
                    video_registry.draw_tile(&mut ctx, &uri);
                }
            });
        }
    }

    // 3. egui overlay pass.
    // Snapshot env from active profile before taking &mut of other fields.
    let env: indexmap::IndexMap<String, String> = app
        .config
        .active_profile()
        .ok()
        .map(|p| p.meta.env.clone())
        .unwrap_or_default();

    // Collect shader/video errors (if any) per cell ahead of the destructure.
    let mut shader_errors: [Option<String>; 16] = Default::default();
    for (i, ts) in app.tiles.iter().enumerate() {
        if let Some(src) = &ts.shader {
            match src {
                crate::tile::TileShaderSource::CustomShader { wgsl_path, .. } => {
                    if let Some(e) = app.shader_cache.last_error(wgsl_path) {
                        shader_errors[i] = Some(e.message.clone());
                    }
                }
                crate::tile::TileShaderSource::Video { uri } => {
                    if let Some(e) = app.video_registry.last_error(uri) {
                        shader_errors[i] = Some(e);
                    }
                }
            }
        }
    }

    let DeckApp {
        tiles,
        bound_actions,
        egui_overlay,
        active_widgets,
        bus,
        state,
        rt,
        icon_loader,
        view_trees,
        theme,
        ..
    } = app;
    let theme = *theme;

    egui_overlay.draw(frame, |ctx| {
        // Tile icons / labels.
        for r in 0..4u8 {
            for c in 0..4u8 {
                let idx = (r as usize) * 4 + c as usize;
                let ts = &tiles[idx];
                let bound = bound_actions.get(&(r, c));
                let kind = bound
                    .map(|b| b.action.kind())
                    .unwrap_or(crate::action::ActionKind::Action);
                let err = shader_errors[idx].as_deref();
                ctx.in_grid_cell(r, c, |ui| {
                    paint_tile(ui, ts, bound, kind, icon_loader, &theme, err);
                });
            }
        }

        // Top-region widgets.
        for (pane_name, widget) in active_widgets.iter_mut() {
            // `ctx.in_top_pane` expects `PaneId` which is `&'static str`. The pane names
            // come from config (owned Strings). We leak them to get static lifetimes —
            // active_widgets is rebuilt on every bind_active_page so leak rate is bounded.
            // TODO: replace with a real string interner.
            #[allow(clippy::mem_forget)]
            let static_id: &'static str = Box::leak(pane_name.clone().into_boxed_str());
            let mut cx = crate::widget::WidgetCx {
                pane: static_id,
                env: &env,
                bus,
                state,
                rt,
                view_trees,
                theme,
            };
            ctx.in_top_pane(static_id, |ui| {
                widget.render(ui, &mut cx);
            });
        }
    });
}

fn paint_tile(
    ui: &mut egui::Ui,
    state: &crate::tile::TileState,
    bound: Option<&crate::app::BoundAction>,
    kind: crate::action::ActionKind,
    loader: &mut crate::icon_loader::IconLoader,
    theme: &crate::theme::Theme,
    shader_err: Option<&str>,
) {
    let rect = ui.max_rect();
    let rounding = egui::Rounding::same(10);
    let tile_rect = rect.shrink(2.0);
    let is_bound = bound.is_some();

    // Back-nav tiles (e.g. deck.page_back) point the chevron left. Detected by a
    // "back", "prev", or "‹" hint in the label or icon — no trait downcasting needed.
    let is_back_nav = matches!(kind, crate::action::ActionKind::Nav)
        && bound
            .map(|b| {
                let l = b
                    .label
                    .as_deref()
                    .map(str::to_ascii_lowercase)
                    .unwrap_or_default();
                let i = b.icon.as_deref().unwrap_or_default();
                l == "back"
                    || l == "prev"
                    || l.starts_with("back ")
                    || l.starts_with("prev ")
                    || i == "‹"
                    || i == "←"
                    || i == "⬅"
                    || i == "◀"
            })
            .unwrap_or(false);

    let state_color32 = state
        .state_color
        .map(|c| egui::Color32::from_rgba_premultiplied(c.0, c.1, c.2, c.3));
    let toggle_on = matches!(kind, crate::action::ActionKind::Toggle) && state_color32.is_some();

    // Per-kind primary accent drives halo, border tint, flash, and chevron.
    let primary_accent: egui::Color32 = match kind {
        crate::action::ActionKind::Nav => theme.accent_alt,
        crate::action::ActionKind::Toggle => state_color32.unwrap_or(theme.info),
        crate::action::ActionKind::Action => theme.accent,
    };

    let painter = ui.painter();

    // 1. Drop shadow.
    let shadow_base = theme.crust;
    for (offset, alpha) in [(1.0, 90u8), (3.0, 55u8), (6.0, 22u8)] {
        let sh = tile_rect.translate(egui::vec2(0.0, offset));
        painter.rect_filled(
            sh,
            rounding,
            egui::Color32::from_rgba_unmultiplied(
                shadow_base.r(),
                shadow_base.g(),
                shadow_base.b(),
                alpha,
            ),
        );
    }

    // 2. Body — vertical gradient, lifted at top, darker at bottom. Toggle-ON tiles
    // tint the body with ~20% of their state color so the active state pops.
    let base_body = match state.bg {
        Some(bg) => egui::Color32::from_rgba_premultiplied(bg.0, bg.1, bg.2, bg.3),
        None => {
            if is_bound {
                theme.surface0
            } else {
                let m = theme.mantle;
                egui::Color32::from_rgba_unmultiplied(m.r(), m.g(), m.b(), 140)
            }
        }
    };
    let top_body = if toggle_on {
        tint(base_body, state_color32.unwrap(), 0.22)
    } else {
        lighten(base_body, 0.08)
    };
    let bottom_body = if toggle_on {
        tint(base_body, state_color32.unwrap(), 0.08)
    } else {
        base_body
    };
    // When a tile has its own shader, paint the card body semi-transparent so the
    // shader output shows through. The gradient still anchors the tile visually.
    let has_shader = state.shader.is_some();
    let (top_body, bottom_body) = if has_shader {
        (with_alpha(top_body, 90), with_alpha(bottom_body, 90))
    } else {
        (top_body, bottom_body)
    };
    paint_vertical_gradient(painter, tile_rect, rounding, top_body, bottom_body);

    // 2b. Soft top highlight + bottom shade for depth.
    let hi = egui::Rect::from_min_max(
        egui::pos2(tile_rect.min.x, tile_rect.min.y),
        egui::pos2(tile_rect.max.x, tile_rect.min.y + 2.0),
    );
    painter.rect_filled(
        hi,
        egui::Rounding {
            nw: (rounding.nw) as u8,
            ne: (rounding.ne) as u8,
            sw: 0,
            se: 0,
        },
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18),
    );
    let lo = egui::Rect::from_min_max(
        egui::pos2(tile_rect.min.x, tile_rect.max.y - 2.0),
        egui::pos2(tile_rect.max.x, tile_rect.max.y),
    );
    painter.rect_filled(
        lo,
        egui::Rounding {
            nw: 0,
            ne: 0,
            sw: (rounding.sw) as u8,
            se: (rounding.se) as u8,
        },
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 34),
    );

    // 3. Border — nav gets the blue accent, toggle-on gets state color, action stays quiet.
    if is_bound {
        let (border_col, border_w) = match kind {
            crate::action::ActionKind::Nav => (theme.accent_alt, 1.5),
            crate::action::ActionKind::Toggle => (state_color32.unwrap_or(theme.surface1), 1.5),
            crate::action::ActionKind::Action => (theme.surface1, 1.0),
        };
        painter.rect_stroke(
            tile_rect,
            rounding,
            egui::Stroke::new(border_w, border_col),
            egui::StrokeKind::Middle,
        );
    } else {
        paint_dashed_rect(painter, tile_rect, rounding, theme.surface1, 4.0, 4.0, 1.0);
        // Faint centered dot to suggest "nothing here" without being loud.
        painter.circle_filled(
            tile_rect.center(),
            2.0,
            egui::Color32::from_rgba_unmultiplied(
                theme.surface1.r(),
                theme.surface1.g(),
                theme.surface1.b(),
                60,
            ),
        );
    }

    // 4. Icon halo — only for toggle-ON tiles, using state color. Plain action
    // tiles look cleaner without the lavender blob behind the icon.
    if is_bound && toggle_on {
        if let Some(sc) = state_color32 {
            paint_icon_halo(painter, tile_rect, sc);
        }
    }

    // 5. Primary-action hint for bound cells with no icon/label yet.
    if is_bound
        && state.icon.is_none()
        && state.label.is_none()
        && bound.map_or(true, |b| b.icon.is_none() && b.label.is_none())
    {
        let a = primary_accent;
        painter.rect_stroke(
            tile_rect.shrink(4.0),
            egui::Rounding::same(8),
            egui::Stroke::new(
                1.0,
                egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 70),
            ),
            egui::StrokeKind::Middle,
        );
    }

    // 6. Press flash — accent-tinted fill + two expanding bloom rings.
    if let Some(until) = state.flash_until {
        let now = std::time::Instant::now();
        if until > now {
            const FLASH_MS: f32 = 280.0;
            let remain_ms = until.duration_since(now).as_millis() as f32;
            let progress = 1.0 - (remain_ms / FLASH_MS).clamp(0.0, 1.0);
            let fade = 1.0 - crate::theme::ease_out_cubic(progress);
            let a = primary_accent;
            let fill_alpha = (fade * 80.0) as u8;
            painter.rect_filled(
                tile_rect,
                rounding,
                egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), fill_alpha),
            );
            for (grow_mul, base_alpha) in [(8.0f32, 160u8), (16.0, 90u8)] {
                let ring_grow = progress * grow_mul;
                let ring_rect = tile_rect.expand(ring_grow);
                let ring_alpha = ((1.0 - progress) * base_alpha as f32) as u8;
                painter.rect_stroke(
                    ring_rect,
                    egui::Rounding::same((10.0 + ring_grow) as u8),
                    egui::Stroke::new(
                        1.5,
                        egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), ring_alpha),
                    ),
                    egui::StrokeKind::Middle,
                );
            }
        }
    }

    // 7. Content — icon + label.
    let inner = rect.shrink(10.0);
    let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    content_ui.set_clip_rect(inner);

    content_ui.vertical_centered(|ui| {
        let cell_size = ui.available_width().min(ui.available_height());
        let icon_target = cell_size * 0.55;

        ui.add_space(cell_size * 0.06);

        let icon_color = if toggle_on {
            state_color32.unwrap_or(theme.text)
        } else {
            theme.text
        };

        match state.icon.as_ref() {
            Some(crate::tile::IconRef::Emoji(s)) => {
                paint_centered_emoji(ui, s, icon_target, icon_color);
            }
            Some(crate::tile::IconRef::Path(p)) => {
                render_image(ui, loader, p, icon_target);
            }
            Some(crate::tile::IconRef::Builtin(name)) => {
                ui.label(
                    egui::RichText::new(format!("[{name}]"))
                        .size(icon_target * 0.5)
                        .color(theme.subtext0),
                );
            }
            None => {
                if let Some(config_icon) = bound.and_then(|b| b.icon.as_deref()) {
                    if config_icon.chars().count() <= 3 {
                        paint_centered_emoji(ui, config_icon, icon_target, icon_color);
                    } else {
                        let p = std::path::PathBuf::from(config_icon);
                        render_image(ui, loader, &p, icon_target);
                    }
                }
            }
        }

        ui.add_space(cell_size * 0.04);

        let label_text = state
            .label
            .clone()
            .or_else(|| bound.and_then(|b| b.label.clone()));
        if let Some(lbl) = label_text {
            let label_size = (cell_size * 0.13).clamp(11.0, 22.0);
            let max_chars = ((cell_size / label_size) * 1.7) as usize;
            let display = truncate_mid(&lbl, max_chars.max(6));
            let (label_color, label_text) = match kind {
                crate::action::ActionKind::Nav => {
                    let t = if is_back_nav {
                        format!("‹ {display}")
                    } else {
                        format!("{display} ›")
                    };
                    (theme.accent_alt, t)
                }
                crate::action::ActionKind::Toggle if toggle_on => {
                    (state_color32.unwrap_or(theme.text), display)
                }
                _ => (theme.text, display),
            };
            ui.label(
                egui::RichText::new(label_text)
                    .size(label_size)
                    .color(label_color),
            );
        }
    });

    // 8. Nav affordances — edge chevron + small top-right "nav" ribbon.
    if is_bound && matches!(kind, crate::action::ActionKind::Nav) {
        let p = ui.painter();
        paint_nav_chevron(p, tile_rect, theme.accent_alt, is_back_nav);
        paint_corner_ribbon(p, tile_rect, "nav", theme.accent_alt, theme.crust);
    }

    // 8b. Toggle pill at the bottom for Toggle kind; plain thin bar otherwise.
    if is_bound && matches!(kind, crate::action::ActionKind::Toggle) {
        if let Some(accent) = state_color32 {
            paint_toggle_pill(ui.painter(), tile_rect, accent);
        }
    } else if let Some(accent) = state.state_color {
        let bar_h = 4.0;
        let inset = 14.0;
        let y_top = rect.max.y - 10.0 - bar_h;
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + inset, y_top),
            egui::pos2(rect.max.x - inset, y_top + bar_h),
        );
        ui.painter().rect_filled(
            bar,
            egui::Rounding::same((bar_h * 0.5) as u8),
            egui::Color32::from_rgba_premultiplied(accent.0, accent.1, accent.2, accent.3),
        );
    }

    // 9. Shader error badge.
    if let Some(err) = shader_err {
        let top_y = rect.min.y + 2.0;
        let badge = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 2.0, top_y),
            egui::pos2(rect.max.x - 2.0, top_y + 18.0),
        );
        ui.painter().rect_filled(
            badge,
            egui::Rounding::same(4),
            egui::Color32::from_rgba_unmultiplied(220, 40, 40, 230),
        );
        let mut short = err.replace('\n', " ");
        if short.chars().count() > 48 {
            short = short.chars().take(45).collect::<String>() + "...";
        }
        ui.painter().text(
            badge.left_center() + egui::vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("shader: {short}"),
            egui::FontId::proportional(10.0),
            egui::Color32::WHITE,
        );
    }
}

/// Convert an egui `Color32` (u8 RGBA) into the normalized `[f32; 4]` a shader
/// expects. Alpha is always 1.0 — shader uniforms treat these as solid swatches
/// the shader itself can alpha-blend.
fn color32_to_rgba(c: egui::Color32) -> [f32; 4] {
    [
        c.r() as f32 / 255.0,
        c.g() as f32 / 255.0,
        c.b() as f32 / 255.0,
        1.0,
    ]
}

/// Convert a core u8 RGBA Color into normalized `[f32; 4]`, preserving alpha.
fn core_color_to_rgba(c: Color) -> [f32; 4] {
    [
        c.0 as f32 / 255.0,
        c.1 as f32 / 255.0,
        c.2 as f32 / 255.0,
        c.3 as f32 / 255.0,
    ]
}

/// Lighten toward white by `amt` in 0..1.
fn with_alpha(c: egui::Color32, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn lighten(c: egui::Color32, amt: f32) -> egui::Color32 {
    let r = (c.r() as f32 + (255.0 - c.r() as f32) * amt).clamp(0.0, 255.0) as u8;
    let g = (c.g() as f32 + (255.0 - c.g() as f32) * amt).clamp(0.0, 255.0) as u8;
    let b = (c.b() as f32 + (255.0 - c.b() as f32) * amt).clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgba_premultiplied(r, g, b, c.a())
}

/// Blend `other` into `c` by `amt` in 0..1. Preserves `c`'s alpha.
fn tint(c: egui::Color32, other: egui::Color32, amt: f32) -> egui::Color32 {
    let r = (c.r() as f32 * (1.0 - amt) + other.r() as f32 * amt) as u8;
    let g = (c.g() as f32 * (1.0 - amt) + other.g() as f32 * amt) as u8;
    let b = (c.b() as f32 * (1.0 - amt) + other.b() as f32 * amt) as u8;
    egui::Color32::from_rgba_premultiplied(r, g, b, c.a())
}

/// Vertical gradient via 8 horizontal strips — smooth enough at tile scale, cheap to draw.
fn paint_vertical_gradient(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: egui::Rounding,
    top: egui::Color32,
    bottom: egui::Color32,
) {
    const STRIPS: usize = 8;
    painter.rect_filled(rect, rounding, bottom);
    let h = rect.height() / STRIPS as f32;
    for i in 0..STRIPS {
        let t = i as f32 / (STRIPS - 1) as f32;
        let col = tint(top, bottom, t);
        let y0 = rect.min.y + i as f32 * h;
        let y1 = y0 + h;
        let r = egui::Rect::from_min_max(egui::pos2(rect.min.x, y0), egui::pos2(rect.max.x, y1));
        let rd = if i == 0 {
            egui::Rounding {
                nw: (rounding.nw) as u8,
                ne: (rounding.ne) as u8,
                sw: 0,
                se: 0,
            }
        } else if i == STRIPS - 1 {
            egui::Rounding {
                nw: 0,
                ne: 0,
                sw: (rounding.sw) as u8,
                se: (rounding.se) as u8,
            }
        } else {
            egui::Rounding::ZERO
        };
        painter.rect_filled(r, rd, col);
    }
}

/// Subtle halo behind the icon — tight + low-alpha. Only used for active toggle tiles.
fn paint_icon_halo(painter: &egui::Painter, rect: egui::Rect, accent: egui::Color32) {
    let center = rect.center() - egui::vec2(0.0, rect.height() * 0.05);
    for (radius_frac, alpha) in [(0.22f32, 14u8), (0.14, 24)] {
        let r = rect.width().min(rect.height()) * radius_frac;
        let rr = egui::Rect::from_center_size(center, egui::vec2(r * 2.0, r * 2.0));
        painter.rect_filled(
            rr,
            egui::Rounding::same((r) as u8),
            egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha),
        );
    }
}

/// Edge chevron pointing right (forward-nav) or left (back-nav).
fn paint_nav_chevron(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32, back: bool) {
    let size = 8.0;
    let pad = 10.0;
    let y = rect.center().y;
    let stroke_col = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 210);
    let stroke = egui::Stroke::new(2.5, stroke_col);
    if back {
        let x = rect.min.x + pad;
        painter.line_segment([egui::pos2(x + size, y - size), egui::pos2(x, y)], stroke);
        painter.line_segment([egui::pos2(x, y), egui::pos2(x + size, y + size)], stroke);
    } else {
        let x = rect.max.x - pad;
        painter.line_segment([egui::pos2(x - size, y - size), egui::pos2(x, y)], stroke);
        painter.line_segment([egui::pos2(x, y), egui::pos2(x - size, y + size)], stroke);
    }
}

/// Small pill-shaped ribbon in the top-right corner of the tile.
fn paint_corner_ribbon(
    painter: &egui::Painter,
    rect: egui::Rect,
    text: &str,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    let font_size = (crate::theme::FONT_SMALL - 1.0).max(9.0);
    let pad_x = 5.0;
    let pad_y = 2.0;
    let galley =
        painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(font_size), fg);
    let size = galley.size();
    let w = size.x + pad_x * 2.0;
    let h = size.y + pad_y * 2.0;
    let ribbon = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - w - 6.0, rect.min.y + 5.0),
        egui::vec2(w, h),
    );
    painter.rect_filled(
        ribbon,
        egui::Rounding::same((h * 0.5) as u8),
        egui::Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 220),
    );
    painter.text(
        ribbon.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(font_size),
        fg,
    );
}

/// Toggle pill — taller, more emphatic than the thin accent stripe.
fn paint_toggle_pill(painter: &egui::Painter, rect: egui::Rect, accent: egui::Color32) {
    let bar_h = 7.0;
    let inset = 18.0;
    let y_top = rect.max.y - 10.0 - bar_h;
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + inset, y_top),
        egui::pos2(rect.max.x - inset, y_top + bar_h),
    );
    painter.rect_filled(
        bar,
        egui::Rounding::same((bar_h * 0.5) as u8),
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 60),
    );
    painter.rect_filled(bar, egui::Rounding::same((bar_h * 0.5) as u8), accent);
    let hi = egui::Rect::from_min_max(bar.min, egui::pos2(bar.max.x, bar.min.y + bar_h * 0.45));
    painter.rect_filled(
        hi,
        egui::Rounding {
            nw: (bar_h * 0.5) as u8,
            ne: (bar_h * 0.5) as u8,
            sw: 0,
            se: 0,
        },
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 50),
    );
}

/// Paint a character (emoji or otherwise) centered vertically inside the
/// label's automatic bounding box. egui's default label baseline leaves
/// emojis top-aligned; this uses a monospace/proportional `FontId` with
/// explicit size so the glyph sits in the middle of the allocated row.
fn paint_centered_emoji(ui: &mut egui::Ui, s: &str, size: f32, color: egui::Color32) {
    let font = egui::FontId::new(size, egui::FontFamily::Proportional);
    ui.label(egui::RichText::new(s).font(font).color(color));
}

/// Truncate with a trailing ellipsis if the string exceeds `max` chars.
fn truncate_mid(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Draw a dashed rounded rectangle border. Uses short line segments along each edge;
/// keeps corners soft by rounding only the segment endpoints.
fn paint_dashed_rect(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: egui::Rounding,
    color: egui::Color32,
    dash: f32,
    gap: f32,
    stroke_w: f32,
) {
    let a = color.a();
    let dim =
        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (a / 2).max(40));
    let stroke = egui::Stroke::new(stroke_w, dim);
    let r = rounding
        .nw
        .max(rounding.ne)
        .max(rounding.sw)
        .max(rounding.se) as f32;
    let r = r.max(2.0);
    let top_y = rect.min.y;
    let bot_y = rect.max.y;
    let left_x = rect.min.x;
    let right_x = rect.max.x;
    // Horizontal edges.
    let mut x = left_x + r;
    while x < right_x - r {
        let x2 = (x + dash).min(right_x - r);
        painter.line_segment([egui::pos2(x, top_y), egui::pos2(x2, top_y)], stroke);
        painter.line_segment([egui::pos2(x, bot_y), egui::pos2(x2, bot_y)], stroke);
        x += dash + gap;
    }
    // Vertical edges.
    let mut y = top_y + r;
    while y < bot_y - r {
        let y2 = (y + dash).min(bot_y - r);
        painter.line_segment([egui::pos2(left_x, y), egui::pos2(left_x, y2)], stroke);
        painter.line_segment([egui::pos2(right_x, y), egui::pos2(right_x, y2)], stroke);
        y += dash + gap;
    }
}

fn render_image(
    ui: &mut egui::Ui,
    loader: &mut crate::icon_loader::IconLoader,
    path: &std::path::Path,
    target_size: f32,
) {
    match loader.load(path) {
        Ok(bytes) => match decode_image(bytes) {
            Some(img) => {
                let uri = format!("bytes://{}", path.display());
                let tex = ui
                    .ctx()
                    .load_texture(uri, img, egui::TextureOptions::LINEAR);
                ui.add(
                    egui::Image::new(egui::load::SizedTexture::from_handle(&tex))
                        .fit_to_exact_size(egui::vec2(target_size, target_size)),
                );
            }
            None => {
                ui.label(
                    egui::RichText::new("decode err")
                        .small()
                        .color(crate::theme::MOCHA_RED),
                );
            }
        },
        Err(e) => {
            ui.label(
                egui::RichText::new(format!("! {e}"))
                    .small()
                    .color(crate::theme::MOCHA_RED),
            );
        }
    }
}

fn decode_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

fn dispatch_down(app: &mut DeckApp, row: u8, col: u8) {
    let Some(bound) = app.bound_actions.get_mut(&(row, col)) else {
        return;
    };
    let tile_state = &mut app.tiles[(row as usize) * 4 + col as usize];
    let env = app
        .config
        .active_profile()
        .ok()
        .map(|p| p.meta.env.clone())
        .unwrap_or_default();
    let binding_id = bound.binding_id.clone();
    let mut cx = crate::action::ActionCx {
        cell: (row, col),
        binding_id: &binding_id,
        tile: TileHandle::new(tile_state),
        env: &env,
        bus: &app.bus,
        state: &mut app.state,
        rt: &app.rt,
    };
    bound.action.on_down(&mut cx);
}

fn dispatch_up(app: &mut DeckApp, row: u8, col: u8) {
    let Some(bound) = app.bound_actions.get_mut(&(row, col)) else {
        return;
    };
    let tile_state = &mut app.tiles[(row as usize) * 4 + col as usize];
    let env = app
        .config
        .active_profile()
        .ok()
        .map(|p| p.meta.env.clone())
        .unwrap_or_default();
    let binding_id = bound.binding_id.clone();
    let mut cx = crate::action::ActionCx {
        cell: (row, col),
        binding_id: &binding_id,
        tile: TileHandle::new(tile_state),
        env: &env,
        bus: &app.bus,
        state: &mut app.state,
        rt: &app.rt,
    };
    bound.action.on_up(&mut cx);
}

pub fn drain_bus(app: &mut DeckApp) {
    use tokio::sync::broadcast::error::TryRecvError;
    loop {
        let ev = match app.deck_bus_rx.try_recv() {
            Ok(e) => e,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Closed) => return,
            Err(TryRecvError::Lagged(n)) => {
                tracing::warn!("deck bus subscriber lagged by {} events", n);
                continue;
            }
        };
        handle_bus_event(app, &ev);
    }
}

fn handle_bus_event(app: &mut DeckApp, ev: &crate::bus::Event) {
    match ev.topic.as_str() {
        "deck.page_switch_request" => {
            if let Some(page) = ev.data.get("page").and_then(|v| v.as_str()) {
                switch_page(app, page.to_string());
            }
        }
        "deck.page_back_request" => {
            if let Some(prev) = app.page_history.pop() {
                set_page(app, prev);
            }
        }
        "deck.profile_switch_request" => {
            if let Some(profile) = ev.data.get("profile").and_then(|v| v.as_str()) {
                app.config.deck.active_profile = profile.to_string();
                app.page_history.clear();
                if let Err(e) = app.bind_active_page() {
                    tracing::warn!("profile switch '{}': bind failed: {}", profile, e);
                } else {
                    app.queue_top_layout_for_active_page();
                    emit_page_appear(app);
                }
            }
        }
        "deck.cycle_request" => {
            let pages: Vec<String> = ev
                .data
                .get("pages")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if pages.is_empty() {
                return;
            }
            let direction = ev
                .data
                .get("direction")
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            let cur = app.active_page.clone();
            let idx = pages.iter().position(|p| p == &cur).unwrap_or(0);
            let len = pages.len() as i64;
            let next_idx = (((idx as i64) + direction).rem_euclid(len)) as usize;
            let next = pages[next_idx].clone();
            switch_page(app, next);
        }
        "deck.scroll_request" => {
            let dr = ev.data.get("dr").and_then(|v| v.as_i64()).unwrap_or(0);
            let dc = ev.data.get("dc").and_then(|v| v.as_i64()).unwrap_or(0);
            apply_scroll(app, dr, dc);
        }
        "widget.action_request" => {
            let Some(action_name) = ev.data.get("action").and_then(|v| v.as_str()) else {
                return;
            };
            let args_table = ev
                .data
                .get("args")
                .and_then(|v| v.as_object())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| json_to_toml(v).map(|tv| (k.clone(), tv)))
                        .collect::<toml::Table>()
                })
                .unwrap_or_default();
            let (cell_r, cell_c) = ev
                .data
                .get("cell")
                .and_then(|v| v.as_array())
                .and_then(|a| {
                    let r = a.first()?.as_u64()? as u8;
                    let c = a.get(1)?.as_u64()? as u8;
                    Some((r, c))
                })
                .unwrap_or((0, 0));
            let mut action = match app.actions.build(action_name, &args_table) {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!("widget.action_request: build {}: {}", action_name, e);
                    return;
                }
            };
            let env = app
                .config
                .active_profile()
                .ok()
                .map(|p| p.meta.env.clone())
                .unwrap_or_default();
            let binding_id = format!("widget:{}", action_name);
            let mut tile_state = crate::tile::TileState::default();
            let mut cx = crate::action::ActionCx {
                cell: (cell_r, cell_c),
                binding_id: &binding_id,
                tile: TileHandle::new(&mut tile_state),
                env: &env,
                bus: &app.bus,
                state: &mut app.state,
                rt: &app.rt,
            };
            action.on_down(&mut cx);
        }
        _ => {}
    }
}

/// Apply a scroll delta to the active page. Clamps so the visible window stays inside
/// the logical bounds. Rebuilds the physical binding, persists the new offset, and
/// publishes `action.scroll` so widgets (e.g. the notification toast) can react.
fn apply_scroll(app: &mut DeckApp, dr: i64, dc: i64) {
    let prev_row = app.scroll_row;
    let prev_col = app.scroll_col;
    let new_row = (app.scroll_row as i64 + dr).max(0) as u8;
    let new_col = (app.scroll_col as i64 + dc).max(0) as u8;
    app.scroll_row = new_row;
    app.scroll_col = new_col;
    app.clamp_scroll();
    if app.scroll_row == prev_row && app.scroll_col == prev_col {
        // Nothing changed (clamped at edge). Still emit the event so widgets can show
        // a "can't scroll past" hint if they want to.
        app.bus.publish(
            "action.scroll",
            serde_json::json!({
                "page": &app.active_page,
                "row": app.scroll_row,
                "col": app.scroll_col,
                "logical_rows": app.logical_rows,
                "logical_cols": app.logical_cols,
                "clamped": true,
            }),
        );
        return;
    }
    if let Err(e) = app.rebuild_physical_binding() {
        tracing::warn!("scroll: rebuild binding failed: {}", e);
        return;
    }
    app.save_scroll_offset();
    app.bus.publish(
        "action.scroll",
        serde_json::json!({
            "page": &app.active_page,
            "row": app.scroll_row,
            "col": app.scroll_col,
            "logical_rows": app.logical_rows,
            "logical_cols": app.logical_cols,
            "clamped": false,
        }),
    );
    emit_page_appear(app);
}

fn switch_page(app: &mut DeckApp, next: String) {
    if next == app.active_page {
        return;
    }
    app.page_history.push(app.active_page.clone());
    set_page(app, next);
}

fn set_page(app: &mut DeckApp, page: String) {
    app.active_page = page;
    app.state.set_last_active_page(app.active_page.clone());
    if let Err(e) = app.bind_active_page() {
        tracing::warn!("page switch: bind failed: {}", e);
        return;
    }
    app.queue_top_layout_for_active_page();
    emit_page_appear(app);
}

fn json_to_toml(v: &serde_json::Value) -> Option<toml::Value> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(toml::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml::Value::Integer(i))
            } else {
                n.as_f64().map(toml::Value::Float)
            }
        }
        serde_json::Value::String(s) => Some(toml::Value::String(s.clone())),
        serde_json::Value::Array(a) => Some(toml::Value::Array(
            a.iter().filter_map(json_to_toml).collect(),
        )),
        serde_json::Value::Object(m) => {
            let mut t = toml::Table::new();
            for (k, v) in m {
                if let Some(tv) = json_to_toml(v) {
                    t.insert(k.clone(), tv);
                }
            }
            Some(toml::Value::Table(t))
        }
    }
}

/// Call on_will_appear for all bound actions once, at app start and on page switch.
pub fn emit_page_appear(app: &mut DeckApp) {
    let keys: Vec<(u8, u8)> = app.bound_actions.keys().copied().collect();
    for (r, c) in keys {
        let bound = app.bound_actions.get_mut(&(r, c)).unwrap();
        let tile_state = &mut app.tiles[(r as usize) * 4 + c as usize];
        let env = app
            .config
            .active_profile()
            .ok()
            .map(|p| p.meta.env.clone())
            .unwrap_or_default();
        let binding_id = bound.binding_id.clone();
        let mut cx = crate::action::ActionCx {
            cell: (r, c),
            binding_id: &binding_id,
            tile: TileHandle::new(tile_state),
            env: &env,
            bus: &app.bus,
            state: &mut app.state,
            rt: &app.rt,
        };
        bound.action.on_will_appear(&mut cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::builtin::{
        deck_page_back::DeckPageBack, deck_page_goto::DeckPageGoto,
        deck_profile_switch::DeckProfileSwitch, shell_run::ShellRun, toggle_cycle_n::ToggleCycleN,
        toggle_onoff::ToggleOnoff,
    };
    use crate::action::{Action, ActionKind, BuildFromArgs};

    #[test]
    fn default_action_kind_is_action() {
        let mut args = toml::Table::new();
        args.insert("cmd".into(), toml::Value::String("true".into()));
        let a = ShellRun::from_args(&args).unwrap();
        assert_eq!(a.kind(), ActionKind::Action);
    }

    #[test]
    fn deck_nav_actions_report_nav_kind() {
        let mut args = toml::Table::new();
        args.insert("page".into(), toml::Value::String("home".into()));
        assert_eq!(
            DeckPageGoto::from_args(&args).unwrap().kind(),
            ActionKind::Nav
        );

        assert_eq!(
            DeckPageBack::from_args(&toml::Table::new()).unwrap().kind(),
            ActionKind::Nav
        );

        let mut args = toml::Table::new();
        args.insert("profile".into(), toml::Value::String("p".into()));
        assert_eq!(
            DeckProfileSwitch::from_args(&args).unwrap().kind(),
            ActionKind::Nav
        );
    }

    #[test]
    fn toggle_actions_report_toggle_kind() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("mute".into()));
        assert_eq!(
            ToggleOnoff::from_args(&args).unwrap().kind(),
            ActionKind::Toggle
        );

        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("mode".into()));
        args.insert("count".into(), toml::Value::Integer(3));
        assert_eq!(
            ToggleCycleN::from_args(&args).unwrap().kind(),
            ActionKind::Toggle
        );
    }

    /// Exercise the paint_tile color helpers for each kind × each theme. We can't exercise
    /// the egui Ui branches without a real frame, so we prove the pure helpers produce
    /// reasonable output and don't panic. Also covers the Frappe theme.
    #[test]
    fn paint_tile_helpers_no_panic_across_kinds_and_themes() {
        for theme in [
            crate::theme::Theme::mocha(),
            crate::theme::Theme::latte(),
            crate::theme::Theme::frappe(),
        ] {
            for kind in [ActionKind::Action, ActionKind::Nav, ActionKind::Toggle] {
                let a = match kind {
                    ActionKind::Nav => theme.accent_alt,
                    ActionKind::Toggle => theme.info,
                    ActionKind::Action => theme.accent,
                };
                let l = lighten(theme.surface0, 0.08);
                let t = tint(theme.surface0, a, 0.22);
                assert!(l.r() >= theme.surface0.r());
                assert!(l.g() >= theme.surface0.g());
                assert!(l.b() >= theme.surface0.b());
                let (ar, br) = (a.r() as i32, theme.surface0.r() as i32);
                let (lo, hi) = (ar.min(br), ar.max(br));
                assert!((t.r() as i32) >= lo - 1 && (t.r() as i32) <= hi + 1);
            }
        }
    }
}
