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

#[test]
fn export_then_apply_moves_a_setup_to_another_library() {
    let a = tempfile::tempdir().unwrap();
    add_probe(a.path());
    assert!(
        run(
            a.path(),
            &[
                "sandbox",
                "set",
                "probe",
                "--level",
                "strict",
                "--network",
                "true"
            ]
        )
        .status
        .success()
    );
    assert!(
        run(
            a.path(),
            &["launch", "set", "probe", "--preset", "x11", "--env", "A=1"]
        )
        .status
        .success()
    );
    assert!(
        run(
            a.path(),
            &[
                "launch",
                "set",
                "probe",
                "--profile",
                "debug",
                "--preset",
                "debug"
            ]
        )
        .status
        .success()
    );
    assert!(
        run(a.path(), &["policy", "probe", "manual"])
            .status
            .code()
            .is_some()
    );
    assert!(
        run(
            a.path(),
            &["launch", "hook", "set", "probe", "post-update", "echo hi"]
        )
        .status
        .success()
    );

    let file = a.path().join("setup.toml");
    let o = run(a.path(), &["export", "-o", file.to_str().unwrap()]);
    assert!(o.status.success(), "{}", err(&o));
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("id = \"probe\"") && text.contains("level = \"strict\""),
        "{text}"
    );

    // Another machine: same app file available locally; apply adds nothing for it, so we point
    // the entry at the file.
    let b = tempfile::tempdir().unwrap();
    let img = b.path().join("Probe.AppImage");
    fs::copy(a.path().join("Applications/Probe.AppImage"), &img).unwrap();
    let edited = text.replace(
        "id = \"probe\"",
        &format!("id = \"probe\"\nfile = \"{}\"", img.display()),
    );
    let file_b = b.path().join("setup.toml");
    fs::write(&file_b, edited).unwrap();

    let o = run(b.path(), &["apply", file_b.to_str().unwrap(), "--dry-run"]);
    assert!(stdout(&o).contains("add from file"), "{}", stdout(&o));
    assert!(
        stdout(&o).contains("runs a command: post-update hook: echo hi"),
        "the dry run shows what would run: {}",
        stdout(&o)
    );
    assert!(
        stdout(&run(b.path(), &["list"])).contains("No apps yet"),
        "dry run changes nothing"
    );

    // Commands from a file are refused unless asked for.
    let o = run(b.path(), &["apply", file_b.to_str().unwrap()]);
    assert!(!o.status.success() && stdout(&o).contains("--allow-commands"));
    let o = run(
        b.path(),
        &["apply", file_b.to_str().unwrap(), "--allow-commands"],
    );
    assert!(o.status.success(), "{}{}", stdout(&o), err(&o));
    assert!(
        stdout(&o).contains("probe: added from file"),
        "{}",
        stdout(&o)
    );

    let shown = stdout(&run(b.path(), &["sandbox", "show", "probe"]));
    assert!(
        shown.contains("level:    custom") && shown.contains("network:  on"),
        "{shown}"
    );
    let preview = stdout(&run(b.path(), &["run", "--print", "probe"]));
    assert!(
        preview.contains("A=1") || preview.contains("--setenv A 1"),
        "{preview}"
    );
    let desktop = fs::read_to_string(
        b.path()
            .join("data/applications/appofholding-probe.desktop"),
    )
    .unwrap();
    assert!(
        desktop.contains("appofholding-debug"),
        "the extra profile became a menu action: {desktop}"
    );
    assert!(stdout(&run(b.path(), &["launch", "hook", "show", "probe"])).contains("echo hi"));

    // Exporting from B gives back an equivalent file (apart from the local file path).
    let again = stdout(&run(b.path(), &["export"]));
    assert!(
        again.contains("id = \"probe\"") && again.contains("level = \"strict\""),
        "{again}"
    );
    // Applying twice is harmless.
    assert!(
        run(
            b.path(),
            &["apply", file_b.to_str().unwrap(), "--allow-commands"]
        )
        .status
        .success()
    );
}

