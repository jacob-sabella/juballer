//! Pure dispatch: turn a press / release event on a configured cell
//! into a list of [`Outcome`]s the caller (the Carla mode entry)
//! pushes to the OSC client + HUD state. No I/O lives here so the
//! whole behaviour table is exercised by unit tests.
//!
//! Event model
//! - `KeyDown`            — finger touches the key
//! - `KeyUp { duration }` — finger lifts; duration since the down event
//!
//! Slot semantics
//! - **Tap** action fires on `KeyUp` if `duration < HOLD_THRESHOLD_MS`
//!   (or always for Momentary, which fires on both edges)
//! - **Hold** action fires on `KeyUp` if `duration >= HOLD_THRESHOLD_MS`
//!
//! The threshold is centralised so the gesture recogniser, the
//! renderer, and the dispatcher all agree on the same number.

use crate::carla::config::{Action, ActionMode, Cell, ParamRef, PluginRef};
use crate::carla::names::NameMap;
use crate::carla::state::ParamValueCache;
use std::time::Duration;

/// Long-press window. Anything held longer than this fires the `hold`
/// slot instead of `tap`. Lines up with the gesture recogniser's
/// short-vs-long-press cutover (≈ jubeat's "long note" feel).
pub const HOLD_THRESHOLD_MS: u64 = 350;

/// Resolved physical input event delivered to a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellEvent {
    KeyDown,
    KeyUp { held: Duration },
}

impl CellEvent {
    pub fn is_long_press_release(&self) -> bool {
        matches!(self, Self::KeyUp { held } if held.as_millis() as u64 >= HOLD_THRESHOLD_MS)
    }
}

/// One side-effect produced by dispatching an event. The caller maps
/// these to OSC writes / UI transitions; keeping the result side-free
/// makes the whole pipeline trivially testable.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Send `/Carla/<plugin>/set_parameter_value <param> <value>`.
    SetParameter {
        plugin: PluginRef,
        param: ParamRef,
        value: f32,
    },
    /// Phase 3 — apply a saved preset to a plugin.
    LoadPreset { plugin: PluginRef, preset: String },
    /// Phase 3 — open the preset picker overlay scoped to one
    /// plugin slot, optionally filtered to a category.
    OpenPresetPicker {
        plugin: PluginRef,
        category: Option<String>,
    },
}

