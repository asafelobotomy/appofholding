use super::*;
use crate::fixture;
use crate::http::fake::FakeHttp;
use crate::library::AddOptions;
use crate::notify::Recorder;
use crate::paths::Dirs;
use sha2::{Digest, Sha256};
use std::process::Command;

const UPD: &str = "gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync";
const API: &str = "https://api.github.com/repos/o/foo/releases/latest";

struct Env {
    tmp: tempfile::TempDir,
    lib: Library,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    Env { tmp, lib }
}

/// Install version 1.0.0 (with embedded GitHub update info).
fn installed(e: &Env) -> AppRecord {
    let p = e.tmp.path().join("Foo-1.0.0-x86_64.AppImage");
    fs::write(
        &p,
        fixture::versioned_appimage("Foo App", "1.0.0", Some(UPD)),
    )
    .unwrap();
    e.lib.add(&p, &AddOptions::default()).unwrap()
}

/// A fake GitHub serving release `ver` whose asset is a real fixture AppImage.
fn github(ver: &str) -> FakeHttp {
    let img = fixture::versioned_appimage("Foo App", ver, Some(UPD));
    let sha = hex::encode(Sha256::digest(&img));
    let rel = format!(
        r#"{{"tag_name":"v{ver}","prerelease":false,"draft":false,"assets":[
              {{"name":"Foo-{ver}-x86_64.AppImage","browser_download_url":"https://dl/foo-{ver}","size":{},"digest":"sha256:{sha}"}}]}}"#,
        img.len()
    );
    FakeHttp::default()
        .json(API, &rel)
        .bytes(&format!("https://dl/foo-{ver}"), img)
}

fn apply(e: &Env, http: &FakeHttp, opts: &ApplyOptions) -> Result<Applied> {
    e.lib.update(http, "foo-app", opts, &mut |_, _| {})
}

#[test]
fn update_swaps_keeps_a_backup_and_refreshes_integration() {
    let e = env();
    let old = installed(&e);
    let http = github("2.0.0");
    let r = apply(&e, &http, &ApplyOptions::default()).unwrap();
    assert_eq!(
        r,
        Applied::Updated {
            from: Some("1.0.0".into()),
            to: "v2.0.0".into(),
            delta: false
        }
    );

    let app = e.lib.get("foo-app").unwrap();
    assert_eq!(app.version.as_deref(), Some("2.0.0"));
    assert!(app.appimage_path.ends_with("Foo-2.0.0-x86_64.AppImage"));
    assert!(app.appimage_path.exists());
    assert!(!old.appimage_path.exists(), "the old file name is gone");
    assert_ne!(app.sha256, old.sha256);

    let kept = e.lib.versions("foo-app").unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].version.as_deref(), Some("1.0.0"));
    assert!(kept[0].path.exists() && kept[0].sha256 == old.sha256);

    // The menu entry still goes through our launcher and points at nothing version-specific.
    let desktop = fs::read_to_string(&app.desktop_file).unwrap();
    assert!(desktop.contains(" run foo-app "), "{desktop}");
    let s = e.lib.source("foo-app").unwrap().unwrap();
    assert_eq!(
        (s.installed_version.as_deref(), s.last_status.as_deref()),
        (Some("v2.0.0"), Some("up-to-date"))
    );
    // Nothing left in the download cache.
    assert!(!e.lib.dirs.app_data_dir().join("downloads/foo-app").exists());

    // Asking again: nothing to do.
    assert_eq!(
        apply(&e, &http, &ApplyOptions::default()).unwrap(),
        Applied::NoUpdate(Status::UpToDate)
    );
}

#[test]
fn an_update_never_overwrites_a_file_that_is_not_this_apps() {
    let e = env();
    let old = installed(&e);
    let dir = old.appimage_path.parent().unwrap();
    // Something else already uses the new version's file name (e.g. another app).
    let squatter = dir.join("Foo-2.0.0-x86_64.AppImage");
    fs::write(&squatter, b"someone else's file").unwrap();
    apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();
    assert_eq!(fs::read(&squatter).unwrap(), b"someone else's file");
    let app = e.lib.get("foo-app").unwrap();
    assert!(
        app.appimage_path
            .ends_with("foo-app-Foo-2.0.0-x86_64.AppImage"),
        "{}",
        app.appimage_path.display()
    );
    assert!(app.appimage_path.exists() && !old.appimage_path.exists());
}

#[test]
fn two_updates_of_the_same_app_cannot_run_at_once() {
    let e = env();
    installed(&e);
    let held = e.lib.lock_app("foo-app").unwrap();
    let err = apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap_err();
    assert!(err.to_string().contains("already being updated"), "{err}");
    drop(held);
    assert!(apply(&e, &github("2.0.0"), &ApplyOptions::default()).is_ok());
}

