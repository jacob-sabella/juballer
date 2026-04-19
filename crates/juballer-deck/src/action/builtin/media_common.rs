//! Platform-native media control. Wayland blocks enigo's synthetic keys, so on
//! Linux we use playerctl (transport) + pactl (volume). Other platforms fall
//! back to enigo's synthetic media keys.

use crate::action::ActionCx;
#[cfg(not(target_os = "linux"))]
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

pub enum MediaCmd {
    PlayPause,
    Next,
    Prev,
    VolUp,
    VolDown,
}

pub fn fire(cx: &mut ActionCx<'_>, topic: &str, cmd: MediaCmd) {
    let topic = format!("{topic}:{}", cx.binding_id);
    let bus = cx.bus.clone();
    cx.rt.spawn(async move {
        let r = run(cmd).await;
        bus.publish(
            topic,
            match r {
                Ok(()) => serde_json::json!({ "ok": true }),
                Err(e) => serde_json::json!({ "error": e }),
            },
        );
    });
    cx.tile.flash(120);
}

#[cfg(target_os = "linux")]
async fn run(cmd: MediaCmd) -> Result<(), String> {
    use tokio::process::Command;
    let out = match cmd {
        MediaCmd::PlayPause => Command::new("playerctl").arg("play-pause").output().await,
        MediaCmd::Next => Command::new("playerctl").arg("next").output().await,
        MediaCmd::Prev => Command::new("playerctl").arg("previous").output().await,
        MediaCmd::VolUp => {
            Command::new("wpctl")
                .args(["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "5%+"])
                .output()
                .await
        }
        MediaCmd::VolDown => {
            Command::new("wpctl")
                .args(["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "5%-"])
                .output()
                .await
        }
    };
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "exit={:?} stderr={}",
            o.status.code(),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(target_os = "linux"))]
async fn run(cmd: MediaCmd) -> Result<(), String> {
    let key = match cmd {
        MediaCmd::PlayPause => Key::MediaPlayPause,
        MediaCmd::Next => Key::MediaNextTrack,
        MediaCmd::Prev => Key::MediaPrevTrack,
        MediaCmd::VolUp => Key::VolumeUp,
        MediaCmd::VolDown => Key::VolumeDown,
    };
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
        enigo
            .key(key, Direction::Click)
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| format!("join: {e}"))?
}
