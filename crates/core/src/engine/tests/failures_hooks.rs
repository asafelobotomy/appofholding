use super::*;

#[test]
fn a_bad_download_never_replaces_the_installed_app() {
    let e = env();
    let old = installed(&e);
    // The server hands out an HTML error page with a valid-looking release.
    let junk = b"<html>rate limited</html>".to_vec();
    let rel = format!(
        r#"{{"tag_name":"v2.0.0","prerelease":false,"draft":false,"assets":[
              {{"name":"Foo-2.0.0-x86_64.AppImage","browser_download_url":"https://dl/x","size":{}}}]}}"#,
        junk.len()
    );
    let http = FakeHttp::default()
        .json(API, &rel)
        .bytes("https://dl/x", junk);
    let err = apply(&e, &http, &ApplyOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("not a usable AppImage") && err.contains("not touched"),
        "{err}"
    );
    let app = e.lib.get("foo-app").unwrap();
    assert_eq!(app.sha256, old.sha256);
    assert!(app.appimage_path.exists());
    assert!(e.lib.versions("foo-app").unwrap().is_empty());
}

#[test]
fn a_persistent_update_failure_is_announced_once_and_named_as_such() {
    let e = env();
    installed(&e);
    e.lib.set_policy("foo-app", Policy::Auto).unwrap();
    // The release is there, but its file cannot be downloaded.
    let mut http = github("2.0.0");
    http.responses.remove("https://dl/foo-2.0.0");
    let notes = Recorder::default();
    for _ in 0..3 {
        let out = e.lib.run_scheduled(&http, &notes).unwrap();
        assert!(matches!(&out[0], Outcome::Failed { .. }), "{out:?}");
    }
    {
        let told = notes.0.borrow();
        assert_eq!(told.len(), 1, "{told:?}");
        assert_eq!(told[0].0, "Foo App: update failed");
    }
    let s = e.lib.source("foo-app").unwrap().unwrap();
    assert!(s.last_error.is_some());
    assert_eq!(s.last_status.as_deref(), Some("update-available"));

    // A check that fails is worded as one.
    e.lib.set_policy("foo-app", Policy::Notify).unwrap();
    e.lib.run_scheduled(&FakeHttp::default(), &notes).unwrap();
    assert_eq!(
        notes.0.borrow().last().unwrap().0,
        "Foo App: update check failed"
    );
}

#[test]
fn a_newer_local_file_updates_the_app_like_a_download() {
    use crate::library::Install;
    let e = env();
    let old = installed(&e);
    let dl = e.tmp.path().join("dl");
    fs::create_dir_all(&dl).unwrap();
    let write = |name: &str, ver: &str| {
        let p = dl.join(name);
        fs::write(&p, fixture::versioned_appimage("Foo App", ver, Some(UPD))).unwrap();
        p
    };

    // How files relate to the installed 1.0.0.
    let newer = write("Foo-2.0.0-x86_64.AppImage", "2.0.0");
    let older = write("Foo-0.9.0-x86_64.AppImage", "0.9.0");
    let same = dl.join("copy.AppImage");
    fs::copy(&old.appimage_path, &same).unwrap();
    let rel = |p: &Path| e.lib.local_version_of(p).unwrap().map(|v| v.relation);
    assert_eq!(rel(&newer), Some(Relation::Newer));
    assert_eq!(rel(&older), Some(Relation::Older));
    assert_eq!(rel(&same), Some(Relation::Same));
    assert_eq!(
        rel(&old.appimage_path),
        None,
        "the app's own file is no other version"
    );
    let other = dl.join("Bar.AppImage");
    fs::write(
        &other,
        fixture::versioned_appimage("Bar App", "1.0.0", None),
    )
    .unwrap();
    assert_eq!(rel(&other), None);
    assert!(
        e.lib
            .update_from_file("foo-app", &other, Install::Copy)
            .is_err()
    );

    // Copy leaves the file; the old version is kept for rollback.
    let app = e
        .lib
        .update_from_file("foo-app", &newer, Install::Copy)
        .unwrap();
    assert!(newer.exists());
    assert_eq!(app.version.as_deref(), Some("2.0.0"));
    assert_eq!(e.lib.versions("foo-app").unwrap().len(), 1);
    let src = e.lib.source("foo-app").unwrap().unwrap();
    assert_eq!(src.installed_version.as_deref(), Some("2.0.0"));

    // Move takes the file; going back to an older one is allowed when asked for.
    let app = e
        .lib
        .update_from_file("foo-app", &older, Install::Move)
        .unwrap();
    assert!(!older.exists());
    assert_eq!(app.version.as_deref(), Some("0.9.0"));
    assert!(app.appimage_path.exists());
}

