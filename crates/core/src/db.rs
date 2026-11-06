//! SQLite-backed library database with versioned migrations.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Error, Result};
use crate::launch::Profile;
use crate::source::{Channel, Origin, Policy, Provider, Source};

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
    // v3: update policy, ETag cache, and kept previous versions for rollback
    "ALTER TABLE sources ADD COLUMN policy TEXT NOT NULL DEFAULT 'notify';
     ALTER TABLE sources ADD COLUMN skipped_version TEXT;
     ALTER TABLE sources ADD COLUMN notified_version TEXT;
     ALTER TABLE sources ADD COLUMN etag TEXT;
     ALTER TABLE sources ADD COLUMN etag_ctx TEXT;
     CREATE TABLE versions (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        app_id     TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
        version    TEXT,
        sha256     TEXT NOT NULL,
        file_name  TEXT NOT NULL,   -- the AppImage's original file name
        path       TEXT NOT NULL,
        created_at INTEGER NOT NULL
     );",
    // v4: launch profiles and hooks
    "CREATE TABLE launch_profiles (
        app_id           TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
        name             TEXT NOT NULL,
        args             TEXT NOT NULL DEFAULT '[]',   -- JSON array
        env              TEXT NOT NULL DEFAULT '{}',   -- JSON object
        prefix           TEXT NOT NULL DEFAULT '[]',   -- JSON array
        presets          TEXT NOT NULL DEFAULT '[]',   -- JSON array
        cwd              TEXT,
        extract_and_run  INTEGER NOT NULL DEFAULT 0,
        elevated         INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (app_id, name)
     );
     CREATE TABLE hooks (
        app_id  TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
        kind    TEXT NOT NULL,   -- post-install | pre-update | post-update
        command TEXT NOT NULL,
        PRIMARY KEY (app_id, kind)
     );",
    // v5: sandbox settings (JSON; absent = sandbox off)
    "CREATE TABLE sandbox (
        app_id   TEXT PRIMARY KEY REFERENCES apps(id) ON DELETE CASCADE,
        settings TEXT NOT NULL
     );",
    // v6: apps adopted in place keep their file wherever the user had it
    "ALTER TABLE apps ADD COLUMN external_file INTEGER NOT NULL DEFAULT 0;",
    // v7: small per-app choices (suggestions dismissed or applied)
    "CREATE TABLE app_settings (
        app_id TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
        key    TEXT NOT NULL,
        value  TEXT NOT NULL,
        PRIMARY KEY (app_id, key)
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
    /// The AppImage lives outside the library folder (adopted in place), so it is the user's
    /// file and removing the app must not delete it unless asked to.
    pub external_file: bool,
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
    pub policy: Policy,
    /// A version the user chose to ignore ("skip this version").
    pub skipped_version: Option<String>,
    /// The version we already told the user about (so notifications fire once).
    pub notified_version: Option<String>,
    pub etag: Option<String>,
    /// What the ETag was computed for; it is only reused while this still matches.
    pub etag_ctx: Option<String>,
}

