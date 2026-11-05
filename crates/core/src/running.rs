//! Finding running instances of an AppImage.
//!
//! An AppImage's runtime stays alive while the app runs and exports `APPIMAGE=<path>` to the
//! app and its children, so scanning `/proc` for either the executable path or that variable
//! finds every process that belongs to it (only our own user's processes are readable, which is
//! exactly the set that matters).

use std::fs;
use std::path::{Path, PathBuf};

/// Set by the sandbox for the app inside it: the AppImage's path as the host sees it (inside,
/// `APPIMAGE` names the in-sandbox path) and the unpacked tree the app runs from.
pub const HOST_APPIMAGE_VAR: &str = "APPOFHOLDING_APPIMAGE";
pub const TREE_VAR: &str = "APPOFHOLDING_TREE";

pub fn running_pids(appimage: &Path) -> Vec<u32> {
    let target = fs::canonicalize(appimage).unwrap_or_else(|_| appimage.to_path_buf());
    let same = |v: &Path| v == target || fs::canonicalize(v).is_ok_and(|c| c == target);
    scan(|proc, env| {
        fs::read_link(proc.join("exe")).is_ok_and(|p| p == target)
            || env_value(env, "APPIMAGE").is_some_and(|v| same(&v))
            || env_value(env, HOST_APPIMAGE_VAR).is_some_and(|v| same(&v))
    })
}

/// Whether a sandboxed app is running from the unpacked tree `dir`.
pub fn tree_in_use(dir: &Path) -> bool {
    !scan(|_, env| env_value(env, TREE_VAR).is_some_and(|v| v == dir)).is_empty()
}

fn env_value(env: &[u8], key: &str) -> Option<PathBuf> {
    let prefix = format!("{key}=");
    env.split(|b| *b == 0).find_map(|kv| {
        kv.strip_prefix(prefix.as_bytes())
            .map(|v| PathBuf::from(String::from_utf8_lossy(v).into_owned()))
    })
}

/// Our own user's processes (other than this one) for which `matches(/proc/<pid>, environ)`.
fn scan(matches: impl Fn(&Path, &[u8]) -> bool) -> Vec<u32> {
    let me = std::process::id();
    let Ok(dir) = fs::read_dir("/proc") else {
        return vec![];
    };
    let mut pids = Vec::new();
    for entry in dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == me {
            continue;
        }
        let proc = entry.path();
        let env = fs::read(proc.join("environ")).unwrap_or_default();
        if matches(&proc, &env) {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn finds_processes_that_carry_the_appimage_variable() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("Foo.AppImage");
        fs::write(&fake, b"x").unwrap();
        assert!(running_pids(&fake).is_empty());

        let mut child = Command::new("sleep")
            .arg("30")
            .env("APPIMAGE", &fake)
            .spawn()
            .unwrap();
        let found = running_pids(&fake);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found, vec![child.id()]);
        assert!(running_pids(&fake).is_empty());
    }

    #[test]
    fn sandboxed_instances_are_found_by_their_host_path_and_tree() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("Foo.AppImage");
        fs::write(&fake, b"x").unwrap();
        let tree = dir.path().join("tree");
        let mut child = Command::new("sleep")
            .arg("30")
            .env("APPIMAGE", "/run/appofholding/Foo.AppImage")
            .env(HOST_APPIMAGE_VAR, &fake)
            .env(TREE_VAR, &tree)
            .spawn()
            .unwrap();
        let found = running_pids(&fake);
        let in_use = tree_in_use(&tree);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found, vec![child.id()]);
        assert!(in_use);
        assert!(!tree_in_use(&tree));
    }

    #[test]
    fn unrelated_files_have_no_processes() {
        assert!(running_pids(Path::new("/nonexistent/Nope.AppImage")).is_empty());
    }
}
