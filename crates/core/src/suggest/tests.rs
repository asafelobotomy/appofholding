use super::*;
use crate::fixture::{self, Item};
use crate::http::fake::FakeHttp;
use crate::library::AddOptions;
use crate::paths::Dirs;

fn electron_like(exec: &str, runtime_note: Option<&str>) -> Vec<u8> {
    let desktop =
        format!("[Desktop Entry]\nType=Application\nName=Chatty\nExec={exec}\nIcon=icon\n");
    fixture::build_appimage(
        runtime_note,
        vec![
            Item::File("/chatty.desktop", desktop.into_bytes()),
            Item::File("/icon.png", fixture::png(256)),
            Item::File("/AppRun", b"#!/bin/sh\nexit 0\n".to_vec()),
            Item::File("/chrome-sandbox", b"x".to_vec()),
            Item::File("/resources/app.asar", b"x".to_vec()),
        ],
    )
}

fn lib_with(image: Vec<u8>) -> (tempfile::TempDir, Library, String) {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    let p = tmp.path().join("Chatty.AppImage");
    fs::write(&p, image).unwrap();
    let id = lib.add(&p, &AddOptions::default()).unwrap().id;
    (tmp, lib, id)
}

const LINUX_DESKTOP: Host = Host {
    wayland: true,
    user_namespaces: true,
    libfuse2: true,
};

fn keys(v: &[Suggestion]) -> Vec<&str> {
    v.iter().map(|s| s.key.as_str()).collect()
}

#[test]
fn an_electron_app_gets_wayland_and_sandbox_suggestions_that_apply_and_undo() {
    let (_t, lib, id) = lib_with(electron_like("AppRun --no-sandbox %U", None));
    let s = lib.suggestions(&id, &LINUX_DESKTOP).unwrap();
    assert_eq!(keys(&s), ["electron-wayland", "chromium-sandbox"]);

    // Wayland: adds the preset, after which it is not suggested again.
    lib.apply_suggestion(&FakeHttp::default(), &id, "electron-wayland")
        .unwrap();
    assert!(
        lib.profile(&id, DEFAULT_PROFILE)
            .unwrap()
            .presets
            .contains(&"electron-auto".to_string())
    );

    // Sandbox: the menu entry loses the app's own --no-sandbox ...
    let entry = || fs::read_to_string(lib.get(&id).unwrap().desktop_file).unwrap();
    assert!(entry().contains("--no-sandbox"));
    lib.apply_suggestion(&FakeHttp::default(), &id, "chromium-sandbox")
        .unwrap();
    assert!(!entry().contains("--no-sandbox"), "{}", entry());
    assert_eq!(
        keys(&lib.suggestions(&id, &LINUX_DESKTOP).unwrap()),
        ["chromium-sandbox-off"]
    );
    // ... and getting it back is one click, after which it is not offered again.
    lib.apply_suggestion(&FakeHttp::default(), &id, "chromium-sandbox-off")
        .unwrap();
    assert!(entry().contains("--no-sandbox"));
    assert!(lib.suggestions(&id, &LINUX_DESKTOP).unwrap().is_empty());
}

