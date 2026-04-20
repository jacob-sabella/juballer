//! Auto-detect the project file loaded by a running Carla process.
//!
//! When the user doesn't set `[carla].project = "…"` in their
//! configuration, we still want name resolution to "just work" if a
//! Carla session is already running. Linux exposes every process's
//! launch arguments under `/proc/<pid>/cmdline`; we grep for a
//! `*.carxp` argument on any process whose `comm` mentions `carla`.
//!
//! Returns `None` on non-Linux platforms or when no carla process is
//! running. The caller treats that the same as an empty NameMap — the
//! deck still launches; cells using named refs just don't resolve.

use std::path::PathBuf;

/// Sniff `/proc` for a running carla process and return the `*.carxp`
/// path it was launched with. First match wins; multiple Carla
/// instances are unusual but the order is `/proc` enumeration order
/// (deterministic per kernel but not stable across reboots).
pub fn detect_running_project() -> Option<PathBuf> {
    detect_in(&LiveProc)
}

/// Trait abstraction over `/proc` so the unit tests can drive a
/// synthetic process table without touching the real filesystem.
trait ProcSource {
    fn entries(&self) -> Box<dyn Iterator<Item = ProcEntry> + '_>;
}

#[derive(Debug, Clone)]
struct ProcEntry {
    comm: String,
    cmdline: Vec<String>,
}

struct LiveProc;

impl ProcSource for LiveProc {
    fn entries(&self) -> Box<dyn Iterator<Item = ProcEntry> + '_> {
        let it = read_live_proc();
        Box::new(it.into_iter())
    }
}

#[cfg(target_os = "linux")]
fn read_live_proc() -> Vec<ProcEntry> {
    let mut out = Vec::new();
    let dir = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return out,
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let comm = std::fs::read_to_string(entry.path().join("comm"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let cmdline_bytes = match std::fs::read(entry.path().join("cmdline")) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if cmdline_bytes.is_empty() {
            continue;
        }
        let args: Vec<String> = cmdline_bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        out.push(ProcEntry {
            comm,
            cmdline: args,
        });
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn read_live_proc() -> Vec<ProcEntry> {
    // Windows / macOS don't expose /proc; auto-detect is a Linux-only
    // convenience. The deck still launches without it.
    Vec::new()
}

fn detect_in(source: &dyn ProcSource) -> Option<PathBuf> {
    source.entries().find_map(|entry| {
        if !looks_like_carla(&entry) {
            return None;
        }
        carxp_arg(&entry.cmdline)
    })
}

/// True for processes whose name OR cmdline mentions carla. The arch
/// `carla` package wraps a Python entry point — `comm` is `python3` —
/// so we also scan the cmdline for "carla" or "carla-rack" tokens.
fn looks_like_carla(entry: &ProcEntry) -> bool {
    if entry.comm.contains("carla") {
        return true;
    }
    entry.cmdline.iter().any(|arg| {
        let basename = std::path::Path::new(arg)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        basename == "carla"
            || basename == "carla-rack"
            || basename == "carla-jack-multi"
            || arg.ends_with("/share/carla/carla")
    })
}

/// Pick the first arg that looks like a `*.carxp` path.
fn carxp_arg(cmdline: &[String]) -> Option<PathBuf> {
    cmdline
        .iter()
        .filter(|arg| arg.ends_with(".carxp"))
        .map(PathBuf::from)
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProc(Vec<ProcEntry>);

    impl ProcSource for FakeProc {
        fn entries(&self) -> Box<dyn Iterator<Item = ProcEntry> + '_> {
            Box::new(self.0.clone().into_iter())
        }
    }

    fn entry(comm: &str, cmdline: &[&str]) -> ProcEntry {
        ProcEntry {
            comm: comm.into(),
            cmdline: cmdline.iter().map(|s| (*s).into()).collect(),
        }
    }

    #[test]
    fn detect_skips_processes_unrelated_to_carla() {
        let src = FakeProc(vec![
            entry("zsh", &["/usr/bin/zsh"]),
            entry("rustc", &["/usr/bin/rustc", "--edition=2021"]),
        ]);
        assert!(detect_in(&src).is_none());
    }

    #[test]
    fn detect_returns_carxp_arg_for_native_carla_binary() {
        let src = FakeProc(vec![entry(
            "carla-rack",
            &["/usr/bin/carla-rack", "/home/user/live.carxp"],
        )]);
        assert_eq!(
            detect_in(&src),
            Some(PathBuf::from("/home/user/live.carxp"))
        );
    }

    #[test]
    fn detect_handles_arch_python_wrapper_with_carla_in_cmdline() {
        // Mirrors what `cat /proc/.../cmdline` shows on Arch:
        // `python3 /usr/share/carla/carla --with-appname=... /tmp/x.carxp`.
        let src = FakeProc(vec![entry(
            "python3",
            &[
                "/usr/bin/python3",
                "/usr/share/carla/carla",
                "--with-appname=/usr/bin/carla",
                "--no-gui",
                "/tmp/probe.carxp",
            ],
        )]);
        assert_eq!(detect_in(&src), Some(PathBuf::from("/tmp/probe.carxp")));
    }

    #[test]
    fn detect_returns_none_when_carla_running_without_a_project_arg() {
        // Carla launched with no project file (cold start). We don't
        // synthesise a project, we just don't help with name resolution.
        let src = FakeProc(vec![entry("carla", &["/usr/bin/carla"])]);
        assert!(detect_in(&src).is_none());
    }

    #[test]
    fn detect_first_carla_wins_when_multiple_processes_present() {
        let src = FakeProc(vec![
            entry("carla", &["/usr/bin/carla", "/tmp/first.carxp"]),
            entry("carla", &["/usr/bin/carla", "/tmp/second.carxp"]),
        ]);
        let result = detect_in(&src).unwrap();
        let stem = result.file_stem().and_then(|s| s.to_str()).unwrap();
        assert!(
            stem == "first" || stem == "second",
            "either first or second is acceptable; got {stem}"
        );
    }

    #[test]
    fn looks_like_carla_recognises_the_share_dir_python_wrapper() {
        let e = entry("python3", &["python3", "/usr/share/carla/carla"]);
        assert!(looks_like_carla(&e));
    }
}
