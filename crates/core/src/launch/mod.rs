//! The launch model: how an app is actually started.
//!
//! A [`Profile`] (arguments, environment, command prefix, working directory, presets, ...)
//! is turned into a concrete [`Plan`] by a pure function, so what will run can always be
//! previewed (`appofholding run --print`) and tested without starting anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::sandbox::Helper;
mod planning;
pub use planning::*;

pub const DEFAULT_PROFILE: &str = "default";

/// Saved launch settings for one app (or one named profile of it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// Extra arguments, placed before whatever the desktop passes (files, URLs).
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Command and arguments placed in front of the app, e.g. `["gamemoderun"]`.
    pub prefix: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Unpack to a temporary directory instead of mounting with FUSE.
    pub extract_and_run: bool,
    /// Start with root privileges through `run0` / `pkexec`. Off unless asked for.
    pub elevated: bool,
    /// Names from [`PRESETS`], applied before the explicit settings above.
    pub presets: Vec<String>,
}

pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub args: &'static [&'static str],
    pub env: &'static [(&'static str, &'static str)],
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "electron-wayland",
        description: "Electron/Chromium apps: run natively on Wayland",
        args: &["--ozone-platform=wayland", "--enable-wayland-ime"],
        env: &[],
    },
    Preset {
        name: "electron-auto",
        description: "Electron/Chromium apps: pick Wayland or X11 automatically",
        args: &["--ozone-platform-hint=auto"],
        env: &[],
    },
    Preset {
        name: "x11",
        description: "Force X11 (XWayland) for GTK, Qt and Chromium based apps",
        args: &["--ozone-platform=x11"],
        env: &[("GDK_BACKEND", "x11"), ("QT_QPA_PLATFORM", "xcb")],
    },
    Preset {
        name: "gtk-wayland",
        description: "GTK apps: prefer Wayland",
        args: &[],
        env: &[("GDK_BACKEND", "wayland,x11")],
    },
    Preset {
        name: "qt-wayland",
        description: "Qt apps: prefer Wayland",
        args: &[],
        env: &[("QT_QPA_PLATFORM", "wayland;xcb")],
    },
    Preset {
        name: "no-sandbox",
        description: "Electron/Chromium apps that fail with a chrome-sandbox error",
        args: &["--no-sandbox"],
        env: &[],
    },
    Preset {
        name: "disable-gpu",
        description: "Electron/Chromium apps with a blank window or GPU crashes",
        args: &["--disable-gpu"],
        env: &[],
    },
    Preset {
        name: "nvidia-offload",
        description: "Run on the discrete NVIDIA GPU (PRIME render offload)",
        args: &[],
        env: &[
            ("__NV_PRIME_RENDER_OFFLOAD", "1"),
            ("__GLX_VENDOR_LIBRARY_NAME", "nvidia"),
        ],
    },
    Preset {
        name: "debug",
        description: "Verbose logging for GLib and Electron apps",
        args: &[],
        env: &[
            ("G_MESSAGES_DEBUG", "all"),
            ("ELECTRON_ENABLE_LOGGING", "1"),
        ],
    },
];

pub fn preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}

/// Which program raises privileges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevator {
    Run0,
    Pkexec,
}

/// `run0` (systemd) if installed, else `pkexec`.
pub fn detect_elevator() -> Option<Elevator> {
    let on_path = |name: &str| {
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
    };
    if on_path("run0") {
        Some(Elevator::Run0)
    } else if on_path("pkexec") {
        Some(Elevator::Pkexec)
    } else {
        None
    }
}

/// Variables a root-launched GUI app needs to reach the user's display and session.
pub const SESSION_ENV: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// A concrete command line, ready to run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    /// A process that must be running before `argv` starts (the filtered D-Bus proxy).
    pub helper: Option<Helper>,
    /// Things worth telling the user (workarounds applied, missing sockets, ...).
    pub notes: Vec<String>,
}

impl Plan {
    /// A copy-pasteable shell rendering, environment first.
    pub fn display(&self) -> String {
        let helper = self
            .helper
            .as_ref()
            .map(|h| {
                format!(
                    "# first, in the background: {}\n",
                    h.argv
                        .iter()
                        .map(|a| shell_quote(a))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            })
            .unwrap_or_default();
        helper + &self.display_command()
    }

    fn display_command(&self) -> String {
        let mut parts: Vec<String> = self
            .env
            .iter()
            .map(|(k, v)| format!("{k}={}", shell_quote(v)))
            .collect();
        parts.extend(self.argv.iter().map(|a| shell_quote(a)));
        let line = parts.join(" ");
        match &self.cwd {
            Some(d) => format!("cd {} && {line}", shell_quote(&d.to_string_lossy())),
            None => line,
        }
    }

    /// Run to completion and return the exit code, managing the helper process (if any).
    /// Without a helper, callers can simply `exec` [`Plan::command`] instead.
    pub fn run(&self) -> Result<i32> {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Stdio;
        use std::time::{Duration, Instant};

        let mut helper_child = None;
        let mut keep_alive = None; // dropping this ends the helper
        if let Some(h) = &self.helper {
            let _ = std::fs::remove_file(&h.ready); // a stale socket would look like success
            let (reader, writer) =
                std::io::pipe().map_err(|e| Error::Other(format!("pipe: {e}")))?;
            keep_alive = Some(writer);
            let mut c = Command::new(&h.argv[0])
                .args(&h.argv[1..])
                .stdin(Stdio::from(reader))
                .stdout(Stdio::null())
                .spawn()
                .map_err(|e| {
                    Error::Other(format!("could not start {} ({e}); install xdg-dbus-proxy or use `--bus off` / `--bus full`", h.argv[0]))
                })?;
            let start = Instant::now();
            while !h.ready.exists() {
                if let Ok(Some(status)) = c.try_wait() {
                    return Err(Error::Other(format!(
                        "{} exited early ({status})",
                        h.argv[0]
                    )));
                }
                if start.elapsed() > Duration::from_secs(5) {
                    let _ = c.kill();
                    return Err(Error::Other(format!(
                        "{} did not become ready in time",
                        h.argv[0]
                    )));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            helper_child = Some(c);
        }
        let status = self
            .command()
            .status()
            .map_err(|e| Error::Other(format!("failed to start {}: {e}", self.argv[0])));
        if let Some(mut c) = helper_child {
            let _ = c.kill();
            let _ = c.wait();
            if let Some(h) = &self.helper {
                let _ = std::fs::remove_file(&h.ready);
            }
        }
        drop(keep_alive);
        let status = status?;
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)))
    }

    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.argv[0]);
        c.args(&self.argv[1..]);
        c.envs(self.env.iter().map(|(k, v)| (k, v)));
        if let Some(d) = &self.cwd {
            c.current_dir(d);
        }
        c
    }
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub fn valid_env_name(k: &str) -> bool {
    let mut cs = k.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Profile and preset names that are safe in file names and `.desktop` group names.
pub fn valid_profile_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 32
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !n.starts_with('-')
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        for k in self.env.keys() {
            if !valid_env_name(k) {
                return Err(Error::Other(format!(
                    "'{k}' is not a valid environment variable name"
                )));
            }
        }
        for p in &self.presets {
            if preset(p).is_none() {
                let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
                return Err(Error::Other(format!(
                    "unknown preset '{p}' (available: {})",
                    names.join(", ")
                )));
            }
        }
        let has_nul = |s: &String| s.contains('\0');
        if self.args.iter().any(has_nul)
            || self.prefix.iter().any(has_nul)
            || self.env.values().any(has_nul)
        {
            return Err(Error::Other(
                "launch settings cannot contain NUL bytes".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
