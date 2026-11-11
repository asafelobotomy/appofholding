//! XDG-aware directory layout. Everything the library writes goes through [`Dirs`], so tests
//! (and `APPOFHOLDING_ROOT` in the CLI) can redirect the whole tree into a throwaway directory.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Dirs {
    /// `$XDG_DATA_HOME` (default `~/.local/share`).
    pub data_home: PathBuf,
    /// `$XDG_CONFIG_HOME` (default `~/.config`).
    pub config_home: PathBuf,
    /// `$XDG_CACHE_HOME` (default `~/.cache`): extracted AppImages for sandboxed launches.
    pub cache_home: PathBuf,
    /// Where AppImages are stored (default `~/Applications`).
    pub library_dir: PathBuf,
    /// Run `update-desktop-database` after changes (off in tests).
    pub refresh_caches: bool,
}

impl Dirs {
    pub fn from_env() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| Error::Other("$HOME is not set".into()))?;
        // The XDG spec says relative values must be ignored.
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        let cache_home = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".cache"));
        Ok(Self {
            cache_home,
            config_home,
            data_home,
            library_dir: home.join("Applications"),
            refresh_caches: true,
        })
    }

    /// The real directories, or everything under `$APPOFHOLDING_ROOT` when that is set (used by
    /// tests and for trying things out without touching your home directory).
    pub fn detect() -> Result<Self> {
        match std::env::var_os("APPOFHOLDING_ROOT") {
            Some(root) if !root.is_empty() => Ok(Self::with_root(&PathBuf::from(root))),
            _ => Self::from_env(),
        }
    }

    /// Everything under `root`; nothing touches the real home directory.
    pub fn with_root(root: &Path) -> Self {
        Self {
            cache_home: root.join("cache"),
            config_home: root.join("config"),
            data_home: root.join("data"),
            library_dir: root.join("Applications"),
            refresh_caches: false,
        }
    }

    /// Where AppImages are unpacked for sandboxed launches, one directory per SHA-256.
    pub fn extract_dir(&self) -> PathBuf {
        self.cache_home.join("appofholding/extracted")
    }

    /// Persistent, private per-app home directory used by sandboxed launches.
    pub fn private_home(&self, id: &str) -> PathBuf {
        self.app_data_dir().join("apps").join(id).join("home")
    }

    pub fn systemd_user_dir(&self) -> PathBuf {
        self.config_home.join("systemd/user")
    }

    /// Previous versions of an app live here, for rollback.
    pub fn versions_dir(&self, id: &str) -> PathBuf {
        self.app_data_dir().join("apps").join(id).join("versions")
    }

    pub fn applications_dir(&self) -> PathBuf {
        self.data_home.join("applications")
    }

    pub fn icons_dir(&self) -> PathBuf {
        self.data_home.join("icons")
    }

    pub fn app_data_dir(&self) -> PathBuf {
        self.data_home.join("appofholding")
    }

    pub fn db_path(&self) -> PathBuf {
        self.app_data_dir().join("db.sqlite")
    }

    pub fn desktop_file(&self, id: &str) -> PathBuf {
        self.applications_dir()
            .join(format!("{}.desktop", desktop_id(id)))
    }
}

/// Name (without extension) used for the generated `.desktop` file and icon.
/// The path to write into menu entries and the timer unit for a program at `exe`. When
/// appOFholding itself runs as an AppImage, `exe` is inside its temporary mount, which vanishes
/// when it exits; the AppImage file itself (`$APPIMAGE`) is the stable path, and it hands `run`
/// and `update` on to the command-line tool.
pub fn stable_exec_path(exe: PathBuf) -> PathBuf {
    stable_exec_path_with(
        exe,
        std::env::var_os("APPIMAGE").map(PathBuf::from),
        std::env::var_os("APPDIR").map(PathBuf::from),
    )
}

fn stable_exec_path_with(
    exe: PathBuf,
    appimage: Option<PathBuf>,
    appdir: Option<PathBuf>,
) -> PathBuf {
    match (appimage, appdir) {
        (Some(image), Some(dir)) if exe.starts_with(&dir) && image.is_absolute() => image,
        _ => exe,
    }
}

pub fn desktop_id(id: &str) -> String {
    format!("appofholding-{id}")
}

/// The app id in a menu entry's desktop id (`appofholding-<id>.desktop`, the inverse of
/// [`desktop_id`]); `None` for an entry appOFholding did not write. Whether that app is in the
/// library is the caller's question.
pub fn app_id_from_desktop_id(desktop_id: &str) -> Option<&str> {
    let id = desktop_id
        .strip_suffix(".desktop")
        .unwrap_or(desktop_id)
        .strip_prefix("appofholding-")?;
    let ok = !id.is_empty()
        && !id.starts_with(['-', '.'])
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    ok.then_some(id)
}

/// `Some Cool App 2.0` -> `some-cool-app-2-0`. Never empty.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "app".into() } else { out }
}

#[cfg(test)]
mod tests {
    #[test]
    fn inside_its_own_appimage_the_appimage_file_is_the_stable_path() {
        use std::path::PathBuf;
        let exe = PathBuf::from("/tmp/.mount_abc/usr/bin/appofholding");
        assert_eq!(
            super::stable_exec_path_with(
                exe.clone(),
                Some("/home/u/Applications/appofholding.AppImage".into()),
                Some("/tmp/.mount_abc".into())
            ),
            PathBuf::from("/home/u/Applications/appofholding.AppImage")
        );
        // Started by some other AppImage (inherited variables), or installed normally.
        assert_eq!(
            super::stable_exec_path_with(
                PathBuf::from("/usr/bin/appofholding"),
                Some("/x/Other.AppImage".into()),
                Some("/tmp/.mount_zzz".into())
            ),
            PathBuf::from("/usr/bin/appofholding")
        );
        assert_eq!(super::stable_exec_path_with(exe.clone(), None, None), exe);
    }

    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Some Cool App 2.0"), "some-cool-app-2-0");
        assert_eq!(slugify("  --  "), "app");
        assert_eq!(slugify("digiKam"), "digikam");
    }
}

#[cfg(test)]
mod desktop_id_tests {
    use super::*;

    #[test]
    fn desktop_ids_map_back_to_app_ids() {
        assert_eq!(
            app_id_from_desktop_id(&format!("{}.desktop", desktop_id("krita"))),
            Some("krita")
        );
        assert_eq!(
            app_id_from_desktop_id("appofholding-some-cool-app-2-0.desktop"),
            Some("some-cool-app-2-0")
        );
        assert_eq!(app_id_from_desktop_id("appofholding-krita"), Some("krita"));
        for bad in [
            "krita.desktop",
            "appofholding-.desktop",
            "appofholding--x.desktop",
            "appofholding-.hidden.desktop",
            "appofholding-a b.desktop",
            "appofholding-../x.desktop",
            "io.github.appofholding.desktop",
        ] {
            assert_eq!(app_id_from_desktop_id(bad), None, "{bad}");
        }
    }
}
