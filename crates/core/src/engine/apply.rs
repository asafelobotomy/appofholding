//! Swapping a new AppImage in, locking an app, pruning and rolling back versions.

use super::*;

impl Library {
    /// Put `new_file` in place of the app's current AppImage. The current file is first kept as
    /// a version for rollback. On success the database and menu entry describe the new file.
    pub(crate) fn swap_in(
        &self,
        app: &AppRecord,
        current_version: Option<&str>,
        new_file: &Path,
        new_name: &str,
        version_hint: Option<&str>,
        keep: usize,
    ) -> Result<AppRecord> {
        let ins = inspect(new_file)?;
        let cur = &app.appimage_path;
        let dir = cur
            .parent()
            .ok_or_else(|| Error::Other("app path has no parent".into()))?;
        let file_name = Path::new(new_name)
            .file_name()
            .ok_or_else(|| Error::Source(format!("bad file name '{new_name}'")))?;
        // Never rename over a file that is not this app's (another app's AppImage with the same
        // asset name, or anything else the user keeps there): fall back to an id-prefixed name.
        let mut dest = dir.join(file_name);
        if &dest != cur && dest.exists() {
            dest = dir.join(format!("{}-{}", app.id, file_name.to_string_lossy()));
            if &dest != cur && dest.exists() {
                return Err(Error::Other(format!(
                    "cannot install {}: {} already exists and belongs to something else",
                    file_name.to_string_lossy(),
                    dest.display()
                )));
            }
        }

        // 1. Keep the current file (hard link where possible, so no extra disk space until
        //    the original is replaced). The backup's name includes the build's hash, so one
        //    left behind by an interrupted update is the same file and is reused, never deleted.
        let known = self.db.list_versions(&app.id)?;
        let have_backup = known
            .iter()
            .any(|v| v.sha256 == app.sha256 && v.path.exists());
        let vdir = self.dirs.versions_dir(&app.id);
        let backup = vdir.join(format!(
            "{}-{}.AppImage",
            slugify(current_version.unwrap_or("unknown")),
            &app.sha256[..app.sha256.len().min(8)]
        ));
        let mut made_backup = false;
        if !have_backup && !backup.exists() {
            fs::create_dir_all(&vdir).map_err(Error::io(&vdir))?;
            if fs::hard_link(cur, &backup).is_err() {
                fs::copy(cur, &backup).map_err(Error::io(&backup))?;
            }
            made_backup = true;
        }
        let undo_backup = || {
            if made_backup {
                let _ = fs::remove_file(&backup);
            }
        };

        // 2. Stage the new file beside the destination (same filesystem => atomic rename).
        let stage = dir.join(format!(".{}.appofholding-new", file_name.to_string_lossy()));
        let staged = (|| -> Result<(String, u64)> {
            if fs::rename(new_file, &stage).is_err() {
                fs::copy(new_file, &stage).map_err(Error::io(&stage))?;
                let _ = fs::remove_file(new_file);
            }
            let mut perms = fs::metadata(&stage)
                .map_err(Error::io(&stage))?
                .permissions();
            perms.set_mode(perms.mode() | 0o755);
            fs::set_permissions(&stage, perms).map_err(Error::io(&stage))?;
            image::hash_file(&stage)
        })();
        let (sha256, size) = match staged {
            Ok(h) => h,
            Err(e) => {
                let _ = fs::remove_file(&stage);
                undo_backup();
                return Err(e);
            }
        };

        // 3. Swap. Renaming over an existing file is atomic: the path is never missing. The old
        //    file (when the name changes) is only removed once everything else has succeeded.
        if let Err(e) = fs::rename(&stage, &dest) {
            let _ = fs::remove_file(&stage);
            undo_backup();
            return Err(Error::io(&dest)(e));
        }

        // 4. Refresh generated integration and the database.
        let finish = || -> Result<AppRecord> {
            let (desktop_file, icon_file) = self.write_integration(&ins, &app.id)?;
            let rec = AppRecord {
                id: app.id.clone(),
                name: ins.name.clone(),
                appimage_path: dest.clone(),
                desktop_file,
                icon_file,
                // Not every AppImage says its version; fall back to what the source told us.
                version: ins.version.clone().or(version_hint.map(str::to_string)),
                sha256: sha256.clone(),
                size,
                appimage_type: ins.image.kind,
                update_info: ins.image.update_info.clone(),
                added_at: app.added_at,
                external_file: app.external_file,
            };
            self.db.update_app(&rec)?;
            if !have_backup {
                let orig = cur
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.db.add_version(
                    &app.id,
                    current_version,
                    &app.sha256,
                    &orig,
                    &backup,
                    now(),
                )?;
            }
            Ok(rec)
        };
        let rec = match finish() {
            Ok(rec) => rec,
            Err(e) => {
                // Put the old version back exactly as it was.
                if &dest != cur {
                    let _ = fs::remove_file(&dest);
                } else {
                    let back = dir.join(format!(
                        ".{}.appofholding-restore",
                        file_name.to_string_lossy()
                    ));
                    let _ = fs::remove_file(&back);
                    if fs::hard_link(&backup, &back).is_ok() || fs::copy(&backup, &back).is_ok() {
                        let _ = fs::rename(&back, cur);
                    }
                }
                if let Ok(old) = inspect(cur) {
                    let _ = self.write_integration(&old, &app.id);
                }
                let _ = self.db.update_app(app);
                if &dest == cur {
                    // The backup is the only other copy until the database knows about it.
                } else {
                    undo_backup();
                }
                return Err(e);
            }
        };
        if &dest != cur {
            let _ = fs::remove_file(cur);
        }
        if let (Some(old), new) = (&app.icon_file, &rec.icon_file)
            && Some(old) != new.as_ref()
        {
            let _ = fs::remove_file(old);
        }
        // Old versions are housekeeping: failing to delete one must not fail the update.
        if let Err(e) = self.prune_versions(&app.id, keep) {
            self.warn(format!("{}: could not remove an old version: {e}", app.id));
        }
        Ok(rec)
    }

