//! A cache of unpacked AppImages. Sandboxed launches run the unpacked tree directly (a sandbox
//! cannot use the AppImage runtime's FUSE mount), keyed by the file's SHA-256 so a new version
//! is unpacked once and an unchanged one starts instantly.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::image::AppImage;
use crate::paths::Dirs;

const MARKER: &str = ".appofholding-complete";
/// `<extract dir>/.lock-<sha256>` serialises unpacking one version.
const LOCK_PREFIX: &str = ".lock-";

pub fn path_for(dirs: &Dirs, sha256: &str) -> PathBuf {
    dirs.extract_dir().join(sha256)
}

pub fn is_extracted(dirs: &Dirs, sha256: &str) -> bool {
    path_for(dirs, sha256).join(MARKER).exists()
}

/// Make sure `appimage` is unpacked; returns the directory. Unpacks into a temporary
/// directory first, so an interrupted extraction never looks complete. Concurrent first
/// launches of the same version take turns (a lock file per hash): the second one finds the
/// tree complete and uses it, so a tree an app is already running from is never replaced.
pub fn ensure(dirs: &Dirs, appimage: &Path, sha256: &str) -> Result<PathBuf> {
    let dest = path_for(dirs, sha256);
    if is_extracted(dirs, sha256) {
        return Ok(dest);
    }
    let root = dirs.extract_dir();
    fs::create_dir_all(&root).map_err(Error::io(&root))?;
    let lock_path = root.join(format!("{LOCK_PREFIX}{sha256}"));
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(Error::io(&lock_path))?;
    lock.lock().map_err(Error::io(&lock_path))?; // released when `lock` is dropped
    if is_extracted(dirs, sha256) {
        return Ok(dest); // another launch finished while we waited
    }
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
        // Under the lock nobody else can complete this tree, so whatever is here is a stale
        // incomplete directory from an interrupted run.
        let _ = fs::remove_dir_all(&dest);
        fs::rename(&tmp, &dest).map_err(Error::io(&dest))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    drop(lock);
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
        // The lock file of a version nobody keeps any more.
        if let Some(hash) = name.strip_prefix(LOCK_PREFIX) {
            if !keep.iter().any(|k| k == hash) && e.path().is_file() {
                let _ = fs::remove_file(e.path());
            }
            continue;
        }
        if keep.contains(&name) || !e.path().is_dir() {
            continue;
        }
        // Another launch may still be unpacking: leave recent temporary directories alone.
        let fresh = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < std::time::Duration::from_secs(3600));
        if name.starts_with(".tmp-") && fresh {
            continue;
        }
        // A sandboxed app may still be running an old version from this tree.
        if crate::running::tree_in_use(&e.path()) {
            continue;
        }
        fs::remove_dir_all(e.path()).map_err(Error::io(e.path()))?;
        removed += 1;
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
    fn concurrent_first_launches_share_one_tree() {
        let (_t, dirs, img) = setup();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let (dirs, img) = (dirs.clone(), img.clone());
                std::thread::spawn(move || ensure(&dirs, &img, "same").unwrap())
            })
            .collect();
        let outs: Vec<PathBuf> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(outs.iter().all(|o| o == &outs[0]));
        assert!(is_extracted(&dirs, "same"));
        assert_eq!(
            fs::read(outs[0].join("usr/share/data/a.txt")).unwrap(),
            b"a"
        );
    }

    #[test]
    fn extraction_never_writes_through_or_over_an_existing_link() {
        // The fixture writer refuses duplicate names, so plant what an earlier entry of a
        // malicious image (a symlink with the same name) would have left in the tree.
        let (tmp, _dirs, img) = setup();
        let image = AppImage::open(&img).unwrap();
        let victim = tmp.path().join("victim");
        fs::write(&victim, b"original").unwrap();
        let outside_dir = tmp.path().join("outside");
        fs::create_dir(&outside_dir).unwrap();

        // A link where a file goes: not followed, not replaced.
        let dest = tmp.path().join("d1");
        fs::create_dir(&dest).unwrap();
        std::os::unix::fs::symlink(&victim, dest.join("AppRun")).unwrap();
        assert!(image.extract_to(&dest).is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"original");

        // A link where a parent directory goes: nothing is created through it.
        let dest = tmp.path().join("d2");
        fs::create_dir(&dest).unwrap();
        std::os::unix::fs::symlink(&outside_dir, dest.join("usr")).unwrap();
        assert!(image.extract_to(&dest).is_err());
        assert_eq!(fs::read_dir(&outside_dir).unwrap().count(), 0);

        // A file where a symlink goes: not replaced.
        let dest = tmp.path().join("d3");
        fs::create_dir_all(dest.join("usr/lib")).unwrap();
        fs::write(dest.join("usr/lib/libfoo.so.1"), b"mine").unwrap();
        assert!(image.extract_to(&dest).is_err());
        assert_eq!(fs::read(dest.join("usr/lib/libfoo.so.1")).unwrap(), b"mine");
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
            // The (empty) lock file stays; `prune` removes it once no app needs the hash.
            .filter(|e| !e.file_name().to_string_lossy().starts_with(LOCK_PREFIX))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn prune_keeps_only_the_listed_hashes() {
        let (_t, dirs, img) = setup();
        ensure(&dirs, &img, "keep").unwrap();
        ensure(&dirs, &img, "drop").unwrap();
        // A recent temporary directory may be another launch still unpacking; an old one is
        // a leftover.
        let busy = dirs.extract_dir().join(".tmp-busy");
        let old = dirs.extract_dir().join(".tmp-leftover");
        fs::create_dir_all(&busy).unwrap();
        fs::create_dir_all(&old).unwrap();
        fs::File::open(&old)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7200))
            .unwrap();
        // A stray file is not a tree and is left alone.
        fs::write(dirs.extract_dir().join("stray"), b"x").unwrap();
        assert!(dirs.extract_dir().join(".lock-drop").is_file());
        assert_eq!(prune(&dirs, &["keep".to_string()]).unwrap(), 2);
        assert!(!dirs.extract_dir().join(".lock-drop").exists());
        assert!(dirs.extract_dir().join(".lock-keep").exists());
        assert!(is_extracted(&dirs, "keep") && !is_extracted(&dirs, "drop"));
        assert!(busy.exists() && !old.exists());
        assert_eq!(
            prune(&Dirs::with_root(Path::new("/nonexistent-root")), &[]).unwrap(),
            0
        );
    }
}
