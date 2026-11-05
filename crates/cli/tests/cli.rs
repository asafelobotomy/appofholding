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
