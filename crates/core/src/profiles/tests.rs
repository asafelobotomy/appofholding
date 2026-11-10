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
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
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

    let d = crate::desktop::DesktopEntry::parse(&fs::read_to_string(&desktop).unwrap()).unwrap();
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
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
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
        .args(crate::sandbox::probe_args())
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
