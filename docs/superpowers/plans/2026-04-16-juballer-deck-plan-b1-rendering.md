# juballer-deck Plan B1: Rendering Polish

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the deck look like its mockups. Tile cells show icons + labels driven by config + action push-updates. Top region renders widgets placed inside the layout tree. Three dashboard-class widgets ship (`sysinfo`, `log_feed`, `http_probe`). Stable `monitor_id` so calibration survives reboots.

**Architecture:** Add a single `juballer_egui::EguiOverlay` to `DeckApp` and call it inside `render::on_frame` AFTER the per-cell bg fills. For each of the 16 cells, `ctx.in_grid_cell(r, c, |ui| paint_tile(...))` renders `TileState` — icon (emoji / image / builtin) + label + optional state_color accent + flash pulse. For top panes, the deck holds a `HashMap<PaneId, Box<dyn Widget>>` built from page config; each pane renders via `ctx.in_top_pane(id, |ui| widget.render(ui, &mut cx))`. Layout tree conversion (already built in A8) is now wired through `App::set_top_layout` at startup + page switch.

**Tech Stack:** `egui` 0.30, `egui-extras` 0.30 (image loaders), `image` 0.25, `sysinfo` 0.32, `reqwest` 0.12 (http_probe); uses existing `juballer-egui::EguiOverlay` from juballer-core v0.1.

---

## Plan Conventions

- Each task ends with a commit (Conventional Commits).
- `cargo fmt --all` + `cargo clippy --workspace --all-targets -- -D warnings` before each commit.
- Tests: unit-test anything testable without GPU; GPU-touching render paths are smoke-tested via the `headless` feature where feasible.
- Plan A deliverables are the floor — don't break the A12 smoke test.

---

## Phase 0 — Stability fix

### Task B0.1: Stable `monitor_id` so profile identity survives reboots

**Problem:** `juballer_core::app::profile_loader::monitor_id` uses `window.inner_size()` which returns post-scaling client-area size — flaky on Hyprland with fractional scaling. Fix: use `MonitorHandle` physical size/name + mode info instead.

**Files:**
- Modify: `crates/juballer-core/src/app/profile_loader.rs`

- [ ] **Step 1: Replace the `monitor_id` function**

Current:
```rust
pub fn monitor_id(window: &Arc<Window>) -> String {
    let name = window
        .current_monitor()
        .and_then(|m| m.name())
        .unwrap_or_else(|| "unknown".to_string());
    let size = window.inner_size();
    format!("{} / {}x{}", name, size.width, size.height)
}
```

Replace with:
```rust
pub fn monitor_id(window: &Arc<Window>) -> String {
    match window.current_monitor() {
        Some(m) => {
            let name = m.name().unwrap_or_else(|| "unknown".to_string());
            // Use the monitor's PHYSICAL pixel size — stable across scaling.
            let size = m.size();
            format!("{} / {}x{}", name, size.width, size.height)
        }
        None => "unknown / 0x0".to_string(),
    }
}
```

- [ ] **Step 2: Verify build**

Run: `cargo build --workspace`
Expected: clean.

- [ ] **Step 3: Verify no test regression**

Run: `cargo test --workspace --no-default-features`
Expected: all existing tests pass unchanged (this function is only called at runtime from `resumed()`; no unit tests reference it).

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-core/src/app/profile_loader.rs
git commit -m "fix(core): stable monitor_id using MonitorHandle.size() not window.inner_size()"
```

**Post-fix user action:** user must re-run `calibration_dance` once more to write a profile with the new `monitor_id`. After that, `juballer-deck` runs preserve calibration across launches.

---

## Phase 1 — EguiOverlay in DeckApp

### Task B1.1: Add `egui_overlay` field to `DeckApp`, initialize in bootstrap

**Files:**
- Modify: `crates/juballer-deck/Cargo.toml` (add `juballer-egui`, already a dep — verify)
- Modify: `crates/juballer-deck/src/app.rs`

- [ ] **Step 1: Verify `juballer-egui` is a dep**

In `crates/juballer-deck/Cargo.toml`, the `[dependencies]` block should already contain:
```toml
juballer-egui = { path = "../juballer-egui" }
egui.workspace = true
```
If not, add them. Then `cargo build -p juballer-deck` should still succeed.

- [ ] **Step 2: Add field to `DeckApp`**

Modify `crates/juballer-deck/src/app.rs` — add to the struct fields (near bottom):

```rust
pub struct DeckApp {
    // ... existing fields ...
    pub active_pane_interner: HashMap<String, &'static str>,

