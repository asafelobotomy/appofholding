//! Per-app settings, profiles, sandbox settings and hooks.

use super::*;

impl Db {
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
}