#[test]
fn suggestions_respect_the_computer_and_the_apps_settings() {
    let (_t, lib, id) = lib_with(electron_like("AppRun --no-sandbox %U", None));
    // X11 session, no user namespaces (e.g. Ubuntu's AppArmor restriction): nothing to say.
    let limited = Host {
        wayland: false,
        user_namespaces: false,
        libfuse2: true,
    };
    assert!(lib.suggestions(&id, &limited).unwrap().is_empty());
    // Our own sandbox already handles Chromium; an app without --no-sandbox needs nothing.
    lib.set_sandbox(
        &id,
        &crate::sandbox::Settings::for_level(crate::sandbox::Level::Standard),
    )
    .unwrap();
    assert_eq!(
        keys(&lib.suggestions(&id, &LINUX_DESKTOP).unwrap()),
        ["electron-wayland"]
    );
    // Dismissed is dismissed.
    lib.dismiss_suggestion(&id, "electron-wayland").unwrap();
    assert!(lib.suggestions(&id, &LINUX_DESKTOP).unwrap().is_empty());

    let (_t2, lib2, id2) = lib_with(electron_like("AppRun %U", None));
    assert_eq!(
        keys(&lib2.suggestions(&id2, &LINUX_DESKTOP).unwrap()),
        ["electron-wayland"]
    );
    // A plain (non-Electron) app gets nothing.
    let tmp = tempfile::tempdir().unwrap();
    let lib3 = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("x")).unwrap();
    let p = tmp.path().join("Foo.AppImage");
    fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
    let id3 = lib3.add(&p, &AddOptions::default()).unwrap().id;
    assert!(lib3.suggestions(&id3, &LINUX_DESKTOP).unwrap().is_empty());
}

#[test]
fn an_old_runtime_without_libfuse2_is_unpacked_instead() {
    // The classic runtime names libfuse.so.2; the note lands in the runtime part of the file.
    let (_t, lib, id) = lib_with(electron_like("AppRun %U", Some("libfuse.so.2")));
    let no_fuse = Host {
        wayland: false,
        user_namespaces: true,
        libfuse2: false,
    };
    assert_eq!(keys(&lib.suggestions(&id, &no_fuse).unwrap()), ["unpack"]);
    lib.apply_suggestion(&FakeHttp::default(), &id, "unpack")
        .unwrap();
    assert!(lib.profile(&id, DEFAULT_PROFILE).unwrap().extract_and_run);
    assert!(lib.suggestions(&id, &no_fuse).unwrap().is_empty());
    // With libfuse2 installed it starts fine as it is.
    let (_t2, lib2, id2) = lib_with(electron_like("AppRun %U", Some("libfuse.so.2")));
    assert!(
        !keys(
            &lib2
                .suggestions(
                    &id2,
                    &Host {
                        libfuse2: true,
                        ..no_fuse
                    }
                )
                .unwrap()
        )
        .contains(&"unpack")
    );
    assert!(
        lib.apply_suggestion(&FakeHttp::default(), &id, "nonsense")
            .is_err()
    );
}

/// An app with no update info whose desktop id and AppStream data hint at GitHub.
fn hinted_app() -> Vec<u8> {
    fixture::build_appimage(
        None,
        vec![
            Item::File(
                "/io.github.someone.squashedname.desktop",
                b"[Desktop Entry]\nType=Application\nName=Squashed Name\nExec=AppRun\nIcon=icon\n"
                    .to_vec(),
            ),
            Item::File("/icon.png", fixture::png(256)),
            Item::File("/AppRun", b"#!/bin/sh\nexit 0\n".to_vec()),
        ],
    )
}

const ONE_APPIMAGE: &str = r#"{"tag_name":"v2.0.0","prerelease":false,"draft":false,"assets":[
    {"name":"Squashed-Name-2.0.0-x86_64.AppImage","browser_download_url":"https://dl/a","size":1},
    {"name":"Squashed-Name-2.0.0-aarch64.AppImage","browser_download_url":"https://dl/b","size":1}]}"#;