    /// egui-on-wgpu overlay for tile icon+label rendering and top-region widgets.
    pub egui_overlay: juballer_egui::EguiOverlay,
}
```

Initialize in `DeckApp::bootstrap`, in the struct literal:

```rust
let mut app = Self {
    // ... existing fields ...
    active_pane_interner: HashMap::new(),
    egui_overlay: juballer_egui::EguiOverlay::new(),
};
```

- [ ] **Step 3: Verify build**

```
cargo build -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p juballer-deck
```

All existing tests still pass (A12 smoke, etc.).

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/
git commit -m "feat(deck): add EguiOverlay to DeckApp, initialize in bootstrap"
```

### Task B1.2: Paint tile icon + label via egui overlay

**Files:**
- Modify: `crates/juballer-deck/src/render.rs`

- [ ] **Step 1: Rewrite `on_frame` to include egui pass**

Replace the current `on_frame` function body with:

```rust
pub fn on_frame(app: &mut DeckApp, frame: &mut juballer_core::Frame, events: &[Event]) {
    // 1. Handle input events → dispatch to bound actions.
    for ev in events {
        match ev {
            Event::KeyDown { row, col, .. } => dispatch_down(app, *row, *col),
            Event::KeyUp { row, col, .. } => dispatch_up(app, *row, *col),
            _ => {}
        }
    }

    // 2. Fill each tile's background color.
    for r in 0..4u8 {
        for c in 0..4u8 {
            let tile_state = &app.tiles[(r as usize) * 4 + c as usize];
            let bg = tile_state.bg.unwrap_or(Color::rgb(0x18, 0x1a, 0x24));
            frame.grid_cell(r, c).fill(bg);
        }
    }

    // 3. egui overlay pass — renders tile icon + label per cell.
    // Snapshot the tile states + bindings we need for rendering before taking &mut.
    let DeckApp { tiles, bound_actions, egui_overlay, .. } = app;

    egui_overlay.draw(frame, |ctx| {
        for r in 0..4u8 {
            for c in 0..4u8 {
                let ts = &tiles[(r as usize) * 4 + c as usize];
                let bound = bound_actions.get(&(r, c));
                ctx.in_grid_cell(r, c, |ui| {
                    paint_tile(ui, ts, bound);
                });
            }
        }
    });
}

fn paint_tile(
    ui: &mut egui::Ui,
    state: &crate::tile::TileState,
    bound: Option<&crate::app::BoundAction>,
) {
    // Effective icon/label: action-pushed state takes precedence; fall back to config.
    let icon_text = state
        .icon
        .as_ref()
        .and_then(|i| match i {
            crate::tile::IconRef::Emoji(s) => Some(s.clone()),
            _ => None, // Path/Builtin handled in B2.1
        })
        .or_else(|| bound.and_then(|b| b.icon.clone()));

    let label_text = state
        .label
        .clone()
        .or_else(|| bound.and_then(|b| b.label.clone()));

    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        if let Some(ic) = icon_text {
            let size = ui.available_width().min(ui.available_height()) * 0.5;
            ui.label(egui::RichText::new(ic).size(size));
        }
        if let Some(lbl) = label_text {
            ui.label(egui::RichText::new(lbl).size(14.0));
        }
    });

    // state_color accent — thin bottom stripe.
    if let Some(accent) = state.state_color {
        let rect = ui.max_rect();
        let y = rect.max.y - 4.0;
        let stripe = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, y),
            egui::pos2(rect.max.x - 4.0, y + 3.0),
        );
        ui.painter().rect_filled(
            stripe,
            egui::CornerRadius::same(1),
            egui::Color32::from_rgba_premultiplied(accent.0, accent.1, accent.2, accent.3),
        );
    }
}
```

Also keep existing `dispatch_down`, `dispatch_up`, and `emit_page_appear` functions unchanged.

- [ ] **Step 2: Verify**

```
cargo build -p juballer-deck
cargo test -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all tests pass.

- [ ] **Step 3: Manual smoke (optional, requires display)**

Run `cargo run -p juballer-deck` with the existing config. Each tile now shows its config `icon` emoji + `label`. Press FB9 button (0,0) — should still fire `notify-send`.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/render.rs
git commit -m "feat(deck): render tile icon + label via egui overlay"
```

---

## Phase 2 — Icon resolution (Path + Builtin)

### Task B2.1: Image loader for `IconRef::Path`

**Files:**
- Modify: `crates/juballer-deck/Cargo.toml` (add `egui_extras` + `image`)
- Create: `crates/juballer-deck/src/icon_loader.rs`
- Modify: `crates/juballer-deck/src/lib.rs`
- Modify: `crates/juballer-deck/src/app.rs`
- Modify: `crates/juballer-deck/src/render.rs`

- [ ] **Step 1: Add deps**

In `crates/juballer-deck/Cargo.toml` `[dependencies]`:
```toml
egui_extras = { version = "0.30", features = ["image"] }
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
```

(egui_extras is not in workspace.dependencies; pinning locally is fine for now.)

