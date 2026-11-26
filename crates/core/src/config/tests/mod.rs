use super::*;
use crate::fixture;
use crate::http::fake::FakeHttp;
use crate::paths::Dirs;
use sha2::{Digest, Sha256};
use std::fs;

fn lib() -> (tempfile::TempDir, Library) {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    (tmp, lib)
}

fn set_up(tmp: &std::path::Path, lib: &Library) {
    let p = tmp.join("Foo.AppImage");
    fs::write(
        &p,
        fixture::simple_appimage(
            "Foo App",
            Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
        ),
    )
    .unwrap();
    lib.add(&p, &AddOptions::default()).unwrap();
    lib.set_policy("foo-app", Policy::Auto).unwrap();
    lib.set_sandbox(
        "foo-app",
        &Settings {
            network: false,
            paths: vec![PathGrant {
                path: "/tmp".into(),
                write: true,
            }],
            ..Settings::for_level(Level::Standard)
        },
    )
    .unwrap();
    let mut p = Profile::default();
    p.args = vec!["--flag".into(), "a b".into()];
    p.presets = vec!["x11".into()];
    p.env.insert("K".into(), "v".into());
    lib.set_profile("foo-app", "default", &p).unwrap();
    lib.set_profile(
        "foo-app",
        "debug",
        &Profile {
            presets: vec!["debug".into()],
            ..Profile::default()
        },
    )
    .unwrap();
    lib.set_hook("foo-app", "post-update", "echo done").unwrap();
}

mod export_apply;
mod validation;
