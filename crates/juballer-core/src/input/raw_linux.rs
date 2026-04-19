//! Linux raw-input via evdev. Spawns a dedicated thread, opens the controller's keyboard
//! device by VID:PID, pushes Events into the EventRing.
#![cfg(all(target_os = "linux", feature = "raw-input"))]

use super::{Event, EventRing, KeyCode, Keymap};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

pub struct RawInputLinux {
    pub join: thread::JoinHandle<()>,
}

impl RawInputLinux {
    pub fn spawn(
        vid: u16,
        pid: u16,
        keymap: Keymap,
        ring: Arc<EventRing>,
    ) -> std::io::Result<Self> {
        let device = find_device(vid, pid)?;
        let join = thread::Builder::new()
            .name("juballer-raw-input".into())
            .spawn(move || {
                run_loop(device, keymap, ring);
            })?;
        Ok(Self { join })
    }
}

fn find_device(vid: u16, pid: u16) -> std::io::Result<evdev::Device> {
    use evdev::EventType;
    // Two passes: first prefer devices that report KEY events (the keyboard interface).
    // FB9 exposes both keyboard + mouse interfaces under the same VID:PID; we want
    // the keyboard one.
    let mut keyboard_match: Option<(std::path::PathBuf, evdev::Device)> = None;
    let mut any_match: Option<(std::path::PathBuf, evdev::Device)> = None;
    for (path, dev) in evdev::enumerate() {
        let id = dev.input_id();
        if id.vendor() == vid && id.product() == pid {
            let supports_key = dev.supported_events().contains(EventType::KEY);
            log::info!(
                "evdev candidate: {:?} vid={:04x} pid={:04x} keys={}",
                path,
                vid,
                pid,
                supports_key
            );
            if supports_key && keyboard_match.is_none() {
                keyboard_match = Some((path.clone(), dev));
            } else if any_match.is_none() {
                any_match = Some((path, dev));
            }
        }
    }
    if let Some((path, dev)) = keyboard_match {
        log::info!("evdev opening keyboard device: {:?}", path);
        return Ok(dev);
    }
    if let Some((path, dev)) = any_match {
        log::warn!(
            "evdev no keyboard interface found; falling back to: {:?}",
            path
        );
        return Ok(dev);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("evdev device {:04x}:{:04x} not found", vid, pid),
    ))
}

fn run_loop(mut dev: evdev::Device, keymap: Keymap, ring: Arc<EventRing>) {
    use evdev::EventSummary;
    // Grab the device exclusively so keystrokes don't leak to the focused window.
    if let Err(e) = dev.grab() {
        log::warn!("evdev grab failed (keys will leak to focused window): {e}");
    } else {
        log::info!("evdev grab acquired — keystrokes will NOT reach other windows");
    }
    loop {
        let events = match dev.fetch_events() {
            Ok(e) => e,
            Err(e) => {
                log::warn!("evdev fetch_events error: {e}");
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
        };
        for ev in events {
            let EventSummary::Key(_, keycode, value) = ev.destructure() else {
                continue;
            };
            let code_str = format!("{:?}", keycode);
            log::info!("evdev: code={} value={}", code_str, value);
            let ts = Instant::now();
            let event = match value {
                1 => match keymap.lookup(&code_str) {
                    Some((row, col)) => Event::KeyDown {
                        row,
                        col,
                        key: KeyCode(code_str),
                        ts,
                    },
                    None => Event::Unmapped {
                        key: KeyCode(code_str),
                        ts,
                    },
                },
                0 => match keymap.lookup(&code_str) {
                    Some((row, col)) => Event::KeyUp {
                        row,
                        col,
                        key: KeyCode(code_str),
                        ts,
                    },
                    None => continue,
                },
                _ => continue, // 2 = repeat; suppress
            };
            ring.try_send(event);
        }
    }
}
