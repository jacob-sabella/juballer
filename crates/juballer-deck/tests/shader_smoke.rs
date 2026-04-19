//! Shader/video integration: bootstrap a DeckApp with a CustomShader-bound tile,
//! exercise TOML deserialization round-trip and action-driven mutation paths, and
//! render one headless frame through the shader pipeline without crashing.
//!
//! Headless-pixel tests are gated on the `headless` feature (which forwards to
//! `juballer-core/headless`); non-GPU unit checks always run.

use juballer_deck::action::{Action, ActionCx};
use juballer_deck::config::{ButtonCfg, ConfigTree, DeckPaths, PageConfig, TileShaderCfg};
use juballer_deck::shader::preprocess_wgsl;
use juballer_deck::tile::{TileHandle, TileShaderSource, TileState};
use juballer_deck::DeckApp;
use std::path::PathBuf;

#[cfg(feature = "headless")]
use juballer_deck::shader::{ShaderPipelineCache, TileUniforms};

fn fixture_with_shader(dir: &std::path::Path, wgsl_abs: &std::path::Path) {
    std::fs::create_dir_all(dir.join("profiles/p/pages")).unwrap();
    std::fs::write(
        dir.join("deck.toml"),
        r##"version = 1
active_profile = "p"

[editor]
bind = "127.0.0.1:7375"

[render]

[log]
level = "info"
"##,
    )
    .unwrap();
    std::fs::write(
        dir.join("profiles/p/profile.toml"),
        r##"name = "p"
default_page = "home"
pages = ["home"]
"##,
    )
    .unwrap();
    std::fs::write(
        dir.join("profiles/p/pages/home.toml"),
        format!(
            r##"[meta]
title = "home"

[[button]]
row = 0
col = 0
action = "shell.run"
args = {{ cmd = "true" }}
icon = "S"
label = "shader"
shader = {{ wgsl = "{}" }}
"##,
            wgsl_abs.display()
        ),
    )
    .unwrap();
}

#[test]
fn button_cfg_deserializes_wgsl_shader() {
    let s = r##"
row = 0
col = 0
action = "shell.run"
args = { cmd = "true" }
shader = { wgsl = "/tmp/plasma.wgsl" }
"##;
    let btn: ButtonCfg = toml::from_str(s).unwrap();
    match btn.shader.expect("shader present") {
        TileShaderCfg::Wgsl { wgsl } => assert_eq!(wgsl, "/tmp/plasma.wgsl"),
        _ => panic!("expected wgsl variant"),
    }
}

#[test]
fn button_cfg_deserializes_video_shader() {
    let s = r##"
row = 1
col = 2
action = "shell.run"
args = { cmd = "true" }
shader = { video = "v4l2:///dev/video0" }
"##;
    let btn: ButtonCfg = toml::from_str(s).unwrap();
    match btn.shader.expect("shader present") {
        TileShaderCfg::Video { video } => assert_eq!(video, "v4l2:///dev/video0"),
        _ => panic!("expected video variant"),
    }
}

#[test]
fn button_cfg_shader_roundtrip() {
    let s = r##"
[meta]
title = "home"

[[button]]
row = 0
col = 0
action = "shell.run"
args = { cmd = "true" }
shader = { wgsl = "/x/y.wgsl" }
"##;
    let page: PageConfig = toml::from_str(s).unwrap();
    let back = toml::to_string(&page).unwrap();
    let page2: PageConfig = toml::from_str(&back).unwrap();
    assert_eq!(page, page2);
}

#[test]
fn bind_active_page_populates_tile_shader() {
    let dir = tempfile::tempdir().unwrap();
    let wgsl = dir.path().join("plasma.wgsl");
    std::fs::write(
        &wgsl,
        "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }",
    )
    .unwrap();
    fixture_with_shader(dir.path(), &wgsl);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let paths = DeckPaths::from_root(dir.path().to_path_buf());
    let app = DeckApp::bootstrap(paths, rt.handle().clone()).unwrap();
    match app.tiles[0].shader.as_ref().expect("tile has shader") {
        TileShaderSource::CustomShader { wgsl_path, .. } => {
            assert_eq!(wgsl_path, &wgsl);
        }
        _ => panic!("expected CustomShader"),
    }
}

