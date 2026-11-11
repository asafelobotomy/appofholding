use super::*;

fn rec(id: &str, path: &str) -> AppRecord {
    AppRecord {
        id: id.into(),
        name: id.to_uppercase(),
        appimage_path: path.into(),
        desktop_file: format!("/d/{id}.desktop").into(),
        icon_file: None,
        version: Some("1.0".into()),
        sha256: "abc".into(),
        size: 42,
        appimage_type: 2,
        update_info: None,
        added_at: 1,
        external_file: false,
    }
}

#[test]
fn migrates_and_crud() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len());
    db.insert(&rec("a", "/x/a")).unwrap();
    db.insert(&rec("b", "/x/b")).unwrap();
    assert_eq!(db.get("a").unwrap().unwrap(), rec("a", "/x/a"));
    assert_eq!(db.find_by_path(Path::new("/x/b")).unwrap().unwrap().id, "b");
    assert_eq!(db.list().unwrap().len(), 2);
    assert!(db.insert(&rec("a2", "/x/a")).is_err(), "path is unique");
    assert!(db.delete("a").unwrap());
    assert!(!db.delete("a").unwrap());
    assert!(db.get("a").unwrap().is_none());
}

#[test]
fn sources_roundtrip_and_keep_state_when_edited() {
    let db = Db::open_in_memory().unwrap();
    db.insert(&rec("a", "/x/a")).unwrap();
    let mut src = Source::new(Provider::Forgejo {
        host: "codeberg.org".into(),
        owner: "o".into(),
        repo: "r".into(),
    });
    src.channel = Channel::Tag("nightly".into());
    src.asset_pattern = Some("A-*.AppImage".into());
    db.upsert_source("a", &src).unwrap();
    db.record_check("a", 100, Some("update-available"), None, Some("v2"))
        .unwrap();
    db.record_installed("a", Some("v1"), Some("dd")).unwrap();

    // Changing the pattern must not wipe tracked state (Gear Lever #450).
    src.asset_pattern = Some("B-*.AppImage".into());
    db.upsert_source("a", &src).unwrap();
    let got = db.get_source("a").unwrap().unwrap();
    assert_eq!(got.source, src);
    assert_eq!(got.last_status.as_deref(), Some("update-available"));
    assert_eq!(got.installed_version.as_deref(), Some("v1"));

    // An error check keeps the last known latest version but records the failure.
    db.record_check("a", 200, None, Some("boom"), None).unwrap();
    let got = db.get_source("a").unwrap().unwrap();
    assert_eq!(
        (got.last_error.as_deref(), got.last_latest.as_deref()),
        (Some("boom"), Some("v2"))
    );

    // Cascade on app deletion.
    db.delete("a").unwrap();
    assert!(db.get_source("a").unwrap().is_none());
}

#[test]
fn every_provider_roundtrips() {
    let db = Db::open_in_memory().unwrap();
    db.insert(&rec("a", "/x/a")).unwrap();
    for p in [
        Provider::GitHub {
            owner: "o".into(),
            repo: "r".into(),
        },
        Provider::GitLab {
            host: "gitlab.com".into(),
            project: "g/sub/p".into(),
        },
        Provider::Url {
            url: "https://x/y.AppImage".into(),
        },
        Provider::Zsync {
            url: "https://x/y.AppImage.zsync".into(),
        },
    ] {
        let mut s = Source::new(p);
        s.origin = Origin::Embedded;
        db.upsert_source("a", &s).unwrap();
        assert_eq!(db.get_source("a").unwrap().unwrap().source, s);
    }
}

#[test]
fn policy_defaults_and_survives_source_edits() {
    let db = Db::open_in_memory().unwrap();
    db.insert(&rec("a", "/x/a")).unwrap();
    let mut s = Source::new(Provider::GitHub {
        owner: "o".into(),
        repo: "r".into(),
    });
    db.upsert_source("a", &s).unwrap();
    assert_eq!(db.get_source("a").unwrap().unwrap().policy, Policy::Notify);
    db.set_policy("a", Policy::Auto).unwrap();
    db.set_skipped("a", Some("v9")).unwrap();
    db.set_etag("a", Some("W/\"1\""), Some("ctx")).unwrap();
    s.asset_pattern = Some("x*".into());
    db.upsert_source("a", &s).unwrap();
    let got = db.get_source("a").unwrap().unwrap();
    assert_eq!(got.policy, Policy::Auto);
    assert_eq!(got.skipped_version.as_deref(), Some("v9"));
    assert_eq!(got.etag_ctx.as_deref(), Some("ctx"));
}

