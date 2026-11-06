//! The update engine: checking (with ETag caching), applying updates safely, rollback, update
//! policies and the scheduled background run.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::assets::Arch;
use crate::check::{self, Report};
use crate::db::{AppRecord, SourceRecord, VersionRecord};
use crate::delta;
use crate::download;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::image;
use crate::library::{Library, inspect};
use crate::notify::Notifier;
use crate::paths::slugify;
use crate::profiles::exec_hook;
use crate::providers::{self, Fetched};
use crate::running;
use crate::source::{Policy, Provider};
use crate::updates::now;
use crate::version::{Installed, Status};

/// How many previous versions to keep for rollback.
pub const DEFAULT_KEEP_VERSIONS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfRunning {
    /// Replace the file on disk anyway. The running instance keeps its old copy mapped and
    /// carries on; the new version starts the next time the app is launched.
    Proceed,
    /// Leave everything alone and try again later (used by the background updater).
    Defer,
}

#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub if_running: IfRunning,
    pub keep_versions: usize,
    /// Ignore pinned/skipped state and apply even if the status is only "unknown".
    pub force: bool,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            if_running: IfRunning::Proceed,
            keep_versions: DEFAULT_KEEP_VERSIONS,
            force: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// Nothing to do (up to date, or the status could not be determined).
    NoUpdate(Status),
    /// Policy forbids updating (pinned, or this version was skipped).
    Skipped(String),
    /// The app is running and the caller asked to wait.
    Deferred { version: String, pids: Vec<u32> },
    Updated {
        from: Option<String>,
        to: String,
        delta: bool,
    },
}

/// Result of a check, possibly answered from the ETag cache.
#[derive(Debug, Clone)]
pub struct CheckState {
    pub status: Status,
    pub latest: Option<String>,
    /// `None` when the server said "not modified" and we reused the last result.
    pub report: Option<Report>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Updated {
        id: String,
        from: Option<String>,
        to: String,
    },
    /// A new version exists; `notified` says whether we told the user this time.
    Available {
        id: String,
        version: String,
        notified: bool,
    },
    Deferred {
        id: String,
        version: String,
    },
    UpToDate {
        id: String,
    },
    Skipped {
        id: String,
        reason: String,
    },
    Failed {
        id: String,
        error: String,
    },
}

impl Library {
    /// What is installed, in the terms the source's identifiers use.
    fn installed_of(
        &self,
        app: &AppRecord,
        rec: &SourceRecord,
    ) -> (Option<String>, Option<String>) {
        let mut version = rec.installed_version.clone().or(app.version.clone());
        if matches!(rec.source.provider, Provider::Zsync { .. })
            && !version.as_deref().is_some_and(|v| v.starts_with("sha1:"))
        {
            // zsync control files identify builds by SHA-1, so hash what is installed and
            // compare like with like (this also makes adopted apps comparable).
            version = image::sha1_file(&app.appimage_path)
                .ok()
                .map(|h| format!("sha1:{h}"));
        }
        (version, rec.installed_digest.clone())
    }

    pub(crate) fn check_impl(
        &self,
        http: &dyn Http,
        id: &str,
        use_cache: bool,
    ) -> Result<CheckState> {
        let app = self.get(id)?;
        let rec = self.db.get_source(id)?.ok_or_else(|| {
            Error::Source(format!(
                "'{id}' has no update source; set one with `appofholding source set`"
            ))
        })?;
        let (version, digest) = self.installed_of(&app, &rec);
        let installed = Installed {
            version: version.as_deref(),
            sha256: digest.as_deref(),
        };
        // The cached ETag is only valid for exactly this question.
        let ctx = format!(
            "{}|{:?}|{:?}|{:?}",
            rec.source.describe(),
            rec.source.asset_pattern,
            version,
            digest
        );
        let etag = if use_cache
            && rec.etag_ctx.as_deref() == Some(ctx.as_str())
            && rec.last_error.is_none()
        {
            rec.last_status.as_ref().and(rec.etag.clone())
        } else {
            None
        };
        match providers::fetch_release_cond(http, &rec.source, etag.as_deref()) {
            Ok(Fetched::NotModified) => {
                self.db
                    .record_check(id, now(), rec.last_status.as_deref(), None, None)?;
                Ok(CheckState {
                    status: rec
                        .last_status
                        .as_deref()
                        .and_then(Status::parse)
                        .unwrap_or(Status::Unknown),
                    latest: rec.last_latest.clone(),
                    report: None,
                })
            }
            Ok(Fetched::Fresh { release, etag }) => {
                let report = check::evaluate(release, &rec.source, &installed, Arch::host());
                self.db.record_check(
                    id,
                    now(),
                    Some(report.status.as_str()),
                    report.problem().as_deref(),
                    Some(report.latest_version()),
                )?;
                self.db.set_etag(id, etag.as_deref(), Some(&ctx))?;
                Ok(CheckState {
                    status: report.status,
                    latest: Some(report.latest_version().to_string()),
                    report: Some(report),
                })
            }
            Err(e) => {
                self.db
                    .record_check(id, now(), None, Some(&e.to_string()), None)?;
                Err(e)
            }
        }
    }

