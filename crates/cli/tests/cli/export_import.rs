use super::*;

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
