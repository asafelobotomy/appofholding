use super::*;

#[test]
fn sandbox_form_saves_normalises_and_validates() {
    let (tmp, lib) = lib();
    add_foo(tmp.path(), &lib);
    let strict = sandbox_defaults("strict").unwrap();
    assert_eq!(
        (strict["network"].clone(), strict["bus"].clone()),
        (json!(false), json!("off"))
    );
    let saved = save_sandbox(&lib, "foo-app", &strict).unwrap();
    assert_eq!(saved["level"], "strict");
    // Flip one switch: it is no longer the strict preset.
    let mut tweaked = strict.clone();
    tweaked["network"] = json!(true);
    tweaked["paths"] = json!([{"path": "/tmp", "write": true}]);
    let saved = save_sandbox(&lib, "foo-app", &tweaked).unwrap();
    assert_eq!(saved["level"], "custom");
    assert_eq!(saved["paths"], json!([{"path": "/tmp", "write": true}]));
    assert!(save_sandbox(&lib, "foo-app", &json!({"level": "paranoid"})).is_err());
    assert!(
        save_sandbox(
            &lib,
            "foo-app",
            &json!({"level": "standard", "home": "x", "bus": "off"})
        )
        .is_err()
    );
    let bad = json!({"level": "none", "paths": [{"path": "/proc", "write": false}]});
    assert!(
        save_sandbox(&lib, "foo-app", &bad)
            .unwrap_err()
            .contains("cannot be shared")
    );
    // Switching it off clears the effect.
    assert_eq!(
        save_sandbox(&lib, "foo-app", &json!({"level": "none"})).unwrap()["level"],
        "none"
    );
}

#[test]
fn preview_shows_the_command_or_the_reason_there_is_none() {
    let (tmp, lib) = lib();
    add_foo(tmp.path(), &lib);
    let p = preview(&lib, "foo-app", "default").unwrap();
    assert!(p.contains("Foo.AppImage"), "{p}");
    assert!(
        preview(&lib, "foo-app", "ghost")
            .unwrap_err()
            .contains("no launch profile")
    );
}

#[test]
fn policy_and_source_forms() {
    let (tmp, lib) = lib();
    add_foo(tmp.path(), &lib);
    set_policy(&lib, "foo-app", "auto").unwrap();
    assert!(set_policy(&lib, "foo-app", "sometimes").is_err());
    set_source(
        &lib,
        "foo-app",
        "https://github.com/o/other/",
        "Other-*.AppImage",
        "latest-pre",
    )
    .unwrap();
    let d = app_detail(&lib, "foo-app").unwrap();
    assert_eq!(d["source"]["describe"], "github.com/o/other [latest-pre]");
    assert_eq!(d["source"]["pattern"], "Other-*.AppImage");
    assert_eq!(d["source"]["policy"], "auto");
    assert!(set_source(&lib, "foo-app", "ftp://x", "", "").is_err());
}

#[test]
fn resolving_a_link_previews_the_release_and_the_chosen_file() {
    let http = FakeHttp::default().json("https://api.github.com/repos/o/foo/releases/latest", REL);
    let v = resolve_link_with(&http, "https://github.com/o/foo/", "", "").unwrap();
    assert_eq!(v["release"], "v2.0.0");
    assert_eq!(v["chosen"], "Foo-2.0.0-x86_64.AppImage");
    assert_eq!(v["assets"][0]["verified"], json!(true));
    assert_eq!(v["problem"], Value::Null);
    // A pattern that matches nothing explains itself instead of failing.
    let v = resolve_link_with(&http, "o/foo", "Nope-*.AppImage", "").unwrap();
    assert_eq!(v["chosen"], Value::Null);
    assert!(
        v["problem"]
            .as_str()
            .unwrap()
            .contains("no AppImage matching")
    );
    assert!(resolve_link_with(&http, "not a link at all", "", "").is_err());
}

