//! Config directory watcher. Debounces notify events + emits a single `ReloadRequested`
//! token on the output channel per quiet interval.

use crate::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub enum ReloadSignal {
    ReloadRequested,
}

/// Spawn a watcher + debouncer thread. Returns a receiver that emits `ReloadRequested`
/// signals, debounced at `quiet_for`.
///
/// `ignore_prefixes` filters notify events whose path lies under any of the given
/// directory prefixes. This is what keeps the rolling log file (which lives
/// inside the config root by default) from re-triggering a reload every write,
/// which would clear+rebuild every widget at log cadence.
pub fn watch(
    root: &Path,
    quiet_for: Duration,
    ignore_prefixes: Vec<PathBuf>,
) -> Result<(RecommendedWatcher, mpsc::Receiver<ReloadSignal>)> {
    let canonical_ignores: Vec<PathBuf> = ignore_prefixes
        .into_iter()
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
        .collect();
    let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = raw_tx.send(res);
    })
    .map_err(|e| crate::Error::Config(format!("watcher init: {e}")))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| crate::Error::Config(format!("watch {root:?}: {e}")))?;

    let (out_tx, out_rx) = mpsc::channel::<ReloadSignal>();
    std::thread::Builder::new()
        .name("juballer-deck-config-watch".into())
        .spawn(move || {
            let mut last_event: Option<Instant> = None;
            loop {
                match raw_rx.recv_timeout(quiet_for / 2) {
                    Ok(Ok(ev)) => {
                        if event_is_relevant(&ev, &canonical_ignores) {
                            last_event = Some(Instant::now());
                        }
                    }
                    Ok(Err(_)) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(t) = last_event {
                            if t.elapsed() >= quiet_for {
                                let _ = out_tx.send(ReloadSignal::ReloadRequested);
                                last_event = None;
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .map_err(|e| crate::Error::Config(format!("watch thread: {e}")))?;

    Ok((watcher, out_rx))
}

/// True if at least one of the event's paths is *not* under any ignored prefix.
/// An event with no paths is treated as relevant (defensive — better one extra
/// reload than a missed config edit).
fn event_is_relevant(ev: &notify::Event, ignores: &[PathBuf]) -> bool {
    if ev.paths.is_empty() {
        return true;
    }
    ev.paths.iter().any(|p| {
        let candidate = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        !ignores.iter().any(|ig| candidate.starts_with(ig))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_trigger_reload_signal() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("deck.toml"),
            "version = 1\nactive_profile = \"x\"\n",
        )
        .unwrap();
        let (_w, rx) = watch(dir.path(), Duration::from_millis(200), Vec::new()).unwrap();

        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(
            dir.path().join("deck.toml"),
            "version = 1\nactive_profile = \"y\"\n",
        )
        .unwrap();

        let got = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("no reload signal");
        assert!(matches!(got, ReloadSignal::ReloadRequested));
    }
}
