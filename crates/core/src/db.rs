//! SQLite-backed library database with versioned migrations.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Error, Result};
use crate::source::{Channel, Origin, Provider, Source};

/// Each entry migrates the schema from version N to N+1 (`PRAGMA user_version`).
const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE apps (
        id            TEXT PRIMARY KEY,
        name          TEXT NOT NULL,
        appimage_path TEXT NOT NULL UNIQUE,
        desktop_file  TEXT NOT NULL,
        icon_file     TEXT,
        version       TEXT,
        sha256        TEXT NOT NULL,
        size          INTEGER NOT NULL,
        appimage_type INTEGER NOT NULL,
        update_info   TEXT,
        added_at      INTEGER NOT NULL
    );",
    // v2: update sources and their tracked state
    "CREATE TABLE sources (
        app_id           TEXT PRIMARY KEY REFERENCES apps(id) ON DELETE CASCADE,
        provider         TEXT NOT NULL,     -- github | gitlab | forgejo | url
        host             TEXT,              -- gitlab / forgejo only
        location         TEXT NOT NULL,     -- owner/repo, project path, or URL
        channel          TEXT NOT NULL,     -- stable | pre | any | tag
        tag              TEXT,
        asset_pattern    TEXT,
        origin           TEXT NOT NULL,     -- embedded | user
        last_checked     INTEGER,
        last_status      TEXT,
        last_error       TEXT,
        last_latest      TEXT,
        installed_version TEXT,
        installed_digest TEXT
    );",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRecord {
    pub id: String,
    pub name: String,
    pub appimage_path: PathBuf,
    pub desktop_file: PathBuf,
    pub icon_file: Option<PathBuf>,
    pub version: Option<String>,
    pub sha256: String,
    pub size: u64,
    pub appimage_type: u8,
    pub update_info: Option<String>,
    pub added_at: i64,
}

/// A saved update source plus what we last learned from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub app_id: String,
    pub source: Source,
    pub last_checked: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_latest: Option<String>,
    pub installed_version: Option<String>,
    pub installed_digest: Option<String>,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let mut db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()?;
        if current > MIGRATIONS.len() {
            return Err(Error::Other(format!(
                "database schema v{current} is newer than this build supports (v{})",
                MIGRATIONS.len()
            )));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<usize> {
        let v: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        Ok(v as usize)
    }

    pub fn insert(&self, a: &AppRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO apps (id,name,appimage_path,desktop_file,icon_file,version,sha256,size,appimage_type,update_info,added_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                a.id,
                a.name,
                a.appimage_path.to_string_lossy(),
                a.desktop_file.to_string_lossy(),
                a.icon_file.as_ref().map(|p| p.to_string_lossy().into_owned()),
                a.version,
                a.sha256,
                a.size as i64,
                a.appimage_type,
                a.update_info,
                a.added_at,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<AppRecord>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM apps WHERE id = ?1", [id], row_to_app)
            .optional()?)
    }

    pub fn find_by_path(&self, path: &Path) -> Result<Option<AppRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM apps WHERE appimage_path = ?1",
                [path.to_string_lossy()],
                row_to_app,
            )
            .optional()?)
    }

    pub fn list(&self) -> Result<Vec<AppRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM apps ORDER BY lower(name), id")?;
        let rows = stmt.query_map([], row_to_app)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

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

    pub fn delete(&self, id: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM apps WHERE id = ?1", [id])? > 0)
    }
}

fn row_to_source(r: &Row<'_>) -> rusqlite::Result<SourceRecord> {
    let bad = |what: &str, v: String| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("bad {what}: {v}").into(),
        )
    };
    let provider: String = r.get("provider")?;
    let host: Option<String> = r.get("host")?;
    let location: String = r.get("location")?;
    let split = |loc: &str| -> Option<(String, String)> {
        loc.split_once('/')
            .map(|(a, b)| (a.to_string(), b.to_string()))
    };
    let provider = match provider.as_str() {
        "github" => {
            let (owner, repo) =
                split(&location).ok_or_else(|| bad("location", location.clone()))?;
            Provider::GitHub { owner, repo }
        }
        "forgejo" => {
            let (owner, repo) =
                split(&location).ok_or_else(|| bad("location", location.clone()))?;
            Provider::Forgejo {
                host: host.unwrap_or_default(),
                owner,
                repo,
            }
        }
        "gitlab" => Provider::GitLab {
            host: host.unwrap_or_default(),
            project: location,
        },
        "url" => Provider::Url { url: location },
        "zsync" => Provider::Zsync { url: location },
        other => return Err(bad("provider", other.into())),
    };
    let channel_s: String = r.get("channel")?;
    let tag: Option<String> = r.get("tag")?;
    let channel = match channel_s.as_str() {
        "stable" => Channel::Stable,
        "pre" => Channel::Prerelease,
        "any" => Channel::Any,
        "tag" => Channel::Tag(tag.unwrap_or_default()),
        other => return Err(bad("channel", other.into())),
    };
    let origin = match r.get::<_, String>("origin")?.as_str() {
        "embedded" => Origin::Embedded,
        _ => Origin::User,
    };
    Ok(SourceRecord {
        app_id: r.get("app_id")?,
        source: Source {
            provider,
            channel,
            asset_pattern: r.get("asset_pattern")?,
            origin,
        },
        last_checked: r.get("last_checked")?,
        last_status: r.get("last_status")?,
        last_error: r.get("last_error")?,
        last_latest: r.get("last_latest")?,
        installed_version: r.get("installed_version")?,
        installed_digest: r.get("installed_digest")?,
    })
}

