use super::*;
use crate::fixture;
use crate::http::fake::FakeHttp;
use crate::library::AddOptions;
use crate::notify::Recorder;
use crate::paths::Dirs;
use sha2::{Digest, Sha256};
use std::process::Command;

const UPD: &str = "gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync";
const API: &str = "https://api.github.com/repos/o/foo/releases/latest";

struct Env {
    tmp: tempfile::TempDir,
    lib: Library,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    Env { tmp, lib }
}

/// Install version 1.0.0 (with embedded GitHub update info).
fn installed(e: &Env) -> AppRecord {
    let p = e.tmp.path().join("Foo-1.0.0-x86_64.AppImage");
    fs::write(
        &p,
        fixture::versioned_appimage("Foo App", "1.0.0", Some(UPD)),
    )
    .unwrap();
    e.lib.add(&p, &AddOptions::default()).unwrap()
}

/// A fake GitHub serving release `ver` whose asset is a real fixture AppImage.
fn github(ver: &str) -> FakeHttp {
    let img = fixture::versioned_appimage("Foo App", ver, Some(UPD));
    let sha = hex::encode(Sha256::digest(&img));
    let rel = format!(
        r#"{{"tag_name":"v{ver}","prerelease":false,"draft":false,"assets":[
              {{"name":"Foo-{ver}-x86_64.AppImage","browser_download_url":"https://dl/foo-{ver}","size":{},"digest":"sha256:{sha}"}}]}}"#,
        img.len()
    );
    FakeHttp::default()
        .json(API, &rel)
        .bytes(&format!("https://dl/foo-{ver}"), img)
}

fn apply(e: &Env, http: &FakeHttp, opts: &ApplyOptions) -> Result<Applied> {
    e.lib.update(http, "foo-app", opts, &mut |_, _| {})
}

mod checks;
mod failures_hooks;
mod update;
