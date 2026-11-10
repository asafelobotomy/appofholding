use super::*;
use crate::fixture;
use crate::http::fake::FakeHttp;
use crate::notify::Recorder;
use sha2::{Digest, Sha256};

const API: &str = "https://api.github.com/repos/asafelobotomy/appofholding/releases/latest";

fn release(tag: &str, pkg: &[u8], appimage: &[u8]) -> String {
    let sha = |b: &[u8]| hex::encode(Sha256::digest(b));
    format!(
        r#"{{"tag_name":"{tag}","prerelease":false,"draft":false,"body":"Notes","assets":[
              {{"name":"appofholding-9.9.9-1-x86_64.pkg.tar.zst","browser_download_url":"https://dl/pkg","size":{},"digest":"sha256:{}"}},
              {{"name":"appofholding-9.9.9-aarch64.pkg.tar.zst","browser_download_url":"https://dl/pkg-arm"}},
              {{"name":"appofholding-9.9.9-x86_64.AppImage","browser_download_url":"https://dl/img","size":{},"digest":"sha256:{}"}}]}}"#,
        pkg.len(),
        sha(pkg),
        appimage.len(),
        sha(appimage)
    )
}

fn pacman() -> InstallKind {
    InstallKind::Pacman {
        package: "appofholding".into(),
    }
}

fn dirs() -> (tempfile::TempDir, Dirs) {
    let tmp = tempfile::tempdir().unwrap();
    let d = Dirs::with_root(tmp.path());
    (tmp, d)
}

#[test]
fn install_kind_detection() {
    let none = |_: &Path| None::<String>;
    let owned = |_: &Path| Some("appofholding".to_string());
    let tmp = tempfile::tempdir().unwrap();
    let img = tmp.path().join("A.AppImage");
    fs::write(&img, b"x").unwrap();
    let exe = Path::new("/usr/bin/appofholding");
    assert_eq!(
        detect_install(exe, Some(&img), None, &owned),
        InstallKind::AppImage { path: img.clone() }
    );
    assert_eq!(detect_install(exe, None, None, &owned), pacman());
    assert_eq!(
        detect_install(
            Path::new("/home/u/.local/bin/appofholding"),
            None,
            Some(Path::new("/home/u")),
            &none
        ),
        InstallKind::UserLocal {
            path: "/home/u/.local/bin/appofholding".into()
        }
    );
    assert_eq!(
        detect_install(exe, None, Some(Path::new("/home/u")), &none),
        InstallKind::Unknown
    );
    // A stale $APPIMAGE that does not exist is ignored.
    assert_eq!(
        detect_install(exe, Some(Path::new("/nope.AppImage")), None, &owned),
        pacman()
    );
}

#[test]
fn reports_newer_older_and_equal_releases_and_picks_the_right_file() {
    let newer = FakeHttp::default().json(API, &release("v99.0.0", b"pkg", b"img"));
    let s = check(&newer, pacman(), Arch::X86_64).unwrap();
    assert!(s.update_available && s.latest == "99.0.0" && s.current == current_version());
    assert_eq!(
        s.asset.unwrap().name,
        "appofholding-9.9.9-1-x86_64.pkg.tar.zst"
    );
    assert_eq!(
        s.release_url,
        "https://github.com/asafelobotomy/appofholding/releases/tag/v99.0.0"
    );

    let img = check(
        &newer,
        InstallKind::AppImage { path: "/a".into() },
        Arch::X86_64,
    )
    .unwrap();
    assert_eq!(
        img.asset.unwrap().name,
        "appofholding-9.9.9-x86_64.AppImage"
    );
    assert!(
        check(&newer, InstallKind::Unknown, Arch::X86_64)
            .unwrap()
            .asset
            .is_none()
    );
    assert_eq!(
        check(&newer, pacman(), Arch::Aarch64)
            .unwrap()
            .asset
            .unwrap()
            .name,
        "appofholding-9.9.9-aarch64.pkg.tar.zst"
    );

    let same = FakeHttp::default().json(
        API,
        &release(&format!("v{}", current_version()), b"p", b"i"),
    );
    assert!(
        !check(&same, pacman(), Arch::X86_64)
            .unwrap()
            .update_available
    );
    let older = FakeHttp::default().json(API, &release("v0.0.1", b"p", b"i"));
    assert!(
        !check(&older, pacman(), Arch::X86_64)
            .unwrap()
            .update_available,
        "never offers a downgrade"
    );
}

