# juballer-core — design spec

**Date:** 2026-04-16
**Status:** Approved (brainstorming complete)
**Scope:** Foundation library + two opt-in companion crates. A separate `juballer-deck` (Stream-Deck-like application) will be specced and built on top in a later cycle.

## Purpose

Build the orchestration foundation for the GAMO2 Rhythm-game **FB9** controller. The lib must:

1. Render a fullscreen display split into a configurable **top region** (arbitrary layout tree) and a **4×4 grid region** that aligns perfectly with the 16 physical buttons of the controller sitting on the lower half of the monitor.
2. Forward keyboard input from the controller to the application as raw, low-latency events tagged by physical grid position.
3. Be performant enough to support both Stream-Deck-style use **and** a future rhythm-game on the same controller.
4. Be unopinionated about how applications draw — expose raw GPU surfaces with optional egui convenience.

The lib does **not** ship: built-in actions, scripting, configuration UI, or any application-level logic. Those belong to `juballer-deck`.

## Target hardware and platforms

- **Controller:** GAMO2 FB9 — 4×4 transparent buttons, USB HID keyboard mode, no host-controlled LEDs.
- **Display:** single monitor, controller sits on lower half (transparent buttons let rendered content show through).
- **OS targets:** Linux (primary), Windows. No macOS in v0.1.
- **GPU:** anything wgpu supports (Vulkan/DX12/Metal/GL fallback).

## Workspace layout

```
juballer/                          (cargo workspace)
├── crates/
│   ├── juballer-core/             window, surfaces, layout, calibration, raw input
│   │   ├── Cargo.toml             deps: winit, wgpu, serde, toml, thiserror, raw-window-handle
│   │   └── src/
│   │       ├── lib.rs             public API: App, Frame, Surface, layout, profile
│   │       ├── window.rs          fullscreen window setup (Linux + Windows)
│   │       ├── geometry.rs        Calibration + Geometry math (mm/px conversion, tile rects, rotation)
│   │       ├── calibration/       defaults (FB9), profile load/save, interactive UI
│   │       ├── layout/            top-region layout tree (Stack/Pane nodes)
│   │       ├── render/            wgpu surface mgmt, offscreen FB, composite pass, RegionDraw
│   │       └── input/             default winit backend; raw-input backend behind feature
│   ├── juballer-egui/             optional: egui_wgpu integration helper
│   │   └── src/lib.rs             EguiOverlay { in_top_pane, in_grid_cell }
│   └── juballer-gestures/         optional: gesture recognizer
│       └── src/lib.rs             Recognizer that consumes raw events -> Tap/Hold/Chord/Swipe
└── Cargo.toml                     workspace
```

`juballer-egui` and `juballer-gestures` depend on `juballer-core`. Core depends on neither. A future `juballer-deck` consumes all three.

## Coordinate system + calibration

**Internal coordinates:** integer pixels, origin top-left of the fullscreen window.

**Calibration profile location:**
- Linux: `${XDG_CONFIG_HOME:-~/.config}/juballer/profile.toml`
- Windows: `%APPDATA%\juballer\profile.toml`

One profile per `(controller_id, monitor_id)` pair, where `controller_id = "VID:PID/SERIAL"` and `monitor_id = "<name> / <WxH>"`.

**Profile schema (TOML):**

```toml
[profile]
controller_id = "1234:5678/SN-ABCD"
monitor_id    = "DELL S2721DGF / 2560x1440"

[grid]
origin_px = { x = 320, y = 720 }
size_px   = { w = 1920, h = 720 }
gap_px    = 12
border_px = 4
rotation_deg = 0.0

[top]
margin_above_grid_px = 8

[keymap]
"0,0" = "KEY_W"
"0,1" = "KEY_E"
# ... 16 entries total (row 0 = top, col 0 = left)
```

**Default profile shipped for FB9** (`defaults/fb9.toml`):
- Geometry: `auto` (lib computes a centered square 4×4 fitting the lower 60 % of screen height).
- Keymap: empty / commented-out until real keycodes are dumped from a physical FB9.
- Rotation: 0°.

**Calibration UI** (built into core, runs on `App::run_calibration()` or first launch with no matching profile):

1. Fullscreen overlay draws the current grid + top region semi-transparently.
2. Four draggable corner handles on the grid; sliders for `gap_px`, `border_px`, `margin_above_grid_px`, `rotation_deg`.
3. Live "press a button to verify" cell highlight.
4. `Save` writes the profile, `Cancel` discards.

**Keymap auto-learn** (separate sub-flow inside calibration; also `App::run_keymap_auto_learn()`):

