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
mod apply;
mod schedule;

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

/// A new version the user has not been told about (see `take_unannounced_updates`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUpdate {
    pub id: String,
    pub name: String,
    pub version: String,
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
}

#[cfg(test)]
mod tests;
