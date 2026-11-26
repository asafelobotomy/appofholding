use super::*;

#[test]
fn the_theme_choice_is_saved_and_validated() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::with_root(tmp.path());
    assert_eq!(theme_load(&dirs), "uq");
    theme_save(&dirs, "gruvbox-light").unwrap();
    assert_eq!(theme_load(&dirs), "gruvbox-light");
    assert!(theme_save(&dirs, "../evil").is_err());
    std::fs::write(gui_file(&dirs, "gui-theme-2"), "garbage").unwrap();
    assert_eq!(theme_load(&dirs), "uq");
}

#[test]
fn the_oysterav_mode_is_a_default_and_is_validated() {
    let (_tmp, lib) = lib();
    assert_eq!(library_defaults(&lib).unwrap()["scan"], "off");
    assert!(set_library_defaults(&lib, &json!({"scan": "loud"})).is_err());
    set_library_defaults(&lib, &json!({"scan": "block"})).unwrap();
    assert_eq!(library_defaults(&lib).unwrap()["scan"], "block");
    // Nothing scanned yet: no verdict on any row.
    assert_eq!(list_apps(&lib).unwrap(), json!([]));
}

#[test]
fn the_library_view_is_saved_and_validated() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::with_root(tmp.path());
    assert_eq!(view_load(&dirs), "list");
    view_save(&dirs, "grid").unwrap();
    assert_eq!(view_load(&dirs), "grid");
    view_save(&dirs, "compact").unwrap();
    assert_eq!(view_load(&dirs), "compact");
    assert!(view_save(&dirs, "../evil").is_err());
    std::fs::write(gui_file(&dirs, "gui-view"), "garbage").unwrap();
    assert_eq!(view_load(&dirs), "list");
}

#[test]
fn library_defaults_are_validated_before_anything_is_saved() {
    let (_tmp, lib) = lib();
    assert_eq!(library_defaults(&lib).unwrap()["policy"], "notify");
    // One bad value: nothing is saved.
    assert!(set_library_defaults(&lib, &json!({"policy": "auto", "keepVersions": 99})).is_err());
    assert!(set_library_defaults(&lib, &json!({"sandbox": "custom"})).is_err());
    assert_eq!(library_defaults(&lib).unwrap()["policy"], "notify");
    set_library_defaults(
        &lib,
        &json!({"policy": "auto", "keepVersions": 0, "sandbox": "standard"}),
    )
    .unwrap();
    let d = library_defaults(&lib).unwrap();
    assert_eq!(
        (
            d["policy"].as_str(),
            d["keepVersions"].as_u64(),
            d["sandbox"].as_str()
        ),
        (Some("auto"), Some(0), Some("standard"))
    );
    assert_eq!(apply_policy_all(&lib, "manual").unwrap()["changed"], 0);
    assert!(apply_policy_all(&lib, "bogus").is_err());
}

#[test]
fn a_pacman_install_updates_from_the_window_through_run0() {
    use appofholding_core::selfupdate::InstallKind;
    use sha2::{Digest, Sha256};
    let pkg = b"fake package".to_vec();
    let api = "https://api.github.com/repos/asafelobotomy/appofholding/releases/latest";
    let release = format!(
        r#"{{"tag_name":"v99.0.0","prerelease":false,"draft":false,"assets":[
          {{"name":"appofholding-99.0.0-1-x86_64.pkg.tar.zst","browser_download_url":"https://dl/pkg","size":{},"digest":"sha256:{}"}}]}}"#,
        pkg.len(),
        hex::encode(Sha256::digest(&pkg))
    );
    let http = FakeHttp::default()
        .json(api, &release)
        .bytes("https://dl/pkg", pkg);
    let tmp = tempfile::tempdir().unwrap();
    let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: tests that set this variable hold ENV_LOCK.
    unsafe { std::env::set_var("APPOFHOLDING_ROOT", tmp.path()) };
    let pacman = || InstallKind::Pacman {
        package: "appofholding".into(),
    };

    let seen = std::cell::RefCell::new(Vec::new());
    let ok = |c: &[String]| -> Result<(), String> {
        seen.borrow_mut().push(c.to_vec());
        Ok(())
    };
    let r = self_update(&http, pacman(), &ok, &mut |_, _| {}).unwrap();
    assert_eq!(
        (r["result"].clone(), r["to"].clone()),
        (json!("installed"), json!("99.0.0"))
    );
    let cmd = &seen.borrow()[0];
    assert_eq!(&cmd[..4], ["run0", "pacman", "-U", "--noconfirm"]);
    assert!(
        cmd[4].ends_with("appofholding-99.0.0-1-x86_64.pkg.tar.zst"),
        "{cmd:?}"
    );

    // Cancelling the password dialog (or any pacman error) is reported, not hidden.
    let refused = |_: &[String]| -> Result<(), String> {
        Err("Failed to start transient service unit: Access denied".into())
    };
    let err = self_update(&http, pacman(), &refused, &mut |_, _| {}).unwrap_err();
    assert_eq!(
        err,
        "appOFholding 99.0.0 was not installed: the password was not given. Nothing was changed."
    );
    let broken =
        |_: &[String]| -> Result<(), String> { Err("error: failed to commit transaction".into()) };
    let err = self_update(&http, pacman(), &broken, &mut |_, _| {}).unwrap_err();
    assert!(err.contains("failed to commit transaction"), "{err}");
}

