use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use appofholding_core::fixture;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_appofholding"))
        .env("APPOFHOLDING_ROOT", root)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn add_list_info_run_remove_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img = root.join("Foo.AppImage");
    // AppRun in the fixture is a script, but the file itself is an ELF stub: `run` is
    // only checked for resolving the id, so we test it against a missing id below.
    fs::write(
        &img,
        fixture::simple_appimage("Foo App", Some("zsync|http://x/y.zsync")),
    )
    .unwrap();

    let o = run(root, &["info", img.to_str().unwrap()]);
    assert!(o.status.success(), "{o:?}");
    let s = stdout(&o);
    assert!(
        s.contains("Foo App") && s.contains("zsync|http://x/y.zsync"),
        "{s}"
    );

    let o = run(root, &["list"]);
    assert!(stdout(&o).contains("No apps yet"));

    let o = run(root, &["add", img.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let desktop = root.join("data/applications/appofholding-foo-app.desktop");
    let text = fs::read_to_string(&desktop).unwrap();
    assert!(text.contains(" run foo-app --flag %U"), "{text}");

    assert!(stdout(&run(root, &["list"])).contains("foo-app"));
    assert!(stdout(&run(root, &["info", "foo-app"])).contains("sha256"));

    let again = run(root, &["add", img.to_str().unwrap()]);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already in the library"));

    let o = run(root, &["remove", "foo-app"]);
    assert!(o.status.success());
    assert!(!desktop.exists());
    assert!(!root.join("Applications/Foo.AppImage").exists());
    assert!(stdout(&run(root, &["list"])).contains("No apps yet"));
}

#[test]
fn run_unknown_id_fails_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let o = run(tmp.path(), &["run", "nope", "--", "--x"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no app with id 'nope'"));
}

#[test]
fn run_passes_through_hyphenated_args() {
    // A real executable script wrapped so the library accepts it is overkill; instead verify
    // clap parsing accepts hyphenated passthrough after the id.
    let tmp = tempfile::tempdir().unwrap();
    let o = run(tmp.path(), &["run", "nope", "--no-sandbox", "%U"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("no app with id 'nope'"));
}

fn add_fixture(root: &Path) -> std::path::PathBuf {
    let img = root.join("Foo.AppImage");
    fs::write(&img, fixture::simple_appimage("Foo App", None)).unwrap();
    let o = run(root, &["add", img.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    root.join("Applications/Foo.AppImage")
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn launch_settings_show_up_in_the_printed_command_and_in_the_real_launch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let installed = add_fixture(root);

    let o = run(
        root,
        &[
            "launch",
            "set",
            "foo-app",
            "--args",
            "--hello \"two words\"",
            "--env",
            "MY_VAR=1",
            "--preset",
            "electron-auto",
            "--prefix",
            "env EXTRA=yes",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));

    // Preview: prefix, program, preset args, own args, then what the desktop passed.
    let o = run(
        root,
        &["run", "--print", "foo-app", "--from-desktop", "file.txt"],
    );
    assert!(o.status.success(), "{}", err(&o));
    let line = stdout(&o);
    assert!(line.starts_with("MY_VAR=1 env EXTRA=yes "), "{line}");
    assert!(
        line.trim_end()
            .ends_with("--ozone-platform-hint=auto --hello 'two words' --from-desktop file.txt"),
        "{line}"
    );

    // Really launching: swap in a script that records what it was given.
    let out = root.join("out.txt");
    fs::write(
        &installed,
        format!("#!/bin/sh\nprintf '%s;' \"$@\" > '{0}'\nprintf '%s;%s' \"$MY_VAR\" \"$EXTRA\" >> '{0}'\n", out.display()),
    )
    .unwrap();
    let o = run(root, &["run", "foo-app", "a.txt"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        "--ozone-platform-hint=auto;--hello;two words;a.txt;1;yes"
    );
}

#[test]
fn profiles_become_menu_actions_and_run_with_their_own_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    add_fixture(root);

    assert!(
        run(
            root,
            &[
                "launch",
                "set",
                "foo-app",
                "--profile",
                "debug",
                "--preset",
                "debug"
            ]
        )
        .status
        .success()
    );
    let desktop =
        fs::read_to_string(root.join("data/applications/appofholding-foo-app.desktop")).unwrap();
    assert!(
        desktop.contains("[Desktop Action appofholding-debug]"),
        "{desktop}"
    );
    assert!(
        desktop.contains(" run --profile debug foo-app %U"),
        "{desktop}"
    );

    let line = stdout(&run(
        root,
        &["run", "--print", "--profile", "debug", "foo-app"],
    ));
    assert!(line.contains("G_MESSAGES_DEBUG=all"), "{line}");
    let default = stdout(&run(root, &["run", "--print", "foo-app"]));
    assert!(
        !default.contains("G_MESSAGES_DEBUG"),
        "the default profile is unaffected: {default}"
    );

    let o = run(root, &["run", "--print", "--profile", "nope", "foo-app"]);
    assert!(
        !o.status.success() && err(&o).contains("no launch profile 'nope'"),
        "{}",
        err(&o)
    );

    assert!(
        run(root, &["launch", "delete", "foo-app", "--profile", "debug"])
            .status
            .success()
    );
    let desktop =
        fs::read_to_string(root.join("data/applications/appofholding-foo-app.desktop")).unwrap();
    assert!(!desktop.contains("appofholding-debug"));
}

#[test]
fn bad_launch_settings_are_rejected_with_a_helpful_message() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    add_fixture(root);
    let o = run(root, &["launch", "set", "foo-app", "--preset", "wayland"]);
    assert!(!o.status.success());
    assert!(
        err(&o).contains("unknown preset 'wayland'") && err(&o).contains("electron-wayland"),
        "{}",
        err(&o)
    );
    let o = run(root, &["launch", "set", "foo-app", "--env", "NOEQUALS"]);
    assert!(err(&o).contains("NAME=value"), "{}", err(&o));
    assert!(stdout(&run(root, &["launch", "presets"])).contains("no-sandbox"));
}

#[test]
fn post_install_hook_runs_once_and_is_remembered() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let marker = root.join("marker");
    let img = root.join("Foo.AppImage");
    fs::write(&img, fixture::simple_appimage("Foo App", None)).unwrap();
    let hook = format!("echo $APPOFHOLDING_ID >> '{}'", marker.display());
    let o = run(
        root,
        &["add", img.to_str().unwrap(), "--post-install", &hook],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(fs::read_to_string(&marker).unwrap(), "foo-app\n");
    assert!(stdout(&run(root, &["launch", "hook", "show", "foo-app"])).contains("post-install"));
    assert!(
        run(root, &["launch", "hook", "run", "foo-app", "post-install"])
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(&marker).unwrap(), "foo-app\nfoo-app\n");
}

fn bwrap_usable() -> bool {
    Command::new("bwrap")
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

/// A probe app: AppRun prints what the sandbox lets it see.
fn add_probe(root: &Path) {
    use appofholding_core::fixture::{Item, build_appimage};
    let desktop = "[Desktop Entry]\nType=Application\nName=Probe\nExec=AppRun\n";
    let apprun = "#!/bin/sh\necho \"HOME_FILES=$(find \"$HOME\" -maxdepth 1 -type f | wc -l)\"\necho \"NET=$(grep -c : /proc/net/dev)\"\necho \"ARGS=$*\"\ntouch \"$HOME/created-by-app\"\n";
    let img = build_appimage(
        None,
        vec![
            Item::File("/probe.desktop", desktop.as_bytes().to_vec()),
            Item::File("/AppRun", apprun.as_bytes().to_vec()),
        ],
    );
    let p = root.join("Probe.AppImage");
    fs::write(&p, img).unwrap();
    let o = run(root, &["add", p.to_str().unwrap()]);
    assert!(o.status.success(), "{}", err(&o));
}

#[test]
fn sandbox_settings_are_shown_validated_and_previewed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    add_probe(root);

    let shown = stdout(&run(root, &["sandbox", "show", "probe"]));
    assert!(
        shown.contains("level:    none") && shown.contains("sandbox set probe"),
        "{shown}"
    );

    let o = run(root, &["sandbox", "set", "probe", "--level", "strict"]);
    assert!(o.status.success(), "{}", err(&o));
    let o = run(
        root,
        &[
            "sandbox",
            "set",
            "probe",
            "--network",
            "true",
            "--allow",
            "/tmp:rw",
        ],
    );
    assert!(
        stdout(&o).contains("now 'custom'"),
        "changing a preset makes it custom: {}",
        stdout(&o)
    );
    let shown = stdout(&run(root, &["sandbox", "show", "probe"]));
    assert!(
        shown.contains("network:  on") && shown.contains("shared:   /tmp (read-write)"),
        "{shown}"
    );

    let preview = stdout(&run(root, &["run", "--print", "probe", "--x"]));
    assert!(
        preview.starts_with("bwrap ") || preview.contains("\nbwrap "),
        "{preview}"
    );
    assert!(
        preview.contains("--bind /tmp /tmp") && preview.trim_end().ends_with("/app/AppRun --x"),
        "{preview}"
    );

    let o = run(root, &["sandbox", "set", "probe", "--level", "paranoid"]);
    assert!(
        !o.status.success() && err(&o).contains("none, standard or strict"),
        "{}",
        err(&o)
    );
    let o = run(root, &["sandbox", "set", "probe", "--allow", "/proc"]);
    assert!(
        !o.status.success() && err(&o).contains("cannot be shared"),
        "{}",
        err(&o)
    );
    let o = run(root, &["sandbox", "set", "probe", "--home", "everywhere"]);
    assert!(err(&o).contains("private, ro or rw"));
    // Turning it off restores the plain launch.
    assert!(
        run(root, &["sandbox", "set", "probe", "--level", "none"])
            .status
            .success()
    );
    assert!(!stdout(&run(root, &["run", "--print", "probe"])).contains("bwrap"));
}

#[test]
fn a_sandboxed_app_really_runs_isolated_and_keeps_its_data_privately() {
    if !bwrap_usable() {
        eprintln!("skipped: bubblewrap is not usable here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    add_probe(root);
    let o = run(
        root,
        &[
            "sandbox",
            "set",
            "probe",
            "--level",
            "strict",
            "--gpu",
            "false",
            "--display",
            "false",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));

    let o = run(root, &["run", "probe", "hello", "world"]);
    let out = stdout(&o);
    assert!(o.status.success(), "{out}\n{}", err(&o));
    assert!(
        out.contains("HOME_FILES=0"),
        "the app starts with an empty private home: {out}"
    );
    assert!(out.contains("NET=1"), "strict has no network: {out}");
    assert!(out.contains("ARGS=hello world"), "{out}");
    assert!(
        err(&o).contains("Unpacking probe"),
        "first launch says what it is doing: {}",
        err(&o)
    );

    // The file the app made lives in its private home; the second launch sees it and is silent.
    let private = root.join("data/appofholding/apps/probe/home/created-by-app");
    assert!(private.exists());
    let o = run(root, &["run", "probe"]);
    assert!(stdout(&o).contains("HOME_FILES=1"), "{}", stdout(&o));
    assert!(!err(&o).contains("Unpacking"));

    // Bypass for debugging: runs like a normal AppImage (here: not executable, so it fails).
    let o = run(root, &["run", "--unsandboxed", "--print", "probe"]);
    assert!(!stdout(&o).contains("bwrap"));

    // Removing the app keeps its data unless asked.
    let o = run(root, &["remove", "probe"]);
    assert!(
        stdout(&o).contains("private data folder was kept") && private.exists(),
        "{}",
        stdout(&o)
    );
    assert!(
        !root
            .join("cache/appofholding/extracted")
            .read_dir()
            .unwrap()
            .any(|e| e.is_ok()),
        "unpacked copy is gone"
    );
    add_probe(root);
    let o = run(root, &["remove", "probe", "--purge-data"]);
    assert!(
        stdout(&o).contains("Deleted its private data") && !private.exists(),
        "{}",
        stdout(&o)
    );
}

#[test]
fn sandbox_check_reports_capabilities() {
    let tmp = tempfile::tempdir().unwrap();
    let out = stdout(&run(tmp.path(), &["sandbox", "check"]));
    assert!(
        out.contains("bubblewrap (bwrap):") && out.contains("xdg-dbus-proxy:"),
        "{out}"
    );
}
