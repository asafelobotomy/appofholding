use super::*;

#[test]
fn restore_never_overwrites_a_file_that_is_there_again() {
    let e = env();
    let s = scan_all(&e);
    let p = plan(
        &e.lib,
        &s,
        &e.home,
        &PlanOptions {
            replace_entries: true,
            ..Default::default()
        },
    )
    .unwrap();
    let idx = p
        .items
        .iter()
        .position(|i| i.found.name == "Gear App")
        .unwrap();
    execute(&e.lib, &p, &[idx]);
    let original = e.lib.dirs.applications_dir().join("gear.desktop");
    fs::write(&original, "a new file").unwrap();
    let batch = list_backups(&e.lib).remove(0).batch;
    assert!(restore(&e.lib, &batch).unwrap().is_empty());
    assert_eq!(fs::read_to_string(&original).unwrap(), "a new file");
    assert_eq!(list_backups(&e.lib).len(), 1, "the backup is kept");
}

#[test]
fn name_clashes_become_conflicts_with_a_suggested_alternative() {
    let e = env();
    write_image(&e.home.join("AppImages/Twin1.AppImage"), "Twin", None);
    write_image(
        &e.home.join("Desktop/Twin2.AppImage"),
        "Twin",
        Some("zsync|https://x/y.zsync"),
    ); // different bytes
    let s = scan_all(&e);
    let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
    let twins: Vec<&Item> = p.items.iter().filter(|i| i.found.name == "Twin").collect();
    assert_eq!(twins.len(), 2);
    assert_eq!(
        twins.iter().filter(|i| i.action == Action::Import).count(),
        1
    );
    let conflict = twins
        .iter()
        .find(|i| matches!(i.action, Action::Conflict { .. }))
        .unwrap();
    let Action::Conflict { reason, alt_id } = &conflict.action else {
        unreachable!()
    };
    assert!(
        reason.contains("also being imported") && alt_id == "twin-2",
        "{reason} / {alt_id}"
    );
    assert_eq!(conflict.id, "twin-2");

    // Choosing the conflicting one imports it under the alternative id.
    let idx = p
        .items
        .iter()
        .position(|i| std::ptr::eq(i, *conflict))
        .unwrap();
    let r = execute(&e.lib, &p, &[idx]);
    assert!(r[0].outcome.is_ok(), "{r:?}");
    assert!(e.lib.get("twin-2").is_ok());

    // An id already in the library is a conflict too.
    let again = scan_all(&e);
    let p2 = plan(&e.lib, &again, &e.home, &PlanOptions::default()).unwrap();
    assert!(
        p2.items.iter().all(|i| i.found.name != "Twin"
            || matches!(i.action, Action::Conflict { .. } | Action::Import))
    );
}

#[test]
fn choosing_where_to_look_and_skipped_items_are_refused() {
    let e = env();
    let elsewhere = e.tmp.path().join("elsewhere");
    write_image(&elsewhere.join("Extra.AppImage"), "Extra App", None);
    let only = scan(
        &e.lib,
        &ScanOptions {
            extra_dirs: vec![elsewhere.clone()],
            only_extra: true,
            ..ScanOptions::new(&e.home)
        },
    )
    .unwrap();
    assert!(only.found.iter().any(|f| f.name == "Extra App"));
    assert!(
        !only.found.iter().any(|f| f.name == "Loose App"),
        "only the folder you named was searched"
    );
    assert_eq!(only.searched, vec![elsewhere]);

    // A move that would overwrite something in the library folder is skipped, not forced.
    let s = scan_all(&e);
    fs::create_dir_all(&e.lib.dirs.library_dir).unwrap();
    fs::write(e.lib.dirs.library_dir.join("Dl.AppImage"), b"occupied").unwrap();
    let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
    let dl = p
        .items
        .iter()
        .position(|i| i.found.name == "Download App")
        .unwrap();
    assert!(matches!(&p.items[dl].action, Action::Skip(w) if w.contains("already exists")));
    let r = execute(&e.lib, &p, &[dl]);
    assert!(r[0].outcome.is_err() && e.home.join("Downloads/Dl.AppImage").exists());
}

#[test]
fn a_newer_download_of_an_installed_app_updates_it_and_an_older_one_is_skipped() {
    let e = env();
    let installed = e.home.join("Apps/Ver-1.0.0.AppImage");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    fs::write(
        &installed,
        fixture::versioned_appimage("Ver App", "1.0.0", None),
    )
    .unwrap();
    e.lib.add(&installed, &AddOptions::default()).unwrap();
    let newer = e.home.join("Downloads/Ver-2.0.0.AppImage");
    fs::write(
        &newer,
        fixture::versioned_appimage("Ver App", "2.0.0", None),
    )
    .unwrap();
    let older = e.home.join("AppImages/Ver-0.5.0.AppImage");
    fs::write(
        &older,
        fixture::versioned_appimage("Ver App", "0.5.0", None),
    )
    .unwrap();

    let s = scan_all(&e);
    let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
    let item = |path: &Path| p.items.iter().position(|i| i.found.path == path).unwrap();
    let up = item(&newer);
    assert_eq!(
        p.items[up].action,
        Action::Update {
            app_id: "ver-app".into(),
            from: Some("1.0.0".into()),
            to: Some("2.0.0".into())
        }
    );
    assert_eq!(p.items[up].mode, Install::Move);
    assert!(matches!(&p.items[item(&older)].action, Action::Skip(why) if why.contains("older")));

    let r = execute(&e.lib, &p, &[up]);
    assert!(r[0].outcome.is_ok(), "{r:?}");
    assert_eq!(r[0].id, "ver-app");
    assert_eq!(
        e.lib.get("ver-app").unwrap().version.as_deref(),
        Some("2.0.0")
    );
    assert!(!newer.exists(), "the download was moved into the library");
    assert_eq!(e.lib.versions("ver-app").unwrap().len(), 1);
}
