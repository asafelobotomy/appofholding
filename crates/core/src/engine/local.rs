//! Updating an app from an AppImage that is already on this computer (a download the user
//! made, a file dropped on the window, one an import scan found) rather than from its source.

use super::*;
use crate::library::Install;
use std::cmp::Ordering;

/// How a file relates to an app already in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// A newer version: it can update the app.
    Newer,
    /// The very same version (or the very same file).
    Same,
    /// An older version.
    Older,
    /// A different build whose version cannot be compared with the installed one.
    Unknown,
}

impl Relation {
    pub fn as_str(self) -> &'static str {
        match self {
            Relation::Newer => "newer",
            Relation::Same => "same",
            Relation::Older => "older",
            Relation::Unknown => "unknown",
        }
    }
}

/// A file that is another version of an installed app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVersion {
    pub id: String,
    pub name: String,
    /// The installed version, as the AppImage itself names it.
    pub installed: Option<String>,
    /// The file's version.
    pub version: Option<String>,
    pub relation: Relation,
}

impl Library {
    /// Whether `path` is another version of an app in the library (same app id, or the same
    /// name), and how its version compares. `None` when it is no app we have, or when it is
    /// the app's own file.
    pub fn local_version_of(&self, path: &Path) -> Result<Option<LocalVersion>> {
        let ins = inspect(path)?;
        let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let apps = self.list()?;
        let Some(app) = apps
            .iter()
            .find(|a| a.id == ins.id)
            .or_else(|| apps.iter().find(|a| a.name == ins.name))
        else {
            return Ok(None);
        };
        if fs::canonicalize(&app.appimage_path).is_ok_and(|p| p == real) {
            return Ok(None);
        }
        let relation = match (ins.version.as_deref(), app.version.as_deref()) {
            (Some(new), Some(old)) => match crate::version::compare(new, old) {
                Some(Ordering::Greater) => Relation::Newer,
                Some(Ordering::Less) => Relation::Older,
                Some(Ordering::Equal) => Relation::Same,
                None if new == old => Relation::Same,
                None => Relation::Unknown,
            },
            _ => Relation::Unknown,
        };
        // A byte-for-byte copy is the same whatever the versions say. Hashing is only worth it
        // when the sizes match.
        let relation = if relation != Relation::Same
            && fs::metadata(&real).is_ok_and(|m| m.len() == app.size)
            && image::hash_file(&real).is_ok_and(|(h, _)| h == app.sha256)
        {
            Relation::Same
        } else {
            relation
        };
        Ok(Some(LocalVersion {
            id: app.id.clone(),
            name: app.name.clone(),
            installed: app.version.clone(),
            version: ins.version,
            relation,
        }))
    }

    /// Replace app `id` with the AppImage at `path`, like an update: the current version is
    /// kept for rollback, the menu entry is rewritten, and the update hooks run. `Move` takes
    /// the file (it no longer exists where it was); `Copy` leaves it. The file must be the
    /// same app (by id or name), but may be any version: going back is the user's choice.
    pub fn update_from_file(&self, id: &str, path: &Path, mode: Install) -> Result<AppRecord> {
        let found = self.local_version_of(path)?;
        if found.as_ref().map(|f| f.id.as_str()) != Some(id) {
            return Err(Error::Other(format!(
                "{} is not a version of '{id}'",
                path.display()
            )));
        }
        let ins = inspect(path)?;
        let (sha256, _) = image::hash_file(path)?;
        let verdict = self.scan_before_install(path, &sha256)?;
        let _lock = self.lock_app(id)?;
        let app = self.get(id)?;
        let rec = self.db.get_source(id)?;

        // Copying goes through a private file first, so the swap is always a rename and the
        // user's file is never touched.
        let staged;
        let new_file = match mode {
            Install::Move => path.to_path_buf(),
            Install::Copy | Install::InPlace => {
                let dir = self.dirs.app_data_dir().join("downloads").join(id);
                fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
                staged = dir.join(path.file_name().unwrap_or_default());
                fs::copy(path, &staged).map_err(Error::io(&staged))?;
                staged.clone()
            }
        };
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{id}.AppImage"));

        let hooks = self.db.list_hooks(id)?;
        let hook = |kind: &str| {
            hooks
                .iter()
                .find(|(k, _)| k == kind)
                .map(|(_, c)| c.clone())
        };
        if let Some(cmd) = hook("pre-update")
            && let Err(e) = exec_hook(&app, "pre-update", &cmd)
        {
            if new_file != path {
                let _ = fs::remove_file(&new_file);
            }
            return Err(Error::Other(format!(
                "{e}; update cancelled, your installed version is untouched"
            )));
        }

        let current = rec
            .as_ref()
            .and_then(|r| r.installed_version.clone())
            .or(app.version.clone());
        let updated = self.swap_in(
            &app,
            current.as_deref(),
            &new_file,
            &file_name,
            ins.version.as_deref(),
            self.keep_versions()?,
            None,
        )?;

        if let Some(v) = &verdict {
            let _ = self.record_verdict(id, v);
        }
        if let Some(src) = rec {
            // What is installed now is what the file says; the next check compares against it.
            self.db
                .record_installed(id, ins.version.as_deref(), Some(&updated.sha256))?;
            let caught_up = match (ins.version.as_deref(), src.last_latest.as_deref()) {
                (Some(v), Some(latest)) => {
                    crate::version::compare(v, latest).is_some_and(|o| o != Ordering::Less)
                }
                _ => false,
            };
            if caught_up {
                self.db
                    .record_check(id, now(), Some(Status::UpToDate.as_str()), None, None)?;
            }
            self.db.set_notified(id, None)?;
            self.db.set_etag(id, None, None)?;
        }
        let _ = self.gc_extracted();
        if let Some(cmd) = hook("post-update")
            && let Err(e) = exec_hook(&updated, "post-update", &cmd)
        {
            self.warn(format!("{id}: {e}"));
        }
        Ok(updated)
    }
}
