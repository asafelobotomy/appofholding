//! Finding running instances of an AppImage.
//!
//! An AppImage's runtime stays alive while the app runs and exports `APPIMAGE=<path>` to the
//! app and its children, so scanning `/proc` for either the executable path or that variable
//! finds every process that belongs to it (only our own user's processes are readable, which is
//! exactly the set that matters).

use std::fs;
use std::path::Path;

pub fn running_pids(appimage: &Path) -> Vec<u32> {
    let target = fs::canonicalize(appimage).unwrap_or_else(|_| appimage.to_path_buf());
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
        let by_exe = fs::read_link(proc.join("exe")).is_ok_and(|p| p == target);
        let by_env = !by_exe
            && fs::read(proc.join("environ")).is_ok_and(|env| {
                env.split(|b| *b == 0).any(|kv| {
                    kv.strip_prefix(b"APPIMAGE=").is_some_and(|v| {
                        let v = Path::new(std::ffi::OsStr::new(
                            &String::from_utf8_lossy(v).into_owned(),
                        ))
                        .to_path_buf();
                        v == target || fs::canonicalize(&v).is_ok_and(|c| c == target)
                    })
                })
            });
        if by_exe || by_env {
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
    fn unrelated_files_have_no_processes() {
        assert!(running_pids(Path::new("/nonexistent/Nope.AppImage")).is_empty());
    }
}