/// A previous version kept on disk for rollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRecord {
    pub id: i64,
    pub app_id: String,
    pub version: Option<String>,
    pub sha256: String,
    pub file_name: String,
    pub path: PathBuf,
    pub created_at: i64,
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
        // The GUI, the CLI and the background timer share one file; wait for each other briefly
        // instead of failing with "database is locked".
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
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
            "INSERT INTO apps (id,name,appimage_path,desktop_file,icon_file,version,sha256,size,appimage_type,update_info,added_at,external_file)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
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
                a.external_file,
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

    /// Replace everything about an app except its id and when it was added.
    pub fn update_app(&self, a: &AppRecord) -> Result<()> {
        self.conn.execute(
            "UPDATE apps SET name=?2, appimage_path=?3, desktop_file=?4, icon_file=?5, version=?6, sha256=?7, size=?8, appimage_type=?9, update_info=?10 WHERE id=?1",
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
            ],
        )?;
        Ok(())
    }

    pub fn add_version(
        &self,
        app_id: &str,
        version: Option<&str>,
        sha256: &str,
        file_name: &str,
        path: &Path,
        at: i64,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO versions (app_id, version, sha256, file_name, path, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![app_id, version, sha256, file_name, path.to_string_lossy(), at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Newest first.
    pub fn list_versions(&self, app_id: &str) -> Result<Vec<VersionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM versions WHERE app_id = ?1 ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([app_id], |r| {
            Ok(VersionRecord {
                id: r.get("id")?,
                app_id: r.get("app_id")?,
                version: r.get("version")?,
                sha256: r.get("sha256")?,
                file_name: r.get("file_name")?,
                path: PathBuf::from(r.get::<_, String>("path")?),
                created_at: r.get("created_at")?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn get_setting(&self, app_id: &str, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM app_settings WHERE app_id = ?1 AND key = ?2",
                [app_id, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, app_id: &str, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => self.conn.execute(
                "INSERT INTO app_settings (app_id,key,value) VALUES (?1,?2,?3)
                 ON CONFLICT(app_id,key) DO UPDATE SET value=?3",
                params![app_id, key, v],
            )?,
            None => self.conn.execute(
                "DELETE FROM app_settings WHERE app_id = ?1 AND key = ?2",
                [app_id, key],
            )?,
        };
        Ok(())
    }

    pub fn put_profile(&self, app_id: &str, name: &str, p: &Profile) -> Result<()> {
        self.conn.execute(
            "INSERT INTO launch_profiles (app_id,name,args,env,prefix,presets,cwd,extract_and_run,elevated)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(app_id,name) DO UPDATE SET args=?3,env=?4,prefix=?5,presets=?6,cwd=?7,extract_and_run=?8,elevated=?9",
            params![
                app_id,
                name,
                serde_json::to_string(&p.args).unwrap_or_default(),
                serde_json::to_string(&p.env).unwrap_or_default(),
                serde_json::to_string(&p.prefix).unwrap_or_default(),
                serde_json::to_string(&p.presets).unwrap_or_default(),
                p.cwd.as_ref().map(|c| c.to_string_lossy().into_owned()),
                p.extract_and_run,
                p.elevated,
            ],
        )?;
        Ok(())
    }

    /// All profiles of an app, `default` first, then by name.
    pub fn list_profiles(&self, app_id: &str) -> Result<Vec<(String, Profile)>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM launch_profiles WHERE app_id = ?1 ORDER BY name != 'default', name",
        )?;
        let rows = stmt.query_map([app_id], |r| {
            let parse = |col: &str| -> rusqlite::Result<String> { r.get(col) };
            Ok((
                r.get::<_, String>("name")?,
                Profile {
                    args: serde_json::from_str(&parse("args")?).unwrap_or_default(),
                    env: serde_json::from_str(&parse("env")?).unwrap_or_default(),
                    prefix: serde_json::from_str(&parse("prefix")?).unwrap_or_default(),
                    presets: serde_json::from_str(&parse("presets")?).unwrap_or_default(),
                    cwd: r.get::<_, Option<String>>("cwd")?.map(PathBuf::from),
                    extract_and_run: r.get("extract_and_run")?,
                    elevated: r.get("elevated")?,
                },
            ))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn delete_profile(&self, app_id: &str, name: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM launch_profiles WHERE app_id = ?1 AND name = ?2",
            params![app_id, name],
        )? > 0)
    }

    pub fn get_sandbox(&self, app_id: &str) -> Result<Option<crate::sandbox::Settings>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT settings FROM sandbox WHERE app_id = ?1",
                [app_id],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| {
            serde_json::from_str(&j)
                .map_err(|e| Error::Other(format!("corrupt sandbox settings for {app_id}: {e}")))
        })
        .transpose()
    }

    /// Every app's saved sandbox settings in one query (apps without a row are absent).
    pub fn list_sandbox(
        &self,
    ) -> Result<std::collections::HashMap<String, crate::sandbox::Settings>> {
        let mut stmt = self.conn.prepare("SELECT app_id, settings FROM sandbox")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id, json) = row?;
            let s = serde_json::from_str(&json)
                .map_err(|e| Error::Other(format!("corrupt sandbox settings for {id}: {e}")))?;
            out.insert(id, s);
        }
        Ok(out)
    }

    pub fn put_sandbox(&self, app_id: &str, s: &crate::sandbox::Settings) -> Result<()> {
        let json = serde_json::to_string(s).map_err(|e| Error::Other(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO sandbox (app_id, settings) VALUES (?1, ?2) ON CONFLICT(app_id) DO UPDATE SET settings = ?2",
            params![app_id, json],
        )?;
        Ok(())
    }

    pub fn set_hook(&self, app_id: &str, kind: &str, command: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO hooks (app_id,kind,command) VALUES (?1,?2,?3)
             ON CONFLICT(app_id,kind) DO UPDATE SET command=?3",
            params![app_id, kind, command],
        )?;
        Ok(())
    }

    pub fn clear_hook(&self, app_id: &str, kind: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM hooks WHERE app_id = ?1 AND kind = ?2",
            params![app_id, kind],
        )? > 0)
    }

    pub fn list_hooks(&self, app_id: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, command FROM hooks WHERE app_id = ?1 ORDER BY kind")?;
        let rows = stmt.query_map([app_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn delete_version(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM versions WHERE id = ?1", [id])?;
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
        policy: Policy::parse(&r.get::<_, String>("policy")?).unwrap_or_default(),
        skipped_version: r.get("skipped_version")?,
        notified_version: r.get("notified_version")?,
        etag: r.get("etag")?,
        etag_ctx: r.get("etag_ctx")?,
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
        external_file: r.get("external_file")?,
    })
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
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
            external_file: false,
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
    fn policy_defaults_and_survives_source_edits() {
        let db = Db::open_in_memory().unwrap();
        db.insert(&rec("a", "/x/a")).unwrap();
        let mut s = Source::new(Provider::GitHub {
            owner: "o".into(),
            repo: "r".into(),
        });
        db.upsert_source("a", &s).unwrap();
        assert_eq!(db.get_source("a").unwrap().unwrap().policy, Policy::Notify);
        db.set_policy("a", Policy::Auto).unwrap();
        db.set_skipped("a", Some("v9")).unwrap();
        db.set_etag("a", Some("W/\"1\""), Some("ctx")).unwrap();
        s.asset_pattern = Some("x*".into());
        db.upsert_source("a", &s).unwrap();
        let got = db.get_source("a").unwrap().unwrap();
        assert_eq!(got.policy, Policy::Auto);
        assert_eq!(got.skipped_version.as_deref(), Some("v9"));
        assert_eq!(got.etag_ctx.as_deref(), Some("ctx"));
    }

    #[test]
    fn versions_and_app_update() {
        let db = Db::open_in_memory().unwrap();
        let mut a = rec("a", "/x/a");
        db.insert(&a).unwrap();
        let v1 = db
            .add_version(
                "a",
                Some("1.0"),
                "s1",
                "A-1.0.AppImage",
                Path::new("/v/1"),
                10,
            )
            .unwrap();
        let v2 = db
            .add_version("a", None, "s2", "A-2.AppImage", Path::new("/v/2"), 20)
            .unwrap();
        let list = db.list_versions("a").unwrap();
        assert_eq!(list.iter().map(|v| v.id).collect::<Vec<_>>(), vec![v2, v1]);
        db.delete_version(v2).unwrap();
        assert_eq!(db.list_versions("a").unwrap().len(), 1);

        a.version = Some("2.0".into());
        a.appimage_path = "/x/new".into();
        a.added_at = 999; // never overwritten
        db.update_app(&a).unwrap();
        let got = db.get("a").unwrap().unwrap();
        assert_eq!(
            (
                got.version.as_deref(),
                got.appimage_path.to_str(),
                got.added_at
            ),
            (Some("2.0"), Some("/x/new"), 1)
        );

        db.delete("a").unwrap();
        assert!(
            db.list_versions("a").unwrap().is_empty(),
            "versions cascade with the app"
        );
    }

    #[test]
    fn profiles_and_hooks_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        db.insert(&rec("a", "/x/a")).unwrap();
        let mut p = Profile::default();
        p.args = vec!["--a b".into(), "c".into()];
        p.env.insert("K".into(), "v=1".into());
        p.prefix = vec!["nice".into()];
        p.presets = vec!["x11".into()];
        p.cwd = Some("/srv".into());
        p.extract_and_run = true;
        db.put_profile("a", "default", &p).unwrap();
        db.put_profile("a", "debug", &Profile::default()).unwrap();
        db.put_profile("a", "default", &p).unwrap(); // upsert
        let list = db.list_profiles("a").unwrap();
        assert_eq!(
            list.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["default", "debug"]
        );
        assert_eq!(list[0].1, p);
        assert!(db.delete_profile("a", "debug").unwrap());

        db.set_hook("a", "post-update", "echo 1").unwrap();
        db.set_hook("a", "post-update", "echo 2").unwrap();
        db.set_hook("a", "pre-update", "true").unwrap();
        assert_eq!(
            db.list_hooks("a").unwrap(),
            vec![
                ("post-update".to_string(), "echo 2".to_string()),
                ("pre-update".to_string(), "true".to_string())
            ]
        );
        assert!(db.clear_hook("a", "pre-update").unwrap());
        db.delete("a").unwrap();
        assert!(
            db.list_profiles("a").unwrap().is_empty() && db.list_hooks("a").unwrap().is_empty()
        );
    }

    #[test]
    fn sandbox_settings_roundtrip() {
        use crate::sandbox::{Bus, HomeAccess, Level, PathGrant, Settings};
        let db = Db::open_in_memory().unwrap();
        db.insert(&rec("a", "/x/a")).unwrap();
        assert!(db.get_sandbox("a").unwrap().is_none());
        let s = Settings {
            home: HomeAccess::ReadOnly,
            bus: Bus::Full,
            paths: vec![PathGrant {
                path: "/srv/x".into(),
                write: true,
            }],
            ..Settings::for_level(Level::Standard)
        }
        .normalized();
        db.put_sandbox("a", &s).unwrap();
        db.put_sandbox("a", &s).unwrap();
        assert_eq!(db.get_sandbox("a").unwrap().unwrap(), s);
        assert_eq!(s.level, Level::Custom);
        db.delete("a").unwrap();
        assert!(db.get_sandbox("a").unwrap().is_none());
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
