//! Smoke tests for the bundled Carla example config. Catches schema
//! drift the moment the example diverges from the loader.

use juballer_deck::carla::config::{ActionMode, Configuration, DisplayMode};
use std::path::PathBuf;

fn sample_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/carla/sample-config.toml")
}

#[test]
fn bundled_sample_config_parses_and_validates() {
    let cfg = Configuration::load(&sample_path())
        .expect("examples/carla/sample-config.toml should parse + validate");
    assert_eq!(cfg.display_name(), "Sample Drum Bus FX");
    assert!(cfg.pages.len() >= 2, "sample exercises sub-page nav");
}

#[test]
fn bundled_sample_config_exercises_all_phase_1_action_modes() {
    let cfg = Configuration::load(&sample_path()).unwrap();
    let mut seen = std::collections::HashSet::new();
    for page in &cfg.pages {
        for cell in &page.cells {
            if let Some(action) = &cell.tap {
                seen.insert(action.mode);
            }
            if let Some(action) = &cell.hold {
                seen.insert(action.mode);
            }
        }
    }
    for mode in [
        ActionMode::BumpUp,
        ActionMode::BumpDown,
        ActionMode::Toggle,
        ActionMode::Momentary,
        ActionMode::Set,
        ActionMode::CarouselNext,
        ActionMode::CarouselPrev,
    ] {
        assert!(
            seen.contains(&mode),
            "sample config should demonstrate {mode:?}"
        );
    }
}

#[test]
fn bundled_sample_includes_meter_and_tuner_display_bindings() {
    let cfg = Configuration::load(&sample_path()).unwrap();
    let mut modes = std::collections::HashSet::new();
    for page in &cfg.pages {
        for cell in &page.cells {
            if let Some(disp) = &cell.display {
                modes.insert(disp.mode);
            }
        }
    }
    assert!(
        modes.contains(&DisplayMode::Meter),
        "sample should demonstrate the meter display"
    );
    assert!(
        modes.contains(&DisplayMode::Tuner),
        "sample should demonstrate the tuner display"
    );
}