#[test]
fn apply_rejects_bad_files_with_a_clear_message() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("bad.toml");
    fs::write(&f, "version = 1\n[[app]]\nid = \"a\"\npolcy = \"x\"\n").unwrap();
    let o = run(tmp.path(), &["apply", f.to_str().unwrap()]);
    assert!(
        !o.status.success() && err(&o).contains("polcy"),
        "{}",
        err(&o)
    );
    let o = run(tmp.path(), &["apply", "/nonexistent/file.toml"]);
    assert!(err(&o).contains("cannot read"));
}

#[test]
fn completions_and_man_pages_are_generated() {
    let tmp = tempfile::tempdir().unwrap();
    let bash = stdout(&run(tmp.path(), &["completions", "bash"]));
    assert!(
        bash.contains("_appofholding") && bash.contains("sandbox"),
        "completion script looks wrong"
    );
    let dir = tmp.path().join("man");
    assert!(
        run(tmp.path(), &["man", dir.to_str().unwrap()])
            .status
            .success()
    );
    let main = fs::read_to_string(dir.join("appofholding.1")).unwrap();
    assert!(
        main.contains(".TH appofholding 1"),
        "{}",
        &main[..200.min(main.len())]
    );
    assert!(
        dir.join("appofholding-sandbox.1").exists() && dir.join("appofholding-export.1").exists()
    );
}

/// Like `run`, but with a pretend home folder to scan.
fn run_home(root: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_appofholding"))
        .env("APPOFHOLDING_ROOT", root)
        .env("HOME", home)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

/// A home folder with a loose AppImage, one integrated by Gear Lever, one in Downloads.
fn import_fixture(root: &Path) -> std::path::PathBuf {
    let home = root.join("home");
    fs::create_dir_all(home.join("AppImages")).unwrap();
    fs::create_dir_all(home.join("Downloads")).unwrap();
    fs::write(
        home.join("AppImages/Loose.AppImage"),
        fixture::simple_appimage("Loose App", None),
    )
    .unwrap();
    let gear = home.join("AppImages/gear.appimage");
    fs::write(
        &gear,
        fixture::simple_appimage(
            "Gear App",
            Some("gh-releases-zsync|o|gear|latest|Gear-*.AppImage.zsync"),
        ),
    )
    .unwrap();
    fs::write(
        home.join("Downloads/Dl.AppImage"),
        fixture::simple_appimage("Download App", None),
    )
    .unwrap();
    let apps = root.join("data/applications");
    fs::create_dir_all(&apps).unwrap();
    fs::write(
        apps.join("gear.desktop"),
        format!("[Desktop Entry]\nType=Application\nName=Gear App\nExec=env DESKTOPINTEGRATION=1 {0} %U\nTryExec={0}\n", gear.display()),
    )
    .unwrap();
    home
}

#[test]
fn import_dry_run_shows_the_whole_plan_and_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let home = import_fixture(root);
    let o = run_home(root, &home, &["import", "--dry-run"]);
    let out = stdout(&o);
    assert!(o.status.success(), "{out}{}", err(&o));
    for want in [
        "Found 3 AppImage",
        "Gear App",
        "integrated by Gear Lever",
        "Loose App",
        "loose file",
        "Download App",
        "moved into",
        "menu entry stays",
        "update source found inside",
        "3 of 3 selected",
        "Dry run: nothing was changed",
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
    assert!(stdout(&run(root, &["list"])).contains("No apps yet"));
    assert!(
        home.join("Downloads/Dl.AppImage").exists(),
        "the file was not moved by a dry run"
    );
    assert!(
        !root
            .join("data/applications/appofholding-gear-app.desktop")
            .exists()
    );
}

#[test]
fn import_without_a_terminal_asks_for_yes_instead_of_guessing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let home = import_fixture(root);
    let o = run_home(root, &home, &["import"]);
    assert!(o.status.success());
    assert!(
        stdout(&o).contains("Nothing was changed. Re-run with --yes"),
        "{}",
        stdout(&o)
    );
    assert!(stdout(&run(root, &["list"])).contains("No apps yet"));
}