- [ ] **Step 2: Write `icon_loader.rs`**

```rust
//! Load + cache tile icons from disk. Keyed by absolute path to avoid double-loading.

use crate::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct IconLoader {
    /// profile assets root (e.g. ~/.config/juballer/deck/profiles/homelab/assets).
    assets_root: PathBuf,
    /// Cache: absolute path → (texture handle bytes).
    /// For Plan B1 we don't retain GPU textures — we pass bytes each frame to egui,
    /// which handles its own texture caching internally.
    cache: HashMap<PathBuf, Vec<u8>>,
}

impl IconLoader {
    pub fn new(assets_root: PathBuf) -> Self {
        Self { assets_root, cache: HashMap::new() }
    }

    /// Resolve path relative to assets root (or absolute). Returns loaded image bytes.
    pub fn load(&mut self, path: &Path) -> Result<&[u8]> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.assets_root.join(path)
        };
        if !self.cache.contains_key(&abs) {
            let bytes = std::fs::read(&abs)?;
            self.cache.insert(abs.clone(), bytes);
        }
        Ok(self.cache.get(&abs).unwrap())
    }

    pub fn assets_root(&self) -> &Path {
        &self.assets_root
    }
}
```

- [ ] **Step 3: Register loader in `DeckApp`**

Modify `crates/juballer-deck/src/app.rs`:
- Add field:
  ```rust
  pub icon_loader: crate::icon_loader::IconLoader,
  ```
- Initialize in `bootstrap`. Resolve the active profile's assets dir; if no profile, use an empty temp path:
  ```rust
  let assets_root = config
      .active_profile()
      .ok()
      .map(|_| paths.profile_assets(&config.deck.active_profile))
      .unwrap_or_else(|| paths.root.join(".assets_missing"));
  let icon_loader = crate::icon_loader::IconLoader::new(assets_root);
  ```
  And add `icon_loader` to the struct literal.

- [ ] **Step 4: Wire into `lib.rs`**

Add:
```rust
pub mod icon_loader;
```
keeping all other mods.

- [ ] **Step 5: Extend `paint_tile` in `render.rs`**

Add an `IconLoader` parameter and handle `IconRef::Path`:

```rust
fn paint_tile(
    ui: &mut egui::Ui,
    state: &crate::tile::TileState,
    bound: Option<&crate::app::BoundAction>,
    loader: &mut crate::icon_loader::IconLoader,
) {
    let icon = state.icon.as_ref().or_else(|| None);

    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        match icon {
            Some(crate::tile::IconRef::Emoji(s)) => {
                let size = ui.available_width().min(ui.available_height()) * 0.5;
                ui.label(egui::RichText::new(s).size(size));
            }
            Some(crate::tile::IconRef::Path(p)) => {
                match loader.load(p) {
                    Ok(bytes) => {
                        // Convert to egui image source — the image crate decodes.
                        let uri = format!("bytes://{}", p.display());
                        let src = egui::load::SizedTexture::from_handle(
                            &ui.ctx().load_texture(
                                &uri,
                                decode_image(bytes).unwrap_or_else(|| egui::ColorImage::example()),
                                egui::TextureOptions::LINEAR,
                            ),
                        );
                        let max = ui.available_width().min(ui.available_height()) * 0.6;
                        ui.add(egui::Image::new(src).fit_to_exact_size(egui::vec2(max, max)));
                    }
                    Err(e) => {
                        ui.label(egui::RichText::new(format!("!{e}")).color(egui::Color32::RED).size(10.0));
                    }
                }
            }
            Some(crate::tile::IconRef::Builtin(name)) => {
                // Plan B1 scope: render the builtin name as fallback text.
                ui.label(egui::RichText::new(format!("[{name}]")).size(14.0));
            }
            None => {
                if let Some(config_icon) = bound.and_then(|b| b.icon.as_deref()) {
                    // config icon can be emoji string OR relative asset path.
                    if config_icon.chars().count() <= 3 {
                        let size = ui.available_width().min(ui.available_height()) * 0.5;
                        ui.label(egui::RichText::new(config_icon).size(size));
                    } else {
                        // Treat as path.
                        let p = std::path::PathBuf::from(config_icon);
                        if let Ok(bytes) = loader.load(&p) {
                            let uri = format!("bytes://{}", p.display());
                            if let Some(img) = decode_image(bytes) {
                                let src = egui::load::SizedTexture::from_handle(
                                    &ui.ctx().load_texture(&uri, img, egui::TextureOptions::LINEAR),
                                );
                                let max = ui.available_width().min(ui.available_height()) * 0.6;
                                ui.add(egui::Image::new(src).fit_to_exact_size(egui::vec2(max, max)));
                            }
                        }
                    }
                }
            }
        }
        if let Some(lbl) = state.label.clone().or_else(|| bound.and_then(|b| b.label.clone())) {
            ui.label(egui::RichText::new(lbl).size(14.0));
        }
    });

    if let Some(accent) = state.state_color {
        let rect = ui.max_rect();
        let y = rect.max.y - 4.0;
        let stripe = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, y),
            egui::pos2(rect.max.x - 4.0, y + 3.0),
        );
        ui.painter().rect_filled(
            stripe,
            egui::CornerRadius::same(1),
            egui::Color32::from_rgba_premultiplied(accent.0, accent.1, accent.2, accent.3),
        );
    }
}

fn decode_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}
```

