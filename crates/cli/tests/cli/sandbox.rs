use super::*;

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
fn sandbox_check_reports_capabilities() {
    let tmp = tempfile::tempdir().unwrap();
    let out = stdout(&run(tmp.path(), &["sandbox", "check"]));
    assert!(
        out.contains("bubblewrap (bwrap):") && out.contains("xdg-dbus-proxy:"),
        "{out}"
    );
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
fn a_downloaded_newer_version_updates_the_app_from_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let write = |name: &str, ver: &str| {
        let p = root.join(name);
        fs::write(&p, fixture::versioned_appimage("Foo App", ver, None)).unwrap();
        p
    };
    let v1 = write("Foo-1.0.0.AppImage", "1.0.0");
    assert!(run(root, &["add", v1.to_str().unwrap()]).status.success());

    // Adding another version is pointed at `update --from`.
    let v2 = write("Foo-2.0.0.AppImage", "2.0.0");
    let o = run(root, &["add", v2.to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(
        err(&o).contains("newer version of 'foo-app'") && err(&o).contains("--from"),
        "{}",
        err(&o)
    );

    // An older one needs --force.
    let v0 = write("Foo-0.1.0.AppImage", "0.1.0");
    let o = run(root, &["update", "foo-app", "--from", v0.to_str().unwrap()]);
    assert!(
        !o.status.success() && err(&o).contains("older"),
        "{}",
        err(&o)
    );

    let o = run(
        root,
        &[
            "update",
            "foo-app",
            "--from",
            v2.to_str().unwrap(),
            "--copy",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert!(stdout(&o).contains("1.0.0 -> 2.0.0"), "{}", stdout(&o));
    assert!(v2.exists(), "--copy leaves the file");
    assert!(stdout(&run(root, &["versions", "foo-app"])).contains("1.0.0"));
}