    /// Like [`Library::check`], but sends the stored ETag so an unchanged GitHub release costs
    /// nothing against the API rate limit. Used by the scheduler.
    pub fn check_cached(&self, http: &dyn Http, id: &str) -> Result<CheckState> {
        self.check_impl(http, id, true)
    }

    pub fn set_policy(&self, id: &str, policy: Policy) -> Result<()> {
        self.source_or_err(id)?;
        self.db.set_policy(id, policy)
    }

    /// Ignore the currently known latest version ("skip this version"). Returns it.
    pub fn skip_latest(&self, id: &str) -> Result<String> {
        let rec = self.source_or_err(id)?;
        let latest = rec.last_latest.ok_or_else(|| {
            Error::Source("no version known yet; run `appofholding check` first".into())
        })?;
        self.db.set_skipped(id, Some(&latest))?;
        Ok(latest)
    }

    pub fn clear_skip(&self, id: &str) -> Result<()> {
        self.source_or_err(id)?;
        self.db.set_skipped(id, None)
    }

    fn source_or_err(&self, id: &str) -> Result<SourceRecord> {
        self.get(id)?;
        self.db
            .get_source(id)?
            .ok_or_else(|| Error::Source(format!("'{id}' has no update source")))
    }

    /// Previous versions kept for rollback, newest first.
    pub fn versions(&self, id: &str) -> Result<Vec<VersionRecord>> {
        self.get(id)?;
        self.db.list_versions(id)
    }