#[test]
fn cached_checks_do_not_ask_github_again_until_forced() {
    let (_t, d) = dirs();
    let http = FakeHttp::default().json(API, &release("v99.0.0", b"p", b"i"));
    assert!(
        !check_cached(&http, &d, pacman(), Arch::X86_64, false)
            .unwrap()
            .from_cache
    );
    let again = check_cached(&http, &d, pacman(), Arch::X86_64, false).unwrap();
    assert!(again.from_cache && again.update_available && again.latest == "99.0.0");
    assert_eq!(http.requests.lock().unwrap().len(), 1);
    assert!(
        !check_cached(&http, &d, pacman(), Arch::X86_64, true)
            .unwrap()
            .from_cache
    );
    assert_eq!(http.requests.lock().unwrap().len(), 2);
}

#[test]
fn the_background_run_announces_a_release_once() {
    let (_t, d) = dirs();
    let http = FakeHttp::default().json(API, &release("v99.0.0", b"p", b"i"));
    let notes = Recorder::default();
    let msg = notify_if_new(&http, &notes, &d, pacman(), Arch::X86_64)
        .unwrap()
        .unwrap();
    assert!(
        msg.contains("99.0.0") && msg.contains("self update --install"),
        "{msg}"
    );
    assert!(
        notify_if_new(&http, &notes, &d, pacman(), Arch::X86_64)
            .unwrap()
            .is_none()
    );
    assert_eq!(notes.0.borrow().len(), 1);
    // A newer release later is announced again.
    let newer = FakeHttp::default().json(API, &release("v100.0.0", b"p", b"i"));
    let mut st = load_state(&d);
    st.last_checked = 0;
    save_state(&d, &st).unwrap();
    assert!(
        notify_if_new(&newer, &notes, &d, pacman(), Arch::X86_64)
            .unwrap()
            .is_some()
    );
    assert_eq!(notes.0.borrow().len(), 2);
    // Nothing to say when up to date.
    let (_t2, d2) = dirs();
    let same = FakeHttp::default().json(
        API,
        &release(&format!("v{}", current_version()), b"p", b"i"),
    );
    assert!(
        notify_if_new(&same, &notes, &d2, pacman(), Arch::X86_64)
            .unwrap()
            .is_none()
    );
}

#[test]
fn pacman_installs_get_a_verified_package_and_the_command_to_install_it() {
    let (tmp, _d) = dirs();
    let pkg = b"fake package bytes".to_vec();
    let http = FakeHttp::default()
        .json(API, &release("v99.0.0", &pkg, b"i"))
        .bytes("https://dl/pkg", pkg.clone());
    let status = check(&http, pacman(), Arch::X86_64).unwrap();
    // A package left over from an earlier update is in the way.
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("appofholding-0.0.1-1-x86_64.pkg.tar.zst"), b"old").unwrap();
    let out = apply(&http, &status, &work, &mut |_, _| {}).unwrap();
    let Applied::PackageReady { to, file, command } = out else {
        panic!("expected a package")
    };
    assert_eq!(to, "99.0.0");
    assert_eq!(fs::read(&file).unwrap(), pkg);
    assert_eq!(&command[..3], ["run0", "pacman", "-U"]);
    assert!(
        !tmp.path()
            .join("work/appofholding-0.0.1-1-x86_64.pkg.tar.zst")
            .exists(),
        "an older download is cleared away"
    );
    assert_eq!(command[3], file.display().to_string());

    // A corrupted download is rejected and leaves no file behind.
    let bad = FakeHttp::default()
        .json(API, &release("v99.0.0", &pkg, b"i"))
        .bytes("https://dl/pkg", b"tampered".to_vec());
    let status = check(&bad, pacman(), Arch::X86_64).unwrap();
    let err = apply(&bad, &status, &tmp.path().join("work2"), &mut |_, _| {}).unwrap_err();
    assert!(
        matches!(err, Error::Checksum { .. }) || err.to_string().contains("wrong size"),
        "{err}"
    );
}

