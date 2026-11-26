use super::*;

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

#[test]
fn list_check_and_schedule_print_json() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let v = json_of(&run(root, &["list", "--json"]));
    assert_eq!(v["version"], 1);
    assert_eq!(v["apps"].as_array().unwrap().len(), 0);
    assert_eq!(v["updates"], 0);

    add_fixture(root);
    let v = json_of(&run(root, &["list", "--json"]));
    let app = &v["apps"][0];
    assert_eq!(app["id"], "foo-app");
    assert_eq!(app["name"], "Foo App");
    assert_eq!(app["desktop_id"], "appofholding-foo-app.desktop");
    assert_eq!(app["has_source"], false);
    assert!(app["policy"].is_null() && app["status"].is_null());
    assert_eq!(app["update"], false);

    // A source nobody answers (port 9, discard): the error is a row, not a failure.
    let o = run(
        root,
        &[
            "source",
            "set",
            "foo-app",
            "http://127.0.0.1:9/Foo.AppImage",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let v = json_of(&run(root, &["check", "--json"]));
    assert_eq!(v["version"], 1);
    assert_eq!(v["updates"], 0);
    let row = &v["apps"][0];
    assert_eq!(row["id"], "foo-app");
    assert!(row["status"].is_null());
    assert!(
        row["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{row}"
    );

    // The same app by its menu entry's desktop id; a foreign entry is refused.
    let v = json_of(&run(
        root,
        &[
            "check",
            "--json",
            "--desktop-id",
            "appofholding-foo-app.desktop",
        ],
    ));
    assert_eq!(v["apps"][0]["id"], "foo-app");
    let o = run(root, &["check", "--desktop-id", "firefox.desktop"]);
    assert!(!o.status.success());
    assert!(
        err(&o).contains("not a menu entry made by appOFholding"),
        "{}",
        err(&o)
    );
    let o = run(
        root,
        &["check", "--desktop-id", "appofholding-nope.desktop"],
    );
    assert!(!o.status.success());

    // The failed check is remembered for `list`, with the policy.
    let v = json_of(&run(root, &["list", "--json"]));
    let app = &v["apps"][0];
    assert_eq!(app["has_source"], true);
    assert_eq!(app["policy"], "notify");
    assert!(app["error"].as_str().is_some(), "{app}");

    let v = json_of(&run(root, &["schedule", "status", "--json"]));
    assert_eq!(
        v,
        serde_json::json!({"version": 1, "installed": false, "interval": null})
    );
}

#[test]
fn list_json_has_size_and_icon() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    add_fixture(root);
    let v = json_of(&run(root, &["list", "--json"]));
    let app = &v["apps"][0];
    assert!(app["size_bytes"].as_u64().is_some_and(|n| n > 0), "{app}");
    assert_eq!(app["external"], false);
    assert!(
        app["icon"].is_null() || app["icon"] == "appofholding-foo-app",
        "{app}"
    );
}

#[test]
fn text_check_fails_when_a_check_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let o = run(root, &["check", "nope"]);
    assert!(!o.status.success(), "{}", stdout(&o));
    assert!(stdout(&o).contains("nope"), "{}", stdout(&o));

    // Every row is printed before the failure, and `--json` still exits 0 (uQ reads the rows).
    add_fixture(root);
    let o = run(
        root,
        &[
            "source",
            "set",
            "foo-app",
            "http://127.0.0.1:9/Foo.AppImage",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let o = run(root, &["check"]);
    assert!(!o.status.success());
    assert!(stdout(&o).contains("foo-app") && stdout(&o).contains("error:"));
    assert!(err(&o).contains("1 check(s) failed"), "{}", err(&o));
    json_of(&run(root, &["check", "--json"]));
}

#[test]
fn defaults_are_shown_validated_and_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let o = run(root, &["defaults", "show", "--json"]);
    assert!(o.status.success(), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["policy"], "notify");
    assert_eq!(v["keepVersions"], 2);
    assert_eq!(v["sandbox"], "none");

    // A bad value changes nothing, not even the good ones given with it.
    let o = run(
        root,
        &["defaults", "set", "--policy", "auto", "--sandbox", "custom"],
    );
    assert!(!o.status.success());
    assert!(err(&o).contains("sandbox must be"), "{}", err(&o));
    assert!(!run(root, &["defaults", "set"]).status.success());
    assert!(
        !run(root, &["defaults", "set", "--keep-versions", "11"])
            .status
            .success()
    );

    let o = run(
        root,
        &[
            "defaults",
            "set",
            "--policy",
            "manual",
            "--keep-versions",
            "4",
            "--sandbox",
            "strict",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let v: serde_json::Value =
        serde_json::from_str(&stdout(&run(root, &["defaults", "show", "--json"]))).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"policy": "manual", "keepVersions": 4, "sandbox": "strict"})
    );
}