#[test]
fn install_check_and_update_flow_with_progress() {
    let (_tmp, lib) = lib();
    let img = fixture::versioned_appimage(
        "Foo App",
        "1.0.0",
        Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
    );
    let sha = hex_sha256(&img);
    let rel = |ver: &str, img: &[u8], sha: &str| {
        format!(
            r#"{{"tag_name":"v{ver}","prerelease":false,"draft":false,"assets":[{{"name":"Foo-{ver}-x86_64.AppImage","browser_download_url":"https://dl/{ver}","size":{},"digest":"sha256:{sha}"}}]}}"#,
            img.len()
        )
    };
    let http = FakeHttp::default()
        .json(
            "https://api.github.com/repos/o/foo/releases/latest",
            &rel("1.0.0", &img, &sha),
        )
        .bytes("https://dl/1.0.0", img.clone());
    let mut seen = 0u32;
    let v = install_link(&lib, &http, "o/foo", "", "", &mut |_, _| seen += 1).unwrap();
    assert_eq!(v["id"], "foo-app");
    assert!(seen > 0, "progress was reported");
    assert_eq!(
        app_detail(&lib, "foo-app").unwrap()["source"]["status"],
        "up-to-date"
    );

    let img2 = fixture::versioned_appimage(
        "Foo App",
        "2.0.0",
        Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
    );
    let sha2 = hex_sha256(&img2);
    let http2 = FakeHttp::default()
        .json(
            "https://api.github.com/repos/o/foo/releases/latest",
            &rel("2.0.0", &img2, &sha2),
        )
        .bytes("https://dl/2.0.0", img2);
    let c = check_app(&lib, &http2, "foo-app").unwrap();
    assert_eq!(
        (c["status"].clone(), c["latest"].clone()),
        (json!("update-available"), json!("v2.0.0"))
    );
    let all = check_all(&lib, &http2).unwrap();
    assert_eq!(all[0]["status"], "update-available");
    assert_eq!(all[0]["update"], true);
    assert_eq!(list_apps(&lib).unwrap()[0]["status"], "update-available");
    assert_eq!(list_apps(&lib).unwrap()[0]["update"], true);

    // A pinned app still reports the new release, but not as an update to act on.
    lib.set_policy("foo-app", Policy::Pinned).unwrap();
    assert_eq!(check_all(&lib, &http2).unwrap()[0]["update"], false);
    assert_eq!(list_apps(&lib).unwrap()[0]["update"], false);
    assert_eq!(
        app_detail(&lib, "foo-app").unwrap()["source"]["update"],
        false
    );
    lib.set_policy("foo-app", Policy::Notify).unwrap();

    let u = update_app(&lib, &http2, "foo-app", &mut |_, _| {}).unwrap();
    assert_eq!(
        (u["result"].clone(), u["to"].clone()),
        (json!("updated"), json!("v2.0.0"))
    );
    assert_eq!(
        app_detail(&lib, "foo-app").unwrap()["versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let again = update_app(&lib, &http2, "foo-app", &mut |_, _| {}).unwrap();
    assert_eq!(again["result"], "none");
    // Errors come back as readable strings, not panics.
    assert!(check_app(&lib, &FakeHttp::default(), "foo-app").is_err());
}

#[test]
fn import_dialog_flow_scan_then_confirm() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join("AppImages")).unwrap();
    std::fs::write(
        home.join("AppImages/Loose.AppImage"),
        fixture::simple_appimage("Loose App", None),
    )
    .unwrap();
    let gear = home.join("AppImages/gear.appimage");
    std::fs::write(&gear, fixture::simple_appimage("Gear App", None)).unwrap();
    let lib = Library::open(
        Dirs::with_root(&tmp.path().join("lib")),
        tmp.path().join("appofholding"),
    )
    .unwrap();
    std::fs::create_dir_all(lib.dirs.applications_dir()).unwrap();
    std::fs::write(
        lib.dirs.applications_dir().join("gear.desktop"),
        format!("[Desktop Entry]\nType=Application\nName=Gear App\nExec=env DESKTOPINTEGRATION=1 {0}\nTryExec={0}\n", gear.display()),
    )
    .unwrap();

    let opts = json!({"mode": "default", "replaceEntries": false});
    let v = import_scan(&lib, &home, &opts).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let g = items.iter().find(|i| i["name"] == "Gear App").unwrap();
    assert_eq!(
        (
            g["owner"].clone(),
            g["tool"].clone(),
            g["action"].clone(),
            g["mode"].clone()
        ),
        (
            json!("tool"),
            json!("Gear Lever"),
            json!("import"),
            json!("in-place")
        )
    );
    assert!(
        g["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("appear twice"))
    );
    assert!(lib.list().unwrap().is_empty(), "scanning is a dry run");

    // Changing the options changes the shown plan.
    let with = import_scan(
        &lib,
        &home,
        &json!({"mode": "copy", "replaceEntries": true}),
    )
    .unwrap();
    let g = with["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "Gear App")
        .unwrap()
        .clone();
    assert_eq!(
        (g["mode"].clone(), g["replaces"].clone()),
        (json!("copy"), json!(1))
    );
    assert!(import_scan(&lib, &home, &json!({"mode": "teleport"})).is_err());

    // The final confirmation imports exactly what was chosen.
    let r = import_run(&lib, &home, &opts, &[gear.display().to_string()]).unwrap();
    assert_eq!(r["imported"], 1);
    assert_eq!(
        lib.list()
            .unwrap()
            .iter()
            .map(|a| a.id.clone())
            .collect::<Vec<_>>(),
        vec!["gear-app".to_string()]
    );
    assert!(
        import_run(&lib, &home, &opts, &["/nonexistent".to_string()])
            .unwrap_err()
            .contains("nothing was selected")
    );
    assert_eq!(import_restore(&lib).unwrap()["restored"], 0);
}