#[test]
fn desktop_file_urls_become_real_paths() {
    assert_eq!(
        local_path("file:///home/u/My%20App%E2%9C%93.AppImage"),
        Path::new("/home/u/My App✓.AppImage")
    );
    assert_eq!(
        local_path("file://localhost/tmp/a%25b"),
        Path::new("/tmp/a%b")
    );
    // Plain paths are taken literally, even with a % in them.
    assert_eq!(local_path("/tmp/100%20done"), Path::new("/tmp/100%20done"));
    // A broken escape is kept as written.
    assert_eq!(local_path("file:///tmp/50%"), Path::new("/tmp/50%"));
}

#[test]
fn desktop_settings_and_autostart() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::with_root(tmp.path());
    let d = desktop_load(&dirs);
    assert_eq!(d["tray"], false);
    assert_eq!(d["notifyUpdates"], true);
    assert_eq!(d["autostart"], false);
    assert_eq!(d["dropAction"], "ask");

    let exe = Path::new("/opt/My Apps/appofholding-gui");
    let on = json!({"tray": true, "closeToTray": true, "startInTray": true, "autostart": true});
    desktop_save(&dirs, &on, exe).unwrap();
    let d = desktop_load(&dirs);
    assert_eq!(d["tray"], true);
    assert_eq!(d["closeToTray"], true);
    assert_eq!(d["minimiseToTray"], false);
    assert_eq!(d["autostart"], true);
    let entry = std::fs::read_to_string(autostart_file(&dirs)).unwrap();
    assert!(
        entry.contains("Exec=\"/opt/My Apps/appofholding-gui\" --tray\n"),
        "{entry}"
    );

    // Without a tray icon it cannot start hidden: the window opens.
    desktop_save(&dirs, &json!({"tray": false, "autostart": true}), exe).unwrap();
    let entry = std::fs::read_to_string(autostart_file(&dirs)).unwrap();
    assert!(!entry.contains("--tray"), "{entry}");

    desktop_save(&dirs, &json!({"autostart": false}), exe).unwrap();
    assert!(!autostart_file(&dirs).exists());
    assert_eq!(desktop_load(&dirs)["autostart"], false);
    // Turning it off twice is fine; a relative path is refused.
    desktop_save(&dirs, &json!({"autostart": false}), exe).unwrap();
    assert!(desktop_save(&dirs, &json!({"autostart": true}), Path::new("gui")).is_err());

    desktop_save(&dirs, &json!({"dropAction": "move"}), exe).unwrap();
    assert_eq!(desktop_load(&dirs)["dropAction"], "move");
    desktop_save(&dirs, &json!({"dropAction": "shred"}), exe).unwrap();
    assert_eq!(desktop_load(&dirs)["dropAction"], "ask");

    std::fs::write(desktop_file(&dirs), "garbage").unwrap();
    assert_eq!(desktop_load(&dirs)["tray"], false);
}

#[test]
fn suggestions_are_listed_applied_and_dismissed_through_the_service() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    add_foo(tmp.path(), &lib);
    // A plain app: nothing to suggest, and the list is a JSON array.
    assert_eq!(suggestions(&lib, "foo-app").unwrap(), json!([]));
    assert!(apply_suggestion(&lib, &FakeHttp::default(), "foo-app", "nonsense").is_err());
    dismiss_suggestion(&lib, "foo-app", "electron-wayland").unwrap();
    assert!(suggestions(&lib, "missing").is_err());
}

