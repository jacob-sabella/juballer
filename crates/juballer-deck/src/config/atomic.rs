//! Atomic file writes: write to a sibling tempfile, fsync, then rename into place.
//!
//! Used by the editor write endpoints to keep the on-disk config tree consistent
//! even if the deck crashes mid-write (the watcher only sees the final contents).

use std::io::Write;
use std::path::Path;

/// Write `contents` to `dest` atomically: produce a tempfile in the same directory,
/// fsync it, then rename over the destination. On success, the file at `dest` reflects
/// the new contents in a single filesystem operation; on any failure the original file
/// (if any) is left untouched.
pub fn atomic_write(dest: &Path, contents: &[u8]) -> std::io::Result<()> {
    let dir = dest.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "atomic_write: destination has no parent directory",
        )
    })?;
    std::fs::create_dir_all(dir)?;

    let mut builder = tempfile::Builder::new();
    builder.prefix(".juballer-atomic-");
    builder.suffix(".tmp");
    let mut tmp = builder.tempfile_in(dir)?;
    tmp.write_all(contents)?;
    // Ensure bytes hit disk before the rename is observable.
    tmp.as_file().sync_all()?;
    tmp.persist(dest).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_new_file() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("sub").join("x.toml");
        atomic_write(&dest, b"hello").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
    }

    #[test]
    fn overwrites_existing_file() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("x.toml");
        std::fs::write(&dest, b"old").unwrap();
        atomic_write(&dest, b"new").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    }
}
