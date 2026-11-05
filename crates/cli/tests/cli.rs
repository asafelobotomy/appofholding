use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use appimgr_core::fixture;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_appimgr"))
        .env("APPIMGR_ROOT", root)
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
    let desktop = root.join("data/applications/appimgr-foo-app.desktop");
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