fn row_to_app(r: &Row<'_>) -> rusqlite::Result<AppRecord> {
    let path = |s: String| PathBuf::from(s);
    Ok(AppRecord {
        id: r.get("id")?,
        name: r.get("name")?,
        appimage_path: path(r.get("appimage_path")?),
        desktop_file: path(r.get("desktop_file")?),
        icon_file: r.get::<_, Option<String>>("icon_file")?.map(path),
        version: r.get("version")?,
        sha256: r.get("sha256")?,
        size: r.get::<_, i64>("size")? as u64,
        appimage_type: r.get("appimage_type")?,
        update_info: r.get("update_info")?,
        added_at: r.get("added_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, path: &str) -> AppRecord {
        AppRecord {
            id: id.into(),
            name: id.to_uppercase(),
            appimage_path: path.into(),
            desktop_file: format!("/d/{id}.desktop").into(),
            icon_file: None,
            version: Some("1.0".into()),
            sha256: "abc".into(),
            size: 42,
            appimage_type: 2,
            update_info: None,
            added_at: 1,
        }
    }

    #[test]
    fn migrates_and_crud() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len());
        db.insert(&rec("a", "/x/a")).unwrap();
        db.insert(&rec("b", "/x/b")).unwrap();
        assert_eq!(db.get("a").unwrap().unwrap(), rec("a", "/x/a"));
        assert_eq!(db.find_by_path(Path::new("/x/b")).unwrap().unwrap().id, "b");
        assert_eq!(db.list().unwrap().len(), 2);
        assert!(db.insert(&rec("a2", "/x/a")).is_err(), "path is unique");
        assert!(db.delete("a").unwrap());
        assert!(!db.delete("a").unwrap());
        assert!(db.get("a").unwrap().is_none());
    }

    #[test]
    fn sources_roundtrip_and_keep_state_when_edited() {
        let db = Db::open_in_memory().unwrap();
        db.insert(&rec("a", "/x/a")).unwrap();
        let mut src = Source::new(Provider::Forgejo {
            host: "codeberg.org".into(),
            owner: "o".into(),
            repo: "r".into(),
        });
        src.channel = Channel::Tag("nightly".into());
        src.asset_pattern = Some("A-*.AppImage".into());
        db.upsert_source("a", &src).unwrap();
        db.record_check("a", 100, Some("update-available"), None, Some("v2"))
            .unwrap();
        db.record_installed("a", Some("v1"), Some("dd")).unwrap();

        // Changing the pattern must not wipe tracked state (Gear Lever #450).
        src.asset_pattern = Some("B-*.AppImage".into());
        db.upsert_source("a", &src).unwrap();
        let got = db.get_source("a").unwrap().unwrap();
        assert_eq!(got.source, src);
        assert_eq!(got.last_status.as_deref(), Some("update-available"));
        assert_eq!(got.installed_version.as_deref(), Some("v1"));

        // An error check keeps the last known latest version but records the failure.
        db.record_check("a", 200, None, Some("boom"), None).unwrap();
        let got = db.get_source("a").unwrap().unwrap();
        assert_eq!(
            (got.last_error.as_deref(), got.last_latest.as_deref()),
            (Some("boom"), Some("v2"))
        );

        // Cascade on app deletion.
        db.delete("a").unwrap();
        assert!(db.get_source("a").unwrap().is_none());
    }

    #[test]
    fn every_provider_roundtrips() {
        let db = Db::open_in_memory().unwrap();
        db.insert(&rec("a", "/x/a")).unwrap();
        for p in [
            Provider::GitHub {
                owner: "o".into(),
                repo: "r".into(),
            },
            Provider::GitLab {
                host: "gitlab.com".into(),
                project: "g/sub/p".into(),
            },
            Provider::Url {
                url: "https://x/y.AppImage".into(),
            },
            Provider::Zsync {
                url: "https://x/y.AppImage.zsync".into(),
            },
        ] {
            let mut s = Source::new(p);
            s.origin = Origin::Embedded;
            db.upsert_source("a", &s).unwrap();
            assert_eq!(db.get_source("a").unwrap().unwrap().source, s);
        }
    }

    #[test]
    fn reopening_does_not_remigrate() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/db.sqlite");
        Db::open(&p).unwrap().insert(&rec("a", "/x/a")).unwrap();
        let db = Db::open(&p).unwrap();
        assert_eq!(db.list().unwrap().len(), 1);
    }
}
