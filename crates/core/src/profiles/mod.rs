//! Library operations for launch profiles and hooks.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::db::AppRecord;
use crate::error::{Error, Result};
use crate::extract;
use crate::launch::{self, DEFAULT_PROFILE, Elevator, Plan, Profile};
use crate::library::{Library, inspect};
use crate::sandbox::{self, Host, Settings};

/// Per-launch context that is not part of the saved configuration.
#[derive(Default)]
pub struct LaunchCtx {
    pub elevator: Option<Elevator>,
    /// Display/session variables for elevated launches.
    pub session_env: Vec<(String, String)>,
    /// Describe the machine explicitly (tests); `None` inspects the real one.
    pub host: Option<Host>,
    /// Skip the sandbox for this launch (debugging).
    pub unsandboxed: bool,
}

pub const HOOK_KINDS: [&str; 3] = ["post-install", "pre-update", "post-update"];
const HOOK_TIMEOUT: Duration = Duration::from_secs(120);

impl Library {
    /// Every profile of an app, `default` first (an unconfigured default is not stored).
    pub fn profiles(&self, id: &str) -> Result<Vec<(String, Profile)>> {
        self.get(id)?;
        self.db.list_profiles(id)
    }

    /// One profile. `default` always exists (empty if never configured); others must exist.
    pub fn profile(&self, id: &str, name: &str) -> Result<Profile> {
        self.get(id)?;
        let all = self.db.list_profiles(id)?;
        match all.into_iter().find(|(n, _)| n == name) {
            Some((_, p)) => Ok(p),
            None if name == DEFAULT_PROFILE => Ok(Profile::default()),
            None => {
                let names: Vec<String> = self
                    .db
                    .list_profiles(id)?
                    .into_iter()
                    .map(|(n, _)| n)
                    .collect();
                Err(Error::Other(format!(
                    "'{id}' has no launch profile '{name}'{}",
                    if names.is_empty() {
                        String::new()
                    } else {
                        format!(" (profiles: {})", names.join(", "))
                    }
                )))
            }
        }
    }

    pub fn set_profile(&self, id: &str, name: &str, profile: &Profile) -> Result<()> {
        self.get(id)?;
        if name != DEFAULT_PROFILE && !launch::valid_profile_name(name) {
            return Err(Error::Other(format!(
                "'{name}' is not a valid profile name (lowercase letters, digits and '-')"
            )));
        }
        profile.validate()?;
        self.db.put_profile(id, name, profile)?;
        if name != DEFAULT_PROFILE {
            self.refresh_integration(id)?; // new right-click menu entry
        }
        Ok(())
    }

    /// Removes a profile. Deleting `default` resets it to nothing configured.
    pub fn delete_profile(&self, id: &str, name: &str) -> Result<bool> {
        self.get(id)?;
        let removed = self.db.delete_profile(id, name)?;
        if removed && name != DEFAULT_PROFILE {
            self.refresh_integration(id)?;
        }
        Ok(removed)
    }

    /// Regenerate the menu entry and icon from the AppImage on disk and the saved settings.
    pub fn refresh_integration(&self, id: &str) -> Result<()> {
        let app = self.get(id)?;
        let ins = inspect(&app.appimage_path)?;
        let (desktop_file, icon_file) = self.write_integration(&ins, id)?;
        if desktop_file != app.desktop_file || icon_file != app.icon_file {
            self.db.update_app(&AppRecord {
                desktop_file,
                icon_file,
                ..app
            })?;
        }
        self.refresh_caches_public();
        Ok(())
    }

    pub fn sandbox_settings(&self, id: &str) -> Result<Settings> {
        self.get(id)?;
        Ok(self.db.get_sandbox(id)?.unwrap_or_default())
    }

    /// Saved sandbox settings for every app that has any, in one query.
    pub fn all_sandbox_settings(&self) -> Result<std::collections::HashMap<String, Settings>> {
        self.db.list_sandbox()
    }

    pub fn set_sandbox(&self, id: &str, settings: &Settings) -> Result<Settings> {
        self.get(id)?;
        let s = settings.clone().normalized();
        s.validate()?;
        self.db.put_sandbox(id, &s)?;
        Ok(s)
    }

    /// Everything needed before a sandboxed start: the unpacked tree and the private home.
    /// Does nothing (and costs nothing) for apps without a sandbox.
    pub fn prepare_launch(&self, id: &str) -> Result<()> {
        let app = self.get(id)?;
        if !self.sandbox_settings(id)?.enabled() {
            return Ok(());
        }
        extract::ensure(&self.dirs, &app.appimage_path, &app.sha256)?;
        let home = self.dirs.private_home(id);
        std::fs::create_dir_all(&home).map_err(Error::io(&home))?;
        Ok(())
    }

    /// Delete unpacked trees that no installed or kept version needs any more.
    pub fn gc_extracted(&self) -> Result<usize> {
        let mut keep: Vec<String> = Vec::new();
        for app in self.db.list()? {
            keep.push(app.sha256.clone());
            keep.extend(
                self.db
                    .list_versions(&app.id)?
                    .into_iter()
                    .map(|v| v.sha256),
            );
        }
        extract::prune(&self.dirs, &keep)
    }

