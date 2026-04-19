//! Subcommand-launch + graceful-return helpers shared across subcommands.
//!
//! A host process (e.g. the deck) `exec()`s into a child subcommand and sets
//! `JUBALLER_RETURN_TO` so the child knows where to land on exit. The child calls [`exit`]
//! at every "user wants out" site (ESC, corner-hold, EXIT cell, natural finish) and either
//! re-execs into the host or, if no return target is set, falls through to
//! `process::exit(code)`.
//!
//! Targets:
//!   * `deck`   → exec the current binary with no args (returns to `DeckApp`)
//!   * `picker` → exec `play` (returns to chart-select)
//!
//! The `picker` arm sets `JUBALLER_RETURN_TO=deck` for the child so the picker's own EXIT
//! still goes home; `picker` is a one-shot re-entry, not a sticky mode.

use std::os::unix::process::CommandExt;

pub const RETURN_ENV: &str = "JUBALLER_RETURN_TO";

/// Exit the current process. Honours `JUBALLER_RETURN_TO`:
/// - `deck`   → re-exec into `DeckApp` (no args)
/// - `picker` → re-exec `play` (no chart arg, opens chart-select picker)
/// - anything else / unset → `std::process::exit(code)`
pub fn exit(code: i32) -> ! {
    let target = std::env::var(RETURN_ENV).ok();
    if let Some(t) = target.as_deref() {
        match t {
            "deck" => {
                std::env::remove_var(RETURN_ENV);
                if let Ok(exe) = std::env::current_exe() {
                    let err = std::process::Command::new(&exe).exec();
                    eprintln!("return-to-deck exec failed: {err}");
                }
            }
            "picker" => {
                // Restore the deck-return semantics for the new picker
                // process so its own EXIT cell still goes home.
                std::env::set_var(RETURN_ENV, "deck");
                if let Ok(exe) = std::env::current_exe() {
                    let err = std::process::Command::new(&exe).arg("play").exec();
                    eprintln!("return-to-picker exec failed: {err}");
                }
            }
            _ => {}
        }
    }
    std::process::exit(code);
}
