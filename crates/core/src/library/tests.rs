use super::*;

#[test]
fn icons_go_in_a_size_folder_icon_themes_search() {
    assert_eq!(hicolor_size(2816), 512);
    assert_eq!(hicolor_size(512), 512);
    assert_eq!(hicolor_size(300), 256);
    assert_eq!(hicolor_size(200), 192);
    assert_eq!(hicolor_size(48), 48);
    assert_eq!(hicolor_size(8), 16);
}

#[test]
fn the_taskbar_can_match_windows_to_the_renamed_entry() {
    let plain =
        DesktopEntry::parse("[Desktop Entry]\nType=Application\nName=Tail\nExec=scale_tail\n")
            .unwrap();
    let text = integrate_entry(
        &plain,
        "scale-tail",
        Path::new("/usr/bin/appofholding"),
        true,
        &[],
        Some("io.github.me.scale_tail"),
        &[],
    );
    assert!(
        text.contains("StartupWMClass=io.github.me.scale_tail"),
        "{text}"
    );
    // What the app declares itself always wins.
    let own = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=G\nExec=AppRun %U\nStartupWMClass=GutCheck\n",
    )
    .unwrap();
    let text = integrate_entry(
        &own,
        "g",
        Path::new("/usr/bin/appofholding"),
        true,
        &[],
        Some("gutcheck"),
        &[],
    );
    assert!(
        text.contains("StartupWMClass=GutCheck") && !text.contains("StartupWMClass=gutcheck"),
        "{text}"
    );
}
use crate::fixture::{self, Item};

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    lib: Library,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    // A path with a space exercises Exec quoting.
    let root = tmp.path().join("my root");
    fs::create_dir_all(&root).unwrap();
    let lib = Library::open(Dirs::with_root(&root), root.join("bin/appofholding")).unwrap();
    Env {
        _tmp: tmp,
        root,
        lib,
    }
}