    /// The exact command that `run` would execute.
    pub fn plan_launch(
        &self,
        id: &str,
        profile: Option<&str>,
        passthrough: &[String],
        ctx: &LaunchCtx,
    ) -> Result<Plan> {
        let app = self.get(id)?;
        if !app.appimage_path.exists() {
            return Err(Error::Other(format!(
                "{} is missing; the AppImage was moved or deleted (re-add it, or `appofholding remove {id}`)",
                app.appimage_path.display()
            )));
        }
        let p = self.profile(id, profile.unwrap_or(DEFAULT_PROFILE))?;
        let settings = self.sandbox_settings(id)?;
        if !settings.enabled() || ctx.unsandboxed {
            return launch::plan(
                &app.appimage_path,
                &p,
                passthrough,
                ctx.elevator,
                &ctx.session_env,
            );
        }
        if p.elevated {
            return Err(Error::Other(
                "this launch profile runs as root, which cannot be combined with a sandbox; \
                 turn one of them off (`launch set --elevated false` or `sandbox set --level none`)"
                    .into(),
            ));
        }
        let resolved = launch::resolve(&p, passthrough)?;
        let host = match &ctx.host {
            Some(h) => h,
            None => &Host::detect()?,
        };
        let appdir = extract::path_for(&self.dirs, &app.sha256);
        let private_home = self.dirs.private_home(id);
        let proxy_socket = host.runtime_dir.join(format!("appofholding-{id}-bus"));
        let sb = sandbox::build(
            &settings,
            host,
            &sandbox::Launch {
                id,
                appdir: &appdir,
                appimage: &app.appimage_path,
                private_home: &private_home,
                proxy_socket: &proxy_socket,
                app_args: &resolved.app_args,
                env: &resolved.env,
                cwd: resolved.cwd.as_deref(),
            },
        )?;
        let mut notes = sb.notes;
        if !extract::is_extracted(&self.dirs, &app.sha256) {
            notes.push("not unpacked yet; the first sandboxed launch unpacks the app (this can take a moment)".into());
        }
        // A launch prefix (gamemoderun, nice, ...) wraps the whole sandbox from the outside.
        let mut argv = resolved.prefix;
        argv.extend(sb.argv);
        Ok(Plan {
            argv,
            env: vec![],
            cwd: None,
            helper: sb.helper,
            notes,
        })
    }

    pub fn set_hook(&self, id: &str, kind: &str, command: &str) -> Result<()> {
        self.get(id)?;
        check_kind(kind)?;
        if command.trim().is_empty() {
            return Err(Error::Other("a hook needs a command".into()));
        }
        self.db.set_hook(id, kind, command)
    }

    pub fn clear_hook(&self, id: &str, kind: &str) -> Result<bool> {
        self.get(id)?;
        check_kind(kind)?;
        self.db.clear_hook(id, kind)
    }

    pub fn hooks(&self, id: &str) -> Result<Vec<(String, String)>> {
        self.get(id)?;
        self.db.list_hooks(id)
    }

    /// Run the hook of this kind, if one is set. Returns whether one ran.
    pub fn run_hook(&self, id: &str, kind: &str) -> Result<bool> {
        let app = self.get(id)?;
        check_kind(kind)?;
        let Some((_, cmd)) = self.db.list_hooks(id)?.into_iter().find(|(k, _)| k == kind) else {
            return Ok(false);
        };
        exec_hook(&app, kind, &cmd)?;
        Ok(true)
    }

    /// Non-fatal problems collected during the last operations (e.g. a failed post-update
    /// hook). The caller decides how to show them.
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.warnings.borrow_mut())
    }

    pub(crate) fn warn(&self, msg: String) {
        self.warnings.borrow_mut().push(msg);
    }
}

pub(crate) fn check_kind(kind: &str) -> Result<()> {
    if HOOK_KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "unknown hook '{kind}' (available: {})",
            HOOK_KINDS.join(", ")
        )))
    }
}

/// Run `command` with `sh -c`, from the AppImage's directory, with `APPOFHOLDING_*` variables
/// describing the app. Killed after two minutes.
pub(crate) fn exec_hook(app: &AppRecord, kind: &str, command: &str) -> Result<()> {
    let dir = app.appimage_path.parent().unwrap_or(Path::new("/"));
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .env("APPOFHOLDING_HOOK", kind)
        .env("APPOFHOLDING_ID", &app.id)
        .env("APPOFHOLDING_NAME", &app.name)
        .env("APPOFHOLDING_PATH", &app.appimage_path)
        .env("APPOFHOLDING_VERSION", app.version.as_deref().unwrap_or(""))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("{kind} hook could not start: {e}")))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut err = String::new();
                if let Some(mut s) = child.stderr.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut err);
                }
                let tail: String = err.trim().lines().last().unwrap_or("").to_string();
                return Err(Error::Other(format!(
                    "{kind} hook failed ({status}){}",
                    if tail.is_empty() {
                        String::new()
                    } else {
                        format!(": {tail}")
                    }
                )));
            }
            Ok(None) if start.elapsed() > HOOK_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Other(format!(
                    "{kind} hook timed out after {}s",
                    HOOK_TIMEOUT.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(Error::Other(format!("{kind} hook: {e}"))),
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests;
