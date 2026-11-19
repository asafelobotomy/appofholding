//! Keeping other programs' launchers working when an app's file name changes on update.
//!
//! An AppImage that integrates itself (a URL handler, an autostart entry) writes its own path
//! into `~/.local/share/applications` or `~/.config/autostart`. An update that renames
//! `Foo-1.0.0.AppImage` to `Foo-2.0.0.AppImage` would leave those entries pointing at nothing.

use std::fs;
use std::path::Path;

use super::*;

/// Rewrite `old` to `new` in the `Exec=` and `TryExec=` lines of the given `.desktop` text.
/// `None` when no such line mentions `old`.
pub(crate) fn retarget_exec(text: &str, old: &str, new: &str) -> Option<String> {
    let mut changed = false;
    let out: Vec<String> = text
        .lines()
        .map(|line| {
            let is_exec = line.starts_with("Exec=") || line.starts_with("TryExec=");
            if is_exec && line.contains(old) {
                changed = true;
                line.replace(old, new)
            } else {
                line.to_string()
            }
        })
        .collect();
    changed.then(|| {
        let mut s = out.join("\n");
        if text.ends_with('\n') {
            s.push('\n');
        }
        s
    })
}

impl Library {
    /// Point menu entries, URL handlers and autostart entries that started `old` at `new`. The
    /// app's own generated entry is rewritten by the update itself, so it is skipped here.
    /// Best effort: a file that cannot be read or written is left alone.
    pub(crate) fn retarget_launchers(&self, app_id: &str, old: &Path, new: &Path) {
        let (Some(old_s), Some(new_s)) = (old.to_str(), new.to_str()) else {
            return;
        };
        if old_s == new_s {
            return;
        }
        let own = self.dirs.desktop_file(app_id);
        let autostart = self.dirs.config_home.join("autostart");
        for dir in [self.dirs.applications_dir(), autostart] {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path == own || path.extension().is_none_or(|e| e != "desktop") {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                if let Some(fixed) = retarget_exec(&text, old_s, new_s) {
                    let tmp = path.with_extension("desktop.appofholding-new");
                    if fs::write(&tmp, fixed).is_ok() && fs::rename(&tmp, &path).is_err() {
                        let _ = fs::remove_file(&tmp);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::retarget_exec;

    #[test]
    fn only_exec_lines_naming_the_old_path_change() {
        let text = "[Desktop Entry]\nName=/h/A/old.AppImage\nExec=\"/h/A/old.AppImage\" %U\nTryExec=/h/A/old.AppImage\nIcon=x\n";
        let fixed = retarget_exec(text, "/h/A/old.AppImage", "/h/A/new.AppImage").unwrap();
        assert_eq!(
            fixed,
            "[Desktop Entry]\nName=/h/A/old.AppImage\nExec=\"/h/A/new.AppImage\" %U\nTryExec=/h/A/new.AppImage\nIcon=x\n"
        );
        assert!(retarget_exec(text, "/h/A/other", "/x").is_none());
    }
}