fn write_image(env: &Env, file: &str, bytes: Vec<u8>) -> PathBuf {
    let dir = env.root.join("downloads");
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(file);
    fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn inspect_reads_metadata_without_running_anything() {
    let e = env();
    let p = write_image(
        &e,
        "Foo-1.2.3.AppImage",
        fixture::simple_appimage("Foo App", Some("gh-releases-zsync|o|r|latest|*.zsync")),
    );
    let i = inspect(&p).unwrap();
    assert_eq!(i.name, "Foo App");
    assert_eq!(i.id, "foo-app");
    assert_eq!(i.version.as_deref(), Some("1.2.3"));
    assert_eq!(
        i.image.update_info.as_deref(),
        Some("gh-releases-zsync|o|r|latest|*.zsync")
    );
    assert_eq!(i.icon.unwrap().size, Some(256));
}

#[test]
fn add_integrates_and_preserves_desktop_content() {
    let e = env();
    let p = write_image(
        &e,
        "Foo.AppImage",
        fixture::simple_appimage("Foo App", None),
    );
    let rec = e.lib.add(&p, &AddOptions::default()).unwrap();

    assert_eq!(rec.id, "foo-app");
    assert!(p.exists(), "copy leaves the original");
    assert!(rec.appimage_path.exists());
    assert_eq!(
        rec.appimage_path.parent().unwrap(),
        fs::canonicalize(&e.lib.dirs.library_dir).unwrap()
    );
    assert_ne!(
        fs::metadata(&rec.appimage_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert!(
        rec.icon_file
            .as_ref()
            .unwrap()
            .ends_with("hicolor/256x256/apps/appofholding-foo-app.png")
    );
    assert!(rec.icon_file.as_ref().unwrap().exists());

    let text = fs::read_to_string(&rec.desktop_file).unwrap();
    let d = DesktopEntry::parse(&text).unwrap();
    let exec = desktop::split_exec(d.main("Exec").unwrap());
    assert_eq!(exec[0], e.lib.exec_bin.to_string_lossy());
    assert_eq!(&exec[1..], ["run", "foo-app", "--flag", "%U"]);
    assert_eq!(d.main("Icon"), Some("appofholding-foo-app"));
    assert_eq!(d.main("Categories"), Some("Utility;"));
    assert_eq!(d.main("X-AppOfHolding-Id"), Some("foo-app"));
    assert_eq!(d.main("StartupNotify"), Some("true"));
    // Desktop actions survive, rewired through our launcher.
    let action = desktop::split_exec(d.get("Desktop Action new", "Exec").unwrap());
    assert_eq!(&action[1..], ["run", "foo-app", "--new-window"]);
    assert_eq!(d.get("Desktop Action new", "Name"), Some("New window"));

    assert_eq!(e.lib.list().unwrap().len(), 1);
    assert_eq!(e.lib.get("foo-app").unwrap(), rec);
}

#[test]
fn adding_twice_is_rejected_and_leaves_no_debris() {
    let e = env();
    let p = write_image(
        &e,
        "Foo.AppImage",
        fixture::simple_appimage("Foo App", None),
    );
    e.lib.add(&p, &AddOptions::default()).unwrap();
    assert!(matches!(
        e.lib.add(&p, &AddOptions::default()),
        Err(Error::AlreadyAdded(_))
    ));
    assert_eq!(e.lib.list().unwrap().len(), 1);
}

#[test]
fn remove_cleans_everything_up() {
    let e = env();
    let p = write_image(
        &e,
        "Foo.AppImage",
        fixture::simple_appimage("Foo App", None),
    );
    let rec = e.lib.add(&p, &AddOptions::default()).unwrap();
    e.lib.remove("foo-app", false).unwrap();
    assert!(!rec.appimage_path.exists());
    assert!(!rec.desktop_file.exists());
    assert!(!rec.icon_file.unwrap().exists());
    assert!(e.lib.list().unwrap().is_empty());
    assert!(matches!(
        e.lib.remove("foo-app", false),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn remove_can_keep_the_appimage() {
    let e = env();
    let p = write_image(
        &e,
        "Foo.AppImage",
        fixture::simple_appimage("Foo App", None),
    );
    let rec = e.lib.add(&p, &AddOptions::default()).unwrap();
    e.lib.remove("foo-app", true).unwrap();
    assert!(rec.appimage_path.exists());
    assert!(!rec.desktop_file.exists());
}

#[test]
fn move_mode_removes_the_original() {
    let e = env();
    let p = write_image(
        &e,
        "Foo.AppImage",
        fixture::simple_appimage("Foo App", None),
    );
    let rec = e
        .lib
        .add(
            &p,
            &AddOptions {
                install: Install::Move,
                id: None,
            },
        )
        .unwrap();
    assert!(!p.exists());
    assert!(rec.appimage_path.exists());
}

#[test]
fn custom_id_and_in_place_add() {
    let e = env();
    fs::create_dir_all(&e.lib.dirs.library_dir).unwrap();
    let p = e.lib.dirs.library_dir.join("Already.AppImage");
    fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
    let rec = e
        .lib
        .add(
            &p,
            &AddOptions {
                install: Install::Copy,
                id: Some("My Foo".into()),
            },
        )
        .unwrap();
    assert_eq!(rec.id, "my-foo");
    assert_eq!(rec.appimage_path, fs::canonicalize(&p).unwrap());
}

#[test]
fn picks_the_right_desktop_file_when_several_exist() {
    // digiKam ships showfoto and digikam entries; we must pick the one matching the file.
    let e = env();
    let showfoto = "[Desktop Entry]\nType=Application\nName=Showfoto\nExec=showfoto %F\n";
    let hidden = "[Desktop Entry]\nType=Application\nName=Helper\nNoDisplay=true\nExec=helper\n";
    let digikam =
        "[Desktop Entry]\nType=Application\nName=digiKam\nExec=digikam %U\nIcon=digikam\n";
    let bytes = fixture::build_appimage(
        None,
        vec![
            Item::File("/org.kde.showfoto.desktop", showfoto.into()),
            Item::File("/a-helper.desktop", hidden.into()),
            Item::File("/org.kde.digikam.desktop", digikam.into()),
            Item::File("/digikam.png", fixture::png(128)),
        ],
    );
    let p = write_image(&e, "digikam-8.4.0-x86-64.appimage", bytes);
    let i = inspect(&p).unwrap();
    assert_eq!(i.name, "digiKam");
    assert_eq!(i.desktop_path, "/org.kde.digikam.desktop");
    assert_eq!(i.icon.unwrap().size, Some(128));
}

#[test]
fn icon_falls_back_to_hicolor_largest() {
    let e = env();
    let desktop = "[Desktop Entry]\nType=Application\nName=Bar\nExec=bar\nIcon=bar\n";
    let bytes = fixture::build_appimage(
        None,
        vec![
            Item::File("/bar.desktop", desktop.into()),
            Item::File(
                "/usr/share/icons/hicolor/48x48/apps/bar.png",
                fixture::png(48),
            ),
            Item::File(
                "/usr/share/icons/hicolor/512x512/apps/bar.png",
                fixture::png(512),
            ),
        ],
    );
    let i = inspect(&write_image(&e, "bar.AppImage", bytes)).unwrap();
    assert_eq!(i.icon.unwrap().size, Some(512));
}

#[test]
fn rejects_non_appimages() {
    let e = env();
    let p = write_image(&e, "text.AppImage", b"hello world, not an elf".to_vec());
    assert!(matches!(inspect(&p), Err(Error::NotAppImage(_))));
    let mut elf_only = fixture::fake_runtime(None);
    elf_only.extend([0u8; 64]);
    let p = write_image(&e, "elf.AppImage", elf_only);
    assert!(matches!(inspect(&p), Err(Error::NotAppImage(_))));
}

#[test]
fn rejects_appimage_without_desktop_entry() {
    let e = env();
    let bytes = fixture::build_appimage(None, vec![Item::File("/AppRun", b"x".to_vec())]);
    let p = write_image(&e, "nodesk.AppImage", bytes);
    assert!(matches!(inspect(&p), Err(Error::NoDesktopEntry)));
}

#[test]
fn failed_add_rolls_back_copied_file() {
    let e = env();
    let bytes = fixture::build_appimage(None, vec![Item::File("/AppRun", b"x".to_vec())]);
    let p = write_image(&e, "nodesk.AppImage", bytes);
    assert!(e.lib.add(&p, &AddOptions::default()).is_err());
    let lib_dir = &e.lib.dirs.library_dir;
    assert!(!lib_dir.exists() || fs::read_dir(lib_dir).unwrap().next().is_none());
}