#[test]
fn hooks_run_around_updates_and_a_failing_pre_update_vetoes_it() {
    let e = env();
    let old = installed(&e);
    let log = e.tmp.path().join("hooks.log");
    let append = |kind: &str| format!("echo {kind}:$APPOFHOLDING_VERSION >> '{}'", log.display());
    e.lib
        .set_hook("foo-app", "pre-update", &append("pre"))
        .unwrap();
    e.lib
        .set_hook("foo-app", "post-update", &append("post"))
        .unwrap();
    apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();
    // pre sees the old version, post the new one.
    assert_eq!(fs::read_to_string(&log).unwrap(), "pre:1.0.0\npost:2.0.0\n");
    assert!(e.lib.take_warnings().is_empty());

    // A failing pre-update hook cancels the next update before anything changes.
    e.lib.set_hook("foo-app", "pre-update", "exit 1").unwrap();
    let before = e.lib.get("foo-app").unwrap();
    let err = apply(&e, &github("3.0.0"), &ApplyOptions::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("update cancelled"), "{err}");
    let after = e.lib.get("foo-app").unwrap();
    assert_eq!(
        (before.sha256.as_str(), before.version.clone()),
        (after.sha256.as_str(), after.version.clone())
    );
    assert_ne!(after.sha256, old.sha256);

    // A failing post-update hook only produces a warning.
    e.lib.set_hook("foo-app", "pre-update", "true").unwrap();
    e.lib.set_hook("foo-app", "post-update", "exit 2").unwrap();
    assert!(matches!(
        apply(&e, &github("3.0.0"), &ApplyOptions::default()).unwrap(),
        Applied::Updated { .. }
    ));
    let warnings = e.lib.take_warnings();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("post-update hook failed"),
        "{warnings:?}"
    );
}

#[test]
fn only_the_newest_versions_are_kept() {
    let e = env();
    installed(&e);
    let keep1 = ApplyOptions {
        keep_versions: 1,
        ..Default::default()
    };
    for v in ["2.0.0", "3.0.0", "4.0.0"] {
        apply(&e, &github(v), &keep1).unwrap();
    }
    let kept = e.lib.versions("foo-app").unwrap();
    assert_eq!(kept.len(), 1);
    assert!(
        kept[0].version.as_deref().unwrap().contains('3'),
        "{kept:?}"
    );
    let files = fs::read_dir(e.lib.dirs.versions_dir("foo-app"))
        .unwrap()
        .count();
    assert_eq!(files, 1, "pruned versions are deleted from disk");
}

#[test]
fn only_actionable_updates_count() {
    let e = env();
    installed(&e);
    let avail = Some("update-available");
    assert!(counts_as_update(avail, Some("v2"), None));
    assert!(!counts_as_update(Some("up-to-date"), Some("v2"), None));
    assert!(!counts_as_update(None, None, None));
    let rec = || e.lib.source("foo-app").unwrap().unwrap();
    assert!(counts_as_update(avail, Some("v2"), Some(&rec())));
    e.lib.set_policy("foo-app", Policy::Pinned).unwrap();
    assert!(!counts_as_update(avail, Some("v2"), Some(&rec())));
    e.lib.set_policy("foo-app", Policy::Notify).unwrap();
    e.lib.check(&github("2.0.0"), "foo-app").unwrap();
    e.lib.skip_latest("foo-app").unwrap();
    assert!(!counts_as_update(avail, Some("v2.0.0"), Some(&rec())));
    assert!(counts_as_update(avail, Some("v3.0.0"), Some(&rec())));
    assert!(counts_as_update(avail, None, Some(&rec())));
}

