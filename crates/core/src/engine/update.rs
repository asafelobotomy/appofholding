//! Applying an update: download or stage the new version, swap it in, keep the old one.

use super::*;

impl Library {
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
        self.update_staged(http, id, opts, progress)
            .map_err(|(_, e)| e)
    }

    /// [`Library::update`], also saying which stage failed. A failure after a successful check
    /// is stored as the source's last error (the check itself clears it), so the background
    /// run can tell a persistent update failure from a new one.
    pub(crate) fn update_staged(
        &self,
        http: &dyn Http,
        id: &str,
        opts: &ApplyOptions,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> std::result::Result<Applied, (Stage, Error)> {
        // A known app, so `id` is safe to name a lock file after. Everything used below is read
        // under the lock, so an update that finishes meanwhile can't leave this one working
        // from the record it replaced.
        self.get(id).map_err(|e| (Stage::Check, e))?;
        let lock = self.lock_app(id).map_err(|e| (Stage::Update, e))?;
        let (status, applied) = self.update_locked(http, id, opts, progress, lock)?;
        match applied {
            Ok(a) => Ok(a),
            Err(e) => {
                let _ = self.db.record_check(
                    id,
                    now(),
                    Some(status.as_str()),
                    Some(&e.to_string()),
                    None,
                );
                Err((Stage::Update, e))
            }
        }
    }

    /// The outer error is a failed check; the inner result is the update itself, with the
    /// status the check found.
    fn update_locked(
        &self,
        http: &dyn Http,
        id: &str,
        opts: &ApplyOptions,
        progress: &mut dyn FnMut(u64, Option<u64>),
        _lock: fs::File,
    ) -> std::result::Result<(Status, Result<Applied>), (Stage, Error)> {
        let check_err = |e| (Stage::Check, e);
        let app = self.get(id).map_err(check_err)?;
        let rec = self.source_or_err(id).map_err(check_err)?;
        if rec.policy == Policy::Pinned && !opts.force {
            return Ok((
                Status::Unknown,
                Ok(Applied::Skipped("pinned to the installed version".into())),
            ));
        }
        let state = self.check_impl(http, id, false).map_err(check_err)?;
        let report = state
            .report
            .ok_or_else(|| check_err(Error::Other("internal: missing report".into())))?;
        if let Some(problem) = report.problem() {
            return Err(check_err(Error::Source(problem)));
        }
        let usable = report.status == Status::UpdateAvailable
            || (opts.force && report.status == Status::Unknown);
        if !usable {
            return Ok((report.status, Ok(Applied::NoUpdate(report.status))));
        }
        let tag = report.latest_version().to_string();
        if !opts.force && rec.skipped_version.as_deref() == Some(tag.as_str()) {
            return Ok((
                report.status,
                Ok(Applied::Skipped(format!("version {tag} was skipped"))),
            ));
        }
        let status = report.status;
        Ok((
            status,
            self.apply_report(http, &app, &rec, report, opts, progress),
        ))
    }

    /// Download, verify and install what `report` found. The caller holds the app's lock.
    fn apply_report(
        &self,
        http: &dyn Http,
        app: &AppRecord,
        rec: &SourceRecord,
        report: Report,
        opts: &ApplyOptions,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<Applied> {
        let id = app.id.as_str();
        let tag = report.latest_version().to_string();
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
                // Held to what a normal download is checked against; anything that doesn't
                // match is thrown away and fetched in full instead.
                match delta::verify(&out, &asset, report.latest_version()) {
                    Ok(sha) => {
                        fetched = Some((out, sha));
                        used_delta = true;
                    }
                    Err(e) => {
                        let _ = fs::remove_file(&out);
                        self.warn(format!(
                            "{id}: delta update discarded ({e}); downloading in full"
                        ));
                    }
                }
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

        // oysterAV looks before anything is replaced; in `block` mode a threat ends the update
        // here, and the download is thrown away.
        let verdict = match self.scan_before_install(&new_file, &new_sha) {
            Ok(v) => v,
            Err(e) => {
                let _ = fs::remove_file(&new_file);
                return Err(e);
            }
        };

        let hooks = self.db.list_hooks(id)?;
        let hook = |kind: &str| {
            hooks
                .iter()
                .find(|(k, _)| k == kind)
                .map(|(_, c)| c.clone())
        };
        if let Some(cmd) = hook("pre-update") {
            // A failing pre-update hook vetoes the update; nothing has been changed yet.
            exec_hook(app, "pre-update", &cmd).map_err(|e| {
                Error::Other(format!(
                    "{e}; update cancelled, your installed version is untouched"
                ))
            })?;
        }

        let (installed_version, _) = self.installed_of(app, rec);
        // An opaque identifier (ETag, SHA-1) is no version to show.
        let hint = (!report.release.opaque).then_some(tag.as_str());
        self.swap_in(
            app,
            installed_version.as_deref(),
            &new_file,
            &asset.name,
            hint,
            opts.keep_versions,
            Some(&new_sha),
        )?;
        self.db.record_installed(id, Some(&tag), Some(&new_sha))?;
        if let Some(v) = &verdict {
            let _ = self.record_verdict(id, v);
        }
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
            from: installed_version.or(app.version.clone()),
            to: tag,
            delta: used_delta,
        })
    }
}
