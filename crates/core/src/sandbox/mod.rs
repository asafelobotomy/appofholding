//! Sandboxing with bubblewrap.
//!
//! A sandboxed app runs from its unpacked tree (see [`crate::extract`]) inside a fresh set of
//! namespaces with an empty root, an empty environment, and only the pieces its settings allow:
//! network, home access, GPU, audio, display sockets, the session bus (optionally filtered),
//! cameras, and extra folders the user granted. Building the `bwrap` command is a pure
//! function of the settings and a description of the host, so it is fully testable.
//!
//! This is real isolation, but weaker than Flatpak: there is no per-app D-Bus identity or
//! portal document store, and a Chromium/Electron app has to turn off its own (nested) sandbox.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
mod args;
mod build;
pub use args::*;
pub use build::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// No sandbox: the app runs like any other program.
    #[default]
    None,
    /// Network, GPU, audio, display and a filtered session bus; private home folder.
    Standard,
    /// No network, no audio, no bus, no devices; private home folder; display and GPU only.
    Strict,
    /// Anything else (settings changed from a preset).
    Custom,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        Some(match s {
            "none" | "off" => Level::None,
            "standard" => Level::Standard,
            "strict" => Level::Strict,
            "custom" => Level::Custom,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Level::None => "none",
            Level::Standard => "standard",
            Level::Strict => "strict",
            Level::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HomeAccess {
    /// The app gets its own persistent, empty home folder.
    Private,
    /// Your real home folder, read-only; the app's config/data/cache go to a private folder.
    ReadOnly,
    /// Your real home folder, writable. (Everything else stays isolated.)
    ReadWrite,
}

impl HomeAccess {
    pub fn parse(s: &str) -> Option<HomeAccess> {
        Some(match s {
            "private" | "none" => HomeAccess::Private,
            "ro" | "readonly" | "read-only" => HomeAccess::ReadOnly,
            "rw" | "readwrite" | "read-write" => HomeAccess::ReadWrite,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            HomeAccess::Private => "private",
            HomeAccess::ReadOnly => "ro",
            HomeAccess::ReadWrite => "rw",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bus {
    Off,
    /// Through `xdg-dbus-proxy`: portals, notifications and tray icons only.
    Filtered,
    /// Your whole session bus (the app can talk to anything on it).
    Full,
}

impl Bus {
    pub fn parse(s: &str) -> Option<Bus> {
        Some(match s {
            "off" | "none" => Bus::Off,
            "filtered" | "filter" => Bus::Filtered,
            "full" | "on" => Bus::Full,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Bus::Off => "off",
            Bus::Filtered => "filtered",
            Bus::Full => "full",
        }
    }
}

/// A host folder made visible inside the sandbox at the same path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathGrant {
    pub path: PathBuf,
    pub write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub level: Level,
    pub network: bool,
    pub home: HomeAccess,
    pub gpu: bool,
    pub audio: bool,
    /// Cameras and other media devices (`/dev/video*`, `/dev/media*`).
    pub devices: bool,
    /// Wayland / X11 sockets. Off = the app cannot open windows.
    pub display: bool,
    pub bus: Bus,
    pub paths: Vec<PathGrant>,
    /// Add `--no-sandbox` for Chromium/Electron apps, whose own sandbox cannot nest inside ours.
    pub chromium_workaround: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::for_level(Level::None)
    }
}

impl Settings {
    pub fn for_level(level: Level) -> Settings {
        let base = Settings {
            level,
            network: true,
            home: HomeAccess::Private,
            gpu: true,
            audio: true,
            devices: false,
            display: true,
            bus: Bus::Filtered,
            paths: vec![],
            chromium_workaround: true,
        };
        match level {
            Level::None | Level::Standard | Level::Custom => base,
            Level::Strict => Settings {
                network: false,
                audio: false,
                bus: Bus::Off,
                ..base
            },
        }
    }

    pub fn enabled(&self) -> bool {
        self.level != Level::None
    }

    /// Mark as `custom` if the settings no longer match their preset.
    pub fn normalized(mut self) -> Settings {
        if matches!(self.level, Level::Standard | Level::Strict)
            && self != Settings::for_level(self.level)
        {
            self.level = Level::Custom;
        }
        self
    }

    pub fn validate(&self) -> Result<()> {
        for g in &self.paths {
            let shown = g.path.display();
            if !g.path.is_absolute() {
                return Err(Error::Other(format!("{shown} is not an absolute path")));
            }
            if g.path.as_os_str().as_encoded_bytes().contains(&0) {
                return Err(Error::Other("paths cannot contain NUL bytes".into()));
            }
            // `..` could climb out of an allowed folder into a forbidden one (`/usr/../run`).
            if g.path.components().any(|c| c == Component::ParentDir) {
                return Err(Error::Other(format!(
                    "{shown} contains `..`; give the folder's real path"
                )));
            }
            // `components` already drops `.` and collapses repeated slashes (`//run` -> `/run`);
            // a symlink to a forbidden place is caught through its resolved path.
            let lexical: PathBuf = g.path.components().collect();
            let resolved = std::fs::canonicalize(&g.path).ok();
            if forbidden_share(&lexical) || resolved.as_deref().is_some_and(forbidden_share) {
                return Err(Error::Other(format!(
                    "{shown} cannot be shared with a sandbox"
                )));
            }
        }
        Ok(())
    }
}

/// The root and the kernel/runtime trees, which a sandbox must never see from the host.
fn forbidden_share(p: &Path) -> bool {
    p == Path::new("/")
        || ["/proc", "/sys", "/dev", "/run"]
            .iter()
            .any(|f| p.starts_with(f))
}

/// What the sandbox builder needs to know about the machine.
pub struct Host {
    pub uid: u32,
    pub home: PathBuf,
    pub runtime_dir: PathBuf,
    /// Selected environment variables (locale, desktop, user name, ...).
    pub env: BTreeMap<String, String>,
    pub wayland_display: Option<String>,
    pub x_display: Option<String>,
    pub xauthority: Option<PathBuf>,
    pub dbus_address: Option<String>,
    pub exists: Box<dyn Fn(&Path) -> bool>,
    /// Entries of `dir` whose names start with `prefix` (e.g. `/dev`, `nvidia`).
    pub list: Box<ListFn>,
}

/// Lists the entries of a directory whose names start with a prefix.
pub type ListFn = dyn Fn(&Path, &str) -> Vec<PathBuf>;

/// Variables copied from the launching environment when present.
const PASS_ENV: &[&str] = &[
    "USER",
    "LOGNAME",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_TIME",
    "TZ",
    "TERM",
    "COLORTERM",
    "XDG_CURRENT_DESKTOP",
    "DESKTOP_SESSION",
    "XCURSOR_THEME",
    "XCURSOR_SIZE",
];

impl Host {
    pub fn detect() -> Result<Host> {
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata("/proc/self")
            .map_err(Error::io("/proc/self"))?
            .uid();
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let home = var("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::Other("$HOME is not set".into()))?;
        let runtime_dir = var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
        let env = PASS_ENV
            .iter()
            .filter_map(|k| var(k).map(|v| (k.to_string(), v)))
            .collect();
        Ok(Host {
            uid,
            home,
            runtime_dir,
            env,
            wayland_display: var("WAYLAND_DISPLAY"),
            x_display: var("DISPLAY"),
            xauthority: var("XAUTHORITY").map(PathBuf::from),
            dbus_address: var("DBUS_SESSION_BUS_ADDRESS"),
            exists: Box::new(|p| p.exists()),
            list: Box::new(|dir, prefix| {
                std::fs::read_dir(dir)
                    .map(|rd| {
                        let mut v: Vec<PathBuf> = rd
                            .flatten()
                            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
                            .map(|e| e.path())
                            .collect();
                        v.sort();
                        v
                    })
                    .unwrap_or_default()
            }),
        })
    }
}

/// A helper process that must be running (and have created `ready`) before the sandbox starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Helper {
    pub argv: Vec<String>,
    pub ready: PathBuf,
}

#[derive(Debug)]
pub struct Sandboxed {
    /// `bwrap ... -- <command>`; the caller puts any launch prefix in front.
    pub argv: Vec<String>,
    pub helper: Option<Helper>,
    /// Things the user should know (applied workarounds, missing sockets, ...).
    pub notes: Vec<String>,
}

pub struct Launch<'a> {
    /// The unpacked AppImage.
    pub appdir: &'a Path,
    /// The original AppImage (exposed read-only as `$APPIMAGE`).
    pub appimage: &'a Path,
    pub private_home: &'a Path,
    /// Where the filtered-bus socket is created (only used for `Bus::Filtered`).
    pub proxy_socket: &'a Path,
    pub app_args: &'a [String],
    pub env: &'a BTreeMap<String, String>,
    pub cwd: Option<&'a Path>,
}

#[cfg(test)]
mod tests;
