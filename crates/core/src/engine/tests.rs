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

#[test]
fn an_imported_app_identical_to_the_release_is_up_to_date_and_remembered() {
    // No version anywhere and no recorded digest: only the file's own hash can tell.
    let e = env();
    let img = fixture::build_appimage(
        None,
        vec![
            fixture::Item::File(
                "/foo.desktop",
                b"[Desktop Entry]\nType=Application\nName=Foo App\nExec=foo\nIcon=icon\n".to_vec(),
            ),
            fixture::Item::File("/icon.png", fixture::png(64)),
        ],
    );
    let sha = hex::encode(Sha256::digest(&img));
    let p = e.tmp.path().join("Foo.AppImage");
    fs::write(&p, &img).unwrap();
    let id = e.lib.add(&p, &AddOptions::default()).unwrap().id;
    e.lib
        .set_source(&id, &crate::source::parse_input("o/foo").unwrap().source)
        .unwrap();
    let rel = |digest: &str| {
        format!(
            r#"{{"tag_name":"v3.0.0","prerelease":false,"draft":false,"assets":[
              {{"name":"Foo-3.0.0-x86_64.AppImage","browser_download_url":"https://dl/x","size":1,"digest":"sha256:{digest}"}}]}}"#
        )
    };
    if Arch::host() != Arch::X86_64 {
        return;
    }
    // A different file is not claimed to be up to date.
    let other = FakeHttp::default().json(API, &rel("00"));
    assert_ne!(e.lib.check(&other, &id).unwrap().status, Status::UpToDate);
    assert!(
        e.lib
            .source(&id)
            .unwrap()
            .unwrap()
            .installed_digest
            .is_none()
    );

    let same = FakeHttp::default().json(API, &rel(&sha));
    assert_eq!(e.lib.check(&same, &id).unwrap().status, Status::UpToDate);
    let rec = e.lib.source(&id).unwrap().unwrap();
    assert_eq!(rec.installed_version.as_deref(), Some("v3.0.0"));
    assert_eq!(rec.installed_digest.as_deref(), Some(sha.as_str()));
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
fn the_installed_release_tag_beats_stale_appstream_metadata() {
    let e = env();
    installed(&e);
    // No X-AppImage-Version, and metainfo whose newest <release> was never bumped.
    let img = fixture::build_appimage(
        Some(UPD),
        vec![
            fixture::Item::File(
                "/io.github.o.foo.desktop",
                b"[Desktop Entry]\nType=Application\nName=Foo App\nExec=foo\nIcon=icon\n".to_vec(),
            ),
            fixture::Item::File("/icon.png", fixture::png(64)),
            fixture::Item::File(
                "/usr/share/metainfo/io.github.o.foo.appdata.xml",
                br#"<component><id>io.github.o.foo.desktop</id><releases><release version="1.0.0"/></releases></component>"#.to_vec(),
            ),
        ],
    );
    let sha = hex::encode(Sha256::digest(&img));
    let rel = format!(
        r#"{{"tag_name":"v2.0.0","prerelease":false,"draft":false,"assets":[
              {{"name":"Foo-2.0.0-x86_64.AppImage","browser_download_url":"https://dl/x","size":{},"digest":"sha256:{sha}"}}]}}"#,
        img.len()
    );
    let http = FakeHttp::default()
        .json(API, &rel)
        .bytes("https://dl/x", img);
    apply(&e, &http, &ApplyOptions::default()).unwrap();
    assert_eq!(
        e.lib.get("foo-app").unwrap().version.as_deref(),
        Some("v2.0.0")
    );
}

#[test]
fn testing_an_unsaved_source_answers_like_check() {
    let e = env();
    let app = installed(&e);
    let sha1 = image::sha1_file(&app.appimage_path).unwrap();
    let control = |sha1: &str| {
        format!("zsync: 0.6.2\nFilename: Foo.AppImage\nSHA-1: {sha1}\nURL: Foo.AppImage\n\n")
            .into_bytes()
    };
    let zs = crate::source::parse_input("zsync|https://h/Foo.AppImage.zsync")
        .unwrap()
        .source;
    let same = FakeHttp::default().bytes("https://h/Foo.AppImage.zsync", control(&sha1));
    let r = e.lib.test_source(&same, "foo-app", &zs).unwrap();
    assert_eq!(r.status, Status::UpToDate);
    let other = FakeHttp::default().bytes("https://h/Foo.AppImage.zsync", control("00ff"));
    assert_eq!(
        e.lib.test_source(&other, "foo-app", &zs).unwrap().status,
        Status::UpdateAvailable
    );
    // Nothing about the saved source changed.
    let s = e.lib.source("foo-app").unwrap().unwrap();
    assert_ne!(s.source, zs);
    assert!(s.last_checked.is_none());

    // The same file as the release's is up to date, as `check` says.
    if Arch::host() == Arch::X86_64 {
        let gh = crate::source::parse_input("o/bar").unwrap().source;
        let rel = format!(
            r#"{{"tag_name":"v9.0.0","prerelease":false,"draft":false,"assets":[
              {{"name":"Foo-9.0.0-x86_64.AppImage","browser_download_url":"https://dl/x","size":1,"digest":"sha256:{}"}}]}}"#,
            app.sha256
        );
        let http =
            FakeHttp::default().json("https://api.github.com/repos/o/bar/releases/latest", &rel);
        assert_eq!(
            e.lib.test_source(&http, "foo-app", &gh).unwrap().status,
            Status::UpToDate
        );
    }
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
