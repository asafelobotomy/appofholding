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
        Ok(Self {
            config_home,
            data_home,
            library_dir: home.join("Applications"),
            refresh_caches: true,
        })
    }

    /// Everything under `root`; nothing touches the real home directory.
    pub fn with_root(root: &Path) -> Self {
        Self {
            config_home: root.join("config"),
            data_home: root.join("data"),
            library_dir: root.join("Applications"),
            refresh_caches: false,
        }
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
pub fn desktop_id(id: &str) -> String {
    format!("appofholding-{id}")
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
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Some Cool App 2.0"), "some-cool-app-2-0");
        assert_eq!(slugify("  --  "), "app");
        assert_eq!(slugify("digiKam"), "digikam");
    }
}