#[test]
fn import_yes_imports_the_selection_and_respects_ownership() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let home = import_fixture(root);
    let o = run_home(root, &home, &["import", "--yes", "--exclude", "Loose App"]);
    let out = stdout(&o);
    assert!(o.status.success(), "{out}{}", err(&o));
    assert!(out.contains("Imported 2 of 2"), "{out}");
    let list = stdout(&run(root, &["list"]));
    assert!(
        list.contains("gear-app") && list.contains("download-app") && !list.contains("loose-app"),
        "{list}"
    );

    // Adopted in place / moved.
    assert!(home.join("AppImages/gear.appimage").exists());
    assert!(
        !home.join("Downloads/Dl.AppImage").exists()
            && root.join("Applications/Dl.AppImage").exists()
    );
    // Its embedded update source came along, and Gear Lever's entry was left alone.
    assert!(stdout(&run(root, &["source", "show", "gear-app"])).contains("github.com/o/gear"));
    assert!(root.join("data/applications/gear.desktop").exists());
    assert!(out.contains("two menu entries"), "{out}");

    // Removing an imported-in-place app keeps the user's file; --delete-file removes it.
    let o = run(root, &["remove", "gear-app"]);
    assert!(
        stdout(&o).contains("The AppImage file was kept"),
        "{}",
        stdout(&o)
    );
    assert!(home.join("AppImages/gear.appimage").exists());
    // Importing again offers it again (it is no longer in the library) and nothing else.
    let again = stdout(&run_home(root, &home, &["import", "--dry-run"]));
    assert!(
        again.contains("Found 2 AppImage")
            && again.contains("Gear App")
            && again.contains("Loose App"),
        "{again}"
    );
    // Importing a second time, this one with deletion on removal.
    assert!(
        run_home(root, &home, &["import", "--yes", "--only", "gear-app"])
            .status
            .success()
    );
    assert!(
        run(root, &["remove", "gear-app", "--delete-file"])
            .status
            .success()
    );
    assert!(!home.join("AppImages/gear.appimage").exists());
}

#[test]
fn import_replace_entries_then_restore_puts_them_back() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let home = import_fixture(root);
    let entry = root.join("data/applications/gear.desktop");
    let original = fs::read_to_string(&entry).unwrap();
    let o = run_home(
        root,
        &home,
        &["import", "--yes", "--only", "gear-app", "--replace-entries"],
    );
    assert!(o.status.success(), "{}{}", stdout(&o), err(&o));
    assert!(
        stdout(&o).contains("backed up 1 other menu entry"),
        "{}",
        stdout(&o)
    );
    assert!(!entry.exists(), "only appOFholding's entry remains");
    assert!(
        root.join("data/applications/appofholding-gear-app.desktop")
            .exists()
    );

    let listed = stdout(&run_home(root, &home, &["import", "restore", "--list"]));
    assert!(listed.contains("gear.desktop"), "{listed}");
    let o = run_home(root, &home, &["import", "restore"]);
    assert!(stdout(&o).contains("Restored"), "{}{}", stdout(&o), err(&o));
    assert_eq!(fs::read_to_string(&entry).unwrap(), original);
    assert!(
        stdout(&run_home(root, &home, &["import", "restore"]))
            .contains("No menu entries are backed up")
    );
}

#[test]
fn import_options_are_validated() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let home = import_fixture(root);
    let o = run_home(root, &home, &["import", "--mode", "teleport", "--dry-run"]);
    assert!(
        !o.status.success() && err(&o).contains("--mode must be"),
        "{}",
        err(&o)
    );
    let o = run_home(
        root,
        &home,
        &[
            "import",
            "--dry-run",
            "--only-dirs",
            "--dir",
            home.join("Downloads").to_str().unwrap(),
        ],
    );
    let out = stdout(&o);
    assert!(
        out.contains("Download App") && !out.contains("Loose App"),
        "{out}"
    );
    let o = run_home(root, &home, &["import", "--dry-run", "--mode", "copy"]);
    assert!(stdout(&o).contains("copied into"), "{}", stdout(&o));
}
