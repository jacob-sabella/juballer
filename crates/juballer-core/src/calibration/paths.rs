use std::path::PathBuf;

/// Resolve the on-disk path of `profile.toml` per platform conventions.
/// Linux:   $XDG_CONFIG_HOME/juballer/profile.toml  (or ~/.config/juballer/profile.toml)
/// Windows: %APPDATA%\juballer\profile.toml
pub fn default_profile_path() -> PathBuf {
    profile_path_inner(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        std::env::var_os("APPDATA"),
        cfg!(target_os = "windows"),
    )
}

fn profile_path_inner(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    appdata: Option<std::ffi::OsString>,
    is_windows: bool,
) -> PathBuf {
    if is_windows {
        if let Some(a) = appdata {
            return PathBuf::from(a).join("juballer").join("profile.toml");
        }
        // Fallback: cwd if APPDATA missing (very unusual).
        return PathBuf::from(".").join("juballer").join("profile.toml");
    }
    if let Some(x) = xdg {
        return PathBuf::from(x).join("juballer").join("profile.toml");
    }
    if let Some(h) = home {
        return PathBuf::from(h)
            .join(".config")
            .join("juballer")
            .join("profile.toml");
    }
    PathBuf::from(".")
        .join(".config")
        .join("juballer")
        .join("profile.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_uses_xdg_when_set() {
        let p = profile_path_inner(Some("/x".into()), Some("/h".into()), None, false);
        assert_eq!(p, PathBuf::from("/x/juballer/profile.toml"));
    }

    #[test]
    fn linux_falls_back_to_home() {
        let p = profile_path_inner(None, Some("/h".into()), None, false);
        assert_eq!(p, PathBuf::from("/h/.config/juballer/profile.toml"));
    }

    #[test]
    fn windows_uses_appdata() {
        let p = profile_path_inner(
            None,
            None,
            Some("C:\\Users\\jacob\\AppData\\Roaming".into()),
            true,
        );
        assert_eq!(
            p,
            PathBuf::from("C:\\Users\\jacob\\AppData\\Roaming")
                .join("juballer")
                .join("profile.toml")
        );
    }
}
