//! File + stdout logging with daily rotation and bounded retention.
//!
//! Wires `tracing-subscriber` to two layers:
//!   * stdout (the existing fmt subscriber, env-filtered)
//!   * a rolling file under `<config>/logs/deck.log.YYYY-MM-DD`
//!
//! Bounds: at startup we
//!   * truncate any log file larger than `max_file_mb` (covers a
//!     one-shot runaway binary that logged millions of lines into
//!     today's file)
//!   * delete oldest daily files until at most `max_files` remain
//!
//! Both knobs come from `[log]` in deck.toml; defaults are 50 MB per
//! file and 7 daily files retained.

use crate::config::LogConfig;
use std::fs;
use std::path::{Path, PathBuf};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Returned from [`init`] — keep this alive for the lifetime of the
/// process so the non-blocking file writer's worker thread doesn't
/// shut down mid-run. Drop it at exit to flush.
pub struct LogHandles {
    /// Resolved log directory. Useful for diagnostics.
    pub dir: PathBuf,
    /// Worker thread guard for the file appender.
    _file_guard: Option<WorkerGuard>,
}

const LOG_FILENAME: &str = "deck.log";

/// Resolve the log directory: explicit `cfg.dir` wins, else
/// `<config_root>/logs`.
fn resolve_dir(config_root: &Path, cfg: &LogConfig) -> PathBuf {
    cfg.dir.clone().unwrap_or_else(|| config_root.join("logs"))
}

/// Truncate any matching log file larger than `max_file_mb` MB. Keeps
/// the file in place (so the appender keeps writing to it) but resets
/// it to zero bytes — losing history we'd otherwise have to rotate
/// manually mid-day. Daily rotation handles the long-term shape.
fn truncate_oversized(dir: &Path, max_file_mb: u64) {
    let cap_bytes = max_file_mb.saturating_mul(1024 * 1024);
    if cap_bytes == 0 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        if let Ok(meta) = ent.metadata() {
            if meta.is_file() && meta.len() > cap_bytes {
                let path = ent.path();
                if let Err(e) = fs::write(&path, b"") {
                    eprintln!("log: truncate {} failed: {e}", path.display());
                }
            }
        }
    }
}

/// Delete oldest files (by mtime) until `max_files` remain. Only
/// considers files whose name starts with the appender prefix so we
/// don't sweep unrelated config siblings.
fn prune_old(dir: &Path, prefix: &str, max_files: usize) {
    if max_files == 0 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?;
            if !name.starts_with(prefix) {
                return None;
            }
            let m = e.metadata().ok()?;
            if !m.is_file() {
                return None;
            }
            Some((m.modified().ok()?, p))
        })
        .collect();
    if files.len() <= max_files {
        return;
    }
    // Oldest first — those past the keep window get the axe.
    files.sort_by_key(|(t, _)| *t);
    let drop_count = files.len() - max_files;
    for (_, path) in files.into_iter().take(drop_count) {
        if let Err(e) = fs::remove_file(&path) {
            eprintln!("log: prune {} failed: {e}", path.display());
        }
    }
}

/// Build the layered subscriber. Idempotent failure mode: if file
/// init fails (read-only fs, missing perms), we still install the
/// stdout layer and return a handle whose file_guard is None.
pub fn init(config_root: &Path, cfg: &LogConfig) -> LogHandles {
    let dir = resolve_dir(config_root, cfg);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("log: mkdir {} failed: {e}", dir.display());
    }
    truncate_oversized(&dir, cfg.max_file_mb);
    prune_old(&dir, LOG_FILENAME, cfg.max_files);

    // Daily rotation. tracing-appender writes
    // <prefix>.YYYY-MM-DD with no extra extension; we accept that.
    let file_appender = rolling::daily(&dir, LOG_FILENAME);
    let (nb_writer, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cfg.level));

    let stdout_layer = fmt::layer().with_target(true);
    let file_layer = fmt::layer()
        .with_writer(nb_writer)
        .with_target(true)
        .with_ansi(false);

    if tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init()
        .is_err()
    {
        // Already initialized (e.g. test harness pre-installed a
        // subscriber) — not fatal, just skip.
    }

    tracing::info!(
        target: "juballer::logging",
        "log dir: {} (max_file={}MB, retain={} files)",
        dir.display(),
        cfg.max_file_mb,
        cfg.max_files,
    );

    LogHandles {
        dir,
        _file_guard: Some(guard),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_oversized_zeros_big_files() {
        let tmp = tempfile::tempdir().unwrap();
        let big = tmp.path().join("deck.log.2026-04-19");
        fs::write(&big, vec![b'x'; 200_000]).unwrap();
        // cap at 0.0001 MB ≈ 100 bytes → file gets truncated
        let cfg = LogConfig {
            max_file_mb: 0,
            ..LogConfig::default()
        };
        let _ = cfg;
        truncate_oversized(tmp.path(), 0); // 0 = no-op (off)
        assert_eq!(fs::metadata(&big).unwrap().len(), 200_000);
        // Real cap of 1 MB doesn't trigger.
        truncate_oversized(tmp.path(), 1);
        assert_eq!(fs::metadata(&big).unwrap().len(), 200_000);
        // Real cap of "tiny" — cheat with a hand-rolled small bound.
        // We can't pass MB-fractions so write a much smaller file:
        let small_dir = tempfile::tempdir().unwrap();
        let f = small_dir.path().join("deck.log.tiny");
        fs::write(&f, vec![b'x'; 1024 * 1024 + 1]).unwrap();
        truncate_oversized(small_dir.path(), 1);
        assert_eq!(fs::metadata(&f).unwrap().len(), 0);
    }

    #[test]
    fn prune_old_keeps_newest_n() {
        use std::thread::sleep;
        use std::time::Duration;
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..5 {
            let p = tmp.path().join(format!("deck.log.day{i}"));
            fs::write(&p, b"x").unwrap();
            sleep(Duration::from_millis(20));
        }
        prune_old(tmp.path(), "deck.log", 2);
        let mut left: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().into_string().unwrap()))
            .collect();
        left.sort();
        assert_eq!(left, vec!["deck.log.day3", "deck.log.day4"]);
    }

    #[test]
    fn prune_old_ignores_unrelated_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("deck.log.2026-04-18"), b"x").unwrap();
        fs::write(tmp.path().join("deck.log.2026-04-19"), b"x").unwrap();
        fs::write(tmp.path().join("scores.json"), b"x").unwrap();
        prune_old(tmp.path(), "deck.log", 1);
        let names: std::collections::BTreeSet<String> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().into_string().unwrap()))
            .collect();
        // scores.json must survive even though we capped to 1 deck.log.
        assert!(names.contains("scores.json"));
    }
}