#[test]
fn appimage_installs_are_replaced_in_place_with_a_backup() {
    let (tmp, _d) = dirs();
    let current = tmp.path().join("appofholding-x86_64.AppImage");
    let old = fixture::versioned_appimage("appOFholding", "0.0.1", None);
    fs::write(&current, &old).unwrap();
    let new = fixture::versioned_appimage("appOFholding", "99.0.0", None);
    let http = FakeHttp::default()
        .json(API, &release("v99.0.0", b"p", &new))
        .bytes("https://dl/img", new.clone());
    let install = InstallKind::AppImage {
        path: current.clone(),
    };
    let status = check(&http, install, Arch::X86_64).unwrap();
    let Applied::Replaced { to, path, backup } =
        apply(&http, &status, &tmp.path().join("work"), &mut |_, _| {}).unwrap()
    else {
        panic!()
    };
    assert_eq!(to, "99.0.0");
    assert_eq!(fs::read(&path).unwrap(), new);
    assert_eq!(
        fs::read(&backup).unwrap(),
        old,
        "the previous version is kept"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o111,
        0,
        "still executable"
    );

    // Something that is not an AppImage never replaces the running one.
    let junk = b"<html>oops</html>".to_vec();
    let http = FakeHttp::default()
        .json(API, &release("v99.0.0", b"p", &junk))
        .bytes("https://dl/img", junk);
    let status = check(
        &http,
        InstallKind::AppImage {
            path: current.clone(),
        },
        Arch::X86_64,
    )
    .unwrap();
    let before = fs::read(&current).unwrap();
    let err = apply(&http, &status, &tmp.path().join("work3"), &mut |_, _| {})
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("not a usable AppImage") && err.contains("nothing was changed"),
        "{err}"
    );
    assert_eq!(fs::read(&current).unwrap(), before);
}

#[test]
fn local_and_unknown_installs_get_instructions_and_nothing_changes() {
    let (tmp, _d) = dirs();
    let http = FakeHttp::default().json(API, &release("v99.0.0", b"p", b"i"));
    for kind in [
        InstallKind::UserLocal {
            path: "/home/u/.local/bin/appofholding".into(),
        },
        InstallKind::Unknown,
    ] {
        let status = check(&http, kind, Arch::X86_64).unwrap();
        let Applied::Manual { instructions, .. } =
            apply(&http, &status, tmp.path(), &mut |_, _| {}).unwrap()
        else {
            panic!()
        };
        assert!(
            instructions.contains("releases/tag/v99.0.0"),
            "{instructions}"
        );
    }
    // Already current.
    let same = FakeHttp::default().json(
        API,
        &release(&format!("v{}", current_version()), b"p", b"i"),
    );
    let status = check(&same, pacman(), Arch::X86_64).unwrap();
    assert_eq!(
        apply(&same, &status, tmp.path(), &mut |_, _| {}).unwrap(),
        Applied::AlreadyCurrent
    );
}

#[test]
fn a_release_without_a_matching_file_explains_itself() {
    let (tmp, _d) = dirs();
    let http = FakeHttp::default().json(
        API,
        r#"{"tag_name":"v99.0.0","prerelease":false,"draft":false,"assets":[]}"#,
    );
    let status = check(&http, pacman(), Arch::X86_64).unwrap();
    let err = apply(&http, &status, tmp.path(), &mut |_, _| {})
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("no file for this installation") && err.contains("releases/tag/v99.0.0"),
        "{err}"
    );
}
