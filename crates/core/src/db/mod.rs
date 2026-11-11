//! SQLite-backed library database with versioned migrations.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Error, Result};
use crate::launch::Profile;
use crate::source::{Channel, Origin, Policy, Provider, Source};
mod settings;
mod sources;

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
mod tests;