    /// Check, download, verify and install the newest version.
    ///
    /// The old file is kept (up to `keep_versions`) so the update can be rolled back, the
    /// replacement is staged next to the target and renamed into place, and a download that
    /// isn't a valid AppImage never replaces anything.
    pub fn update(
        &self,
        http: &dyn Http,
        id: &str,
        opts: &ApplyOptions,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<Applied> {
        let app = self.get(id)?;
        let rec = self.source_or_err(id)?;
        if rec.policy == Policy::Pinned && !opts.force {
            return Ok(Applied::Skipped("pinned to the installed version".into()));
        }
        let state = self.check_impl(http, id, false)?;
        let report = state
            .report
            .ok_or_else(|| Error::Other("internal: missing report".into()))?;
        if let Some(problem) = report.problem() {
            return Err(Error::Source(problem));
        }
        let usable = report.status == Status::UpdateAvailable
            || (opts.force && report.status == Status::Unknown);
        if !usable {
            return Ok(Applied::NoUpdate(report.status));
        }
        let tag = report.latest_version().to_string();
        if !opts.force && rec.skipped_version.as_deref() == Some(tag.as_str()) {
            return Ok(Applied::Skipped(format!("version {tag} was skipped")));
        }
        let _lock = self.lock_app(id)?;
        let pids = running::running_pids(&app.appimage_path);
        if opts.if_running == IfRunning::Defer && !pids.is_empty() {
            return Ok(Applied::Deferred { version: tag, pids });
        }

        let asset = report
            .asset()
            .cloned()
            .ok_or_else(|| Error::Source("no asset selected".into()))?;
        // Kept between attempts so an interrupted download resumes.
        let dir = self.dirs.app_data_dir().join("downloads").join(id);

        // Delta update if this is a zsync source and the tool is installed.
        let mut used_delta = false;
        let mut fetched: Option<(PathBuf, String)> = None;
        if let Provider::Zsync { url } = &rec.source.provider {
            fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
            let out = dir.join(Path::new(&asset.name).file_name().unwrap_or_default());
            if delta::try_delta(url, &app.appimage_path, &out) {
                let (sha, _) = image::hash_file(&out)?;
                fetched = Some((out, sha));
                used_delta = true;
            }
        }
        let (new_file, new_sha) = match fetched {
            Some(f) => f,
            None => download::download_asset(http, &asset, &dir, progress)?,
        };

        if let Err(e) = inspect(&new_file) {
            let _ = fs::remove_file(&new_file);
            return Err(Error::Source(format!(
                "the downloaded file for {tag} is not a usable AppImage ({e}); your installed version was not touched"
            )));
        }

        let hooks = self.db.list_hooks(id)?;
        let hook = |kind: &str| {
            hooks
                .iter()
                .find(|(k, _)| k == kind)
                .map(|(_, c)| c.clone())
        };
        if let Some(cmd) = hook("pre-update") {
            // A failing pre-update hook vetoes the update; nothing has been changed yet.
            exec_hook(&app, "pre-update", &cmd).map_err(|e| {
                Error::Other(format!(
                    "{e}; update cancelled, your installed version is untouched"
                ))
            })?;
        }

        let (installed_version, _) = self.installed_of(&app, &rec);
        self.swap_in(
            &app,
            installed_version.as_deref(),
            &new_file,
            &asset.name,
            Some(&tag),
            opts.keep_versions,
        )?;
        self.db.record_installed(id, Some(&tag), Some(&new_sha))?;
        self.db
            .record_check(id, now(), Some(Status::UpToDate.as_str()), None, Some(&tag))?;
        self.db.set_notified(id, None)?;
        self.db.set_etag(id, None, None)?;
        let _ = self.gc_extracted();
        let _ = fs::remove_dir_all(&dir);
        if let Some(cmd) = hook("post-update") {
            let now_installed = self.get(id)?;
            if let Err(e) = exec_hook(&now_installed, "post-update", &cmd) {
                // The update itself succeeded, so this is only reported.
                self.warn(format!("{id}: {e}"));
            }
        }
        Ok(Applied::Updated {
            from: installed_version.or(app.version),
            to: tag,
            delta: used_delta,
        })
    }

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

    /// The background run: respects each app's policy, never aborts on one app's failure, and
    /// tells the user about news once.
    pub fn run_scheduled(
        &self,
        http: &(dyn Http + Sync),
        notifier: &dyn Notifier,
    ) -> Result<Vec<Outcome>> {
        let mut out = Vec::new();
        let records = self.db.list_sources()?;
        // Apps that only need a look at the server are checked a few at a time; automatic
        // updates download and install, so those run one after another below.
        let ask: Vec<String> = records
            .iter()
            .filter(|r| !matches!(r.policy, Policy::Pinned | Policy::Auto))
            .map(|r| r.app_id.clone())
            .collect();
        let mut checked: std::collections::HashMap<String, Result<CheckState>> = ask
            .iter()
            .cloned()
            .zip(self.par_map(&ask, |lib, id| lib.check_impl(http, id, true)))
            .collect();
        for rec in records {
            let id = rec.app_id.clone();
            let name = self.get(&id).map(|a| a.name).unwrap_or_else(|_| id.clone());
            let pre = checked.remove(&id);
            out.push(match self.scheduled_one(http, notifier, &rec, &name, pre) {
                Ok(o) => o,
                Err(e) => {
                    // Say so once per distinct problem, not on every run, and never for apps the
                    // user asked to keep quiet about.
                    let new_problem = rec.last_error.as_deref() != Some(e.to_string().as_str());
                    if new_problem && rec.policy != Policy::Manual {
                        notifier.notify(&format!("{name}: update check failed"), &e.to_string());
                    }
                    Outcome::Failed {
                        id,
                        error: e.to_string(),
                    }
                }
            });
        }
        Ok(out)
    }

    fn scheduled_one(
        &self,
        http: &dyn Http,
        notifier: &dyn Notifier,
        rec: &SourceRecord,
        name: &str,
        pre: Option<Result<CheckState>>,
    ) -> Result<Outcome> {
        let id = rec.app_id.clone();
        if rec.policy == Policy::Pinned {
            return Ok(Outcome::Skipped {
                id,
                reason: "pinned".into(),
            });
        }
        if rec.policy == Policy::Auto {
            let opts = ApplyOptions {
                if_running: IfRunning::Defer,
                ..ApplyOptions::default()
            };
            return Ok(match self.update(http, &id, &opts, &mut |_, _| {})? {
                Applied::Updated { from, to, .. } => {
                    notifier.notify(
                        &format!("{name} updated"),
                        &format!("Now at {to}. The new version starts next time you open it."),
                    );
                    Outcome::Updated { id, from, to }
                }
                Applied::NoUpdate(_) => Outcome::UpToDate { id },
                Applied::Skipped(reason) => Outcome::Skipped { id, reason },
                Applied::Deferred { version, .. } => Outcome::Deferred { id, version },
            });
        }
        let state = match pre {
            Some(r) => r?,
            None => self.check_impl(http, &id, true)?,
        };
        if state.status != Status::UpdateAvailable {
            return Ok(Outcome::UpToDate { id });
        }
        let version = state.latest.unwrap_or_default();
        if rec.skipped_version.as_deref() == Some(version.as_str()) {
            return Ok(Outcome::Skipped {
                id,
                reason: format!("version {version} was skipped"),
            });
        }
        let mut notified = false;
        if rec.policy == Policy::Notify && rec.notified_version.as_deref() != Some(version.as_str())
        {
            notifier.notify(
                &format!("{name}: update available"),
                &format!(
                    "Version {version} is available. Run `appofholding update {id}` to install it."
                ),
            );
            self.db.set_notified(&id, Some(&version))?;
            notified = true;
        }
        Ok(Outcome::Available {
            id,
            version,
            notified,
        })
    }
}

#[cfg(test)]
mod tests {
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
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
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