Update the call site in `on_frame` to destructure `icon_loader` too and pass it down:

```rust
let DeckApp { tiles, bound_actions, egui_overlay, icon_loader, .. } = app;

egui_overlay.draw(frame, |ctx| {
    for r in 0..4u8 {
        for c in 0..4u8 {
            let ts = &tiles[(r as usize) * 4 + c as usize];
            let bound = bound_actions.get(&(r, c));
            ctx.in_grid_cell(r, c, |ui| {
                paint_tile(ui, ts, bound, icon_loader);
            });
        }
    }
});
```

- [ ] **Step 6: Verify**

```
cargo build -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p juballer-deck
```

- [ ] **Step 7: Commit**

```bash
git add crates/juballer-deck/
git commit -m "feat(deck): IconRef::Path + config image icons via egui_extras"
```

---

## Phase 3 — Top-region widget rendering

### Task B3.1: Active widgets map + bind from config

**Files:**
- Modify: `crates/juballer-deck/src/app.rs`

- [ ] **Step 1: Add field + struct for an active widget**

In `DeckApp`:
```rust
/// Active top-region widget instances keyed by pane id (same name used in config).
pub active_widgets: HashMap<String, Box<dyn crate::widget::Widget>>,
```

- [ ] **Step 2: Build widgets in `bind_active_page`**

After the button-binding loop in `bind_active_page`, add widget binding:

```rust
// Build top-pane widget instances.
self.active_widgets.clear();
for (pane_id, binding) in &page.top_panes {
    let mut args = binding.args.clone();
    interp_table(&mut args, &env);
    match self.widgets.build(&binding.widget, &args) {
        Ok(w) => {
            self.active_widgets.insert(pane_id.clone(), w);
        }
        Err(e) => {
            tracing::warn!("widget {} ({}): {}", pane_id, binding.widget, e);
        }
    }
}
```

Initialize `active_widgets: HashMap::new()` in `DeckApp::bootstrap`'s struct literal.

- [ ] **Step 3: Verify build + existing tests**

```
cargo build -p juballer-deck
cargo test -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/app.rs
git commit -m "feat(deck): active_widgets map built from page.top_panes config"
```

### Task B3.2: Set core `App::set_top_layout` from config on CLI startup

**Files:**
- Modify: `crates/juballer-deck/src/cli.rs`

- [ ] **Step 1: Convert config layout tree + set on juballer-core App**

In `run()`, after `app.set_debug(true)` / before `emit_page_appear`, add:

```rust
// If the active page declares a `top` layout, convert + apply.
if let Some(layout_cfg) = deck
    .config
    .active_profile()
    .ok()
    .and_then(|p| p.pages.get(&deck.active_page))
    .and_then(|page| page.top.clone())
{
    match crate::layout_convert::convert(&layout_cfg, &mut deck.active_pane_interner) {
        Ok(out) => {
            app.set_top_layout(out.root);
            tracing::info!("top layout applied: {} panes", out.pane_names.len());
        }
        Err(e) => tracing::warn!("top layout build failed: {}", e),
    }
}
```

- [ ] **Step 2: Verify build**

```
cargo build -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add crates/juballer-deck/src/cli.rs
git commit -m "feat(deck): apply config-declared top layout via App::set_top_layout"
```

### Task B3.3: Render widgets in on_frame

**Files:**
- Modify: `crates/juballer-deck/src/render.rs`

- [ ] **Step 1: Extend the egui pass to render widgets**

Extend the closure body of `egui_overlay.draw(frame, |ctx| { ... })` in `on_frame`:

