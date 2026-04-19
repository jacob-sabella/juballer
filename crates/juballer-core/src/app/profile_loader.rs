use crate::calibration::{default_profile_path, Profile};
use std::sync::Arc;
use winit::window::Window;

/// Build a controller_id string from VID:PID. Empty VID:PID maps to `"unknown"`.
pub fn controller_id(vid: u16, pid: u16) -> String {
    if vid == 0 && pid == 0 {
        "unknown".to_string()
    } else {
        format!("{:04x}:{:04x}", vid, pid)
    }
}

/// Build a monitor_id string from the window's current monitor.
pub fn monitor_id(window: &Arc<Window>) -> String {
    match window.current_monitor() {
        Some(m) => {
            let name = m.name().unwrap_or_else(|| "unknown".to_string());
            let size = m.size();
            format!("{} / {}x{}", name, size.width, size.height)
        }
        None => "unknown / 0x0".to_string(),
    }
}

/// Load a profile from disk, or build a fresh default and persist it. Matches the
/// stored profile's (controller_id, monitor_id) pair — if they differ, writes a new
/// default that matches the current pair.
pub fn load_or_create_profile(
    controller_id: &str,
    monitor_id: &str,
    monitor_w: u32,
    monitor_h: u32,
) -> Profile {
    let path = default_profile_path();
    if let Ok(mut p) = Profile::load(&path) {
        // Primary match: controller_id. If it matches and keymap is non-empty,
        // keep this profile (preserves calibration across monitor_id flakiness).
        if p.profile.controller_id == controller_id && !p.keymap.is_empty() {
            if p.profile.monitor_id != monitor_id {
                log::info!(
                    "profile monitor_id updated: {} → {} (controller match, keymap preserved)",
                    p.profile.monitor_id,
                    monitor_id
                );
                p.profile.monitor_id = monitor_id.to_string();
                if let Err(e) = p.save(&path) {
                    log::warn!("could not persist updated profile: {}", e);
                }
            }
            return p;
        }
        if p.profile.controller_id == controller_id && p.profile.monitor_id == monitor_id {
            return p;
        }
        log::info!(
            "profile mismatches current (controller={}, monitor={}); generating default",
            controller_id,
            monitor_id
        );
    }
    let p = Profile::default_for(controller_id, monitor_id, monitor_w, monitor_h);
    if let Err(e) = p.save(&path) {
        log::warn!("could not persist default profile to {:?}: {}", path, e);
    }
    p
}