#[test]
fn dropped_files_are_described_and_added_by_mode() {
    let (tmp, lib) = lib();
    let elsewhere = tmp.path().join("Downloads");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let mk = |dir: &Path, file: &str, name: &str| {
        let p = dir.join(file);
        std::fs::write(&p, fixture::simple_appimage(name, None)).unwrap();
        p.display().to_string()
    };
    let moved = mk(&elsewhere, "Moved.AppImage", "Moved App");
    let copied = mk(&elsewhere, "Copied.AppImage", "Copied App");
    let kept = mk(&elsewhere, "Kept.AppImage", "Kept App");
    std::fs::create_dir_all(&lib.dirs.library_dir).unwrap();
    let already = mk(&lib.dirs.library_dir, "Already.AppImage", "Already App");

    let d = describe_drop(
        &lib,
        &[
            format!("file://{moved}"),
            already.clone(),
            elsewhere.display().to_string(), // a folder: left out
            "/nonexistent/x.AppImage".into(),
        ],
    );
    let d = d.as_array().unwrap();
    assert_eq!(d.len(), 2, "{d:?}");
    assert_eq!(d[0]["name"], "Moved.AppImage");
    assert_eq!(d[0]["inLibrary"], false);
    assert_eq!(d[1]["inLibrary"], true);

    assert!(d[0]["known"].is_null());

    let r = add_file(&lib, &moved, "move").unwrap();
    assert_eq!(r["moved"], true);
    assert!(!Path::new(&moved).exists(), "moved away");
    assert!(lib.dirs.library_dir.join("Moved.AppImage").exists());

    // Another build of an app we have is offered as an update of it, not as a new app.
    let next = elsewhere.join("Moved-2.AppImage");
    std::fs::write(&next, fixture::versioned_appimage("Moved App", "2.0", None)).unwrap();
    let d = describe_drop(&lib, &[next.display().to_string()]);
    assert_eq!(d[0]["known"]["id"], "moved-app");
    assert_eq!(d[0]["known"]["relation"], "newer");
    let r = update_from_file(&lib, "moved-app", &next.display().to_string(), "copy").unwrap();
    assert_eq!(r["to"], "2.0");
    assert!(next.exists(), "copied, not moved");

    add_file(&lib, &copied, "copy").unwrap();
    assert!(Path::new(&copied).exists() && lib.dirs.library_dir.join("Copied.AppImage").exists());

    let r = add_file(&lib, &kept, "in-place").unwrap();
    let rec = lib.get(r["id"].as_str().unwrap()).unwrap();
    assert!(rec.external_file && rec.appimage_path == std::fs::canonicalize(&kept).unwrap());

    add_file(&lib, &already, "move").unwrap();
    assert!(
        Path::new(&already).exists(),
        "already in the library: stays put"
    );

    assert!(add_file(&lib, &kept, "sideways").is_err());
}

#[test]
fn list_and_detail_describe_an_app_for_the_interface() {
    let (tmp, lib) = lib();
    assert_eq!(list_apps(&lib).unwrap(), json!([]));
    add_foo(tmp.path(), &lib);
    let list = list_apps(&lib).unwrap();
    let a = &list[0];
    assert_eq!(
        (a["id"].as_str(), a["name"].as_str(), a["version"].as_str()),
        (Some("foo-app"), Some("Foo App"), Some("1.2.3"))
    );
    assert!(a["icon"].as_str().unwrap().starts_with("file://") && a["hasSource"] == json!(true));
    assert_eq!(a["sandbox"], "none");

    let d = app_detail(&lib, "foo-app").unwrap();
    assert_eq!(d["source"]["origin"], "embedded");
    assert_eq!(d["source"]["policy"], "notify");
    assert_eq!(d["profiles"], json!(["default"]));
    assert_eq!(d["sandbox"]["level"], "none");
    assert_eq!(d["running"], json!(false));
    assert!(app_detail(&lib, "nope").is_err());
}

#[test]
fn launch_profile_round_trips_through_the_form_representation() {
    let (tmp, lib) = lib();
    add_foo(tmp.path(), &lib);
    let form = json!({
        "args": "--hello \"two words\"", "env": "A=1\nB=x y\n\n", "prefix": "nice -n 5",
        "cwd": "/srv", "presets": ["x11", "debug"], "extractAndRun": true, "elevated": false
    });
    save_profile(&lib, "foo-app", "default", &form).unwrap();
    let back = profile_detail(&lib, "foo-app", "default").unwrap();
    assert_eq!(
        back["args"],
        "--hello 'two words'".replace('\'', "\"").as_str()
    );
    assert_eq!(back["env"], "A=1\nB=x y");
    assert_eq!(back["prefix"], "nice -n 5");
    assert_eq!(back["presets"], json!(["x11", "debug"]));
    assert_eq!(back["extractAndRun"], json!(true));

    assert!(
        save_profile(&lib, "foo-app", "default", &json!({"env": "NOEQUALS"}))
            .unwrap_err()
            .contains("NAME=value")
    );
    assert!(
        save_profile(&lib, "foo-app", "default", &json!({"presets": ["nope"]}))
            .unwrap_err()
            .contains("unknown preset")
    );
    // An extra profile shows up in the detail's profile list and can be removed again.
    save_profile(&lib, "foo-app", "debug", &json!({"presets": ["debug"]})).unwrap();
    assert_eq!(
        app_detail(&lib, "foo-app").unwrap()["profiles"],
        json!(["default", "debug"])
    );
    delete_profile(&lib, "foo-app", "debug").unwrap();
    assert_eq!(
        app_detail(&lib, "foo-app").unwrap()["profiles"],
        json!(["default"])
    );
}
