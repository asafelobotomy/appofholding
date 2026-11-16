//! oysterAV scanning, with a stand-in `oyst-cli` that follows oysterAV's documented contract: a file whose name
//! contains `bad` is a threat, anything else is clean.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use appofholding_core::fixture;
use serde_json::Value;

const FAKE: &str = r#"#!/bin/sh
echo '{"event":"progress","stage":"clamav","percent":100.0}'
case "$3" in
  *bad*) echo '{"event":"result","clean":false,"findings":[{"pack":"clamav","path":"'"$3"'","threat_name":"Test.Threat"}],"pack_errors":[]}'; exit 1 ;;
  *) echo '{"event":"result","clean":true,"findings":[],"pack_errors":[]}'; exit 0 ;;
esac
"#;

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    fake: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    fs::create_dir_all(&root).unwrap();
    let fake = tmp.path().join("oyst-cli");
    fs::write(&fake, FAKE).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    Env {
        _tmp: tmp,
        root,
        fake,
    }
}

fn run(e: &Env, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_appofholding"))
        .env("APPOFHOLDING_ROOT", &e.root)
        .env("APPOFHOLDING_OYST_CLI", &e.fake)
        .args(args)
        .output()
        .unwrap()
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stdout)))
}

fn image(e: &Env, file: &str, app: &str) -> PathBuf {
    let p = e.root.join(file);
    fs::write(&p, fixture::simple_appimage(app, None)).unwrap();
    p
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn scan_of(e: &Env, id: &str) -> Value {
    let list = json(&run(e, &["list", "--json"]));
    list["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap_or_else(|| panic!("{list}"))["scan"]
        .clone()
}

#[test]
fn the_mode_is_a_setting_and_off_scans_nothing() {
    let e = env();
    let shown = json(&run(&e, &["config", "show", "--json"]));
    assert_eq!(shown["settings"]["scan"], "off");
    assert_eq!(shown["defaults"]["scan"], "off");
    assert!(
        !run(&e, &["config", "set", "scan", "loud", "--json"])
            .status
            .success()
    );
    assert!(
        run(&e, &["config", "set", "scan", "warn", "--json"])
            .status
            .success()
    );
    assert_eq!(
        json(&run(&e, &["config", "show", "--json"]))["settings"]["scan"],
        "warn"
    );
    assert!(
        run(&e, &["config", "reset", "scan", "--json"])
            .status
            .success()
    );
    // Off: adding keeps no verdict.
    let img = image(&e, "Foo.AppImage", "Foo App");
    assert!(run(&e, &["add", path(&img)]).status.success());
    assert_eq!(scan_of(&e, "foo-app"), Value::Null);
}

#[test]
fn warn_scans_what_is_added_and_keeps_a_threat_in_the_library() {
    let e = env();
    run(&e, &["config", "set", "scan", "warn"]);
    let ok = image(&e, "Foo.AppImage", "Foo App");
    let bad = image(&e, "bad-thing.AppImage", "Bad Thing");
    assert!(run(&e, &["add", path(&ok)]).status.success());
    assert!(
        run(&e, &["add", path(&bad)]).status.success(),
        "warn adds it, with the verdict"
    );
    assert_eq!(scan_of(&e, "foo-app")["state"], "clean");
    let v = scan_of(&e, "bad-thing");
    assert_eq!(
        (v["state"].as_str(), v["detail"].as_str()),
        (Some("threat"), Some("Test.Threat"))
    );
}

#[test]
fn block_refuses_a_threat_before_anything_is_created() {
    let e = env();
    run(&e, &["config", "set", "scan", "block"]);
    let bad = image(&e, "bad-thing.AppImage", "Bad Thing");
    let o = run(&e, &["add", path(&bad)]);
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("oysterAV found Test.Threat") && err.contains("untouched"),
        "{err}"
    );
    assert!(
        json(&run(&e, &["list", "--json"]))["apps"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!e.root.join("Applications/bad-thing.AppImage").exists());
    assert!(bad.exists(), "the file is left where it was");
    // A clean one still goes in.
    let ok = image(&e, "Foo.AppImage", "Foo App");
    assert!(run(&e, &["add", path(&ok)]).status.success());
}

#[test]
fn scan_on_request_reports_every_app_and_exits_1_on_a_threat() {
    let e = env();
    let ok = image(&e, "Foo.AppImage", "Foo App");
    let bad = image(&e, "bad-thing.AppImage", "Bad Thing");
    run(&e, &["add", path(&ok)]);
    run(&e, &["add", path(&bad)]);
    let o = run(&e, &["scan", "--all", "--json"]);
    assert_eq!(o.status.code(), Some(1));
    let doc = json(&o);
    assert_eq!(
        (doc["threats"].as_i64(), doc["errors"].as_i64()),
        (Some(1), Some(0))
    );
    let states: Vec<_> = doc["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["id"].as_str().unwrap(), r["state"].as_str().unwrap()))
        .collect();
    assert!(
        states.contains(&("foo-app", "clean")) && states.contains(&("bad-thing", "threat")),
        "{states:?}"
    );
    assert_eq!(
        scan_of(&e, "bad-thing")["state"],
        "threat",
        "the verdict is kept"
    );
    // One app, streamed.
    let o = run(&e, &["scan", "foo-app", "--ndjson"]);
    assert!(o.status.success());
    let lines: Vec<Value> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines
            .iter()
            .map(|l| l["event"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["start", "scanned", "done"]
    );
    // No app named, nothing to guess.
    assert!(!run(&e, &["scan"]).status.success());
    assert_eq!(json(&run(&e, &["scan", "--json"]))["ok"], false);
}

#[test]
fn a_missing_scanner_is_an_error_on_the_row_not_a_crash() {
    let mut e = env();
    let ok = image(&e, "Foo.AppImage", "Foo App");
    run(&e, &["add", path(&ok)]);
    e.fake = PathBuf::from("/nonexistent/oyst-cli");
    let o = run(&e, &["scan", "foo-app", "--json"]);
    assert!(o.status.success(), "an unscannable app is not a threat");
    let doc = json(&o);
    assert_eq!(doc["errors"], 1);
    assert!(
        doc["results"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("not installed"),
        "{doc}"
    );
}

#[test]
fn a_verdict_never_applies_to_a_different_build() {
    let e = env();
    let ok = image(&e, "Foo.AppImage", "Foo App");
    run(&e, &["add", path(&ok)]);
    run(&e, &["scan", "--all"]);
    assert_eq!(scan_of(&e, "foo-app")["state"], "clean");
    // Replace the installed file by another build of the same app (an update from a file).
    let newer = e.root.join("Foo-2.AppImage");
    fs::write(
        &newer,
        fixture::versioned_appimage("Foo App", "2.0.0", None),
    )
    .unwrap();
    assert!(
        run(&e, &["update", "foo-app", "--from", path(&newer), "--copy"])
            .status
            .success()
    );
    assert_eq!(
        scan_of(&e, "foo-app"),
        Value::Null,
        "the old verdict was about the old build"
    );
}

#[test]
fn block_stops_an_update_from_a_file_that_has_a_threat() {
    let e = env();
    run(&e, &["config", "set", "scan", "block"]);
    let ok = image(&e, "Foo.AppImage", "Foo App");
    run(&e, &["add", path(&ok)]);
    let before = json(&run(&e, &["list", "--json"]));
    let bad = e.root.join("bad-Foo-2.AppImage");
    fs::write(&bad, fixture::versioned_appimage("Foo App", "2.0.0", None)).unwrap();
    let o = run(&e, &["update", "foo-app", "--from", path(&bad), "--copy"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("oysterAV found Test.Threat"));
    let after = json(&run(&e, &["list", "--json"]));
    assert_eq!(
        after["apps"][0]["appimage"], before["apps"][0]["appimage"],
        "the installed file is untouched"
    );
    assert!(bad.exists(), "the user's file is untouched");
}
