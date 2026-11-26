use super::*;

#[test]
fn failed_add_rolls_back_copied_file() {
    let e = env();
    let bytes = fixture::build_appimage(None, vec![Item::File("/AppRun", b"x".to_vec())]);
    let p = write_image(&e, "nodesk.AppImage", bytes);
    assert!(e.lib.add(&p, &AddOptions::default()).is_err());
    let lib_dir = &e.lib.dirs.library_dir;
    assert!(!lib_dir.exists() || fs::read_dir(lib_dir).unwrap().next().is_none());
}

#[test]
fn an_entrys_own_arguments_never_become_options_of_run() {
    let evil = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=E\nExec=AppRun --unsandboxed --profile x %U\n\
         [Desktop Action a]\nExec=AppRun --print\n",
    )
    .unwrap();
    let text = integrate_entry(
        &evil,
        "e",
        Path::new("/usr/bin/appofholding"),
        false,
        &[],
        None,
        &[],
    );
    let d = DesktopEntry::parse(&text).unwrap();
    let exec = desktop::split_exec(d.main("Exec").unwrap());
    assert_eq!(
        exec[1..],
        ["run", "e", "--", "--unsandboxed", "--profile", "x", "%U"]
    );
    let action = desktop::split_exec(d.get("Desktop Action a", "Exec").unwrap());
    assert_eq!(action[1..], ["run", "e", "--", "--print"]);
}

#[test]
fn a_duplicate_exec_line_or_group_cannot_bypass_the_launcher() {
    let entry = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=E\nExec=AppRun\nExec=AppRun --raw\n\
         [Desktop Action a]\nExec=AppRun\n[Desktop Action a]\nExec=AppRun --raw\n\
         [Desktop Entry]\nExec=AppRun --raw\n",
    )
    .unwrap();
    let text = integrate_entry(
        &entry,
        "e",
        Path::new("/usr/bin/appofholding"),
        false,
        &[],
        None,
        &[],
    );
    assert!(!text.contains("--raw"), "{text}");
    for line in text.lines().filter(|l| l.starts_with("Exec=")) {
        assert!(
            line.starts_with("Exec=/usr/bin/appofholding run e --"),
            "{text}"
        );
    }
    assert_eq!(text.matches("[Desktop Entry]").count(), 1);
    assert_eq!(text.matches("[Desktop Action a]").count(), 1);
}

#[test]
fn percent_signs_keep_their_meaning() {
    let entry = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=E\nExec=AppRun --zoom=100%% %F\n",
    )
    .unwrap();
    let text = integrate_entry(
        &entry,
        "e",
        Path::new("/usr/bin/appofholding"),
        false,
        &[],
        None,
        &[],
    );
    assert!(
        text.contains("Exec=/usr/bin/appofholding run e -- --zoom=100%% %F\n"),
        "{text}"
    );
}

#[test]
fn entries_written_before_the_fix_are_rewritten_on_open() {
    let e = env();
    let p = write_image(
        &e,
        "Foo.AppImage",
        fixture::simple_appimage("Foo App", None),
    );
    let rec = e.lib.add(&p, &AddOptions::default()).unwrap();
    // What an older version wrote, and the flag the database upgrade sets.
    let old = "[Desktop Entry]\nType=Application\nName=Foo App\nExec=/x/appofholding run foo-app --flag %U\n";
    fs::write(&rec.desktop_file, old).unwrap();
    e.lib
        .db
        .set_setting("foo-app", REINTEGRATE, Some("1"))
        .unwrap();
    let lib = Library::open(e.lib.dirs.clone(), e.lib.exec_bin.clone()).unwrap();
    let d = DesktopEntry::parse(&fs::read_to_string(&rec.desktop_file).unwrap()).unwrap();
    let exec = desktop::split_exec(d.main("Exec").unwrap());
    assert_eq!(&exec[1..], ["run", "foo-app", "--", "--flag", "%U"]);
    assert!(lib.db.apps_with_setting(REINTEGRATE).unwrap().is_empty());
}
