//! The scheduled background run and the news it announces.

use super::*;

impl Library {
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

    /// Updates found by earlier checks that the user has not been told about yet, now marked as
    /// told. Shares its record with `run_scheduled`, so each new version is announced once,
    /// whichever of the background timer and the running app sees it first. Only apps set to
    /// `notify` are included: `auto` apps install themselves, `manual` and `pinned` stay quiet,
    /// and a skipped version is never announced.
    pub fn take_unannounced_updates(&self) -> Result<Vec<NewUpdate>> {
        let mut out = Vec::new();
        for rec in self.db.list_sources()? {
            if rec.policy != Policy::Notify
                || rec.last_status.as_deref().and_then(Status::parse)
                    != Some(Status::UpdateAvailable)
            {
                continue;
            }
            let Some(version) = rec.last_latest.clone() else {
                continue;
            };
            if rec.skipped_version.as_deref() == Some(version.as_str())
                || rec.notified_version.as_deref() == Some(version.as_str())
            {
                continue;
            }
            let name = self
                .get(&rec.app_id)
                .map(|a| a.name)
                .unwrap_or_else(|_| rec.app_id.clone());
            self.db.set_notified(&rec.app_id, Some(&version))?;
            out.push(NewUpdate {
                id: rec.app_id,
                name,
                version,
            });
        }
        Ok(out)
    }

    pub(super) fn scheduled_one(
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