#[test]
fn tile_set_shader_action_mutates_tile() {
    use juballer_deck::action::builtin::tile_set_shader::TileSetShader;
    use juballer_deck::action::BuildFromArgs;
    use juballer_deck::bus::EventBus;
    use juballer_deck::StateStore;

    let mut args = toml::Table::new();
    args.insert("wgsl".into(), toml::Value::String("/some/path.wgsl".into()));
    let mut action = TileSetShader::from_args(&args).unwrap();

    let mut tile = TileState::default();
    let bus = EventBus::default();
    let mut state =
        StateStore::open(tempfile::tempdir().unwrap().path().join("state.toml")).unwrap();
    let env = indexmap::IndexMap::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let rt_handle = rt.handle().clone();
    {
        let mut cx = ActionCx {
            cell: (0, 0),
            binding_id: "test:0,0",
            tile: TileHandle::new(&mut tile),
            env: &env,
            bus: &bus,
            state: &mut state,
            rt: &rt_handle,
        };
        action.on_down(&mut cx);
    }
    match tile.shader.expect("shader set") {
        TileShaderSource::CustomShader { wgsl_path, .. } => {
            assert_eq!(wgsl_path, PathBuf::from("/some/path.wgsl"));
        }
        _ => panic!("expected CustomShader"),
    }
}

#[test]
fn tile_clear_shader_action_clears() {
    use juballer_deck::action::builtin::tile_clear_shader::TileClearShader;
    use juballer_deck::action::BuildFromArgs;
    use juballer_deck::bus::EventBus;
    use juballer_deck::StateStore;

    let mut action = TileClearShader::from_args(&toml::Table::new()).unwrap();

    let mut tile = TileState {
        shader: Some(TileShaderSource::CustomShader {
            wgsl_path: PathBuf::from("/x.wgsl"),
            params: Default::default(),
        }),
        ..Default::default()
    };
    let bus = EventBus::default();
    let mut state =
        StateStore::open(tempfile::tempdir().unwrap().path().join("state.toml")).unwrap();
    let env = indexmap::IndexMap::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let rt_handle = rt.handle().clone();
    {
        let mut cx = ActionCx {
            cell: (0, 0),
            binding_id: "test:0,0",
            tile: TileHandle::new(&mut tile),
            env: &env,
            bus: &bus,
            state: &mut state,
            rt: &rt_handle,
        };
        action.on_down(&mut cx);
    }
    assert!(tile.shader.is_none());
}

#[test]
fn preprocess_injects_vs_main() {
    let src =
        "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(u.time, 0.0, 0.0, 1.0); }";
    let out = preprocess_wgsl(src);
    assert!(out.contains("fn vs_main"));
    assert!(out.contains("struct Uniforms"));
}