1. Lib darkens display, draws calibrated grid, highlights cell (0,0) with pulsing outline.
2. Caption: "Press the highlighted button. Press Esc to cancel."
3. Captures next physical keypress (any keycode), records `keycode → (0,0)`.
4. Walks all 16 cells row-major.
5. Validates: no duplicates. Conflict → highlights both, re-prompts.
6. Pulses grid green, writes profile, fires `Event::CalibrationDone`.

**Rotation:** stored as a single angle in degrees. Render layer applies a rotation transform around the grid center when compositing. No hit-testing complications because input arrives via the keyboard, not touch.

## Top-region layout primitive

```rust
pub enum Node {
    Stack { dir: Axis, gap_px: u16, children: Vec<(Sizing, Node)> },
    Pane(PaneId),
}
pub enum Axis { Horizontal, Vertical }
pub enum Sizing { Fixed(u16), Ratio(f32), Auto }
```

**Layout pass** is a pure function: given the top region's outer rect, returns `IndexMap<PaneId, Rect>`. Sub-microsecond for any practical tree.

Borders, padding, and styling are NOT in the layout tree. Borders are drawn by the render layer using `border_px` from the profile. Padding/margin live in user code.

**Example matching the mockup:**

```rust
use juballer_core::layout::{Node, Axis, Sizing::*};

let top = Node::Stack {
    dir: Axis::Vertical, gap_px: 10,
    children: vec![
        (Fixed(48),   Node::Pane("header")),
        (Ratio(1.0),  Node::Stack {
            dir: Axis::Horizontal, gap_px: 10,
            children: vec![
                (Ratio(1.2), Node::Pane("focus")),
                (Ratio(1.0), Node::Pane("events")),
                (Ratio(0.7), Node::Pane("pages")),
            ],
        }),
    ],
};
app.set_top_layout(top);
```

## Render API

**Frame model.** Each frame:

1. Lib acquires swapchain texture.
2. Lib clears an **offscreen "logical" framebuffer** (window-sized) to `bg_color`.
3. Lib calls user's `draw(frame, events)` callback. App draws into per-region handles at axis-aligned coordinates.
4. Lib runs **composite pass**: blits offscreen FB → swapchain with the calibration rotation transform; draws borders + bg on top.
5. Present.

**Region handles:**

```rust
pub struct Frame<'a> { /* … */ }

impl Frame<'_> {
    pub fn grid_cell(&mut self, row: u8, col: u8) -> RegionDraw<'_>;
    pub fn top_pane(&mut self, id: PaneId) -> RegionDraw<'_>;
}

pub struct RegionDraw<'a> {
    pub viewport: Rect,
    pub gpu: GpuCtx<'a>,
}

impl RegionDraw<'_> {
    pub fn fill(&mut self, color: Rgba);
    pub fn render_pass(&mut self) -> wgpu::RenderPass<'_>;
    pub fn blit_texture(&mut self, tex: &wgpu::TextureView);
}
```

**`juballer-egui`** (separate crate, opt-in):

```rust
let mut overlay = juballer_egui::EguiOverlay::new(&app)?;

overlay.draw(&mut frame, |ctx| {
    ctx.in_top_pane("focus", |ui| { ui.heading("Karmine Corp vs G2"); });
    ctx.in_grid_cell(0, 0, |ui| { ui.add(egui::Button::new("▶")); });
});
```

**The lib draws automatically:** background fill, borders between cells / between top-region panes / between top region + grid (using `border_px`), optional debug overlay (cell row/col labels) toggled with `App::set_debug(true)`.

**The lib does NOT draw:** fonts, images, animations, tweening, tile-content layout. Those are app concerns or `juballer-egui`'s responsibility.

## Input pipeline

**Two backends, picked at compile time:**

| backend | feature flag | source | latency p99 | requires |
|---------|--------------|--------|-------------|----------|
| **default** | none | `winit` keyboard events | ≤ 5 ms | nothing |
| **low-latency** | `--features raw-input` | Linux: `evdev-rs`; Windows: `RegisterRawInputDevices` (RIDEV_INPUTSINK) | ≤ 1 ms | Linux: user in `input` group; Windows: nothing |

Both backends produce the same `Event` stream. App code is identical.

**Event types:**

```rust
pub enum Event {
    KeyDown { row: u8, col: u8, key: KeyCode, ts: Instant },
    KeyUp   { row: u8, col: u8, key: KeyCode, ts: Instant },
    Unmapped { key: KeyCode, ts: Instant },
    CalibrationDone(Profile),
    WindowResized { w: u32, h: u32 },
    Quit,
}
```

`ts = std::time::Instant`, captured at the earliest possible point (kernel-side for evdev, message-pump-side for winit).

**Key repeat is suppressed** in both backends. Apps see one `KeyDown` + one `KeyUp` per physical press.

