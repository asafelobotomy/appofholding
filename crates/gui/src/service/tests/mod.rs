use super::*;
use appofholding_core::fixture;
use appofholding_core::http::fake::FakeHttp;

fn lib() -> (tempfile::TempDir, Library) {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    (tmp, lib)
}

/// Serialises the tests that point `APPOFHOLDING_ROOT` somewhere (it is process-wide).
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn add_foo(tmp: &Path, lib: &Library) {
    let p = tmp.join("Foo.AppImage");
    std::fs::write(
        &p,
        fixture::simple_appimage(
            "Foo App",
            Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
        ),
    )
    .unwrap();
    lib.add(&p, &AddOptions::default()).unwrap();
}

const REL: &str = r#"{"tag_name":"v2.0.0","prerelease":false,"draft":false,"body":"Notes","assets":[
    {"name":"Foo-2.0.0-x86_64.AppImage","browser_download_url":"https://dl/a","digest":"sha256:aa"},
    {"name":"Foo-2.0.0-x86_64.AppImage.zsync","browser_download_url":"https://dl/z"}]}"#;

fn hex_sha256(b: &[u8]) -> String {
    use std::process::Stdio;
    // Avoid another dependency: use the core library's hashing through a temp file.
    let f = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(f.path(), b).unwrap();
    let _ = Stdio::null();
    appofholding_core::image::hash_file(f.path()).unwrap().0
}

mod forms_flows;
mod self_schedule;
mod settings_library;
