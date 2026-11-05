//! SQLite-backed library database with versioned migrations.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Error, Result};

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

    pub fn delete(&self, id: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM apps WHERE id = ?1", [id])? > 0)
    }
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
    fn reopening_does_not_remigrate() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/db.sqlite");
        Db::open(&p).unwrap().insert(&rec("a", "/x/a")).unwrap();
        let db = Db::open(&p).unwrap();
        assert_eq!(db.list().unwrap().len(), 1);
    }
}