/// Top-level dispatch. Walks the cell's tap/hold slots and emits the
/// right outcomes for the given event. Updates `cache` with whatever
/// new values were written so toggle/bump/carousel can read back the
/// current value next time.
///
/// `names` resolves `PluginRef::Name` / `ParamRef::Name` references
/// to numeric indices via the name map built from the user's .carxp
/// project. Name-only refs that don't appear in the map are dropped
/// with a warning — see [`fire_action`] for the short-circuit path.
pub fn dispatch(
    cell: &Cell,
    event: CellEvent,
    cache: &mut ParamValueCache,
    names: &NameMap,
) -> Vec<Outcome> {
    let mut out = Vec::new();
    match event {
        CellEvent::KeyDown => {
            if let Some(action) = &cell.tap {
                if matches!(action.mode, ActionMode::Momentary) {
                    fire_action(action, ActionEdge::Down, cache, names, &mut out);
                }
            }
        }
        CellEvent::KeyUp { held } => {
            let long = held.as_millis() as u64 >= HOLD_THRESHOLD_MS;
            if long {
                if let Some(action) = &cell.hold {
                    fire_action(action, ActionEdge::Up, cache, names, &mut out);
                }
                // Momentary on tap still needs an off-edge even when
                // the press qualified as a hold — otherwise the param
                // sticks high after release.
                if let Some(action) = &cell.tap {
                    if matches!(action.mode, ActionMode::Momentary) {
                        fire_action(action, ActionEdge::Up, cache, names, &mut out);
                    }
                }
            } else if let Some(action) = &cell.tap {
                fire_action(action, ActionEdge::Up, cache, names, &mut out);
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ActionEdge {
    /// Press-down — only momentary uses this.
    Down,
    /// Press-up / hold-threshold — every other mode fires here.
    Up,
}

fn fire_action(
    action: &Action,
    edge: ActionEdge,
    cache: &mut ParamValueCache,
    names: &NameMap,
    out: &mut Vec<Outcome>,
) {
    let Some(plugin_id) = names.resolve_plugin(&action.plugin) else {
        // Name didn't resolve in the loaded project map. Skip
        // dispatch and keep the cache clean; the operator will see a
        // warning at run() startup if the name was wrong.
        tracing::warn!(
            target: "juballer::carla::dispatch",
            "plugin {:?} not in name map; dropping action",
            action.plugin
        );
        return;
    };
    let param_id = action
        .param
        .as_ref()
        .and_then(|p| names.resolve_param(plugin_id, p));
    // The Outcome should always carry the *resolved* numeric refs so
    // the OSC client doesn't have to redo name resolution downstream.
    let resolved_plugin = PluginRef::Index(plugin_id);
    let resolved_param = param_id.map(PluginRef::Index);

    match action.mode {
        ActionMode::BumpUp | ActionMode::BumpDown => {
            let Some(param_id) = param_id else { return };
            let Some(step) = action.step else { return };
            let signed_step = if matches!(action.mode, ActionMode::BumpUp) {
                step
            } else {
                -step
            };
            let current = cache.get(plugin_id, param_id).unwrap_or(0.0);
            let raw = current + signed_step;
            let clamped = clamp(raw, action.min, action.max);
            cache.set(plugin_id, param_id, clamped);
            out.push(Outcome::SetParameter {
                plugin: resolved_plugin.clone(),
                param: resolved_param.clone().unwrap(),
                value: clamped,
            });
        }
        ActionMode::Toggle => {
            let Some(param_id) = param_id else { return };
            let on = action.resolved_on_value();
            let off = action.resolved_off_value();
            let current = cache.get(plugin_id, param_id).unwrap_or(off);
            let next = if (current - on).abs() < f32::EPSILON {
                off
            } else {
                on
            };
            cache.set(plugin_id, param_id, next);
            out.push(Outcome::SetParameter {
                plugin: resolved_plugin.clone(),
                param: resolved_param.clone().unwrap(),
                value: next,
            });
        }
        ActionMode::Momentary => {
            let Some(param_id) = param_id else { return };
            let value = match edge {
                ActionEdge::Down => action.resolved_on_value(),
                ActionEdge::Up => action.resolved_off_value(),
            };
            cache.set(plugin_id, param_id, value);
            out.push(Outcome::SetParameter {
                plugin: resolved_plugin.clone(),
                param: resolved_param.clone().unwrap(),
                value,
            });
        }
        ActionMode::Set => {
            let Some(param_id) = param_id else { return };
            let Some(value) = action.value else { return };
            cache.set(plugin_id, param_id, value);
            out.push(Outcome::SetParameter {
                plugin: resolved_plugin.clone(),
                param: resolved_param.clone().unwrap(),
                value,
            });
        }
        ActionMode::CarouselNext | ActionMode::CarouselPrev => {
            let Some(param_id) = param_id else { return };
            let Some(values) = action.values.as_deref() else {
                return;
            };
            if values.is_empty() {
                return;
            }
            let current = cache.get(plugin_id, param_id);
            let idx = current
                .and_then(|v| values.iter().position(|&x| (x - v).abs() < f32::EPSILON))
                .unwrap_or(0);
            let next_idx = if matches!(action.mode, ActionMode::CarouselNext) {
                (idx + 1) % values.len()
            } else if idx == 0 {
                values.len() - 1
            } else {
                idx - 1
            };
            let next = values[next_idx];
            cache.set(plugin_id, param_id, next);
            out.push(Outcome::SetParameter {
                plugin: resolved_plugin.clone(),
                param: resolved_param.clone().unwrap(),
                value: next,
            });
        }
        ActionMode::LoadPreset => {
            let Some(preset) = action.preset.clone() else {
                return;
            };
            out.push(Outcome::LoadPreset {
                plugin: resolved_plugin,
                preset,
            });
        }
        ActionMode::OpenPresetPicker => {
            out.push(Outcome::OpenPresetPicker {
                plugin: resolved_plugin,
                category: action.category.clone(),
            });
        }
    }
}

fn clamp(value: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    let lo = min.unwrap_or(f32::NEG_INFINITY);
    let hi = max.unwrap_or(f32::INFINITY);
    value.clamp(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carla::config::{Action, ActionMode, Cell};

    fn empty_cell() -> Cell {
        Cell {
            row: 0,
            col: 0,
            label: None,
            tap: None,
            hold: None,
            display: None,
        }
    }

    fn action_set(plugin: u32, param: u32, value: f32) -> Action {
        Action {
            plugin: PluginRef::Index(plugin),
            param: Some(PluginRef::Index(param)),
            mode: ActionMode::Set,
            step: None,
            min: None,
            max: None,
            value: Some(value),
            on_value: None,
            off_value: None,
            values: None,
            value_labels: None,
            preset: None,
            category: None,
        }
    }

    fn quick_release() -> CellEvent {
        CellEvent::KeyUp {
            held: Duration::from_millis(50),
        }
    }

    fn long_release() -> CellEvent {
        CellEvent::KeyUp {
            held: Duration::from_millis(HOLD_THRESHOLD_MS + 100),
        }
    }

    #[test]
    fn tap_set_writes_value_and_caches_it() {
        let mut cell = empty_cell();
        cell.tap = Some(action_set(1, 2, 0.7));
        let mut cache = ParamValueCache::new();
        let out = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        assert_eq!(out.len(), 1);
        match &out[0] {
            Outcome::SetParameter { value, .. } => assert!((value - 0.7).abs() < 1e-6),
            _ => panic!("expected set"),
        }
        assert_eq!(cache.get(1, 2), Some(0.7));
    }

    #[test]
    fn tap_does_not_fire_on_long_press_release() {
        let mut cell = empty_cell();
        cell.tap = Some(action_set(1, 2, 0.7));
        let mut cache = ParamValueCache::new();
        let out = dispatch(&cell, long_release(), &mut cache, &NameMap::empty());
        assert!(out.is_empty(), "tap should yield to hold on long press");
    }

    #[test]
    fn hold_fires_only_on_long_press_release() {
        let mut cell = empty_cell();
        cell.hold = Some(action_set(3, 4, 0.0));
        let mut cache = ParamValueCache::new();
        assert!(dispatch(&cell, quick_release(), &mut cache, &NameMap::empty()).is_empty());
        let out = dispatch(&cell, long_release(), &mut cache, &NameMap::empty());
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn bump_up_clamps_to_max() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::BumpUp,
            step: Some(0.3),
            min: Some(0.0),
            max: Some(1.0),
            ..action_set(1, 2, 0.0)
        });
        let mut cache = ParamValueCache::new();
        cache.set(1, 2, 0.9);
        let out = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        match &out[0] {
            Outcome::SetParameter { value, .. } => {
                assert!((value - 1.0).abs() < 1e-6, "should clamp to max");
            }
            _ => panic!("expected set"),
        }
    }

    #[test]
    fn bump_down_clamps_to_min_and_uses_zero_when_unset() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::BumpDown,
            step: Some(0.4),
            min: Some(0.0),
            max: None,
            ..action_set(1, 2, 0.0)
        });
        let mut cache = ParamValueCache::new();
        // No prior value cached → starts at 0.0, decrement clamps to 0.0
        let out = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        match &out[0] {
            Outcome::SetParameter { value, .. } => assert_eq!(*value, 0.0),
            _ => panic!("expected set"),
        }
    }

    #[test]
    fn toggle_flips_between_on_and_off_values() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::Toggle,
            on_value: Some(1.0),
            off_value: Some(0.0),
            ..action_set(1, 2, 0.0)
        });
        let mut cache = ParamValueCache::new();
        // First press: cache has nothing → defaults to off → flips to on.
        let a = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        assert_eq!(cache.get(1, 2), Some(1.0));
        // Second press: cache has on → flips to off.
        let b = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        assert_eq!(cache.get(1, 2), Some(0.0));
        assert_ne!(a, b);
    }

    #[test]
    fn momentary_fires_on_both_key_down_and_key_up() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::Momentary,
            on_value: Some(1.0),
            off_value: Some(0.0),
            ..action_set(1, 2, 0.0)
        });
        let mut cache = ParamValueCache::new();
        let down = dispatch(&cell, CellEvent::KeyDown, &mut cache, &NameMap::empty());
        match &down[0] {
            Outcome::SetParameter { value, .. } => assert_eq!(*value, 1.0),
            _ => panic!("expected on"),
        }
        let up = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        match &up[0] {
            Outcome::SetParameter { value, .. } => assert_eq!(*value, 0.0),
            _ => panic!("expected off"),
        }
    }

    #[test]
    fn momentary_off_edge_still_fires_after_a_long_press() {
        // Even when the press qualifies as a hold (long_release), the
        // momentary mode on the tap slot needs its off-edge so the
        // parameter doesn't stick high.
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::Momentary,
            on_value: Some(1.0),
            off_value: Some(0.0),
            ..action_set(1, 2, 0.0)
        });
        let mut cache = ParamValueCache::new();
        // Skip the down event for clarity — the up event alone should
        // still emit the off-value.
        let up = dispatch(&cell, long_release(), &mut cache, &NameMap::empty());
        assert_eq!(up.len(), 1);
        match &up[0] {
            Outcome::SetParameter { value, .. } => assert_eq!(*value, 0.0),
            _ => panic!("expected off-edge"),
        }
    }

    #[test]
    fn carousel_next_walks_values_forward_and_wraps() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::CarouselNext,
            values: Some(vec![0.0, 0.25, 0.5, 0.75]),
            ..action_set(1, 2, 0.0)
        });
        let mut cache = ParamValueCache::new();
        // Cache empty → starts at index 0 → advance to 0.25.
        let a = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        match &a[0] {
            Outcome::SetParameter { value, .. } => assert_eq!(*value, 0.25),
            _ => panic!("expected set"),
        }
        // Walk to the end then wrap.
        dispatch(&cell, quick_release(), &mut cache, &NameMap::empty()); // 0.5
        dispatch(&cell, quick_release(), &mut cache, &NameMap::empty()); // 0.75
        let last = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        match &last[0] {
            Outcome::SetParameter { value, .. } => assert_eq!(*value, 0.0),
            _ => panic!("expected wrap"),
        }
    }

    #[test]
    fn carousel_prev_walks_values_backward_and_wraps() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::CarouselPrev,
            values: Some(vec![0.0, 1.0, 2.0]),
            ..action_set(1, 2, 0.0)
        });
        let mut cache = ParamValueCache::new();
        // Empty cache → idx=0 → prev wraps to last (2.0).
        let a = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        match &a[0] {
            Outcome::SetParameter { value, .. } => assert_eq!(*value, 2.0),
            _ => panic!("expected wrap to last"),
        }
        // Now at 2.0 → idx=2 → prev to 1.0.
        let b = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        match &b[0] {
            Outcome::SetParameter { value, .. } => assert_eq!(*value, 1.0),
            _ => panic!("expected previous"),
        }
    }

    #[test]
    fn name_only_plugin_ref_short_circuits_phase1_dispatch() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            plugin: PluginRef::Name("Roomy".into()),
            ..action_set(0, 0, 0.5)
        });
        let mut cache = ParamValueCache::new();
        assert!(dispatch(&cell, quick_release(), &mut cache, &NameMap::empty()).is_empty());
        assert!(cache.is_empty(), "cache must not record name-only writes");
    }

    #[test]
    fn empty_cell_yields_no_outcomes() {
        let mut cache = ParamValueCache::new();
        assert!(dispatch(
            &empty_cell(),
            quick_release(),
            &mut cache,
            &NameMap::empty()
        )
        .is_empty());
        assert!(dispatch(
            &empty_cell(),
            CellEvent::KeyDown,
            &mut cache,
            &NameMap::empty()
        )
        .is_empty());
    }

    #[test]
    fn load_preset_emits_preset_outcome() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::LoadPreset,
            preset: Some("vintage_marshall".into()),
            ..action_set(1, 0, 0.0)
        });
        let mut cache = ParamValueCache::new();
        let out = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        assert_eq!(
            out[0],
            Outcome::LoadPreset {
                plugin: PluginRef::Index(1),
                preset: "vintage_marshall".into(),
            }
        );
    }

    #[test]
    fn open_preset_picker_emits_picker_outcome_with_optional_category() {
        let mut cell = empty_cell();
        cell.tap = Some(Action {
            mode: ActionMode::OpenPresetPicker,
            category: Some("guitar-cabs".into()),
            ..action_set(0, 0, 0.0)
        });
        let mut cache = ParamValueCache::new();
        let out = dispatch(&cell, quick_release(), &mut cache, &NameMap::empty());
        assert_eq!(
            out[0],
            Outcome::OpenPresetPicker {
                plugin: PluginRef::Index(0),
                category: Some("guitar-cabs".into()),
            }
        );
    }
}