#[test]
fn config_shows_sets_and_resets_every_setting() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let v = json_of(&run(root, &["config", "show", "--json"]));
    assert_eq!(v["version"], 1);
    assert_eq!(v["settings"]["policy"], "notify");
    assert_eq!(v["settings"]["keep_versions"], 2);
    assert_eq!(v["settings"]["sandbox"], "none");
    assert_eq!(v["settings"]["schedule"], "off");
    assert_eq!(v["limits"]["max_keep_versions"], 10);

    let v = json_of(&run(root, &["config", "set", "policy", "auto", "--json"]));
    assert_eq!(v["settings"]["policy"], "auto");
    json_of(&run(
        root,
        &["config", "set", "keep_versions", "4", "--json"],
    ));
    json_of(&run(
        root,
        &["config", "set", "sandbox", "standard", "--json"],
    ));
    let v = json_of(&run(root, &["config", "show", "--json"]));
    assert_eq!(v["settings"]["keep_versions"], 4);
    assert_eq!(v["settings"]["sandbox"], "standard");
    // The older command reads the same values.
    let v = json_of(&run(root, &["defaults", "show", "--json"]));
    assert_eq!(v["policy"], "auto");

    json_of(&run(
        root,
        &["config", "set", "schedule", "weekly", "--json"],
    ));
    let v = json_of(&run(root, &["config", "show", "--json"]));
    assert_eq!(v["settings"]["schedule"], "weekly");
    json_of(&run(root, &["config", "reset", "schedule", "--json"]));
    let v = json_of(&run(root, &["config", "show", "--json"]));
    assert_eq!(v["settings"]["schedule"], "off");

    json_of(&run(root, &["config", "reset", "keep_versions", "--json"]));
    let v = json_of(&run(root, &["config", "show", "--json"]));
    assert_eq!(v["settings"]["keep_versions"], 2);

    // A bad value is refused, with the reason, and changes nothing.
    for (k, val) in [
        ("policy", "sometimes"),
        ("keep_versions", "11"),
        ("keep_versions", "lots"),
        ("sandbox", "full"),
        ("schedule", "monthly"),
        ("colour", "red"),
    ] {
        let v = failed_json(&run(root, &["config", "set", k, val, "--json"]));
        assert_eq!(v["ok"], false, "{k} {val}");
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()));
    }
    let v = json_of(&run(root, &["config", "show", "--json"]));
    assert_eq!(v["settings"]["policy"], "auto");
    assert_eq!(v["settings"]["keep_versions"], 2);

    let text = stdout(&run(root, &["config", "show"]));
    assert!(
        text.contains("policy") && text.contains("schedule"),
        "{text}"
    );
}

#[test]
fn config_policy_all_changes_apps_with_a_source() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    add_fixture(root);
    let o = run(
        root,
        &[
            "source",
            "set",
            "foo-app",
            "http://127.0.0.1:9/Foo.AppImage",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let v = json_of(&run(
        root,
        &["config", "set", "policy", "manual", "--all", "--json"],
    ));
    assert_eq!(v["changed_apps"], 1);
    let v = json_of(&run(root, &["list", "--json"]));
    assert_eq!(v["apps"][0]["policy"], "manual");
    let v = failed_json(&run(
        root,
        &["config", "set", "sandbox", "none", "--all", "--json"],
    ));
    assert!(v["error"].as_str().unwrap().contains("--all"));
}
