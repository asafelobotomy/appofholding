//! The launch model: how an app is actually started.
//!
//! A [`Profile`] (arguments, environment, command prefix, working directory, presets, ...)
//! is turned into a concrete [`Plan`] by a pure function, so what will run can always be
//! previewed (`appofholding run --print`) and tested without starting anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

impl Plan {
    /// A copy-pasteable shell rendering, environment first.
    pub fn display(&self) -> String {
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

/// Build the command to run `appimage` with `profile`. `passthrough` is what the desktop or
/// the user supplied after the app id (files, URLs, the AppImage's own default flags).
///
/// Order: `[elevator] [prefix...] appimage [preset args] [profile args] [passthrough...]`.
/// Explicit environment overrides preset environment.
pub fn plan(
    appimage: &Path,
    profile: &Profile,
    passthrough: &[String],
    elevator: Option<Elevator>,
    session_env: &[(String, String)],
) -> Result<Plan> {
    profile.validate()?;

    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut app_args: Vec<String> = Vec::new();
    for name in &profile.presets {
        let p = preset(name).expect("validated");
        app_args.extend(p.args.iter().map(|a| a.to_string()));
        env.extend(p.env.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    }
    app_args.extend(profile.args.iter().cloned());
    app_args.extend(passthrough.iter().cloned());
    env.extend(profile.env.clone());
    if profile.extract_and_run {
        // Understood by every type-2 runtime; avoids needing FUSE.
        env.insert("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into());
    }

    let mut argv: Vec<String> = Vec::new();
    let mut plan_env: Vec<(String, String)> = Vec::new();
    let mut cwd = profile.cwd.clone();

    if profile.elevated {
        let tool = elevator.ok_or_else(|| {
            Error::Other(
                "this profile runs with elevated privileges, but neither run0 nor pkexec was found"
                    .into(),
            )
        })?;
        let mut all: Vec<(String, String)> = session_env.to_vec();
        all.extend(env);
        match tool {
            Elevator::Run0 => {
                argv.push("run0".into());
                for (k, v) in &all {
                    argv.push(format!("--setenv={k}={v}"));
                }
                if let Some(d) = cwd.take() {
                    argv.push(format!("--chdir={}", d.to_string_lossy()));
                }
            }
            Elevator::Pkexec => {
                // pkexec scrubs the environment, so pass it through `env`. It cannot change directory.
                argv.extend(["pkexec".into(), "env".into()]);
                argv.extend(all.iter().map(|(k, v)| format!("{k}={v}")));
                cwd = None;
            }
        }
    } else {
        plan_env = env.into_iter().collect();
    }

    argv.extend(profile.prefix.iter().cloned());
    argv.push(appimage.to_string_lossy().into_owned());
    argv.extend(app_args);
    Ok(Plan {
        argv,
        env: plan_env,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(f: impl FnOnce(&mut Profile)) -> Profile {
        let mut x = Profile::default();
        f(&mut x);
        x
    }

    fn argv(pl: &Plan) -> Vec<&str> {
        pl.argv.iter().map(String::as_str).collect()
    }

    #[test]
    fn empty_profile_just_runs_the_appimage_with_passthrough() {
        let pl = plan(
            Path::new("/a/Foo.AppImage"),
            &Profile::default(),
            &["--x".into(), "file.txt".into()],
            None,
            &[],
        )
        .unwrap();
        assert_eq!(argv(&pl), ["/a/Foo.AppImage", "--x", "file.txt"]);
        assert!(pl.env.is_empty() && pl.cwd.is_none());
    }

    #[test]
    fn order_is_prefix_app_presets_profile_args_passthrough() {
        let prof = p(|x| {
            x.prefix = vec!["gamemoderun".into(), "-v".into()];
            x.presets = vec!["electron-wayland".into()];
            x.args = vec!["--my-flag".into()];
        });
        let pl = plan(
            Path::new("/a/F.AppImage"),
            &prof,
            &["%file".into()],
            None,
            &[],
        )
        .unwrap();
        assert_eq!(
            argv(&pl),
            [
                "gamemoderun",
                "-v",
                "/a/F.AppImage",
                "--ozone-platform=wayland",
                "--enable-wayland-ime",
                "--my-flag",
                "%file"
            ]
        );
    }

    #[test]
    fn explicit_env_beats_preset_env_and_extract_and_run_sets_the_variable() {
        let prof = p(|x| {
            x.presets = vec!["x11".into()];
            x.env.insert("GDK_BACKEND".into(), "wayland".into());
            x.env.insert("FOO".into(), "a b".into());
            x.extract_and_run = true;
        });
        let pl = plan(Path::new("/a/F.AppImage"), &prof, &[], None, &[]).unwrap();
        let get = |k: &str| pl.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("GDK_BACKEND"), Some("wayland"));
        assert_eq!(get("QT_QPA_PLATFORM"), Some("xcb"));
        assert_eq!(get("APPIMAGE_EXTRACT_AND_RUN"), Some("1"));
        assert!(pl.display().contains("FOO='a b'"), "{}", pl.display());
    }

    #[test]
    fn elevation_uses_run0_with_session_and_app_environment() {
        let prof = p(|x| {
            x.elevated = true;
            x.cwd = Some("/srv".into());
            x.env.insert("K".into(), "v".into());
            x.prefix = vec!["nice".into()];
        });
        let session = vec![("DISPLAY".to_string(), ":0".to_string())];
        let pl = plan(
            Path::new("/a/F.AppImage"),
            &prof,
            &["arg".into()],
            Some(Elevator::Run0),
            &session,
        )
        .unwrap();
        assert_eq!(
            argv(&pl),
            [
                "run0",
                "--setenv=DISPLAY=:0",
                "--setenv=K=v",
                "--chdir=/srv",
                "nice",
                "/a/F.AppImage",
                "arg"
            ]
        );
        assert!(
            pl.env.is_empty() && pl.cwd.is_none(),
            "nothing is applied to the unprivileged launcher itself"
        );
    }

    #[test]
    fn elevation_with_pkexec_goes_through_env() {
        let prof = p(|x| x.elevated = true);
        let pl = plan(
            Path::new("/a/F.AppImage"),
            &prof,
            &[],
            Some(Elevator::Pkexec),
            &[("DISPLAY".into(), ":1".into())],
        )
        .unwrap();
        assert_eq!(argv(&pl), ["pkexec", "env", "DISPLAY=:1", "/a/F.AppImage"]);
    }

    #[test]
    fn elevation_without_a_tool_is_an_error_not_a_silent_unprivileged_launch() {
        let prof = p(|x| x.elevated = true);
        let err = plan(Path::new("/a/F.AppImage"), &prof, &[], None, &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("neither run0 nor pkexec"), "{err}");
    }

    #[test]
    fn validation() {
        assert!(
            p(|x| x.presets = vec!["nope".into()])
                .validate()
                .unwrap_err()
                .to_string()
                .contains("available:")
        );
        assert!(
            p(|x| {
                x.env.insert("1BAD".into(), "x".into());
            })
            .validate()
            .is_err()
        );
        assert!(
            p(|x| {
                x.env.insert("GOOD_1".into(), "x".into());
            })
            .validate()
            .is_ok()
        );
        assert!(p(|x| x.args = vec!["a\0b".into()]).validate().is_err());
        assert!(
            valid_profile_name("debug-2")
                && !valid_profile_name("Debug")
                && !valid_profile_name("-x")
                && !valid_profile_name("")
        );
        assert!(
            PRESETS.iter().all(|p| valid_profile_name(p.name)),
            "built-in preset names follow the naming rule"
        );
    }

    #[test]
    fn display_quotes_for_the_shell() {
        let pl = Plan {
            argv: vec!["/a b/F".into(), "it's".into()],
            env: vec![],
            cwd: Some("/x y".into()),
        };
        assert_eq!(pl.display(), "cd '/x y' && '/a b/F' 'it'\\''s'");
    }

    #[test]
    fn a_plan_really_runs_with_its_args_env_and_cwd() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("app.sh");
        std::fs::write(&script, "#!/bin/sh\nprintf '%s|' \"$@\" > out.txt\nprintf '%s|%s' \"$MY_VAR\" \"$(pwd)\" >> out.txt\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let prof = p(|x| {
            x.args = vec!["one two".into()];
            x.env.insert("MY_VAR".into(), "hello".into());
            x.cwd = Some(work.clone());
        });
        let pl = plan(&script, &prof, &["last".into()], None, &[]).unwrap();
        assert!(pl.command().status().unwrap().success());
        let out = std::fs::read_to_string(work.join("out.txt")).unwrap();
        assert_eq!(
            out,
            format!(
                "one two|last|hello|{}",
                std::fs::canonicalize(&work).unwrap().display()
            )
        );
    }
}
