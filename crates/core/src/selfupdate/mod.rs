//! appOFholding updating itself.
//!
//! Where the program came from decides what "update" can mean:
//!
//! * **pacman package** (files owned by the package manager): download the release's package,
//!   verify its checksum, and hand over the exact `run0 pacman -U` command. We never overwrite
//!   files that pacman owns.
//! * **AppImage** (`$APPIMAGE` is set): download the new AppImage, verify it, and replace the
//!   running file in place, keeping the previous one next to it.
//! * **user-local or unknown installs**: tell the user how to update; nothing is changed.
//!
//! Release information comes from this project's GitHub releases.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::assets::{self, Arch, Asset, Selection};
use crate::download;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::library::{inspect, write_atomic};
use crate::notify::Notifier;
use crate::paths::Dirs;
use crate::providers;
use crate::source::{Provider, Source};
mod apply;
use crate::version;
pub use apply::*;

pub const REPO_OWNER: &str = "asafelobotomy";
pub const REPO_NAME: &str = "appofholding";
/// How long a cached "latest version" answer is trusted before asking GitHub again.
pub const CACHE_SECONDS: i64 = 6 * 3600;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Files owned by this pacman package.
    Pacman {
        package: String,
    },
    /// Running from this AppImage file.
    AppImage {
        path: PathBuf,
    },
    /// Installed somewhere under the home folder (for example `make install PREFIX=~/.local`).
    UserLocal {
        path: PathBuf,
    },
    Unknown,
}

impl InstallKind {
    pub fn label(&self) -> &'static str {
        match self {
            InstallKind::Pacman { .. } => "pacman",
            InstallKind::AppImage { .. } => "appimage",
            InstallKind::UserLocal { .. } => "local",
            InstallKind::Unknown => "unknown",
        }
    }
}

/// Decide how this copy was installed. `owner_of` asks the package manager who owns a file.
pub fn detect_install(
    exe: &Path,
    appimage_env: Option<&Path>,
    home: Option<&Path>,
    owner_of: &dyn Fn(&Path) -> Option<String>,
) -> InstallKind {
    if let Some(p) = appimage_env.filter(|p| p.is_file()) {
        return InstallKind::AppImage {
            path: p.to_path_buf(),
        };
    }
    if let Some(package) = owner_of(exe) {
        return InstallKind::Pacman { package };
    }
    if home.is_some_and(|h| exe.starts_with(h)) {
        return InstallKind::UserLocal {
            path: exe.to_path_buf(),
        };
    }
    InstallKind::Unknown
}

/// The install kind of the running program.
pub fn detect_current() -> InstallKind {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    detect_install(&exe, appimage.as_deref(), home.as_deref(), &|p| {
        let out = Command::new("pacman").arg("-Qqo").arg(p).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

#[derive(Debug, Clone)]
pub struct SelfStatus {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
    pub release_url: String,
    pub notes: Option<String>,
    pub install: InstallKind,
    /// The release file that fits this install kind and architecture, if any.
    pub asset: Option<Asset>,
    /// Answered from the local cache without asking GitHub.
    pub from_cache: bool,
}

fn arch_name(a: Arch) -> &'static str {
    match a {
        Arch::X86_64 => "x86_64",
        Arch::Aarch64 => "aarch64",
        Arch::Armv7 => "armv7h",
        Arch::X86 => "i686",
    }
}

fn asset_pattern(kind: &InstallKind, arch: Arch) -> Option<String> {
    match kind {
        InstallKind::Pacman { .. } => {
            Some(format!("appofholding-*-{}.pkg.tar.zst", arch_name(arch)))
        }
        InstallKind::AppImage { .. } => {
            Some(format!("appofholding-*{}*.AppImage", arch_name(arch)))
        }
        _ => None,
    }
}

fn release_url(tag: &str) -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/tag/{tag}")
}

fn is_newer(current: &str, latest: &str) -> bool {
    version::compare(current, latest) == Some(Ordering::Less)
}

/// Ask GitHub for the newest release and compare it with the running version.
pub fn check(http: &dyn Http, install: InstallKind, arch: Arch) -> Result<SelfStatus> {
    let src = Source::new(Provider::GitHub {
        owner: REPO_OWNER.into(),
        repo: REPO_NAME.into(),
    });
    let rel = providers::fetch_release(http, &src)?;
    let latest = rel.tag.trim_start_matches('v').to_string();
    let asset = match asset_pattern(&install, arch) {
        Some(p) => match assets::select(&rel.assets, Some(&p), arch) {
            Selection::One(a) => Some(a),
            _ => None,
        },
        None => None,
    };
    Ok(SelfStatus {
        current: current_version().into(),
        update_available: is_newer(current_version(), &latest),
        release_url: release_url(&rel.tag),
        latest,
        notes: rel.notes,
        install,
        asset,
        from_cache: false,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfState {
    pub last_checked: i64,
    pub latest: Option<String>,
    /// The version the user was last told about (so notifications fire once).
    pub notified: Option<String>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn state_path(dirs: &Dirs) -> PathBuf {
    dirs.app_data_dir().join("self-update.json")
}

pub fn load_state(dirs: &Dirs) -> SelfState {
    fs::read_to_string(state_path(dirs))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(dirs: &Dirs, s: &SelfState) -> Result<()> {
    write_atomic(
        &state_path(dirs),
        serde_json::to_string_pretty(s)
            .unwrap_or_default()
            .as_bytes(),
    )
}

/// Like [`check`], but trusts a recent answer instead of asking GitHub every time.
/// `force` always asks.
pub fn check_cached(
    http: &dyn Http,
    dirs: &Dirs,
    install: InstallKind,
    arch: Arch,
    force: bool,
) -> Result<SelfStatus> {
    let state = load_state(dirs);
    if !force
        && now() - state.last_checked < CACHE_SECONDS
        && let Some(latest) = state.latest.clone()
    {
        return Ok(SelfStatus {
            current: current_version().into(),
            update_available: is_newer(current_version(), &latest),
            release_url: release_url(&format!("v{latest}")),
            latest,
            notes: None,
            install,
            asset: None,
            from_cache: true,
        });
    }
    let status = check(http, install, arch)?;
    save_state(
        dirs,
        &SelfState {
            last_checked: now(),
            latest: Some(status.latest.clone()),
            notified: state.notified,
        },
    )?;
    Ok(status)
}

/// For the background updater: tell the user about a new release once. Returns the message if
/// a notification was sent.
pub fn notify_if_new(
    http: &dyn Http,
    notifier: &dyn Notifier,
    dirs: &Dirs,
    install: InstallKind,
    arch: Arch,
) -> Result<Option<String>> {
    let status = check_cached(http, dirs, install, arch, false)?;
    let mut state = load_state(dirs);
    if !status.update_available || state.notified.as_deref() == Some(status.latest.as_str()) {
        return Ok(None);
    }
    let how = match &status.install {
        InstallKind::Pacman { .. } => {
            "Run `appofholding self update --install` to update.".to_string()
        }
        InstallKind::AppImage { .. } => "Run `appofholding self update` to update.".to_string(),
        _ => format!("See {}", status.release_url),
    };
    let body = format!(
        "Version {} is available (you have {}). {how}",
        status.latest, status.current
    );
    notifier.notify("appOFholding: update available", &body);
    state.notified = Some(status.latest.clone());
    save_state(dirs, &state)?;
    Ok(Some(body))
}

#[cfg(test)]
mod tests;
