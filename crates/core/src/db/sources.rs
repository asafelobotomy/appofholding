//! App sources and the update-check state stored with them.

use super::*;

impl Db {
    /// Create or replace the *configuration* of an app's source. Tracked state (last check,
    /// installed version) is kept, so editing a source never loses history.
    pub fn upsert_source(&self, app_id: &str, s: &Source) -> Result<()> {
        let (provider, host, location) = match &s.provider {
            Provider::GitHub { owner, repo } => ("github", None, format!("{owner}/{repo}")),
            Provider::GitLab { host, project } => ("gitlab", Some(host.clone()), project.clone()),
            Provider::Forgejo { host, owner, repo } => {
                ("forgejo", Some(host.clone()), format!("{owner}/{repo}"))
            }
            Provider::Url { url } => ("url", None, url.clone()),
            Provider::Zsync { url } => ("zsync", None, url.clone()),
        };
        let (channel, tag) = match &s.channel {
            Channel::Stable => ("stable", None),
            Channel::Prerelease => ("pre", None),
            Channel::Any => ("any", None),
            Channel::Tag(t) => ("tag", Some(t.clone())),
        };
        let origin = match s.origin {
            Origin::Embedded => "embedded",
            Origin::User => "user",
        };
        self.conn.execute(
            "INSERT INTO sources (app_id,provider,host,location,channel,tag,asset_pattern,origin)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(app_id) DO UPDATE SET provider=?2,host=?3,location=?4,channel=?5,tag=?6,asset_pattern=?7,origin=?8",
            params![app_id, provider, host, location, channel, tag, s.asset_pattern, origin],
        )?;
        Ok(())
    }

    pub fn get_source(&self, app_id: &str) -> Result<Option<SourceRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM sources WHERE app_id = ?1",
                [app_id],
                row_to_source,
            )
            .optional()?)
    }

    pub fn list_sources(&self) -> Result<Vec<SourceRecord>> {
        let mut stmt = self.conn.prepare("SELECT * FROM sources ORDER BY app_id")?;
        let rows = stmt.query_map([], row_to_source)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn delete_source(&self, app_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM sources WHERE app_id = ?1", [app_id])?
            > 0)
    }

    /// Every check ends in an explicit recorded state: a status or an error, never "pending".
    pub fn record_check(
        &self,
        app_id: &str,
        at: i64,
        status: Option<&str>,
        error: Option<&str>,
        latest: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET last_checked=?2, last_status=?3, last_error=?4, last_latest=COALESCE(?5,last_latest) WHERE app_id=?1",
            params![app_id, at, status, error, latest],
        )?;
        Ok(())
    }

    pub fn record_installed(
        &self,
        app_id: &str,
        version: Option<&str>,
        digest: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET installed_version=?2, installed_digest=?3 WHERE app_id=?1",
            params![app_id, version, digest],
        )?;
        Ok(())
    }

    pub fn set_policy(&self, app_id: &str, p: Policy) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET policy=?2 WHERE app_id=?1",
            params![app_id, p.as_str()],
        )?;
        Ok(())
    }

    pub fn set_skipped(&self, app_id: &str, v: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET skipped_version=?2 WHERE app_id=?1",
            params![app_id, v],
        )?;
        Ok(())
    }

    pub fn set_notified(&self, app_id: &str, v: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET notified_version=?2 WHERE app_id=?1",
            params![app_id, v],
        )?;
        Ok(())
    }

    pub fn set_etag(&self, app_id: &str, etag: Option<&str>, ctx: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET etag=?2, etag_ctx=?3 WHERE app_id=?1",
            params![app_id, etag, ctx],
        )?;
        Ok(())
    }
}