    #[test]
    fn update_swaps_keeps_a_backup_and_refreshes_integration() {
        let e = env();
        let old = installed(&e);
        let http = github("2.0.0");
        let r = apply(&e, &http, &ApplyOptions::default()).unwrap();
        assert_eq!(
            r,
            Applied::Updated {
                from: Some("1.0.0".into()),
                to: "v2.0.0".into(),
                delta: false
            }
        );

        let app = e.lib.get("foo-app").unwrap();
        assert_eq!(app.version.as_deref(), Some("2.0.0"));
        assert!(app.appimage_path.ends_with("Foo-2.0.0-x86_64.AppImage"));
        assert!(app.appimage_path.exists());
        assert!(!old.appimage_path.exists(), "the old file name is gone");
        assert_ne!(app.sha256, old.sha256);

        let kept = e.lib.versions("foo-app").unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].version.as_deref(), Some("1.0.0"));
        assert!(kept[0].path.exists() && kept[0].sha256 == old.sha256);

        // The menu entry still goes through our launcher and points at nothing version-specific.
        let desktop = fs::read_to_string(&app.desktop_file).unwrap();
        assert!(desktop.contains(" run foo-app "), "{desktop}");
        let s = e.lib.source("foo-app").unwrap().unwrap();
        assert_eq!(
            (s.installed_version.as_deref(), s.last_status.as_deref()),
            (Some("v2.0.0"), Some("up-to-date"))
        );
        // Nothing left in the download cache.
        assert!(!e.lib.dirs.app_data_dir().join("downloads/foo-app").exists());