    /// Hold this while changing an app's files, so the GUI, the command line and the
    /// background timer never update (or roll back) the same app at the same time. The lock is
    /// released when the returned file is dropped, including when the process dies.
    pub(crate) fn lock_app(&self, id: &str) -> Result<fs::File> {
        let dir = self.dirs.app_data_dir().join("locks");
        fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
        let path = dir.join(format!("{id}.lock"));
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(Error::io(&path))?;
        // A lock just released can look taken for a moment: a process started by another
        // thread holds a copy of the descriptor between fork and exec. Only a lock that stays
        // taken means another update is really running.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match f.try_lock() {
                Ok(()) => return Ok(f),
                Err(fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(Error::Other(format!(
                        "'{id}' is already being updated by another appOFholding window or task; try again when it has finished"
                    )));
                }
                Err(fs::TryLockError::Error(e)) => return Err(Error::io(&path)(e)),
            }
        }
    }

    pub(crate) fn prune_versions(&self, id: &str, keep: usize) -> Result<()> {
        for v in self.db.list_versions(id)?.into_iter().skip(keep) {
            match fs::remove_file(&v.path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::io(&v.path)(e)),
            }
            self.db.delete_version(v.id)?;
        }
        Ok(())
    }

    /// Switch back to a kept version (`to` matches its version string, a SHA-256 prefix, or
    /// defaults to the newest kept). The version you leave is kept too, so this is reversible,
    /// and the version you rolled back from is marked skipped so auto-update doesn't undo it.
    pub fn rollback(&self, id: &str, to: Option<&str>) -> Result<AppRecord> {
        let app = self.get(id)?;
        let _lock = self.lock_app(id)?;
        let versions = self.db.list_versions(id)?;
        if versions.is_empty() {
            return Err(Error::Source(format!(
                "no previous version of '{id}' is kept"
            )));
        }
        let target = match to {
            None => &versions[0],
            Some(t) => versions
                .iter()
                .find(|v| v.version.as_deref() == Some(t) || v.sha256.starts_with(t))
                .ok_or_else(|| {
                    Error::Source(format!(
                        "no kept version matches '{t}' (kept: {})",
                        versions
                            .iter()
                            .map(|v| v
                                .version
                                .clone()
                                .unwrap_or_else(|| v.sha256[..8].to_string()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                })?,
        };
        let src = self.db.get_source(id)?;
        let current_version = src
            .as_ref()
            .and_then(|s| s.installed_version.clone())
            .or(app.version.clone());
        // The kept file moves into place, so nothing is pruned during a rollback.
        let rec = self.swap_in(
            &app,
            current_version.as_deref(),
            &target.path,
            &target.file_name,
            target.version.as_deref(),
            usize::MAX,
        )?;
        self.db.delete_version(target.id)?;
        if let Some(src) = src {
            self.db
                .record_installed(id, target.version.as_deref(), Some(&target.sha256))?;
            if let Some(latest) = src
                .last_latest
                .filter(|l| Some(l.as_str()) != target.version.as_deref())
            {
                self.db.set_skipped(id, Some(&latest))?;
            }
        }
        Ok(rec)
    }
}
