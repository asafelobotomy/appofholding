use super::*;
use crate::fixture::{self, Item};
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
    lib.apply_suggestion(&id, "electron-wayland").unwrap();
    assert!(
        lib.profile(&id, DEFAULT_PROFILE)
            .unwrap()
            .presets
            .contains(&"electron-auto".to_string())
    );

    // Sandbox: the menu entry loses the app's own --no-sandbox ...
    let entry = || fs::read_to_string(lib.get(&id).unwrap().desktop_file).unwrap();
    assert!(entry().contains("--no-sandbox"));
    lib.apply_suggestion(&id, "chromium-sandbox").unwrap();
    assert!(!entry().contains("--no-sandbox"), "{}", entry());
    assert_eq!(
        keys(&lib.suggestions(&id, &LINUX_DESKTOP).unwrap()),
        ["chromium-sandbox-off"]
    );
    // ... and getting it back is one click, after which it is not offered again.
    lib.apply_suggestion(&id, "chromium-sandbox-off").unwrap();
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
    lib.apply_suggestion(&id, "unpack").unwrap();
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
    assert!(lib.apply_suggestion(&id, "nonsense").is_err());
}
