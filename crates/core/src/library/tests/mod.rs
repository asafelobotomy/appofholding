use super::*;

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

mod entries;
mod integration;