#[test]
fn versions_and_app_update() {
    let db = Db::open_in_memory().unwrap();
    let mut a = rec("a", "/x/a");
    db.insert(&a).unwrap();
    let v1 = db
        .add_version(
            "a",
            Some("1.0"),
            "s1",
            "A-1.0.AppImage",
            Path::new("/v/1"),
            10,
        )
        .unwrap();
    let v2 = db
        .add_version("a", None, "s2", "A-2.AppImage", Path::new("/v/2"), 20)
        .unwrap();
    let list = db.list_versions("a").unwrap();
    assert_eq!(list.iter().map(|v| v.id).collect::<Vec<_>>(), vec![v2, v1]);
    db.delete_version(v2).unwrap();
    assert_eq!(db.list_versions("a").unwrap().len(), 1);

    a.version = Some("2.0".into());
    a.appimage_path = "/x/new".into();
    a.added_at = 999; // never overwritten
    db.update_app(&a).unwrap();
    let got = db.get("a").unwrap().unwrap();
    assert_eq!(
        (
            got.version.as_deref(),
            got.appimage_path.to_str(),
            got.added_at
        ),
        (Some("2.0"), Some("/x/new"), 1)
    );

    db.delete("a").unwrap();
    assert!(
        db.list_versions("a").unwrap().is_empty(),
        "versions cascade with the app"
    );
}

#[test]
fn profiles_and_hooks_roundtrip() {
    let db = Db::open_in_memory().unwrap();
    db.insert(&rec("a", "/x/a")).unwrap();
    let mut p = Profile::default();
    p.args = vec!["--a b".into(), "c".into()];
    p.env.insert("K".into(), "v=1".into());
    p.prefix = vec!["nice".into()];
    p.presets = vec!["x11".into()];
    p.cwd = Some("/srv".into());
    p.extract_and_run = true;
    db.put_profile("a", "default", &p).unwrap();
    db.put_profile("a", "debug", &Profile::default()).unwrap();
    db.put_profile("a", "default", &p).unwrap(); // upsert
    let list = db.list_profiles("a").unwrap();
    assert_eq!(
        list.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["default", "debug"]
    );
    assert_eq!(list[0].1, p);
    assert!(db.delete_profile("a", "debug").unwrap());

    db.set_hook("a", "post-update", "echo 1").unwrap();
    db.set_hook("a", "post-update", "echo 2").unwrap();
    db.set_hook("a", "pre-update", "true").unwrap();
    assert_eq!(
        db.list_hooks("a").unwrap(),
        vec![
            ("post-update".to_string(), "echo 2".to_string()),
            ("pre-update".to_string(), "true".to_string())
        ]
    );
    assert!(db.clear_hook("a", "pre-update").unwrap());
    db.delete("a").unwrap();
    assert!(db.list_profiles("a").unwrap().is_empty() && db.list_hooks("a").unwrap().is_empty());
}

#[test]
fn sandbox_settings_roundtrip() {
    use crate::sandbox::{Bus, HomeAccess, Level, PathGrant, Settings};
    let db = Db::open_in_memory().unwrap();
    db.insert(&rec("a", "/x/a")).unwrap();
    assert!(db.get_sandbox("a").unwrap().is_none());
    let s = Settings {
        home: HomeAccess::ReadOnly,
        bus: Bus::Full,
        paths: vec![PathGrant {
            path: "/srv/x".into(),
            write: true,
        }],
        ..Settings::for_level(Level::Standard)
    }
    .normalized();
    db.put_sandbox("a", &s).unwrap();
    db.put_sandbox("a", &s).unwrap();
    assert_eq!(db.get_sandbox("a").unwrap().unwrap(), s);
    assert_eq!(s.level, Level::Custom);
    db.delete("a").unwrap();
    assert!(db.get_sandbox("a").unwrap().is_none());
}

#[test]
fn reopening_does_not_remigrate() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("sub/db.sqlite");
    Db::open(&p).unwrap().insert(&rec("a", "/x/a")).unwrap();
    let db = Db::open(&p).unwrap();
    assert_eq!(db.list().unwrap().len(), 1);
}