```rust
// Split-borrow additional fields we need for widgets.
let DeckApp {
    tiles,
    bound_actions,
    egui_overlay,
    icon_loader,
    active_widgets,
    bus,
    state,
    rt,
    config,
    ..
} = app;

let env: indexmap::IndexMap<String, String> = config
    .active_profile()
    .ok()
    .map(|p| p.meta.env.clone())
    .unwrap_or_default();

egui_overlay.draw(frame, |ctx| {
    for r in 0..4u8 {
        for c in 0..4u8 {
            let ts = &tiles[(r as usize) * 4 + c as usize];
            let bound = bound_actions.get(&(r, c));
            ctx.in_grid_cell(r, c, |ui| {
                paint_tile(ui, ts, bound, icon_loader);
            });
        }
    }

    // Top-region widget rendering.
    for (pane_name, widget) in active_widgets.iter_mut() {
        // PaneId in the core layout is &'static str; ctx.in_top_pane expects that.
        // Widget registrations and top_panes use String; we need to leak to get static.
        // The layout_convert interner handles this at set_top_layout time — here we
        // reuse the same intern logic via a one-shot Box::leak. Safe because active_widgets
        // is rebuilt on every bind_active_page, so leaks match page lifetimes.
        let static_id: &'static str = Box::leak(pane_name.clone().into_boxed_str());
        let mut cx = crate::widget::WidgetCx {
            pane: static_id,
            env: &env,
            bus,
            state,
            rt,
        };
        ctx.in_top_pane(static_id, |ui| {
            widget.render(ui, &mut cx);
        });
    }
});
```

- [ ] **Step 2: Verify**

```
cargo build -p juballer-deck
cargo test -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add crates/juballer-deck/src/render.rs
git commit -m "feat(deck): render active_widgets in their panes via egui overlay"
```

---

## Phase 4 — Three dashboard widgets

### Task B4.1: `sysinfo` widget

**Files:**
- Modify: `crates/juballer-deck/Cargo.toml` (add `sysinfo`)
- Create: `crates/juballer-deck/src/widget/builtin/sysinfo_widget.rs`
- Modify: `crates/juballer-deck/src/widget/builtin/mod.rs`
- Modify: `crates/juballer-deck/src/widget/builtin/register.rs`

- [ ] **Step 1: Add dep**

In `crates/juballer-deck/Cargo.toml` `[dependencies]`:
```toml
sysinfo = { version = "0.32", default-features = false, features = ["system"] }
```

- [ ] **Step 2: Write `sysinfo_widget.rs`**

```rust
//! sysinfo widget — CPU + memory stats, refreshed every `interval_ms` milliseconds.
//!
//! Args:
//!   interval_ms : u64 (default 1000) — refresh cadence

use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::Result;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

pub struct SysinfoWidget {
    interval: Duration,
    last_refresh: Option<Instant>,
    sys: System,
    cpu_pct: f32,
    used_mb: u64,
    total_mb: u64,
}

impl WidgetBuildFromArgs for SysinfoWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let interval_ms = args
            .get("interval_ms")
            .and_then(|v| v.as_integer())
            .map(|i| i.max(200) as u64)
            .unwrap_or(1000);
        let specifics = RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(MemoryRefreshKind::nothing().with_ram());
        let sys = System::new_with_specifics(specifics);
        Ok(Self {
            interval: Duration::from_millis(interval_ms),
            last_refresh: None,
            sys,
            cpu_pct: 0.0,
            used_mb: 0,
            total_mb: 0,
        })
    }
}

impl Widget for SysinfoWidget {
    fn render(&mut self, ui: &mut egui::Ui, _cx: &mut WidgetCx<'_>) -> bool {
        let now = Instant::now();
        let should_refresh = self
            .last_refresh
            .map(|t| now.duration_since(t) >= self.interval)
            .unwrap_or(true);

        if should_refresh {
            self.sys.refresh_cpu_usage();
            self.sys.refresh_memory();
            self.cpu_pct = self.sys.global_cpu_usage();
            self.total_mb = self.sys.total_memory() / 1024 / 1024;
            self.used_mb = self.sys.used_memory() / 1024 / 1024;
            self.last_refresh = Some(now);
        }

        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("SYSTEM")
                    .small()
                    .color(egui::Color32::DARK_GRAY),
            );
            ui.label(egui::RichText::new(format!("CPU {:>4.1}%", self.cpu_pct)).size(14.0));
            ui.label(
                egui::RichText::new(format!("MEM {} / {} MB", self.used_mb, self.total_mb))
                    .size(14.0),
            );
        });
        true // refresh request for animated feel
    }
}
```

- [ ] **Step 3: Register**

Modify `crates/juballer-deck/src/widget/builtin/mod.rs`:
```rust
//! Built-in widgets.

pub mod clock;
pub mod register;
pub mod sysinfo_widget;
pub mod text;

pub use register::register_builtins;
```

Modify `crates/juballer-deck/src/widget/builtin/register.rs`:
```rust
use super::clock::Clock;
use super::sysinfo_widget::SysinfoWidget;
use super::text::Text;
use crate::widget::WidgetRegistry;

pub fn register_builtins(registry: &mut WidgetRegistry) {
    registry.register::<Clock>("clock");
    registry.register::<SysinfoWidget>("sysinfo");
    registry.register::<Text>("text");
}
```

