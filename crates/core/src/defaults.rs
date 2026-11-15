//! Library-wide defaults: settings that apply to every app rather than to one.
//!
//! They live in the library's database, so the window, the command line and the background
//! timer all follow the same choices.

use crate::engine::DEFAULT_KEEP_VERSIONS;
use crate::error::{Error, Result};
use crate::library::Library;
use crate::sandbox::{Level, Settings};
use crate::source::Policy;

const POLICY: &str = "default-policy";
const KEEP_VERSIONS: &str = "keep-versions";
const SANDBOX: &str = "default-sandbox";

/// The most previous versions that may be kept per app.
pub const MAX_KEEP_VERSIONS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Defaults {
    /// The update policy a newly found update source starts with.
    pub policy: Policy,
    /// How many previous versions an update keeps for rollback.
    pub keep_versions: usize,
    /// The sandbox a newly added app starts with (`None` = no sandbox).
    pub sandbox: Level,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            policy: Policy::Notify,
            keep_versions: DEFAULT_KEEP_VERSIONS,
            sandbox: Level::None,
        }
    }
}

impl Library {
    /// The library-wide defaults; anything never set (or unreadable) is the built-in default.
    pub fn defaults(&self) -> Result<Defaults> {
        let d = Defaults::default();
        Ok(Defaults {
            policy: self
                .db
                .get_library_setting(POLICY)?
                .and_then(|v| Policy::parse(&v))
                .unwrap_or(d.policy),
            keep_versions: self.keep_versions()?,
            sandbox: self
                .db
                .get_library_setting(SANDBOX)?
                .and_then(|v| Level::parse(&v))
                .filter(|l| matches!(l, Level::None | Level::Standard | Level::Strict))
                .unwrap_or(d.sandbox),
        })
    }

    /// How many previous versions an update keeps.
    pub fn keep_versions(&self) -> Result<usize> {
        Ok(self
            .db
            .get_library_setting(KEEP_VERSIONS)?
            .and_then(|v| v.parse::<usize>().ok())
            .map(|n| n.min(MAX_KEEP_VERSIONS))
            .unwrap_or(DEFAULT_KEEP_VERSIONS))
    }

    /// The policy new update sources start with. Existing apps keep theirs (see
    /// [`Library::set_policy_all`]).
    pub fn set_default_policy(&self, p: Policy) -> Result<()> {
        self.db.set_library_setting(POLICY, Some(p.as_str()))
    }

    /// Give every app that has an update source the same policy. Returns how many changed.
    pub fn set_policy_all(&self, p: Policy) -> Result<usize> {
        self.db.set_policy_all(p)
    }

    /// How many previous versions to keep. Lowering it deletes the extra ones right away.
    pub fn set_keep_versions(&self, n: usize) -> Result<()> {
        if n > MAX_KEEP_VERSIONS {
            return Err(Error::Other(format!(
                "at most {MAX_KEEP_VERSIONS} previous versions can be kept"
            )));
        }
        let old = self.keep_versions()?;
        self.db
            .set_library_setting(KEEP_VERSIONS, Some(&n.to_string()))?;
        if n < old {
            for app in self.list()? {
                // Under the app's lock, so an update in progress is not pulled from under it.
                let _lock = self.lock_app(&app.id)?;
                self.prune_versions(&app.id, n)?;
            }
        }
        Ok(())
    }

    /// The sandbox newly added apps start with: `none`, `standard` or `strict`.
    pub fn set_default_sandbox(&self, level: Level) -> Result<()> {
        if level == Level::Custom {
            return Err(Error::Other(
                "the default sandbox must be none, standard or strict".into(),
            ));
        }
        self.db.set_library_setting(SANDBOX, Some(level.as_str()))
    }

    /// Give a newly added app the default sandbox, if there is one.
    pub(crate) fn apply_default_sandbox(&self, id: &str) -> Result<()> {
        let level = self.defaults()?.sandbox;
        if level != Level::None && self.db.get_sandbox(id)?.is_none() {
            self.set_sandbox(id, &Settings::for_level(level))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