#[test]
fn a_detected_update_source_is_confirmed_before_it_is_saved() {
    if crate::assets::Arch::host() != crate::assets::Arch::X86_64 {
        return;
    }
    let (_t, lib, id) = lib_with(hinted_app());
    let s = lib.suggestions(&id, &LINUX_DESKTOP).unwrap();
    assert_eq!(keys(&s), ["update-source"]);
    assert_eq!(
        s[0].title,
        "Get updates from github.com/someone/squashedname"
    );
    assert!(
        s[0].detail
            .contains("its app id io.github.someone.squashedname")
    );
    assert!(s[0].detail.contains("1 other possible place"));

    // Neither spelling has a usable release: nothing is saved and the error says why.
    let none = FakeHttp::default().json(
        "https://api.github.com/repos/someone/squashed-name/releases/latest",
        r#"{"tag_name":"v1","prerelease":false,"draft":false,"assets":[{"name":"src.tar.gz","browser_download_url":"https://dl/s","size":1}]}"#,
    );
    let err = lib
        .apply_suggestion(&none, &id, "update-source")
        .unwrap_err()
        .to_string();
    assert!(err.contains("github.com/someone/squashedname"), "{err}");
    assert!(err.contains("has no AppImage for this computer"), "{err}");
    assert!(lib.source(&id).unwrap().is_none());

    // The squashed id 404s, the name's spelling has an AppImage: that one is used.
    let http = FakeHttp::default().json(
        "https://api.github.com/repos/someone/squashed-name/releases/latest",
        ONE_APPIMAGE,
    );
    let msg = lib.apply_suggestion(&http, &id, "update-source").unwrap();
    assert!(msg.contains("github.com/someone/squashed-name"), "{msg}");
    assert!(msg.contains("Squashed-Name-2.0.0-x86_64.AppImage"), "{msg}");
    let saved = lib.source(&id).unwrap().unwrap();
    assert_eq!(
        saved.source.describe(),
        "github.com/someone/squashed-name [latest]"
    );
    // With a source, it is not suggested again.
    assert!(lib.suggestions(&id, &LINUX_DESKTOP).unwrap().is_empty());
}

#[test]
fn apps_with_embedded_update_info_get_no_source_suggestion() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("x")).unwrap();
    let p = tmp.path().join("Foo.AppImage");
    fs::write(
        &p,
        fixture::simple_appimage("Foo App", Some("gh-releases-zsync|o|r|latest|Foo-*.zsync")),
    )
    .unwrap();
    let id = lib.add(&p, &AddOptions::default()).unwrap().id;
    lib.clear_source(&id).unwrap();
    assert!(lib.suggestions(&id, &LINUX_DESKTOP).unwrap().is_empty());
}

#[test]
fn a_dismissed_source_suggestion_stays_away() {
    let (_t, lib, id) = lib_with(hinted_app());
    lib.dismiss_suggestion(&id, "update-source").unwrap();
    assert!(lib.suggestions(&id, &LINUX_DESKTOP).unwrap().is_empty());
}

#[test]
fn the_likely_source_is_named_without_going_online() {
    let (_t, lib, id) = lib_with(hinted_app());
    assert_eq!(
        lib.detected_source(&id).as_deref(),
        Some("github.com/someone/squashedname")
    );
    let (_t2, lib2, id2) = lib_with(electron_like("AppRun", None));
    assert_eq!(lib2.detected_source(&id2), None);
}

#[test]
fn a_release_of_another_project_is_refused_unless_it_is_the_very_same_file() {
    if crate::assets::Arch::host() != crate::assets::Arch::X86_64 {
        return;
    }
    let (_t, lib, id) = lib_with(hinted_app());
    let other = |digest: &str| {
        format!(
            r#"{{"tag_name":"v1","prerelease":false,"draft":false,"assets":[
            {{"name":"search-plugins-x86_64.AppImage","browser_download_url":"https://dl/x","size":1,"digest":"{digest}"}}]}}"#
        )
    };
    let url = "https://api.github.com/repos/someone/squashedname/releases/latest";
    let err = lib
        .apply_suggestion(
            &FakeHttp::default().json(url, &other("sha256:00")),
            &id,
            "update-source",
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not look like this app"), "{err}");
    assert!(lib.source(&id).unwrap().is_none());

    // The same bytes as the installed file prove it, whatever the file is called.
    let mine = lib.get(&id).unwrap().sha256;
    let msg = lib
        .apply_suggestion(
            &FakeHttp::default().json(url, &other(&format!("sha256:{mine}"))),
            &id,
            "update-source",
        )
        .unwrap();
    assert!(msg.contains("the file you have"), "{msg}");
    assert!(lib.source(&id).unwrap().is_some());
}