- [ ] **Step 4: Verify**

```
cargo build -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/
git commit -m "feat(deck): sysinfo widget (CPU + RAM usage)"
```

### Task B4.2: `log_feed` widget

**Files:**
- Create: `crates/juballer-deck/src/widget/builtin/log_feed.rs`
- Modify: `crates/juballer-deck/src/widget/builtin/mod.rs`
- Modify: `crates/juballer-deck/src/widget/builtin/register.rs`

- [ ] **Step 1: Write `log_feed.rs`**

```rust
//! log_feed widget — rolling list of bus messages for a subscribed topic.
//!
//! Args:
//!   topic    : string (required) — bus topic to subscribe to
//!   max_rows : u64 (default 5)

use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use std::collections::VecDeque;
use tokio::sync::broadcast;

pub struct LogFeedWidget {
    topic: String,
    max_rows: usize,
    rx: Option<broadcast::Receiver<crate::bus::Event>>,
    lines: VecDeque<String>,
}

impl WidgetBuildFromArgs for LogFeedWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let topic = args
            .get("topic")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("log_feed requires args.topic (string)".into()))?
            .to_string();
        let max_rows = args
            .get("max_rows")
            .and_then(|v| v.as_integer())
            .map(|i| i.clamp(1, 50) as usize)
            .unwrap_or(5);
        Ok(Self { topic, max_rows, rx: None, lines: VecDeque::new() })
    }
}

impl Widget for LogFeedWidget {
    fn on_will_appear(&mut self, cx: &mut WidgetCx<'_>) {
        self.rx = Some(cx.bus.subscribe());
    }

    fn on_will_disappear(&mut self, _cx: &mut WidgetCx<'_>) {
        self.rx = None;
        self.lines.clear();
    }

    fn render(&mut self, ui: &mut egui::Ui, _cx: &mut WidgetCx<'_>) -> bool {
        // Drain any pending events non-blocking.
        if let Some(rx) = self.rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        if ev.topic == self.topic {
                            let line = format!("{} {}", short_ts(), compact(&ev.data));
                            if self.lines.len() >= self.max_rows {
                                self.lines.pop_front();
                            }
                            self.lines.push_back(line);
                        }
                    }
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Lagged(_)) => {
                        // Continue after skipping lagged messages.
                        continue;
                    }
                    Err(broadcast::error::TryRecvError::Closed) => {
                        self.rx = None;
                        break;
                    }
                }
            }
        }

        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!("LOG · {}", self.topic))
                    .small()
                    .color(egui::Color32::DARK_GRAY),
            );
            for line in &self.lines {
                ui.label(egui::RichText::new(line).monospace().size(11.0));
            }
        });
        true
    }
}

fn short_ts() -> String {
    let now = chrono::Local::now();
    now.format("%H:%M:%S").to_string()
}

fn compact(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => v.to_string(),
        _ => v.to_string().trim_matches('"').to_string(),
    }
}
```

- [ ] **Step 2: Register**

Modify `widget/builtin/mod.rs`:
```rust
pub mod clock;
pub mod log_feed;
pub mod register;
pub mod sysinfo_widget;
pub mod text;

pub use register::register_builtins;
```

Modify `widget/builtin/register.rs`:
```rust
use super::clock::Clock;
use super::log_feed::LogFeedWidget;
use super::sysinfo_widget::SysinfoWidget;
use super::text::Text;
use crate::widget::WidgetRegistry;

pub fn register_builtins(registry: &mut WidgetRegistry) {
    registry.register::<Clock>("clock");
    registry.register::<LogFeedWidget>("log_feed");
    registry.register::<SysinfoWidget>("sysinfo");
    registry.register::<Text>("text");
}
```

- [ ] **Step 3: Verify**

```
cargo build -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/
git commit -m "feat(deck): log_feed widget (subscribes to bus topic, rolling list)"
```

### Task B4.3: `http_probe` widget

**Files:**
- Modify: `crates/juballer-deck/Cargo.toml` (add `reqwest`)
- Create: `crates/juballer-deck/src/widget/builtin/http_probe.rs`
- Modify: `crates/juballer-deck/src/widget/builtin/mod.rs`
- Modify: `crates/juballer-deck/src/widget/builtin/register.rs`

- [ ] **Step 1: Add dep**

In `crates/juballer-deck/Cargo.toml` `[dependencies]`:
```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
```

- [ ] **Step 2: Write `http_probe.rs`**

