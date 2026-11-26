use super::*;

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
fn oysterav_looks_at_a_download_before_it_replaces_anything() {
    use crate::scan::{ScanMode, ScanState};
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    // A stand-in oyst-cli that finds a threat in anything named 2.0.0. Only this test sets
    // `scan` away from off, so no other test in this process ever starts it.
    let fake = e.tmp.path().join("oyst-cli");
    fs::write(
        &fake,
        "#!/bin/sh\ncase \"$3\" in *2.0.0*) echo '{\"event\":\"result\",\"clean\":false,\"findings\":[{\"threat_name\":\"Test.Threat\"}],\"pack_errors\":[]}'; exit 1;; esac\necho '{\"event\":\"result\",\"clean\":true,\"findings\":[],\"pack_errors\":[]}'\n",
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: nothing else in this process reads or sets this variable.
    unsafe { std::env::set_var("APPOFHOLDING_OYST_CLI", &fake) };

    let old = installed(&e);
    e.lib.set_scan_mode(ScanMode::Block).unwrap();
    let http = github("2.0.0");
    let err = apply(&e, &http, &ApplyOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("oysterAV found Test.Threat") && err.contains("untouched"),
        "{err}"
    );
    let app = e.lib.get("foo-app").unwrap();
    assert_eq!(app.sha256, old.sha256, "the installed app was not touched");
    assert!(e.lib.versions("foo-app").unwrap().is_empty());
    assert!(
        !e.lib
            .dirs
            .app_data_dir()
            .join("downloads/foo-app/Foo-2.0.0-x86_64.AppImage")
            .exists(),
        "the flagged download is thrown away"
    );

    // `warn` installs it and keeps what oysterAV said, about this build only.
    e.lib.set_scan_mode(ScanMode::Warn).unwrap();
    assert!(matches!(
        apply(&e, &http, &ApplyOptions::default()),
        Ok(Applied::Updated { .. })
    ));
    let v = e.lib.scan_verdict("foo-app").unwrap().unwrap();
    assert_eq!(
        (v.state, v.detail.as_str()),
        (ScanState::Threat, "Test.Threat")
    );
}