**Keymap loading:** on `AppBuilder::build()`, lib reads `[keymap]`. If empty / missing entries, lib stages `KeymapAutoLearn` to fire automatically on the next `run()` call. Apps can also force it via `App::run_keymap_auto_learn()`.

**Gesture recognizer (`juballer-gestures`, opt-in):**

```rust
let mut rec = juballer_gestures::Recognizer::with_defaults();

for ev in events {
    for g in rec.feed(ev) {
        match g {
            Gesture::Tap { row, col, dur }      => {},
            Gesture::Hold { row, col, dur }     => {},
            Gesture::Chord { cells, ts }        => {},
            Gesture::Swipe { path, dur }        => {},
        }
    }
}
```

Defaults: tap < 250 ms, hold ≥ 400 ms, chord window 50 ms, swipe window 80 ms between cells. All overridable via `Recognizer::builder()`.

## Performance contract

**Frame budget targets** (lib overhead per frame, NOT counting app draw):

| Workload | Refresh | Frame budget | Lib overhead | App headroom |
|----------|---------|--------------|--------------|--------------|
| Stream-Deck use | 60 Hz | 16.6 ms | ≤ 0.5 ms | ≥ 16 ms |
| Rhythm — comfortable | 144 Hz | 6.9 ms | ≤ 0.5 ms | ≥ 6 ms |
| Rhythm — competitive | 240 Hz | 4.16 ms | ≤ 0.7 ms | ≥ 3 ms |

**Input latency target** (key press → readable event in app callback):
- `raw-input`: ≤ 1 ms p99
- default `winit`: ≤ 5 ms p99

**Render pipeline knobs:**

```rust
App::builder()
    .present_mode(PresentMode::Mailbox)        // Fifo | Mailbox | Immediate
    .swapchain_buffers(2)                      // 2 (low latency) or 3 (smoother)
    .target_refresh(RefreshTarget::Monitor)    // Monitor | Fixed(hz) | Unlimited
    .build()?;
```

`Mailbox` = recommended deck use. `Immediate` = recommended rhythm with VRR off. `Fifo` = classic vsync.

**Threading model:**
- **Render thread** = main thread, winit event loop in `Poll` mode. Frame cadence is render-driven.
- **Input thread** (only with `raw-input`) = dedicated OS thread reading evdev/RawInput in a tight loop, pushing `Event`s into a lock-free SPSC ring. Backpressure = drop with a metric, never block render.
- **No mutex on the hot path.** No allocations in render or input loops after init.

**Composite pass cost:** one fullscreen textured quad with a 2×3 rotation matrix uniform. ~30 µs at 1440p, ~80 µs at 4K on a modern iGPU.

**No per-frame heap alloc contract:**
- Region handles are stack-only borrows.
- Pane rect lookup uses `IndexMap<PaneId, Rect>`, rebuilt only when `set_top_layout()` is called.
- Event delivery uses a pre-allocated `Vec<Event>` reused each frame.
- CI benchmark (`bench_no_alloc`) asserts 0 allocations during a 1000-frame steady-state run via `dhat`. Failure breaks CI.

## Public API surface (v0.1)

```rust
// types
pub struct App { /* … */ }
pub struct AppBuilder { /* … */ }
pub struct Frame<'a> { /* … */ }
pub struct RegionDraw<'a> { /* … */ }
pub struct Profile { /* … */ }
pub struct Rect { x: i32, y: i32, w: u32, h: u32 }
pub struct Color(pub u8, pub u8, pub u8, pub u8);
pub enum PresentMode { Fifo, Mailbox, Immediate }
pub enum RefreshTarget { Monitor, Fixed(u32), Unlimited }
pub enum Event { /* see Input section */ }
pub mod layout { pub enum Node, Axis, Sizing; pub type PaneId = &'static str; }

// builder + lifecycle
impl App {
    pub fn builder() -> AppBuilder;
    pub fn set_top_layout(&mut self, root: layout::Node);
    pub fn run<F>(self, draw: F) -> Result<()>
    where F: FnMut(&mut Frame, &[Event]);
    pub fn run_calibration(&mut self) -> Result<()>;
    pub fn run_keymap_auto_learn(&mut self) -> Result<()>;
    pub fn profile(&self) -> &Profile;
    pub fn set_debug(&mut self, on: bool);
}
```

End-to-end example:

