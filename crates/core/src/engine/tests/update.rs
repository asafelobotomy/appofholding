use super::*;

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
fn an_update_that_renames_the_file_repoints_other_launchers_at_it() {
    let e = env();
    let old = installed(&e);
    let old_path = old.appimage_path.display().to_string();
    // The app wrote its own URL handler and autostart entry using the file's path.
    let handler = e
        .lib
        .dirs
        .applications_dir()
        .join("foo-url-handler.desktop");
    let autostart = e.lib.dirs.config_home.join("autostart/foo.desktop");
    fs::create_dir_all(autostart.parent().unwrap()).unwrap();
    fs::write(
        &handler,
        format!("[Desktop Entry]\nExec=\"{old_path}\" %u\n"),
    )
    .unwrap();
    fs::write(&autostart, format!("[Desktop Entry]\nExec={old_path}\n")).unwrap();
    apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();
    let new_path = e
        .lib
        .get("foo-app")
        .unwrap()
        .appimage_path
        .display()
        .to_string();
    assert_ne!(new_path, old_path);
    assert_eq!(
        fs::read_to_string(&handler).unwrap(),
        format!("[Desktop Entry]\nExec=\"{new_path}\" %u\n")
    );
    assert_eq!(
        fs::read_to_string(&autostart).unwrap(),
        format!("[Desktop Entry]\nExec={new_path}\n")
    );
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
