use super::*;

#[test]
fn remove_dry_run_json_plans_without_removing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file = add_fixture(root);

    let v = json_of(&run(root, &["remove", "foo-app", "--dry-run", "--json"]));
    assert_eq!(v["version"], 1);
    assert_eq!(v["would_refuse"], false);
    let changes = v["changes"].as_array().unwrap();
    let groups: Vec<&str> = changes.iter().filter_map(|c| c["group"].as_str()).collect();
    assert!(
        groups.contains(&"Menu") && groups.contains(&"AppImage"),
        "{v}"
    );
    let image = changes.iter().find(|c| c["group"] == "AppImage").unwrap();
    assert_eq!(image["reversible"], false);
    assert!(image["bytes"].as_u64().unwrap() > 0);
    assert!(file.exists(), "a dry run removes nothing");
    assert!(stdout(&run(root, &["list"])).contains("foo-app"));

    // With --keep-file the AppImage is not part of the plan.
    let v = json_of(&run(
        root,
        &["remove", "foo-app", "--keep-file", "--dry-run", "--json"],
    ));
    assert!(
        v["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["group"] != "AppImage")
    );

    // An unknown app fails with the reason in the envelope.
    let v = failed_json(&run(root, &["remove", "nope", "--dry-run", "--json"]));
    assert_eq!(v["ok"], false);

    let v = json_of(&run(root, &["remove", "foo-app", "--json"]));
    assert_eq!(v["ok"], true);
    assert_eq!(v["id"], "foo-app");
    assert!(!file.exists());
    assert!(stdout(&run(root, &["list"])).contains("No apps yet"));
}

#[test]
fn update_dry_run_json_lists_what_the_last_check_found() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    add_fixture(root);
    // Nothing found yet: the plan says so and refuses to be confirmed.
    let v = json_of(&run(root, &["update", "--dry-run", "--json"]));
    assert_eq!(v["would_refuse"], true);
    assert!(v["changes"].as_array().unwrap().is_empty());
    assert!(!v["reason"].as_str().unwrap().is_empty());
    // --json needs --dry-run, and a dry run excludes streaming.
    assert!(!run(root, &["update", "--json"]).status.success());
    assert!(
        !run(root, &["update", "--dry-run", "--ndjson"])
            .status
            .success()
    );
}

#[test]
fn check_and_update_stream_ndjson() {
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

    let o = run(root, &["check", "--ndjson"]);
    assert!(o.status.success(), "{}", err(&o));
    let ev = lines_of(&o);
    assert_eq!(ev.first().unwrap()["event"], "start");
    assert_eq!(ev.first().unwrap()["total"], 1);
    assert_eq!(ev[1]["event"], "checked");
    assert_eq!(ev[1]["id"], "foo-app");
    assert_eq!(ev[1]["index"], 1);
    assert!(ev[1]["error"].as_str().is_some_and(|e| !e.is_empty()));
    assert_eq!(ev.last().unwrap()["event"], "done");
    assert_eq!(ev.last().unwrap()["updates"], 0);

    // An update that can't be fetched is a result line, then a non-zero exit.
    let o = run(root, &["update", "--ndjson"]);
    let ev = lines_of(&o);
    assert_eq!(ev[0]["event"], "start");
    assert_eq!(ev[0]["name"], "Foo App");
    let result = ev.iter().find(|e| e["event"] == "result").unwrap();
    assert_eq!(result["app"], "foo-app");
    assert_eq!(result["outcome"], "failed");
    assert!(!result["message"].as_str().unwrap().is_empty());
    assert_eq!(ev.last().unwrap()["event"], "done");
    assert_eq!(ev.last().unwrap()["failed"], 1);
    assert!(!o.status.success());
}

#[test]
fn versions_and_rollback_speak_json_and_plan_first() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let write = |name: &str, ver: &str| {
        let p = root.join(name);
        fs::write(&p, fixture::versioned_appimage("Foo App", ver, None)).unwrap();
        p
    };
    let v1 = write("Foo-1.0.0.AppImage", "1.0.0");
    assert!(run(root, &["add", v1.to_str().unwrap()]).status.success());

    // Nothing kept yet: an empty list, and a plan that refuses.
    let v = json_of(&run(root, &["versions", "foo-app", "--json"]));
    assert_eq!(v["current"]["version"], "1.0.0");
    assert!(v["kept"].as_array().unwrap().is_empty());
    let v = json_of(&run(root, &["rollback", "foo-app", "--dry-run", "--json"]));
    assert_eq!(v["would_refuse"], true);

    let v2 = write("Foo-2.0.0.AppImage", "2.0.0");
    let o = run(root, &["update", "foo-app", "--from", v2.to_str().unwrap()]);
    assert!(o.status.success(), "{}", err(&o));

    let v = json_of(&run(root, &["versions", "foo-app", "--json"]));
    assert_eq!(v["current"]["version"], "2.0.0");
    let kept = &v["kept"][0];
    assert_eq!(kept["version"], "1.0.0");
    let sha = kept["sha256"].as_str().unwrap().to_string();
    assert!(kept["bytes"].as_u64().unwrap() > 0);

    let v = json_of(&run(
        root,
        &[
            "rollback",
            "foo-app",
            "--dry-run",
            "--json",
            "--to",
            &sha[..12],
        ],
    ));
    assert_eq!(v["would_refuse"], false);
    assert!(v["summary"].as_str().unwrap().contains("2.0.0"), "{v}");
    assert!(
        stdout(&run(root, &["list"])).contains("2.0.0"),
        "a dry run changes nothing"
    );

    let v = failed_json(&run(
        root,
        &["rollback", "foo-app", "--json", "--to", "ffffffff"],
    ));
    assert_eq!(v["ok"], false);

    let v = json_of(&run(
        root,
        &["rollback", "foo-app", "--json", "--to", &sha[..12]],
    ));
    assert_eq!(v["to"], "1.0.0");
    let v = json_of(&run(root, &["versions", "foo-app", "--json"]));
    assert_eq!(v["current"]["version"], "1.0.0");
}