/// Headless sanity: render one frame with a CustomShader tile bound to a stock
/// shader. Uses juballer-core's headless path + the deck's ShaderPipelineCache.
#[cfg(feature = "headless")]
#[test]
fn headless_render_with_shader_tile_does_not_crash() {
    use indexmap::IndexMap;
    use juballer_core::calibration::Profile;
    use juballer_core::layout::PaneId;
    use juballer_core::{geometry, render, Color};

    let tmp = tempfile::tempdir().unwrap();
    let wgsl_path = tmp.path().join("solid_time.wgsl");
    std::fs::write(
        &wgsl_path,
        include_str!("../examples/shaders/solid_time.wgsl"),
    )
    .unwrap();

    let w = 640u32;
    let h = 480u32;
    let profile = Profile::default_for("a", "b", w, h);
    let cells = geometry::cell_rects(&profile.grid);
    let panes: IndexMap<PaneId, juballer_core::Rect> = IndexMap::new();

    let cache = std::sync::Arc::new(std::sync::Mutex::new(ShaderPipelineCache::new()));
    let wgsl_for_closure = wgsl_path.clone();
    let pixels = pollster::block_on(render::headless::render_to_rgba(
        w,
        h,
        Color::rgb(0x0b, 0x0d, 0x12),
        &cells,
        &panes,
        0.0,
        move |frame, _| {
            let c = cache.clone();
            let path = wgsl_for_closure.clone();
            frame.with_tile_raw(0, 0, |mut ctx| {
                let uniforms = TileUniforms {
                    resolution: [ctx.viewport.2, ctx.viewport.3],
                    time: 1.0,
                    delta_time: 0.016,
                    cursor: [0.0, 0.0],
                    kind: 0.0,
                    bound: 1.0,
                    toggle_on: 0.0,
                    flash: 0.0,
                    _pad0: [0.0, 0.0],
                    accent: [1.0, 1.0, 1.0, 1.0],
                    state: [1.0, 1.0, 1.0, 1.0],
                    spectrum: [[0.0; 4]; 4],
                };
                let mut cache = c.lock().unwrap();
                cache.draw_tile(&mut ctx, &path, &uniforms);
            });
        },
    ));
    assert_eq!(pixels.len(), (w * h * 4) as usize);
    // Sample the middle of cell (0,0). The solid_time shader at t=1 produces non-bg color.
    let rect = cells[0];
    let sx = (rect.x + rect.w as i32 / 2) as u32;
    let sy = (rect.y + rect.h as i32 / 2) as u32;
    let idx = ((sy * w + sx) * 4) as usize;
    let r = pixels[idx];
    let g = pixels[idx + 1];
    let b = pixels[idx + 2];
    // At t=1 the three channels are 0.5 + 0.5*sin(1 + 2.094*k). None should be pure bg.
    let differs = |c: u8, bg: u8| (c as i32 - bg as i32).abs() > 10;
    assert!(
        differs(r, 0x0b) || differs(g, 0x0d) || differs(b, 0x12),
        "shader pixel indistinguishable from bg: {r:#x} {g:#x} {b:#x}"
    );
}

#[cfg(feature = "headless")]
#[test]
fn broken_shader_records_error_but_does_not_crash() {
    use indexmap::IndexMap;
    use juballer_core::calibration::Profile;
    use juballer_core::layout::PaneId;
    use juballer_core::{geometry, render, Color};

    let tmp = tempfile::tempdir().unwrap();
    let wgsl_path = tmp.path().join("broken.wgsl");
    std::fs::write(&wgsl_path, "this is not WGSL, obviously").unwrap();

    let w = 320u32;
    let h = 240u32;
    let profile = Profile::default_for("a", "b", w, h);
    let cells = geometry::cell_rects(&profile.grid);
    let panes: IndexMap<PaneId, juballer_core::Rect> = IndexMap::new();

    let cache = std::sync::Arc::new(std::sync::Mutex::new(ShaderPipelineCache::new()));
    let cache_clone = cache.clone();
    let wgsl_clone = wgsl_path.clone();
    let _ = pollster::block_on(render::headless::render_to_rgba(
        w,
        h,
        Color::rgb(0x0b, 0x0d, 0x12),
        &cells,
        &panes,
        0.0,
        move |frame, _| {
            frame.with_tile_raw(0, 0, |mut ctx| {
                let uniforms = TileUniforms {
                    resolution: [ctx.viewport.2, ctx.viewport.3],
                    time: 0.0,
                    delta_time: 0.0,
                    cursor: [0.0, 0.0],
                    kind: 0.0,
                    bound: 0.0,
                    toggle_on: 0.0,
                    flash: 0.0,
                    _pad0: [0.0, 0.0],
                    accent: [0.0, 0.0, 0.0, 1.0],
                    state: [0.0, 0.0, 0.0, 1.0],
                    spectrum: [[0.0; 4]; 4],
                };
                cache_clone
                    .lock()
                    .unwrap()
                    .draw_tile(&mut ctx, &wgsl_clone, &uniforms);
            });
        },
    ));
    let c = cache.lock().unwrap();
    assert!(
        c.last_error(&wgsl_path).is_some(),
        "broken shader should have recorded an error"
    );
}