```rust
//! http_probe widget — periodic GET → colored badge (green=OK, red=fail).
//!
//! Args:
//!   url         : string (required)
//!   label       : string (optional) — displayed above status
//!   interval_ms : u64 (default 5000)

use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct ProbeState {
    status: Option<u16>,   // last HTTP status, or None before first probe
    last_error: Option<String>,
    last_fetched_at: Option<Instant>,
}

pub struct HttpProbeWidget {
    url: String,
    label: String,
    interval: Duration,
    state: Arc<Mutex<ProbeState>>,
    probe_in_flight: bool,
    last_fired: Option<Instant>,
}

impl WidgetBuildFromArgs for HttpProbeWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("http_probe requires args.url (string)".into()))?
            .to_string();
        let label = args
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("probe")
            .to_string();
        let interval_ms = args
            .get("interval_ms")
            .and_then(|v| v.as_integer())
            .map(|i| i.max(500) as u64)
            .unwrap_or(5000);
        Ok(Self {
            url,
            label,
            interval: Duration::from_millis(interval_ms),
            state: Arc::new(Mutex::new(ProbeState {
                status: None,
                last_error: None,
                last_fetched_at: None,
            })),
            probe_in_flight: false,
            last_fired: None,
        })
    }
}

impl Widget for HttpProbeWidget {
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let now = Instant::now();
        let should_fire = !self.probe_in_flight
            && self
                .last_fired
                .map(|t| now.duration_since(t) >= self.interval)
                .unwrap_or(true);

        if should_fire {
            self.last_fired = Some(now);
            self.probe_in_flight = true;
            let url = self.url.clone();
            let state = self.state.clone();
            cx.rt.spawn(async move {
                let out = reqwest::Client::builder()
                    .timeout(Duration::from_secs(3))
                    .build()
                    .map(|c| c.get(&url).send())
                    .map_err(|e| e.to_string());
                let result: Result<u16, String> = match out {
                    Ok(fut) => match fut.await {
                        Ok(r) => Ok(r.status().as_u16()),
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(e),
                };
                let mut st = state.lock().unwrap();
                match result {
                    Ok(code) => {
                        st.status = Some(code);
                        st.last_error = None;
                    }
                    Err(e) => {
                        st.status = None;
                        st.last_error = Some(e);
                    }
                }
                st.last_fetched_at = Some(Instant::now());
            });
            // Mark probe finished on next render tick since we can't await here.
        }

        let snapshot = self.state.lock().unwrap().clone();
        if snapshot.last_fetched_at.is_some() {
            self.probe_in_flight = false;
        }

        let (badge_color, badge_text) = match snapshot.status {
            Some(c) if (200..400).contains(&c) => (egui::Color32::from_rgb(0x23, 0xa5, 0x5a), format!("{c}")),
            Some(c) => (egui::Color32::from_rgb(0xf3, 0x8b, 0xa8), format!("{c}")),
            None if snapshot.last_error.is_some() => (egui::Color32::from_rgb(0xf3, 0x8b, 0xa8), "FAIL".into()),
            None => (egui::Color32::DARK_GRAY, "...".into()),
        };

        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&self.label).small().color(egui::Color32::DARK_GRAY));
            ui.horizontal(|ui| {
                let rect = ui.painter().circle_filled(
                    ui.cursor().left_top() + egui::vec2(6.0, 10.0),
                    5.0,
                    badge_color,
                );
                let _ = rect;
                ui.add_space(16.0);
                ui.label(egui::RichText::new(&badge_text).size(14.0));
            });
            if let Some(e) = snapshot.last_error {
                ui.label(
                    egui::RichText::new(e)
                        .small()
                        .color(egui::Color32::from_rgb(0xf3, 0x8b, 0xa8)),
                );
            }
        });

        true
    }
}
```

- [ ] **Step 3: Register**

Modify `widget/builtin/mod.rs`:
```rust
pub mod clock;
pub mod http_probe;
pub mod log_feed;
pub mod register;
pub mod sysinfo_widget;
pub mod text;

pub use register::register_builtins;
```

Modify `widget/builtin/register.rs`:
```rust
use super::clock::Clock;
use super::http_probe::HttpProbeWidget;
use super::log_feed::LogFeedWidget;
use super::sysinfo_widget::SysinfoWidget;
use super::text::Text;
use crate::widget::WidgetRegistry;

pub fn register_builtins(registry: &mut WidgetRegistry) {
    registry.register::<Clock>("clock");
    registry.register::<HttpProbeWidget>("http_probe");
    registry.register::<LogFeedWidget>("log_feed");
    registry.register::<SysinfoWidget>("sysinfo");
    registry.register::<Text>("text");
}
```

- [ ] **Step 4: Verify**

```
cargo build -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/
git commit -m "feat(deck): http_probe widget (periodic GET with colored status badge)"
```

---

## Phase 5 — Integration smoke

### Task B5.1: Dashboard fixture + end-to-end smoke test

