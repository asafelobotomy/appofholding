use super::*;

#[test]
fn self_update_json_for_the_banner() {
    use appofholding_core::selfupdate::InstallKind;
    let api = "https://api.github.com/repos/asafelobotomy/appofholding/releases/latest";
    let http = FakeHttp::default().json(
        api,
        r#"{"tag_name":"v99.0.0","prerelease":false,"draft":false,"assets":[]}"#,
    );
    let tmp = tempfile::tempdir().unwrap();
    let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: tests that set this variable hold ENV_LOCK.
    unsafe { std::env::set_var("APPOFHOLDING_ROOT", tmp.path()) };
    let v = self_check(
        &http,
        InstallKind::Pacman {
            package: "appofholding".into(),
        },
        true,
    )
    .unwrap();
    assert_eq!(
        (
            v["available"].clone(),
            v["latest"].clone(),
            v["install"].clone()
        ),
        (json!(true), json!("99.0.0"), json!("pacman"))
    );
    assert_eq!(v["command"], "appofholding self update --install");
    assert!(
        v["url"]
            .as_str()
            .unwrap()
            .ends_with("/releases/tag/v99.0.0")
    );
    // The second answer comes from the cache and still says an update exists.
    let again = self_check(&http, InstallKind::AppImage { path: "/x".into() }, false).unwrap();
    assert_eq!(
        (
            again["fromCache"].clone(),
            again["available"].clone(),
            again["command"].clone()
        ),
        (json!(true), json!(true), json!(""))
    );
    assert_eq!(v["canInstall"], json!(true));
    // An install we cannot update ourselves gets guidance, not a half-finished update.
    let never = |_: &[String]| -> Result<(), String> { panic!("nothing may run") };
    let r = self_update(&http, InstallKind::Unknown, &never, &mut |_, _| {}).unwrap();
    assert_eq!(r["result"], "manual");
    assert!(
        r["instructions"]
            .as_str()
            .unwrap()
            .contains("releases/tag/v99.0.0")
    );
    unsafe { std::env::remove_var("APPOFHOLDING_ROOT") };
}

#[test]
fn add_remove_and_schedule() {
    let (tmp, lib) = lib();
    let p = tmp.path().join("Foo.AppImage");
    std::fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
    let v = add_file(&lib, &format!("file://{}", p.display()), "").unwrap();
    assert_eq!(v["id"], "foo-app");
    assert!(
        add_file(&lib, p.to_str().unwrap(), "").is_err(),
        "adding twice is reported"
    );
    remove_app(&lib, "foo-app", true).unwrap();
    assert_eq!(list_apps(&lib).unwrap(), json!([]));

    let dirs = Dirs::with_root(tmp.path());
    assert_eq!(schedule_status(&dirs)["installed"], json!(false));
    schedule_install(&dirs, "weekly").unwrap();
    assert_eq!(schedule_status(&dirs)["installed"], json!(true));
    assert!(schedule_install(&dirs, "yearly").is_err());
    schedule_remove(&dirs).unwrap();
    assert_eq!(schedule_status(&dirs)["installed"], json!(false));
}