```rust
use juballer_core::{App, PresentMode, RefreshTarget, Color};
use juballer_core::layout::{Node, Axis, Sizing::*};

fn main() -> juballer_core::Result<()> {
    let mut app = App::builder()
        .title("juballer-deck")
        .present_mode(PresentMode::Mailbox)
        .target_refresh(RefreshTarget::Monitor)
        .bg_color(Color(0x0b, 0x0d, 0x12, 0xff))
        .controller_vid_pid(0x1234, 0x5678)
        .build()?;

    app.set_top_layout(Node::Stack {
        dir: Axis::Vertical, gap_px: 10,
        children: vec![
            (Fixed(48),  Node::Pane("header")),
            (Ratio(1.0), Node::Stack {
                dir: Axis::Horizontal, gap_px: 10,
                children: vec![
                    (Ratio(1.2), Node::Pane("focus")),
                    (Ratio(1.0), Node::Pane("events")),
                    (Ratio(0.7), Node::Pane("pages")),
                ],
            }),
        ],
    });

    app.run(|frame, events| {
        for _ev in events { /* handle input */ }
        frame.top_pane("header").fill(Color(0x11, 0x14, 0x1b, 0xff));
        frame.top_pane("focus").fill(Color(0x11, 0x14, 0x1b, 0xff));
        for row in 0..4 {
            for col in 0..4 {
                frame.grid_cell(row, col).fill(Color(0x11, 0x14, 0x1b, 0xff));
            }
        }
    })
}
```

## Errors

Single `juballer_core::Error` enum via `thiserror`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")] Config(String),
    #[error("profile io: {0}")] ProfileIo(#[from] std::io::Error),
    #[error("profile parse: {0}")] ProfileParse(#[from] toml::de::Error),
    #[error("gpu init: {0}")] GpuInit(String),
    #[error("window: {0}")] Window(#[from] winit::error::OsError),
    #[error("input backend: {0}")] Input(String),
    #[error("calibration cancelled")] CalibrationCancelled,
    #[error("monitor not found: {0}")] MonitorNotFound(String),
}
pub type Result<T> = std::result::Result<T, Error>;
```

Public API never panics on user-facing input. `debug_assert!` only for internal invariants. No `unwrap()` in `lib.rs`.

## Testing strategy

**Unit tests** (per module, every push):
- `layout`: 30+ cases for the Stack solver — mixed `Fixed`/`Ratio`/`Auto`, nested trees, gaps, zero-size regions.
- `geometry`: calibration math (mm→px, rotation transform composition, cell rect derivation).
- `keymap`: validation (no duplicate keycodes, all 16 cells filled), profile round-trip.
- `input`: keymap lookup + repeat suppression on synthetic event sequences.

**Integration tests** (headless, in CI):
- **Headless backend** behind `--features headless` — uses an offscreen `wgpu::Device` (LavaPipe on Linux CI, WARP on Windows CI). No window opens.
- End-to-end frame: build `App` → set layout → push synthetic events → run N frames → capture offscreen FB → compare hash against golden PNG.
- Snapshots: empty grid, calibration overlay, debug overlay, rotated grid (5°), each layout-tree shape. Stored in `tests/snapshots/`. Regenerate via `cargo test -- --bless`.

**Benchmarks + perf regression** (criterion, on demand + nightly CI):
- `bench_layout`: solver throughput (target > 1 M layouts/s for the mockup tree).
- `bench_input_pipeline`: 1000-event burst through evdev mock → `Event` callback (target p99 < 1 ms).
- `bench_no_alloc`: 1000-frame steady-state wrapped in `dhat::Profiler`; CI fails if any allocation occurs after frame 1.

**Hardware-in-the-loop** (manual, gated behind `--features hw-tests`):
- `examples/calibration_dance` — full calibration + auto-learn flow, dumps profile.
- `examples/echo_grid` — fills cell on press, clears on release. Visual smoke test.
- `examples/latency_probe` — bright flash on press for photodiode-friendly latency measurement.

**CI matrix:** `linux-x86_64`, `windows-x86_64`. Both run unit + integration + alloc bench. Nightly runs full criterion benches and posts deltas.

## Out of scope (deferred to later cycles)

- macOS support.
- LED control (FB9 has none host-controllable).
- Multi-display setups (single display only).
- Built-in actions, scripting, or any application-level concerns — those belong to `juballer-deck`.
- Audio, asset loading, font rendering — app responsibility (or via `juballer-egui`).
- Touch / mouse input on the grid — keyboard only.

## Open items resolved during brainstorming

| Question | Resolution |
|----------|------------|
| Physical setup | A — single display, controller on lower half (cabinet style) |
| Render backend | D — raw wgpu surface + optional egui overlay |
| OS targets | A — Linux + Windows |
| Top region structure | C — arbitrary layout tree |
| Calibration | C — known FB9 defaults + interactive UI for fine-tune |
| Input model | B — raw events + opt-in gesture recognizer |
| Keymap | III — auto-learn calibration step |
| Crate structure | 2 — small workspace (core + egui + gestures) |
| Rotation support | Single rotation angle (axis-aligned base + rotate composite) |