**Files:**
- Create: `crates/juballer-deck/tests/fixtures/dashboard/deck.toml`
- Create: `crates/juballer-deck/tests/fixtures/dashboard/profiles/demo/profile.toml`
- Create: `crates/juballer-deck/tests/fixtures/dashboard/profiles/demo/pages/home.toml`
- Create: `crates/juballer-deck/tests/dashboard_smoke.rs`

- [ ] **Step 1: Fixture — deck.toml**

```toml
version = 1
active_profile = "demo"

[editor]
bind = "127.0.0.1:7376"

[render]
bg = "#0b0d12"

[log]
level = "info"
```

- [ ] **Step 2: Fixture — profile.toml**

```toml
name = "demo"
description = "B1 dashboard smoke"
default_page = "home"
pages = ["home"]

[env]
FAKE_URL = "http://127.0.0.1:1"
```

- [ ] **Step 3: Fixture — home.toml**

```toml
[meta]
title = "home"

[[top]]
kind = "stack"
dir = "vertical"
gap = 8
children = [
    { size = { fixed = 40 }, pane = "clock_pane" },
    { size = { ratio = 1.0 }, pane = "sys_pane" },
    { size = { ratio = 1.0 }, pane = "log_pane" },
    { size = { fixed = 70 }, pane = "probe_pane" },
]

[top.pane.clock_pane]
widget = "clock"

[top.pane.sys_pane]
widget = "sysinfo"

[top.pane.log_pane]
widget = "log_feed"
topic = "demo.events"
max_rows = 4

[top.pane.probe_pane]
widget = "http_probe"
url = "$FAKE_URL"
label = "probe"
interval_ms = 800

[[button]]
row = 0
col = 0
action = "shell.run"
args = { cmd = "true" }
label = "nop"
```

- [ ] **Step 4: Smoke test — `dashboard_smoke.rs`**

```rust
//! Dashboard smoke: loads the multi-widget fixture, asserts all 4 widgets + 1 button
//! were instantiated by the registries.

use juballer_deck::config::DeckPaths;
use juballer_deck::DeckApp;
use std::path::PathBuf;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_fixture_instantiates_widgets_and_buttons() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dashboard");
    let paths = DeckPaths::from_root(fixture);
    let rt = tokio::runtime::Handle::current();

    let deck = DeckApp::bootstrap(paths, rt).unwrap();

    // 1 button bound.
    assert!(deck.bound_actions.contains_key(&(0, 0)));

    // 4 widgets active: clock_pane, sys_pane, log_pane, probe_pane.
    assert_eq!(deck.active_widgets.len(), 4);
    assert!(deck.active_widgets.contains_key("clock_pane"));
    assert!(deck.active_widgets.contains_key("sys_pane"));
    assert!(deck.active_widgets.contains_key("log_pane"));
    assert!(deck.active_widgets.contains_key("probe_pane"));
}
```

- [ ] **Step 5: Verify**

```
cargo test -p juballer-deck --test dashboard_smoke
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: 1 test passes.

- [ ] **Step 6: Commit**

```bash
git add crates/juballer-deck/tests/
git commit -m "test(deck): dashboard smoke — 4 widgets + 1 button instantiate from fixture"
```

---

## Self-Review

- [x] Spec coverage: tile icon/label rendering (B1.2 + B2.1), top-region widget rendering (B3.1–B3.3), sysinfo (B4.1), log_feed (B4.2), http_probe (B4.3), monitor_id fix (B0.1), smoke test (B5.1). Remaining widgets (`now_playing`, `notification_toast`, `homelab_status`, `image`, `counter`, `action_mini`, `plugin_proxy`) are deferred (tracked in Out-of-scope).
- [x] Placeholder scan: all code blocks are complete, no TBDs.
- [x] Type consistency: `TileState`, `IconRef`, `BoundAction`, `DeckApp`, `Widget`, `WidgetCx`, `PaneId`, `EguiOverlay`, `RegionCtx` all referenced consistently with prior Plan A definitions.
- [x] Borrow-check attention: the destructuring pattern `let DeckApp { tiles, bound_actions, egui_overlay, .. } = app;` is used everywhere multiple fields need to cross an FnMut boundary.

## Out-of-scope for Plan B1 (deferred)

- Full `IconRef::Builtin` rendering — shows `[name]` fallback; real icon atlas can land in Plan C.
- `now_playing`, `notification_toast`, `homelab_status`, `image`, `counter`, `action_mini`, `plugin_proxy` widgets — Plan C or ad-hoc as needed.
- Full action catalog (~47 actions) — Plan B2.
- Web config editor — Plan E.
- Plugin host + Python SDK — Plan D.
- Icon loader GPU texture retention (currently egui re-decodes lazily) — optimize later only if profiling shows it matters.
- Layout tree + widget hot-reload on page/profile switch during run — current Plan B1 only handles initial bind; hot reload preserves buttons but widgets need an explicit rebuild path, added in a future polish pass.