#[test]
fn unannounced_updates_are_handed_out_once_and_shared_with_the_timer() {
    let e = env();
    installed(&e);
    let http = github("2.0.0");
    assert!(e.lib.take_unannounced_updates().unwrap().is_empty());

    e.lib.check(&http, "foo-app").unwrap();
    let fresh = e.lib.take_unannounced_updates().unwrap();
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].id, "foo-app");
    assert_eq!(fresh[0].version, "v2.0.0");
    assert!(
        e.lib.take_unannounced_updates().unwrap().is_empty(),
        "told once"
    );

    // The timer saw it as told too.
    let notes = Recorder::default();
    e.lib.run_scheduled(&http, &notes).unwrap();
    assert!(notes.0.borrow().is_empty());

    // A skipped version, and apps that are not set to notify, stay quiet.
    let http3 = github("3.0.0");
    e.lib.check(&http3, "foo-app").unwrap();
    e.lib.skip_latest("foo-app").unwrap();
    assert!(e.lib.take_unannounced_updates().unwrap().is_empty());
    e.lib.clear_skip("foo-app").unwrap();
    e.lib.set_policy("foo-app", Policy::Manual).unwrap();
    assert!(e.lib.take_unannounced_updates().unwrap().is_empty());
    e.lib.set_policy("foo-app", Policy::Notify).unwrap();
    assert_eq!(e.lib.take_unannounced_updates().unwrap().len(), 1);
}

#[test]
fn scheduler_applies_auto_notifies_once_and_survives_failures() {
    let e = env();
    installed(&e);
    let notes = Recorder::default();

    // notify (default): told once per version.
    let http = github("2.0.0");
    let first = e.lib.run_scheduled(&http, &notes).unwrap();
    assert_eq!(
        first,
        vec![Outcome::Available {
            id: "foo-app".into(),
            version: "v2.0.0".into(),
            notified: true
        }]
    );
    let second = e.lib.run_scheduled(&http, &notes).unwrap();
    assert_eq!(
        second,
        vec![Outcome::Available {
            id: "foo-app".into(),
            version: "v2.0.0".into(),
            notified: false
        }]
    );
    assert_eq!(notes.0.borrow().len(), 1);
    assert_eq!(
        e.lib.get("foo-app").unwrap().version.as_deref(),
        Some("1.0.0"),
        "notify never installs"
    );

    // failures are reported per app, not fatal
    let failing = e.lib.run_scheduled(&FakeHttp::default(), &notes).unwrap();
    assert!(matches!(&failing[0], Outcome::Failed { .. }));
    assert_eq!(notes.0.borrow().len(), 2);
    // ...and the same problem again is not announced again.
    let again = e.lib.run_scheduled(&FakeHttp::default(), &notes).unwrap();
    assert!(matches!(&again[0], Outcome::Failed { .. }));
    assert_eq!(
        notes.0.borrow().len(),
        2,
        "one notification per distinct problem"
    );
    assert!(
        e.lib
            .source("foo-app")
            .unwrap()
            .unwrap()
            .last_error
            .is_some()
    );

    // auto installs and says so
    e.lib.set_policy("foo-app", Policy::Auto).unwrap();
    let auto = e.lib.run_scheduled(&http, &notes).unwrap();
    assert!(
        matches!(&auto[0], Outcome::Updated { to, .. } if to == "v2.0.0"),
        "{auto:?}"
    );
    assert_eq!(
        e.lib.get("foo-app").unwrap().version.as_deref(),
        Some("2.0.0")
    );
    assert!(notes.0.borrow().last().unwrap().0.contains("updated"));

    // pinned apps are left alone
    e.lib.set_policy("foo-app", Policy::Pinned).unwrap();
    let pinned = e.lib.run_scheduled(&github("3.0.0"), &notes).unwrap();
    assert!(matches!(pinned[0], Outcome::Skipped { .. }));
}

#[test]
fn scheduler_defers_auto_update_of_a_running_app() {
    let e = env();
    let old = installed(&e);
    e.lib.set_policy("foo-app", Policy::Auto).unwrap();
    let mut child = Command::new("sleep")
        .arg("30")
        .env("APPIMAGE", &old.appimage_path)
        .spawn()
        .unwrap();
    let out = e
        .lib
        .run_scheduled(&github("2.0.0"), &Recorder::default())
        .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(matches!(out[0], Outcome::Deferred { .. }), "{out:?}");
    assert_eq!(
        e.lib.get("foo-app").unwrap().version.as_deref(),
        Some("1.0.0")
    );
}
