//! A cache of unpacked AppImages. Sandboxed launches run the unpacked tree directly (a sandbox
//! cannot use the AppImage runtime's FUSE mount), keyed by the file's SHA-256 so a new version
//! is unpacked once and an unchanged one starts instantly.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::image::AppImage;
use crate::paths::Dirs;

const MARKER: &str = ".appofholding-complete";

pub fn path_for(dirs: &Dirs, sha256: &str) -> PathBuf {
    dirs.extract_dir().join(sha256)
}

pub fn is_extracted(dirs: &Dirs, sha256: &str) -> bool {
    path_for(dirs, sha256).join(MARKER).exists()
}

/// Make sure `appimage` is unpacked; returns the directory. Unpacks into a temporary
/// directory first, so an interrupted extraction never looks complete.
pub fn ensure(dirs: &Dirs, appimage: &Path, sha256: &str) -> Result<PathBuf> {
    let dest = path_for(dirs, sha256);
    if is_extracted(dirs, sha256) {
        return Ok(dest);
    }
    let root = dirs.extract_dir();
    fs::create_dir_all(&root).map_err(Error::io(&root))?;
    let tmp = root.join(format!(
        ".tmp-{}-{}",
        &sha256[..sha256.len().min(12)],
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir(&tmp).map_err(Error::io(&tmp))?;
    let result = (|| -> Result<()> {
        AppImage::open(appimage)?.extract_to(&tmp)?;
        fs::write(tmp.join(MARKER), b"").map_err(Error::io(&tmp))?;
        let _ = fs::remove_dir_all(&dest); // a stale incomplete directory
        fs::rename(&tmp, &dest).map_err(Error::io(&dest))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result.map(|()| dest)
}

/// Delete every unpacked tree whose SHA-256 is not in `keep` (and any leftovers of
/// interrupted extractions). Returns how many were removed.
pub fn prune(dirs: &Dirs, keep: &[String]) -> Result<usize> {
    let root = dirs.extract_dir();
    let Ok(rd) = fs::read_dir(&root) else {
        return Ok(0);
    };
    let mut removed = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) {
            fs::remove_dir_all(e.path()).map_err(Error::io(e.path()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{self, Item};
    use std::os::unix::fs::PermissionsExt;

    fn setup() -> (tempfile::TempDir, Dirs, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::with_root(tmp.path());
        let p = tmp.path().join("Foo.AppImage");
        let bytes = fixture::build_appimage(
            None,
            vec![
                Item::File("/AppRun", b"#!/bin/sh\necho hi\n".to_vec()),
                Item::File("/usr/lib/libfoo.so", b"lib".to_vec()),
                Item::File("/usr/share/data/a.txt", b"a".to_vec()),
                Item::Symlink {
                    path: "/usr/lib/libfoo.so.1",
                    target: "libfoo.so",
                },
                Item::Symlink {
                    path: "/escape",
                    target: "../../../etc/passwd",
                },
            ],
        );
        fs::write(&p, bytes).unwrap();
        (tmp, dirs, p)
    }

    #[test]
    fn unpacks_files_dirs_symlinks_and_permissions() {
        let (_t, dirs, img) = setup();
        let out = ensure(&dirs, &img, "abc123").unwrap();
        assert_eq!(fs::read(out.join("usr/share/data/a.txt")).unwrap(), b"a");
        assert_eq!(
            fs::read_link(out.join("usr/lib/libfoo.so.1")).unwrap(),
            Path::new("libfoo.so")
        );
        assert_eq!(
            fs::metadata(out.join("AppRun"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0o111
        );
        assert!(is_extracted(&dirs, "abc123"));
        // A second call is a cache hit: it must not touch the tree.
        fs::write(out.join("sentinel"), b"x").unwrap();
        ensure(&dirs, &img, "abc123").unwrap();
        assert!(out.join("sentinel").exists());
    }

    #[test]
    fn symlinks_are_kept_as_links_never_followed_outside() {
        let (_t, dirs, img) = setup();
        let out = ensure(&dirs, &img, "s").unwrap();
        // The link is recorded as-is (it only has meaning inside the sandbox); nothing was copied.
        assert!(
            fs::symlink_metadata(out.join("escape"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn failed_extraction_leaves_nothing_that_looks_complete() {
        let (tmp, dirs, _) = setup();
        let bad = tmp.path().join("bad.AppImage");
        fs::write(&bad, b"not an appimage").unwrap();
        assert!(ensure(&dirs, &bad, "bad").is_err());
        assert!(!is_extracted(&dirs, "bad"));
        let leftovers: Vec<_> = fs::read_dir(dirs.extract_dir())
            .unwrap()
            .flatten()
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn prune_keeps_only_the_listed_hashes() {
        let (_t, dirs, img) = setup();
        ensure(&dirs, &img, "keep").unwrap();
        ensure(&dirs, &img, "drop").unwrap();
        fs::create_dir_all(dirs.extract_dir().join(".tmp-leftover")).unwrap();
        assert_eq!(prune(&dirs, &["keep".to_string()]).unwrap(), 2);
        assert!(is_extracted(&dirs, "keep") && !is_extracted(&dirs, "drop"));
        assert_eq!(
            prune(&Dirs::with_root(Path::new("/nonexistent-root")), &[]).unwrap(),
            0
        );
    }
}
