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
#[allow(clippy::field_reassign_with_default)]
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
                &LaunchCtx::default(),
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
                .plan_launch("foo-app", Some("debug"), &[], &LaunchCtx::default())
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

    // ---- sandbox integration

    use crate::sandbox::{Bus, Level};
    use std::collections::BTreeMap;

    fn test_host(root: &Path) -> Host {
        Host {
            uid: 1000,
            home: root.join("realhome"),
            runtime_dir: root.join("run"),
            env: BTreeMap::new(),
            wayland_display: None,
            x_display: None,
            xauthority: None,
            dbus_address: None,
            exists: Box::new(|p| p.exists()),
            list: Box::new(|_, _| vec![]),
        }
    }

    fn ctx(root: &Path) -> LaunchCtx {
        LaunchCtx {
            host: Some(test_host(root)),
            ..Default::default()
        }
    }

    fn quiet_strict() -> Settings {
        Settings {
            gpu: false,
            audio: false,
            display: false,
            bus: Bus::Off,
            ..Settings::for_level(Level::Strict)
        }
    }

    /// An app whose AppRun reports its environment, added to a fresh library.
    fn sandboxed_env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
        std::fs::create_dir_all(tmp.path().join("realhome")).unwrap();
        let desktop = "[Desktop Entry]\nType=Application\nName=Probe\nExec=AppRun\n";
        let apprun = "#!/bin/sh\necho \"HOME=$HOME\"\necho \"ARGS=$*\"\necho \"NET=$(grep -c : /proc/net/dev)\"\necho hi > \"$HOME/made\"\n";
        let bytes = fixture::build_appimage(
            None,
            vec![
                fixture::Item::File("/probe.desktop", desktop.as_bytes().to_vec()),
                fixture::Item::File("/AppRun", apprun.as_bytes().to_vec()),
            ],
        );
        let p = tmp.path().join("Probe.AppImage");
        fs::write(&p, bytes).unwrap();
        lib.add(&p, &AddOptions::default()).unwrap();
        Env { tmp, lib }
    }

    fn bwrap_ok() -> bool {
        std::process::Command::new("bwrap")
            .args([
                "--unshare-all",
                "--ro-bind",
                "/usr",
                "/usr",
                "--symlink",
                "usr/bin",
                "/bin",
                "--symlink",
                "usr/lib",
                "/lib",
                "--symlink",
                "usr/lib",
                "/lib64",
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "/bin/true",
            ])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn sandbox_defaults_off_and_settings_are_validated_and_normalised() {
        let e = env();
        assert!(!e.lib.sandbox_settings("foo-app").unwrap().enabled());
        let plain = e
            .lib
            .plan_launch("foo-app", None, &[], &LaunchCtx::default())
            .unwrap();
        assert!(
            plain.argv[0].ends_with("Applications/Foo.AppImage"),
            "no sandbox -> the AppImage runs directly"
        );

        let tweaked = Settings {
            network: false,
            ..Settings::for_level(Level::Standard)
        };
        assert_eq!(
            e.lib.set_sandbox("foo-app", &tweaked).unwrap().level,
            Level::Custom
        );
        assert_eq!(
            e.lib.sandbox_settings("foo-app").unwrap().level,
            Level::Custom
        );
        let bad = Settings {
            paths: vec![crate::sandbox::PathGrant {
                path: "/".into(),
                write: false,
            }],
            ..Settings::for_level(Level::Standard)
        };
        assert!(e.lib.set_sandbox("foo-app", &bad).is_err());
        assert!(e.lib.set_sandbox("nope", &tweaked).is_err());
    }

    #[test]
    fn sandboxed_plan_wraps_with_bwrap_puts_the_prefix_outside_and_can_be_bypassed() {
        let e = sandboxed_env();
        e.lib.set_sandbox("probe", &quiet_strict()).unwrap();
        let mut prof = Profile::default();
        prof.prefix = vec!["nice".into()];
        prof.args = vec!["--mine".into()];
        e.lib.set_profile("probe", "default", &prof).unwrap();

        let plan = e
            .lib
            .plan_launch("probe", None, &["x.txt".into()], &ctx(e.tmp.path()))
            .unwrap();
        assert_eq!(
            &plan.argv[..2],
            ["nice", "bwrap"],
            "the prefix wraps the sandbox from outside"
        );
        let dd = plan.argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(&plan.argv[dd + 1..], ["/app/AppRun", "--mine", "x.txt"]);
        assert!(
            plan.notes.iter().any(|n| n.contains("not unpacked yet")),
            "{:?}",
            plan.notes
        );

        let direct = e
            .lib
            .plan_launch(
                "probe",
                None,
                &[],
                &LaunchCtx {
                    unsandboxed: true,
                    ..ctx(e.tmp.path())
                },
            )
            .unwrap();
        assert!(!direct.argv.contains(&"bwrap".to_string()));
    }

    #[test]
    fn elevated_and_sandboxed_cannot_be_combined() {
        let e = sandboxed_env();
        e.lib.set_sandbox("probe", &quiet_strict()).unwrap();
        let mut prof = Profile::default();
        prof.elevated = true;
        e.lib.set_profile("probe", "default", &prof).unwrap();
        let err = e
            .lib
            .plan_launch("probe", None, &[], &ctx(e.tmp.path()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot be combined with a sandbox"), "{err}");
    }

    #[test]
    fn prepare_unpacks_once_and_the_whole_pipeline_runs_for_real() {
        if !bwrap_ok() {
            eprintln!("skipped: bubblewrap is not usable here");
            return;
        }
        let e = sandboxed_env();
        let app = e.lib.get("probe").unwrap();
        assert!(!extract::is_extracted(&e.lib.dirs, &app.sha256));
        e.lib.set_sandbox("probe", &quiet_strict()).unwrap();
        e.lib.prepare_launch("probe").unwrap();
        assert!(extract::is_extracted(&e.lib.dirs, &app.sha256));
        assert!(e.lib.dirs.private_home("probe").is_dir());

        let plan = e
            .lib
            .plan_launch(
                "probe",
                None,
                &["one".into(), "two".into()],
                &ctx(e.tmp.path()),
            )
            .unwrap();
        assert!(plan.notes.iter().all(|n| !n.contains("not unpacked")));
        let out = plan.command().output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "{text}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            text.contains("ARGS=one two") && text.contains("NET=1"),
            "{text}"
        );
        // The app wrote into its private home, not the real one.
        assert!(e.lib.dirs.private_home("probe").join("made").exists());
        assert!(!e.tmp.path().join("realhome/made").exists());

        // Dropping the sandbox needs no unpacked copy; garbage collection reclaims it.
        e.lib.set_sandbox("probe", &Settings::default()).unwrap();
        assert_eq!(
            e.lib.gc_extracted().unwrap(),
            0,
            "still the installed version's hash"
        );
        e.lib.remove("probe", false).unwrap();
        assert!(
            !extract::is_extracted(&e.lib.dirs, &app.sha256),
            "removing the app removes its unpacked copy"
        );
        assert!(
            e.lib.dirs.private_home("probe").is_dir(),
            "but never the app's own data"
        );
        assert!(e.lib.purge_private_home("probe").unwrap());
        assert!(!e.lib.purge_private_home("probe").unwrap());
    }

    #[test]
    fn missing_appimage_is_explained() {
        let e = env();
        let app = e.lib.get("foo-app").unwrap();
        fs::remove_file(&app.appimage_path).unwrap();
        let err = e
            .lib
            .plan_launch("foo-app", None, &[], &LaunchCtx::default())
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
