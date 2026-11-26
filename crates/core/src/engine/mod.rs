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
use crate::providers::{self, Fetched, Release};
use crate::running;
use crate::source::{Policy, Provider, Source};
use crate::updates::now;
use crate::version::{Installed, Status};
mod apply;
mod local;
mod retarget;
mod schedule;
mod update;
pub use local::{LocalVersion, Relation};

/// Per-app setting holding "<sha256>:<sha1>" of the installed file (see `installed_sha1`).
const SHA1_CACHE: &str = "sha1";

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

/// Whether a check result counts as an update the user can act on: the status is
/// `update-available`, the app is not pinned, and `latest` is not the version the user chose
/// to skip. `status` and `latest` are as stored (`SourceRecord::last_status` /
/// `last_latest`, or `Status::as_str()` and `CheckState::latest` of a fresh check); `src` is the
/// app's source record, if it has one.
pub fn counts_as_update(
    status: Option<&str>,
    latest: Option<&str>,
    src: Option<&SourceRecord>,
) -> bool {
    status == Some(Status::UpdateAvailable.as_str())
        && src.is_none_or(|s| {
            s.policy != Policy::Pinned
                && (latest.is_none() || s.skipped_version.as_deref() != latest)
        })
}

/// Which part of an update went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Asking the source what is new.
    Check,
    /// Downloading, verifying or installing it.
    Update,
}

impl Library {
    /// What is installed, in the terms the source's identifiers use.
    fn installed_of(
        &self,
        app: &AppRecord,
        rec: &SourceRecord,
    ) -> (Option<String>, Option<String>) {
        self.installed_for(
            app,
            &rec.source,
            rec.installed_version.clone(),
            rec.installed_digest.clone(),
        )
    }

    fn installed_for(
        &self,
        app: &AppRecord,
        source: &Source,
        installed_version: Option<String>,
        installed_digest: Option<String>,
    ) -> (Option<String>, Option<String>) {
        let mut version = installed_version.or(app.version.clone());
        if matches!(source.provider, Provider::Zsync { .. })
            && !version.as_deref().is_some_and(|v| v.starts_with("sha1:"))
        {
            // zsync control files identify builds by SHA-1, so hash what is installed and
            // compare like with like (this also makes adopted apps comparable).
            version = self.installed_sha1(app).map(|h| format!("sha1:{h}"));
        }
        (version, installed_digest)
    }

    /// The SHA-1 of the installed AppImage. Reading a whole AppImage takes a while, so it is
    /// remembered with the file's SHA-256 and computed again only when the file changed.
    fn installed_sha1(&self, app: &AppRecord) -> Option<String> {
        let cached = self.db.get_setting(&app.id, SHA1_CACHE).ok().flatten();
        if let Some(h) = cached
            .as_deref()
            .and_then(|v| v.strip_prefix(&app.sha256))
            .and_then(|v| v.strip_prefix(':'))
        {
            return Some(h.to_string());
        }
        let h = image::sha1_file(&app.appimage_path).ok()?;
        let _ = self
            .db
            .set_setting(&app.id, SHA1_CACHE, Some(&format!("{}:{h}", app.sha256)));
        Some(h)
    }

    /// Decide what a fetched release means for the installed app. Besides
    /// [`check::evaluate`], an app with no recorded digest whose installed file is byte for
    /// byte the release's file is up to date whatever the versions say; the second value says
    /// that rule applied.
    fn judge(
        &self,
        app: &AppRecord,
        source: &Source,
        version: Option<&str>,
        digest: Option<&str>,
        release: Release,
    ) -> (Report, bool) {
        let installed = Installed {
            version,
            sha256: digest,
        };
        let mut report = check::evaluate(release, source, &installed, Arch::host());
        let same_file = digest.is_none()
            && report
                .asset()
                .and_then(|a| a.sha256.as_deref())
                .is_some_and(|h| h.eq_ignore_ascii_case(&app.sha256));
        if same_file {
            report.status = Status::UpToDate;
        }
        (report, same_file)
    }

    /// Test `source` for the installed app `id` without saving anything: the same answer
    /// [`Library::check`] would give if `source` were the app's saved source (SHA-1 comparison
    /// for zsync sources, an identical file counting as up to date), but nothing is recorded.
    /// What is known about the installed version is reused only when `source` points at the same
    /// place as the saved source (e.g. only its asset pattern or channel differ).
    pub fn test_source(&self, http: &dyn Http, id: &str, source: &Source) -> Result<Report> {
        let app = self.get(id)?;
        let (known_version, known_digest) = match self.db.get_source(id)? {
            Some(rec) if rec.source.provider == source.provider => {
                (rec.installed_version, rec.installed_digest)
            }
            _ => (None, None),
        };
        let (version, digest) = self.installed_for(&app, source, known_version, known_digest);
        let release = providers::fetch_release(http, source)?;
        Ok(self
            .judge(&app, source, version.as_deref(), digest.as_deref(), release)
            .0)
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
                // An app that came in from elsewhere (imported, or added before it had a
                // source) has no recorded digest. If the release's file is byte for byte the
                // installed one, it is up to date whatever the versions say; remember it, so
                // later checks compare against this release.
                let (report, same_file) = self.judge(
                    &app,
                    &rec.source,
                    version.as_deref(),
                    digest.as_deref(),
                    release,
                );
                if same_file {
                    self.db.record_installed(
                        id,
                        Some(report.latest_version()),
                        Some(&app.sha256),
                    )?;
                }
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
}

#[cfg(test)]
mod tests;
