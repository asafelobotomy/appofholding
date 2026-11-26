use super::*;

#[test]
fn finds_loose_tool_managed_and_app_owned_appimages_and_ignores_the_rest() {
    let e = env();
    let s = scan_all(&e);
    let names: Vec<&str> = s.found.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["Download App", "Foo Owned", "Gear App", "Loose App"]
    );
    assert_eq!(by_name(&s, "Loose App").owner, Owner::Loose);
    assert_eq!(by_name(&s, "Download App").owner, Owner::Loose);
    assert!(
        matches!(&by_name(&s, "Gear App").owner, Owner::Tool { tool, entries } if tool == "Gear Lever" && entries.len() == 1)
    );
    assert!(
        matches!(&by_name(&s, "Foo Owned").owner, Owner::App { entries } if entries.len() == 1),
        "reached through its menu entry, though it has no .AppImage name"
    );
    assert!(by_name(&s, "Download App").has_update_info);
    assert!(
        s.skipped
            .iter()
            .any(|(p, why)| p.ends_with("Fake.AppImage") && why.contains("not an AppImage")),
        "{:?}",
        s.skipped
    );
    assert!(
        !s.found.iter().any(|f| f.name == "Hidden"),
        "hidden folders are not searched"
    );
}

#[test]
fn an_apps_own_entry_for_a_tool_managed_file_is_never_replaced() {
    // The real-world case: Gear Lever integrated the file, and the app also made its own
    // menu entry that launches it through a symlink.
    let e = env();
    let gl = e.home.join("AppImages/gear.appimage");
    let link = e.home.join(".local/share/gearapp/appimage");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&gl, &link).unwrap();
    let apps_own = entry(
        &e.lib,
        "gearapp.desktop",
        &format!(
            "Name=Gear App\nX-Gear-Integrator=appimage\nExec={} -- %U",
            link.display()
        ),
    );
    let s = scan_all(&e);
    let gear = by_name(&s, "Gear App");
    assert!(
        matches!(&gear.owner, Owner::Tool { tool, entries } if tool == "Gear Lever" && entries.len() == 1)
    );
    assert_eq!(gear.extra_entries, vec![apps_own.clone()]);

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
    let item = p.items.iter().find(|i| i.found.name == "Gear App").unwrap();
    assert_eq!(
        item.replace_entries,
        vec![e.lib.dirs.applications_dir().join("gear.desktop")]
    );
    assert!(item.notes.iter().any(|n| n.contains("never touched")));
    let idx = p
        .items
        .iter()
        .position(|i| i.found.name == "Gear App")
        .unwrap();
    execute(&e.lib, &p, &[idx]);
    assert!(apps_own.exists(), "the app's own menu entry survives");
    assert!(!e.lib.dirs.applications_dir().join("gear.desktop").exists());
}

#[test]
fn an_unrecognised_menu_entry_in_an_appimage_folder_is_left_alone() {
    let e = env();
    let f = e.home.join("AppImages/Handmade.AppImage");
    write_image(&f, "Handmade", None);
    let own = entry(
        &e.lib,
        "handmade.desktop",
        &format!("Name=Handmade\nExec={}", f.display()),
    );
    let s = scan_all(&e);
    assert!(
        matches!(&by_name(&s, "Handmade").owner, Owner::Entry { entries } if entries == &vec![own.clone()])
    );
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
    assert!(
        p.items
            .iter()
            .find(|i| i.found.name == "Handmade")
            .unwrap()
            .replace_entries
            .is_empty(),
        "only known tools' entries are ever replaced"
    );
}

#[test]
fn already_imported_files_and_copies_of_them_are_not_offered() {
    let e = env();
    e.lib
        .add(
            &e.home.join("AppImages/Loose.AppImage"),
            &AddOptions {
                install: Install::InPlace,
                id: None,
            },
        )
        .unwrap();
    // a byte-identical copy somewhere else
    fs::copy(
        e.home.join("AppImages/Loose.AppImage"),
        e.home.join("Downloads/LooseCopy.AppImage"),
    )
    .unwrap();
    let s = scan_all(&e);
    assert!(!s.found.iter().any(|f| f.name == "Loose App"));
    let why: Vec<&str> = s.skipped.iter().map(|(_, w)| w.as_str()).collect();
    assert!(
        why.iter()
            .any(|w| w.contains("already in the library as 'loose-app'")),
        "{why:?}"
    );
    assert!(
        why.iter().any(|w| w.contains("identical to 'loose-app'")),
        "{why:?}"
    );
}