        // Asking again: nothing to do.
        assert_eq!(
            apply(&e, &http, &ApplyOptions::default()).unwrap(),
            Applied::NoUpdate(Status::UpToDate)
        );
    }

    #[test]
    fn an_update_never_overwrites_a_file_that_is_not_this_apps() {
        let e = env();
        let old = installed(&e);
        let dir = old.appimage_path.parent().unwrap();
        // Something else already uses the new version's file name (e.g. another app).
        let squatter = dir.join("Foo-2.0.0-x86_64.AppImage");
        fs::write(&squatter, b"someone else's file").unwrap();
        apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();
        assert_eq!(fs::read(&squatter).unwrap(), b"someone else's file");
        let app = e.lib.get("foo-app").unwrap();
        assert!(
            app.appimage_path
                .ends_with("foo-app-Foo-2.0.0-x86_64.AppImage"),
            "{}",
            app.appimage_path.display()
        );
        assert!(app.appimage_path.exists() && !old.appimage_path.exists());
    }

    #[test]
    fn two_updates_of_the_same_app_cannot_run_at_once() {
        let e = env();
        installed(&e);
        let held = e.lib.lock_app("foo-app").unwrap();
        let err = apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap_err();
        assert!(err.to_string().contains("already being updated"), "{err}");
        drop(held);
        assert!(apply(&e, &github("2.0.0"), &ApplyOptions::default()).is_ok());
    }

    #[test]
    fn rollback_restores_the_old_version_and_stops_auto_update_bouncing_back() {
        let e = env();
        let old = installed(&e);
        apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();

        let rec = e.lib.rollback("foo-app", None).unwrap();
        assert_eq!(rec.version.as_deref(), Some("1.0.0"));
        assert_eq!(rec.sha256, old.sha256);
        assert!(rec.appimage_path.exists());
        // 2.0.0 is now the kept one, so the rollback is itself reversible.
        let kept = e.lib.versions("foo-app").unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].version.as_deref(), Some("v2.0.0"));

        let s = e.lib.source("foo-app").unwrap().unwrap();
        assert_eq!(s.skipped_version.as_deref(), Some("v2.0.0"));
        // The scheduler won't reinstall the version we just left.
        e.lib.set_policy("foo-app", Policy::Auto).unwrap();
        let outcomes = e
            .lib
            .run_scheduled(&github("2.0.0"), &Recorder::default())
            .unwrap();
        assert!(
            matches!(outcomes[0], Outcome::Skipped { .. }),
            "{outcomes:?}"
        );
        assert_eq!(
            e.lib.get("foo-app").unwrap().version.as_deref(),
            Some("1.0.0")
        );
        // A forced update still works.
        assert!(matches!(
            apply(
                &e,
                &github("2.0.0"),
                &ApplyOptions {
                    force: true,
                    ..Default::default()
                }
            )
            .unwrap(),
            Applied::Updated { .. }
        ));
    }

    #[test]
    fn rollback_with_nothing_kept_or_unknown_target_is_a_clear_error() {
        let e = env();
        installed(&e);
        assert!(
            e.lib
                .rollback("foo-app", None)
                .unwrap_err()
                .to_string()
                .contains("no previous version")
        );
        apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();
        let err = e
            .lib
            .rollback("foo-app", Some("9.9.9"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("no kept version matches") && err.contains("1.0.0"),
            "{err}"
        );
        // Selecting by version string works.
        assert_eq!(
            e.lib
                .rollback("foo-app", Some("1.0.0"))
                .unwrap()
                .version
                .as_deref(),
            Some("1.0.0")
        );
    }

    #[test]
    fn running_app_is_deferred_by_the_background_updater_only() {
        let e = env();
        let old = installed(&e);
        let mut child = Command::new("sleep")
            .arg("30")
            .env("APPIMAGE", &old.appimage_path)
            .spawn()
            .unwrap();
        let http = github("2.0.0");

        let deferred = apply(
            &e,
            &http,
            &ApplyOptions {
                if_running: IfRunning::Defer,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            matches!(&deferred, Applied::Deferred { pids, .. } if *pids == vec![child.id()]),
            "{deferred:?}"
        );
        assert!(old.appimage_path.exists(), "nothing was touched");
        assert_eq!(
            e.lib.get("foo-app").unwrap().version.as_deref(),
            Some("1.0.0")
        );

        // An explicit update still goes ahead (the running copy keeps working from its mapping).
        assert!(matches!(
            apply(&e, &http, &ApplyOptions::default()).unwrap(),
            Applied::Updated { .. }
        ));
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn pinned_and_skipped_versions_are_respected() {
        let e = env();
        installed(&e);
        let http = github("2.0.0");
        e.lib.set_policy("foo-app", Policy::Pinned).unwrap();
        assert!(matches!(
            apply(&e, &http, &ApplyOptions::default()).unwrap(),
            Applied::Skipped(_)
        ));
        assert_eq!(
            e.lib.get("foo-app").unwrap().version.as_deref(),
            Some("1.0.0")
        );

        e.lib.set_policy("foo-app", Policy::Notify).unwrap();
        e.lib.check(&http, "foo-app").unwrap();
        assert_eq!(e.lib.skip_latest("foo-app").unwrap(), "v2.0.0");
        assert!(matches!(
            apply(&e, &http, &ApplyOptions::default()).unwrap(),
            Applied::Skipped(_)
        ));
        // A newer release is not covered by the skip.
        assert!(matches!(
            apply(&e, &github("3.0.0"), &ApplyOptions::default()).unwrap(),
            Applied::Updated { .. }
        ));
    }

    #[test]
    fn a_bad_download_never_replaces_the_installed_app() {
        let e = env();
        let old = installed(&e);
        // The server hands out an HTML error page with a valid-looking release.
        let junk = b"<html>rate limited</html>".to_vec();
        let rel = format!(
            r#"{{"tag_name":"v2.0.0","prerelease":false,"draft":false,"assets":[
              {{"name":"Foo-2.0.0-x86_64.AppImage","browser_download_url":"https://dl/x","size":{}}}]}}"#,
            junk.len()
        );
        let http = FakeHttp::default()
            .json(API, &rel)
            .bytes("https://dl/x", junk);
        let err = apply(&e, &http, &ApplyOptions::default())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("not a usable AppImage") && err.contains("not touched"),
            "{err}"
        );
        let app = e.lib.get("foo-app").unwrap();
        assert_eq!(app.sha256, old.sha256);
        assert!(app.appimage_path.exists());
        assert!(e.lib.versions("foo-app").unwrap().is_empty());
    }

    #[test]
    fn hooks_run_around_updates_and_a_failing_pre_update_vetoes_it() {
        let e = env();
        let old = installed(&e);
        let log = e.tmp.path().join("hooks.log");
        let append =
            |kind: &str| format!("echo {kind}:$APPOFHOLDING_VERSION >> '{}'", log.display());
        e.lib
            .set_hook("foo-app", "pre-update", &append("pre"))
            .unwrap();
        e.lib
            .set_hook("foo-app", "post-update", &append("post"))
            .unwrap();
        apply(&e, &github("2.0.0"), &ApplyOptions::default()).unwrap();
        // pre sees the old version, post the new one.
        assert_eq!(fs::read_to_string(&log).unwrap(), "pre:1.0.0\npost:2.0.0\n");
        assert!(e.lib.take_warnings().is_empty());

        // A failing pre-update hook cancels the next update before anything changes.
        e.lib.set_hook("foo-app", "pre-update", "exit 1").unwrap();
        let before = e.lib.get("foo-app").unwrap();
        let err = apply(&e, &github("3.0.0"), &ApplyOptions::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("update cancelled"), "{err}");
        let after = e.lib.get("foo-app").unwrap();
        assert_eq!(
            (before.sha256.as_str(), before.version.clone()),
            (after.sha256.as_str(), after.version.clone())
        );
        assert_ne!(after.sha256, old.sha256);

        // A failing post-update hook only produces a warning.
        e.lib.set_hook("foo-app", "pre-update", "true").unwrap();
        e.lib.set_hook("foo-app", "post-update", "exit 2").unwrap();
        assert!(matches!(
            apply(&e, &github("3.0.0"), &ApplyOptions::default()).unwrap(),
            Applied::Updated { .. }
        ));
        let warnings = e.lib.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("post-update hook failed"),
            "{warnings:?}"
        );
    }

    #[test]
    fn only_the_newest_versions_are_kept() {
        let e = env();
        installed(&e);
        let keep1 = ApplyOptions {
            keep_versions: 1,
            ..Default::default()
        };
        for v in ["2.0.0", "3.0.0", "4.0.0"] {
            apply(&e, &github(v), &keep1).unwrap();
        }
        let kept = e.lib.versions("foo-app").unwrap();
        assert_eq!(kept.len(), 1);
        assert!(
            kept[0].version.as_deref().unwrap().contains('3'),
            "{kept:?}"
        );
        let files = fs::read_dir(e.lib.dirs.versions_dir("foo-app"))
            .unwrap()
            .count();
        assert_eq!(files, 1, "pruned versions are deleted from disk");
    }

    #[test]
    fn scheduler_applies_auto_notifies_once_and_survives_failures() {
        let e = env();
        installed(&e);
        let notes = Recorder::default();

        // notify (default): told once per version.
        let http = github("2.0.0");
        let first = e.lib.run_scheduled(&http, &notes).unwrap();
        assert_eq!(
            first,
            vec![Outcome::Available {
                id: "foo-app".into(),
                version: "v2.0.0".into(),
                notified: true
            }]
        );
        let second = e.lib.run_scheduled(&http, &notes).unwrap();
        assert_eq!(
            second,
            vec![Outcome::Available {
                id: "foo-app".into(),
                version: "v2.0.0".into(),
                notified: false
            }]
        );
        assert_eq!(notes.0.borrow().len(), 1);
        assert_eq!(
            e.lib.get("foo-app").unwrap().version.as_deref(),
            Some("1.0.0"),
            "notify never installs"
        );

        // failures are reported per app, not fatal
        let failing = e.lib.run_scheduled(&FakeHttp::default(), &notes).unwrap();
        assert!(matches!(&failing[0], Outcome::Failed { .. }));
        assert_eq!(notes.0.borrow().len(), 2);
        // ...and the same problem again is not announced again.
        let again = e.lib.run_scheduled(&FakeHttp::default(), &notes).unwrap();
        assert!(matches!(&again[0], Outcome::Failed { .. }));
        assert_eq!(
            notes.0.borrow().len(),
            2,
            "one notification per distinct problem"
        );
        assert!(
            e.lib
                .source("foo-app")
                .unwrap()
                .unwrap()
                .last_error
                .is_some()
        );

        // auto installs and says so
        e.lib.set_policy("foo-app", Policy::Auto).unwrap();
        let auto = e.lib.run_scheduled(&http, &notes).unwrap();
        assert!(
            matches!(&auto[0], Outcome::Updated { to, .. } if to == "v2.0.0"),
            "{auto:?}"
        );
        assert_eq!(
            e.lib.get("foo-app").unwrap().version.as_deref(),
            Some("2.0.0")
        );
        assert!(notes.0.borrow().last().unwrap().0.contains("updated"));

        // pinned apps are left alone
        e.lib.set_policy("foo-app", Policy::Pinned).unwrap();
        let pinned = e.lib.run_scheduled(&github("3.0.0"), &notes).unwrap();
        assert!(matches!(pinned[0], Outcome::Skipped { .. }));
    }

    #[test]
    fn scheduler_defers_auto_update_of_a_running_app() {
        let e = env();
        let old = installed(&e);
        e.lib.set_policy("foo-app", Policy::Auto).unwrap();
        let mut child = Command::new("sleep")
            .arg("30")
            .env("APPIMAGE", &old.appimage_path)
            .spawn()
            .unwrap();
        let out = e
            .lib
            .run_scheduled(&github("2.0.0"), &Recorder::default())
            .unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(matches!(out[0], Outcome::Deferred { .. }), "{out:?}");
        assert_eq!(
            e.lib.get("foo-app").unwrap().version.as_deref(),
            Some("1.0.0")
        );
    }

    #[test]
    fn unchanged_releases_are_answered_from_the_etag_cache() {
        let e = env();
        installed(&e);
        let mut http = github("2.0.0");
        http.responses
            .get_mut(API)
            .unwrap()
            .headers
            .push(("etag".into(), "W/\"r1\"".into()));

        let first = e.lib.check_cached(&http, "foo-app").unwrap();
        assert!(first.report.is_some());
        assert_eq!(first.status, Status::UpdateAvailable);
        let second = e.lib.check_cached(&http, "foo-app").unwrap();
        assert!(
            second.report.is_none(),
            "304: no body parsed, previous result reused"
        );
        assert_eq!(
            (second.status, second.latest.as_deref()),
            (Status::UpdateAvailable, Some("v2.0.0"))
        );

        // Changing the source's question (here: the pattern) invalidates the cache.
        let mut src = e.lib.source("foo-app").unwrap().unwrap().source;
        src.asset_pattern = Some("Foo-*.AppImage".into());
        e.lib.set_source("foo-app", &src).unwrap();
        assert!(
            e.lib
                .check_cached(&http, "foo-app")
                .unwrap()
                .report
                .is_some()
        );

        // A forced full check never uses the cache.
        assert!(e.lib.check(&http, "foo-app").is_ok());
    }
}
