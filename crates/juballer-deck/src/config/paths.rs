//! Resolve on-disk paths for the deck config tree.

use std::path::PathBuf;

/// Default config directory: ${XDG_CONFIG_HOME:-~/.config}/juballer/deck
/// Windows: %APPDATA%/juballer/deck
pub fn default_config_dir() -> PathBuf {
    resolve_config_dir(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        std::env::var_os("APPDATA"),
        cfg!(target_os = "windows"),
    )
}

fn resolve_config_dir(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    appdata: Option<std::ffi::OsString>,
    is_windows: bool,
) -> PathBuf {
    if is_windows {
        if let Some(a) = appdata {
            return PathBuf::from(a).join("juballer").join("deck");
        }
        return PathBuf::from(".").join("juballer").join("deck");
    }
    if let Some(x) = xdg {
        return PathBuf::from(x).join("juballer").join("deck");
    }
    if let Some(h) = home {
        return PathBuf::from(h)
            .join(".config")
            .join("juballer")
            .join("deck");
    }
    PathBuf::from(".")
        .join(".config")
        .join("juballer")
        .join("deck")
}

#[derive(Debug, Clone)]
pub struct DeckPaths {
    pub root: PathBuf,
    pub deck_toml: PathBuf,
    pub profiles_dir: PathBuf,
    pub plugins_dir: PathBuf,
    pub state_toml: PathBuf,
}

impl DeckPaths {
    pub fn from_root(root: PathBuf) -> Self {
        let deck_toml = root.join("deck.toml");
        let profiles_dir = root.join("profiles");
        let plugins_dir = root.join("plugins");
        let state_toml = root.join("state.toml");
        Self {
            root,
            deck_toml,
            profiles_dir,
            plugins_dir,
            state_toml,
        }
    }

    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_dir.join(name)
    }

    pub fn profile_meta_toml(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("profile.toml")
    }

    pub fn profile_page_toml(&self, name: &str, page: &str) -> PathBuf {
        self.profile_dir(name)
            .join("pages")
            .join(format!("{page}.toml"))
    }

    pub fn profile_assets(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("assets")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_xdg() {
        let p = resolve_config_dir(Some("/x".into()), Some("/h".into()), None, false);
        assert_eq!(p, PathBuf::from("/x/juballer/deck"));
    }

    #[test]
    fn linux_home_fallback() {
        let p = resolve_config_dir(None, Some("/h".into()), None, false);
        assert_eq!(p, PathBuf::from("/h/.config/juballer/deck"));
    }

    #[test]
    fn windows_appdata() {
        let p = resolve_config_dir(
            None,
            None,
            Some("C:\\Users\\x\\AppData\\Roaming".into()),
            true,
        );
        let mut expected = PathBuf::from("C:\\Users\\x\\AppData\\Roaming");
        expected.push("juballer");
        expected.push("deck");
        assert_eq!(p, expected);
    }

    #[test]
    fn deck_paths_shape() {
        let p = DeckPaths::from_root(PathBuf::from("/etc/deck"));
        assert_eq!(p.deck_toml, PathBuf::from("/etc/deck/deck.toml"));
        assert_eq!(p.profiles_dir, PathBuf::from("/etc/deck/profiles"));
        assert_eq!(p.plugins_dir, PathBuf::from("/etc/deck/plugins"));
        assert_eq!(p.state_toml, PathBuf::from("/etc/deck/state.toml"));
        assert_eq!(
            p.profile_meta_toml("home"),
            PathBuf::from("/etc/deck/profiles/home/profile.toml")
        );
        assert_eq!(
            p.profile_page_toml("home", "main"),
            PathBuf::from("/etc/deck/profiles/home/pages/main.toml")
        );
    }
}