#[test]
fn the_plan_is_a_dry_run_that_changes_nothing() {
    let e = env();
    let before = snapshot(e.tmp.path());
    let s = scan_all(&e);
    let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
    let get = |n: &str| p.items.iter().find(|i| i.found.name == n).unwrap();
    assert_eq!(get("Loose App").mode, Install::InPlace);
    assert_eq!(
        get("Download App").mode,
        Install::Move,
        "files in Downloads are moved into the library"
    );
    assert_eq!(get("Gear App").mode, Install::InPlace);
    assert!(
        get("Gear App")
            .notes
            .iter()
            .any(|n| n.contains("appear twice"))
    );
    let foo = get("Foo Owned");
    assert_eq!(foo.mode, Install::InPlace);
    assert!(foo.policy_manual && foo.notes.iter().any(|n| n.contains("keeps this file")));
    assert!(p.items.iter().all(|i| i.action == Action::Import));
    assert_eq!(snapshot(e.tmp.path()), before, "planning touched nothing");
    assert!(e.lib.list().unwrap().is_empty());

    // App-owned files are never moved or copied, whatever mode is asked for.
    let forced = plan(
        &e.lib,
        &s,
        &e.home,
        &PlanOptions {
            mode: Some(Install::Copy),
            replace_entries: true,
        },
    )
    .unwrap();
    let foo = forced
        .items
        .iter()
        .find(|i| i.found.name == "Foo Owned")
        .unwrap();
    assert_eq!(foo.mode, Install::InPlace);
    assert!(
        foo.replace_entries.is_empty(),
        "an app's own menu entry is never replaced"
    );
    let gear = forced
        .items
        .iter()
        .find(|i| i.found.name == "Gear App")
        .unwrap();
    assert_eq!((gear.mode, gear.replace_entries.len()), (Install::Copy, 1));
}

#[test]
fn importing_follows_the_plan_and_respects_whose_files_they_are() {
    let e = env();
    let s = scan_all(&e);
    let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
    let all: Vec<usize> = (0..p.items.len()).collect();
    let results = execute(&e.lib, &p, &all);
    assert!(results.iter().all(|r| r.outcome.is_ok()), "{results:?}");
    assert_eq!(e.lib.list().unwrap().len(), 4);

    // Adopted in place: still where it was, and flagged as the user's file.
    let loose = e.lib.get("loose-app").unwrap();
    assert_eq!(
        loose.appimage_path,
        fs::canonicalize(e.home.join("AppImages/Loose.AppImage")).unwrap()
    );
    assert!(loose.external_file);
    // Moved from Downloads into the library folder.
    let dl = e.lib.get("download-app").unwrap();
    assert!(
        !e.home.join("Downloads/Dl.AppImage").exists()
            && dl
                .appimage_path
                .starts_with(fs::canonicalize(&e.lib.dirs.library_dir).unwrap())
    );
    assert!(!dl.external_file);
    // The app-owned one: in place, updates switched to manual, source kept.
    let foo = e.lib.get("foo-owned").unwrap();
    assert!(foo.external_file);
    assert_eq!(
        e.lib.source("foo-owned").unwrap().unwrap().policy,
        Policy::Manual
    );
    assert_eq!(
        e.lib.source("download-app").unwrap().unwrap().policy,
        Policy::Notify,
        "others keep the default"
    );
    // The other tool's menu entry was left alone.
    assert!(e.lib.dirs.applications_dir().join("gear.desktop").exists());

    // Removing an adopted app keeps the user's file; a library-owned file goes.
    e.lib.remove("loose-app", false).unwrap();
    assert!(e.home.join("AppImages/Loose.AppImage").exists());
    e.lib.remove("download-app", false).unwrap();
    assert!(!dl.appimage_path.exists());
    e.lib
        .remove_with("gear-app", crate::library::FilePolicy::Delete)
        .unwrap();
    assert!(
        !e.home.join("AppImages/gear.appimage").exists(),
        "deleting the file takes an explicit request"
    );
}

#[test]
fn replacing_another_tools_menu_entry_is_backed_up_and_reversible() {
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
    let gear_idx = p
        .items
        .iter()
        .position(|i| i.found.name == "Gear App")
        .unwrap();
    let original = e.lib.dirs.applications_dir().join("gear.desktop");
    let text = fs::read_to_string(&original).unwrap();
    let r = execute(&e.lib, &p, &[gear_idx]);
    assert!(
        r[0].outcome
            .as_ref()
            .unwrap()
            .iter()
            .any(|n| n.contains("backed up 1 other menu entry"))
    );
    assert!(!original.exists(), "only our menu entry remains");
    let backups = list_backups(&e.lib);
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].files, vec![original.clone()]);

    let restored = restore(&e.lib, &backups[0].batch).unwrap();
    assert_eq!(restored, vec![original.clone()]);
    assert_eq!(
        fs::read_to_string(&original).unwrap(),
        text,
        "byte for byte what it was"
    );
    assert!(list_backups(&e.lib).is_empty());
}