#[test]
fn rollback_restores_the_old_version_and_stops_auto_update_bouncing_back() {
    let e = env();
    let old = installed(&e);
    apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();

    let rec = e.lib.rollback("foo-app", None).unwrap();
    assert_eq!(rec.version.as_deref(), Some("1.0.0"));
    assert_eq!(rec.sha256, old.sha256);
    assert!(rec.appimage_path.exists());
    // 2.0.0 is now the kept one, so the rollback is itself reversible.
    let kept = e.lib.versions("foo-app").unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].version.as_deref(), Some("v2.0.0"));

    let s = e.lib.source("foo-app").unwrap().unwrap();
    assert_eq!(s.skipped_version.as_deref(), Some("v2.0.0"));
    // The scheduler won't reinstall the version we just left.
    e.lib.set_policy("foo-app", Policy::Auto).unwrap();
    let outcomes = e
        .lib
        .run_scheduled(&github("2.0.0"), &Recorder::default())
        .unwrap();
    assert!(
        matches!(outcomes[0], Outcome::Skipped { .. }),
        "{outcomes:?}"
    );
    assert_eq!(
        e.lib.get("foo-app").unwrap().version.as_deref(),
        Some("1.0.0")
    );
    // A forced update still works.
    assert!(matches!(
        apply(
            &e,
            &github("2.0.0"),
            &ApplyOptions {
                force: true,
                ..Default::default()
            }
        )
        .unwrap(),
        Applied::Updated { .. }
    ));
}

#[test]
fn rollback_with_nothing_kept_or_unknown_target_is_a_clear_error() {
    let e = env();
    installed(&e);
    assert!(
        e.lib
            .rollback("foo-app", None)
            .unwrap_err()
            .to_string()
            .contains("no previous version")
    );
    apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();
    let err = e
        .lib
        .rollback("foo-app", Some("9.9.9"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("no kept version matches") && err.contains("1.0.0"),
        "{err}"
    );
    // Selecting by version string works.
    assert_eq!(
        e.lib
            .rollback("foo-app", Some("1.0.0"))
            .unwrap()
            .version
            .as_deref(),
        Some("1.0.0")
    );
}

#[test]
fn running_app_is_deferred_by_the_background_updater_only() {
    let e = env();
    let old = installed(&e);
    let mut child = Command::new("sleep")
        .arg("30")
        .env("APPIMAGE", &old.appimage_path)
        .spawn()
        .unwrap();
    let http = github("2.0.0");

    let deferred = apply(
        &e,
        &http,
        &ApplyOptions {
            if_running: IfRunning::Defer,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        matches!(&deferred, Applied::Deferred { pids, .. } if *pids == vec![child.id()]),
        "{deferred:?}"
    );
    assert!(old.appimage_path.exists(), "nothing was touched");
    assert_eq!(
        e.lib.get("foo-app").unwrap().version.as_deref(),
        Some("1.0.0")
    );

    // An explicit update still goes ahead (the running copy keeps working from its mapping).
    assert!(matches!(
        apply(&e, &http, &ApplyOptions::default()).unwrap(),
        Applied::Updated { .. }
    ));
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn pinned_and_skipped_versions_are_respected() {
    let e = env();
    installed(&e);
    let http = github("2.0.0");
    e.lib.set_policy("foo-app", Policy::Pinned).unwrap();
    assert!(matches!(
        apply(&e, &http, &ApplyOptions::default()).unwrap(),
        Applied::Skipped(_)
    ));
    assert_eq!(
        e.lib.get("foo-app").unwrap().version.as_deref(),
        Some("1.0.0")
    );

    e.lib.set_policy("foo-app", Policy::Notify).unwrap();
    e.lib.check(&http, "foo-app").unwrap();
    assert_eq!(e.lib.skip_latest("foo-app").unwrap(), "v2.0.0");
    assert!(matches!(
        apply(&e, &http, &ApplyOptions::default()).unwrap(),
        Applied::Skipped(_)
    ));
    // A newer release is not covered by the skip.
    assert!(matches!(
        apply(&e, &github("3.0.0"), &ApplyOptions::default()).unwrap(),
        Applied::Updated { .. }
    ));
}

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

#[test]
fn unchanged_releases_are_answered_from_the_etag_cache() {
    let e = env();
    installed(&e);
    let mut http = github("2.0.0");
    http.responses
        .get_mut(API)
        .unwrap()
        .headers
        .push(("etag".into(), "W/\"r1\"".into()));

    let first = e.lib.check_cached(&http, "foo-app").unwrap();
    assert!(first.report.is_some());
    assert_eq!(first.status, Status::UpdateAvailable);
    let second = e.lib.check_cached(&http, "foo-app").unwrap();
    assert!(
        second.report.is_none(),
        "304: no body parsed, previous result reused"
    );
    assert_eq!(
        (second.status, second.latest.as_deref()),
        (Status::UpdateAvailable, Some("v2.0.0"))
    );

    // Changing the source's question (here: the pattern) invalidates the cache.
    let mut src = e.lib.source("foo-app").unwrap().unwrap().source;
    src.asset_pattern = Some("Foo-*.AppImage".into());
    e.lib.set_source("foo-app", &src).unwrap();
    assert!(
        e.lib
            .check_cached(&http, "foo-app")
            .unwrap()
            .report
            .is_some()
    );

    // A forced full check never uses the cache.
    assert!(e.lib.check(&http, "foo-app").is_ok());
}