/// Every preset shader under `examples/shaders/` must compile on wgpu without
/// recording a shader error. Covers both the state-aware presets (nav_pulse,
/// toggle_bar, press_ripple, ambient_warmth, kind_glow, empty_dotgrid) and the
/// plain ones (plasma, waves, matrix_rain, solid_time).
#[cfg(feature = "headless")]
#[test]
fn all_preset_shaders_compile() {
    use indexmap::IndexMap;
    use juballer_core::calibration::Profile;
    use juballer_core::layout::PaneId;
    use juballer_core::{geometry, render, Color};

    let presets: &[(&str, &str)] = &[
        (
            "plasma.wgsl",
            include_str!("../examples/shaders/plasma.wgsl"),
        ),
        ("waves.wgsl", include_str!("../examples/shaders/waves.wgsl")),
        (
            "matrix_rain.wgsl",
            include_str!("../examples/shaders/matrix_rain.wgsl"),
        ),
        (
            "solid_time.wgsl",
            include_str!("../examples/shaders/solid_time.wgsl"),
        ),
        (
            "nav_pulse.wgsl",
            include_str!("../examples/shaders/nav_pulse.wgsl"),
        ),
        (
            "toggle_bar.wgsl",
            include_str!("../examples/shaders/toggle_bar.wgsl"),
        ),
        (
            "press_ripple.wgsl",
            include_str!("../examples/shaders/press_ripple.wgsl"),
        ),
        (
            "ambient_warmth.wgsl",
            include_str!("../examples/shaders/ambient_warmth.wgsl"),
        ),
        (
            "kind_glow.wgsl",
            include_str!("../examples/shaders/kind_glow.wgsl"),
        ),
        (
            "empty_dotgrid.wgsl",
            include_str!("../examples/shaders/empty_dotgrid.wgsl"),
        ),
    ];

    let tmp = tempfile::tempdir().unwrap();
    let w = 320u32;
    let h = 240u32;
    let profile = Profile::default_for("a", "b", w, h);
    let cells = geometry::cell_rects(&profile.grid);
    let panes: IndexMap<PaneId, juballer_core::Rect> = IndexMap::new();

    for (name, src) in presets {
        let path = tmp.path().join(name);
        std::fs::write(&path, src).unwrap();

        let cache = std::sync::Arc::new(std::sync::Mutex::new(ShaderPipelineCache::new()));
        let cache_c = cache.clone();
        let path_c = path.clone();
        let _ = pollster::block_on(render::headless::render_to_rgba(
            w,
            h,
            Color::rgb(0x0b, 0x0d, 0x12),
            &cells,
            &panes,
            0.0,
            move |frame, _| {
                frame.with_tile_raw(0, 0, |mut ctx| {
                    let uniforms = TileUniforms {
                        resolution: [ctx.viewport.2, ctx.viewport.3],
                        time: 0.5,
                        delta_time: 0.016,
                        cursor: [0.0, 0.0],
                        kind: 1.0,
                        bound: 1.0,
                        toggle_on: 1.0,
                        flash: 0.4,
                        _pad0: [0.0, 0.0],
                        accent: [0.7, 0.75, 1.0, 1.0],
                        state: [0.65, 0.9, 0.6, 1.0],
                        spectrum: [[0.0; 4]; 4],
                    };
                    cache_c
                        .lock()
                        .unwrap()
                        .draw_tile(&mut ctx, &path_c, &uniforms);
                });
            },
        ));
        let c = cache.lock().unwrap();
        assert!(
            c.last_error(&path).is_none(),
            "preset {} failed to compile: {:?}",
            name,
            c.last_error(&path).map(|e| &e.message)
        );
    }
}

/// ConfigTree.load round-trip test: full load of a deck with a shader-bound button.
#[test]
fn config_tree_loads_deck_with_shader_button() {
    let dir = tempfile::tempdir().unwrap();
    let wgsl = dir.path().join("p.wgsl");
    std::fs::write(
        &wgsl,
        "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }",
    )
    .unwrap();
    fixture_with_shader(dir.path(), &wgsl);
    let paths = DeckPaths::from_root(dir.path().to_path_buf());
    let tree = ConfigTree::load(&paths).unwrap();
    let page = tree.lookup_page("home").expect("page");
    assert_eq!(page.buttons.len(), 1);
    assert!(page.buttons[0].shader.is_some());
}
