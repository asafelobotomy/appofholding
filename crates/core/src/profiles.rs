//! Library operations for launch profiles and hooks.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::db::AppRecord;
use crate::error::{Error, Result};
use crate::launch::{self, DEFAULT_PROFILE, Elevator, Plan, Profile};
use crate::library::{Library, inspect};

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

    /// The exact command that `run` would execute.
    pub fn plan_launch(
        &self,
        id: &str,
        profile: Option<&str>,
        passthrough: &[String],
        elevator: Option<Elevator>,
        session_env: &[(String, String)],
    ) -> Result<Plan> {
        let app = self.get(id)?;
        if !app.appimage_path.exists() {
            return Err(Error::Other(format!(
                "{} is missing; the AppImage was moved or deleted (re-add it, or `appofholding remove {id}`)",
                app.appimage_path.display()
            )));
        }
        let p = self.profile(id, profile.unwrap_or(DEFAULT_PROFILE))?;
        launch::plan(&app.appimage_path, &p, passthrough, elevator, session_env)
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

fn check_kind(kind: &str) -> Result<()> {
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
mod tests {
    use super::*;
    use crate::fixture;
    use crate::library::AddOptions;
    use crate::paths::Dirs;
    use std::fs;

    struct Env {
        tmp: tempfile::TempDir,
        lib: Library,
    }

    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
        let p = tmp.path().join("Foo.AppImage");
        fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
        lib.add(&p, &AddOptions::default()).unwrap();
        Env { tmp, lib }
    }

    #[test]
    fn default_profile_always_exists_others_must_be_created() {
        let e = env();
        assert_eq!(
            e.lib.profile("foo-app", "default").unwrap(),
            Profile::default()
        );
        let err = e.lib.profile("foo-app", "debug").unwrap_err().to_string();
        assert!(err.contains("no launch profile 'debug'"), "{err}");
    }

    #[test]
    fn plan_uses_the_saved_profile_and_passthrough() {
        let e = env();
        let mut p = Profile::default();
        p.presets = vec!["electron-auto".into()];
        p.args = vec!["--mine".into()];
        p.env.insert("A".into(), "1".into());
        e.lib.set_profile("foo-app", "default", &p).unwrap();
        let plan = e
            .lib
            .plan_launch(
                "foo-app",
                None,
                &["--flag".into(), "x.txt".into()],
                None,
                &[],
            )
            .unwrap();
        assert!(plan.argv[0].ends_with("Applications/Foo.AppImage"));
        assert_eq!(
            &plan.argv[1..],
            ["--ozone-platform-hint=auto", "--mine", "--flag", "x.txt"]
        );
        assert_eq!(plan.env, vec![("A".to_string(), "1".to_string())]);
    }

    #[test]
    fn extra_profiles_become_right_click_actions_and_disappear_when_deleted() {
        let e = env();
        let desktop = e.lib.get("foo-app").unwrap().desktop_file;
        let mut dbg = Profile::default();
        dbg.presets = vec!["debug".into()];
        e.lib.set_profile("foo-app", "debug", &dbg).unwrap();
        e.lib
            .set_profile("foo-app", "x11", &Profile::default())
            .unwrap();

        let d =
            crate::desktop::DesktopEntry::parse(&fs::read_to_string(&desktop).unwrap()).unwrap();
        // The AppImage's own `new` action is kept; ours are appended to the Actions list.
        assert_eq!(
            d.main("Actions"),
            Some("appofholding-debug;appofholding-x11;")
        );
        assert_eq!(
            d.get("Desktop Action appofholding-debug", "Name"),
            Some("Launch: debug")
        );
        let exec =
            crate::desktop::split_exec(d.get("Desktop Action appofholding-debug", "Exec").unwrap());
        assert_eq!(&exec[1..], ["run", "--profile", "debug", "foo-app", "%U"]);
        assert!(
            d.get("Desktop Action new", "Exec").is_some(),
            "original actions survive"
        );

        assert!(e.lib.delete_profile("foo-app", "debug").unwrap());
        let text = fs::read_to_string(&desktop).unwrap();
        assert!(
            !text.contains("appofholding-debug") && text.contains("appofholding-x11"),
            "{text}"
        );
        assert!(
            e.lib
                .plan_launch("foo-app", Some("debug"), &[], None, &[])
                .is_err()
        );
    }

    #[test]
    fn profile_names_and_contents_are_validated() {
        let e = env();
        assert!(
            e.lib
                .set_profile("foo-app", "Bad Name", &Profile::default())
                .is_err()
        );
        let mut bad = Profile::default();
        bad.presets = vec!["nope".into()];
        assert!(e.lib.set_profile("foo-app", "default", &bad).is_err());
        assert!(
            e.lib
                .set_profile("nope", "default", &Profile::default())
                .is_err()
        );
    }

    #[test]
    fn missing_appimage_is_explained() {
        let e = env();
        let app = e.lib.get("foo-app").unwrap();
        fs::remove_file(&app.appimage_path).unwrap();
        let err = e
            .lib
            .plan_launch("foo-app", None, &[], None, &[])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("is missing") && err.contains("appofholding remove foo-app"),
            "{err}"
        );
    }

    #[test]
    fn hooks_run_with_app_environment_and_report_failures() {
        let e = env();
        let out = e.tmp.path().join("hook.out");
        e.lib
            .set_hook("foo-app", "post-install", &format!("printf '%s|%s|%s' \"$APPOFHOLDING_HOOK\" \"$APPOFHOLDING_ID\" \"$APPOFHOLDING_VERSION\" > '{}'", out.display()))
            .unwrap();
        assert!(e.lib.run_hook("foo-app", "post-install").unwrap());
        assert_eq!(
            fs::read_to_string(&out).unwrap(),
            "post-install|foo-app|1.2.3"
        );
        assert!(
            !e.lib.run_hook("foo-app", "pre-update").unwrap(),
            "no hook set -> nothing ran"
        );

        e.lib
            .set_hook("foo-app", "pre-update", "echo boom >&2; exit 3")
            .unwrap();
        let err = e
            .lib
            .run_hook("foo-app", "pre-update")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("pre-update hook failed") && err.contains("boom"),
            "{err}"
        );

        assert!(e.lib.set_hook("foo-app", "whenever", "true").is_err());
        assert!(e.lib.set_hook("foo-app", "post-update", "  ").is_err());
        assert_eq!(e.lib.hooks("foo-app").unwrap().len(), 2);
        assert!(e.lib.clear_hook("foo-app", "pre-update").unwrap());
    }
}
